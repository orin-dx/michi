//! Executes michi-core's `Pipeline`/`PipelineStep`/`StepStatus` data model:
//! runs each step through a caller-supplied [`Step`] implementation, applies
//! retry/backoff and circuit-breaking on top of michi-resilience's pure
//! retry math, and writes each step's real outcome back into
//! `PipelineStep.status`.

#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

use std::time::Duration;

/// The crate's own execution-error type, implementing `std::error::Error`
/// via thiserror.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    /// HTTP-transport failure.
    #[error("http {status}: {message}")]
    Http {
        /// HTTP status code returned by the failed request.
        status: u16,
        /// Human-readable failure message.
        message: String,
        /// Server-supplied retry delay, if the caller already parsed one.
        retry_after: Option<Duration>,
    },
    /// A step attempt exceeded its configured timeout.
    #[error("step timed out after {elapsed_ms}ms")]
    Timeout {
        /// Milliseconds elapsed before the attempt was aborted.
        elapsed_ms: u64,
    },
    /// Generic step failure not tied to HTTP transport.
    #[error("{message}")]
    Failed {
        /// Human-readable failure message.
        message: String,
        /// Whether this failure is worth retrying.
        retryable: bool,
    },
    /// The circuit breaker short-circuited the call without invoking the step.
    #[error("circuit open, retry after {retry_after_ms}ms")]
    CircuitOpen {
        /// Milliseconds until the circuit may allow a probe call. On the
        /// Open-phase short-circuit path this is a precise remaining wait.
        /// On a losing concurrent HalfOpen caller's path (see
        /// [`CircuitBreaker::call`]'s HalfOpen single-flight gate) this is
        /// instead a conservative fixed estimate equal to the breaker's
        /// configured `open_duration_ms`, since the in-flight probe's
        /// outcome (and thus the true wait) is not yet known.
        retry_after_ms: u64,
    },
    /// A pipeline step failed, wrapping which step and its underlying cause.
    #[error("step {step_id} ({step_name}) failed: {source}")]
    StepFailed {
        /// The failed step's id.
        step_id: String,
        /// The failed step's name.
        step_name: String,
        /// The underlying error that caused this step to fail.
        source: Box<ExecutionError>,
    },
    /// `runners.len()` did not match `pipeline.steps.len()`.
    #[error("expected {expected} runners, got {got}")]
    StepCountMismatch {
        /// Expected runner count (`pipeline.steps.len()`).
        expected: usize,
        /// Actual runner count supplied (`runners.len()`).
        got: usize,
    },
    /// A depends_on edge, or an id-set mismatch, referenced a step id absent
    /// from the pipeline (or vice versa).
    #[error("unknown step id: {step_id}")]
    UnknownStepId {
        /// The offending step id.
        step_id: String,
    },
    /// A duplicate step id was found where dependency resolution requires
    /// each id to be unique.
    #[error("duplicate step id: {step_id}")]
    DuplicateStepId {
        /// The duplicated step id.
        step_id: String,
    },
    /// The dependency graph passed to execute_pipeline_parallel contains a
    /// cycle.
    #[error("cyclic dependency involving step id: {step_id}")]
    CyclicDependency {
        /// A step id that is part of the detected cycle.
        step_id: String,
    },
    /// Cancellation was observed before the pipeline run completed every
    /// step.
    #[error("pipeline execution cancelled")]
    Cancelled,
}

impl ExecutionError {
    /// Classifies whether retrying is worthwhile for this error. `Timeout`
    /// reports `true`, but [`CircuitBreaker::call`] only re-invokes a timed-out
    /// step that opts in via [`Step::is_idempotent`].
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http { status, .. } => michi_core::resilience::is_retryable_status(*status),
            Self::Timeout { .. } => true,
            Self::Failed { retryable, .. } => *retryable,
            Self::CircuitOpen { .. } => true,
            Self::StepFailed { source, .. } => source.is_retryable(),
            Self::StepCountMismatch { .. } => false,
            Self::UnknownStepId { .. } => false,
            Self::DuplicateStepId { .. } => false,
            Self::CyclicDependency { .. } => false,
            Self::Cancelled => false,
        }
    }
}

/// Extracts the caller-supplied retry hint from an error, recursing through
/// [`ExecutionError::StepFailed`] into its wrapped `source` so a step
/// failure never discards an inner `Http`/`CircuitOpen` retry hint.
fn retry_after_for(err: &ExecutionError) -> Option<Duration> {
    match err {
        ExecutionError::Http { retry_after, .. } => *retry_after,
        ExecutionError::CircuitOpen { retry_after_ms } => Some(Duration::from_millis(*retry_after_ms)),
        ExecutionError::StepFailed { source, .. } => retry_after_for(source),
        ExecutionError::Timeout { .. }
        | ExecutionError::Failed { .. }
        | ExecutionError::StepCountMismatch { .. }
        | ExecutionError::UnknownStepId { .. }
        | ExecutionError::DuplicateStepId { .. }
        | ExecutionError::CyclicDependency { .. }
        | ExecutionError::Cancelled => None,
    }
}

/// A cooperative, gate-only cancellation signal. Cheap to clone (an `Arc`
/// clone): hold one instance on the task that decides to cancel and pass
/// another into the in-flight `execute_pipeline`/`execute_pipeline_parallel`
/// call. `is_cancelled` is a synchronous, non-blocking check -- there is no
/// awaitable variant.
#[derive(Clone)]
pub struct CancellationToken(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancellationToken {
    /// Constructs a token in the not-cancelled state.
    #[must_use]
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }

    /// Signals cancellation. Idempotent -- safe to call more than once, and
    /// safe to call before the associated run starts or after it has
    /// already returned.
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Returns whether [`Self::cancel`] has been called on this token or any
    /// clone of it.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

use std::future::Future;
use std::pin::Pin;

/// What a pipeline step invokes. `attempt` is 0-indexed and supplied by
/// `CircuitBreaker::call`, incrementing by 1 on each retry. Object-safe by
/// construction, so `Vec<Box<dyn Step>>` is usable without an async-trait
/// dependency.
///
/// # Retry eligibility
///
/// [`CircuitBreaker::call`] may invoke [`Step::run`] more than once. Which
/// failures lead to a re-invocation:
///
/// - **Errors the step returns itself** (other than a timeout) are retried
///   exactly when [`ExecutionError::is_retryable`] says so. The step is
///   classifying its own outcome there (e.g. `Failed { retryable }`, an HTTP
///   status), so it is responsible for only marking an error retryable when
///   the attempt had no effect or repeating it is safe.
/// - **Timeouts are ambiguous outcomes.** When an attempt exceeds the
///   breaker's per-attempt timeout, its future is dropped mid-flight: it may
///   already have committed its side effect. The same holds for an
///   [`ExecutionError::Timeout`] the step returns, directly or wrapped in
///   [`ExecutionError::StepFailed`]. A timed-out step is re-invoked **only**
///   if [`Step::is_idempotent`] returns `true`; by default it is not, and the
///   timeout is returned after the single attempt.
pub trait Step: Send + Sync {
    /// Runs one attempt of this step.
    fn run<'a>(&'a self, attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>>;

    /// Whether running this step more than once has the same effect as
    /// running it once, so the breaker may re-invoke it after an ambiguous
    /// (timed-out) attempt. Defaults to `false`: a step must opt in to being
    /// retried after a timeout. See the trait-level "Retry eligibility"
    /// section.
    fn is_idempotent(&self) -> bool {
        false
    }
}

/// Adapter produced by [`step_fn`], implementing [`Step`] over an async
/// closure for callers who don't need custom state.
pub struct FnStep<F> {
    f: F,
    idempotent: bool,
}

impl<F> FnStep<F> {
    /// Declares this closure step idempotent, opting it in to being
    /// re-invoked after a timed-out attempt (see [`Step::is_idempotent`]).
    #[must_use]
    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }
}

/// Ergonomic adapter from an async closure into a value implementing
/// [`Step`], for callers who don't need custom state. The returned step is
/// not idempotent; call [`FnStep::idempotent`] to opt in to timeout retries.
pub fn step_fn<F, Fut>(f: F) -> FnStep<F>
where
    F: Fn(u32) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    FnStep { f, idempotent: false }
}

impl<F, Fut> Step for FnStep<F>
where
    F: Fn(u32) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    fn run<'a>(&'a self, attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
        Box::pin((self.f)(attempt))
    }

    fn is_idempotent(&self) -> bool {
        self.idempotent
    }
}

/// Whether `err` is an ambiguous outcome -- the attempt may or may not have
/// taken effect -- recursing through [`ExecutionError::StepFailed`].
fn is_ambiguous_outcome(err: &ExecutionError) -> bool {
    match err {
        ExecutionError::Timeout { .. } => true,
        ExecutionError::StepFailed { source, .. } => is_ambiguous_outcome(source),
        _ => false,
    }
}

impl From<ExecutionError> for michi_core::DomainError {
    fn from(err: ExecutionError) -> Self {
        match err {
            ExecutionError::Http { status, message, retry_after } => {
                let code = match status {
                    429 => michi_core::ErrorCode::RateLimited,
                    502..=504 => michi_core::ErrorCode::Unavailable,
                    _ => michi_core::ErrorCode::ExternalFailure,
                };
                let retryable = michi_core::resilience::is_retryable_status(status);
                let mut domain_err = michi_core::DomainError::new(code, message).retryable(retryable);
                if let Some(retry_after) = retry_after {
                    domain_err = domain_err.retry_after(retry_after);
                }
                domain_err
            }
            ExecutionError::Timeout { elapsed_ms } => michi_core::DomainError::new(
                michi_core::ErrorCode::Timeout,
                format!("step timed out after {elapsed_ms}ms"),
            )
            .retryable(true),
            ExecutionError::Failed { message, retryable } => {
                michi_core::DomainError::new(michi_core::ErrorCode::ExternalFailure, message).retryable(retryable)
            }
            ExecutionError::CircuitOpen { retry_after_ms } => michi_core::DomainError::new(
                michi_core::ErrorCode::Unavailable,
                format!("circuit open, retry after {retry_after_ms}ms"),
            )
            .retryable(true)
            .retry_after(std::time::Duration::from_millis(retry_after_ms)),
            ExecutionError::StepFailed { step_id, step_name, source } => {
                let inner: michi_core::DomainError = (*source).into();
                let message = format!("step {step_id} ({step_name}) failed: {}", inner.message);
                let mut domain_err = michi_core::DomainError::new(inner.code, message).retryable(inner.retryable);
                if let Some(retry_after) = inner.retry_after {
                    domain_err = domain_err.retry_after(retry_after);
                }
                domain_err
            }
            ExecutionError::StepCountMismatch { expected, got } => michi_core::DomainError::new(
                michi_core::ErrorCode::InvalidInput,
                format!("expected {expected} runners, got {got}"),
            )
            .retryable(false),
            ExecutionError::UnknownStepId { step_id } => {
                michi_core::DomainError::new(michi_core::ErrorCode::InvalidInput, format!("unknown step id: {step_id}"))
                    .retryable(false)
            }
            ExecutionError::DuplicateStepId { step_id } => michi_core::DomainError::new(
                michi_core::ErrorCode::InvalidInput,
                format!("duplicate step id: {step_id}"),
            )
            .retryable(false),
            ExecutionError::CyclicDependency { step_id } => michi_core::DomainError::new(
                michi_core::ErrorCode::InvalidInput,
                format!("cyclic dependency involving step id: {step_id}"),
            )
            .retryable(false),
            ExecutionError::Cancelled => {
                michi_core::DomainError::new(michi_core::ErrorCode::Unavailable, "pipeline execution cancelled")
                    .retryable(false)
            }
        }
    }
}

#[allow(
    clippy::disallowed_types,
    reason = "HashMap used only for internal state below; each use site carries its own justification"
)]
use std::collections::{HashMap, HashSet};

/// A dependency graph over step ids, consumed by [`execute_pipeline_parallel`].
#[derive(Debug)]
pub struct StepDependencies {
    ids: HashSet<String>,
    #[allow(clippy::disallowed_types, reason = "internal adjacency map, never rendered as output")]
    edges: HashMap<String, Vec<String>>,
}

impl StepDependencies {
    /// Constructs an empty dependency graph (no edges) over ids. Fails
    /// with ExecutionError::DuplicateStepId if ids contains the same
    /// id more than once.
    pub fn new(ids: &[String]) -> Result<Self, ExecutionError> {
        let mut seen = HashSet::new();
        for id in ids {
            if !seen.insert(id.clone()) {
                return Err(ExecutionError::DuplicateStepId { step_id: id.clone() });
            }
        }
        #[allow(clippy::disallowed_types, reason = "internal adjacency map, never rendered as output")]
        let edges = HashMap::new();
        Ok(Self { ids: seen, edges })
    }

    /// Declares that dependent must not be invoked until dependency has
    /// reached StepStatus::Completed. Calling this repeatedly for the same
    /// dependent accumulates additional prerequisite edges rather than
    /// overwriting prior ones.
    pub fn depends_on(&mut self, dependent: &str, dependency: &str) {
        self.edges.entry(dependent.to_string()).or_default().push(dependency.to_string());
    }
}

/// Returns Some(step_id) for one step id participating in a cycle formed
/// by deps's edges (including a 1-node self-dependency), or None if the
/// graph is acyclic. Uses a standard 3-color DFS: White (unvisited), Gray
/// (on the current DFS path), Black (fully explored). The traversal is
/// iterative over an explicit heap-allocated stack, so graph depth is bounded
/// by memory rather than by the thread's call stack (a recursive version
/// aborted the process on a 100,000-node cycle). The reported id is the
/// Gray node a back edge reaches, which always lies on the cycle.
#[allow(clippy::disallowed_types, reason = "internal DFS coloring scratch state, never rendered as output")]
fn detect_cycle(deps: &StepDependencies) -> Option<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut color: HashMap<&str, Color> = deps.ids.iter().map(|id| (id.as_str(), Color::White)).collect();
    // Each frame is (node, index of the next prerequisite edge to explore).
    let mut stack: Vec<(&str, usize)> = Vec::new();
    for root in &deps.ids {
        if color.get(root.as_str()).copied() != Some(Color::White) {
            continue;
        }
        color.insert(root.as_str(), Color::Gray);
        stack.push((root.as_str(), 0));
        while let Some(frame) = stack.last_mut() {
            let (node, next) = *frame;
            let prereq = deps.edges.get(node).and_then(|prereqs| prereqs.get(next));
            match prereq {
                Some(prereq) => {
                    frame.1 += 1;
                    let p = prereq.as_str();
                    match color.get(p).copied() {
                        Some(Color::Gray) => return Some(p.to_string()),
                        Some(Color::White) => {
                            color.insert(p, Color::Gray);
                            stack.push((p, 0));
                        }
                        // Black: already fully explored. None: an edge
                        // endpoint outside the id set, which callers reject
                        // as UnknownStepId before this runs.
                        Some(Color::Black) | None => {}
                    }
                }
                None => {
                    color.insert(node, Color::Black);
                    stack.pop();
                }
            }
        }
    }
    None
}

/// The circuit breaker's current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerPhase {
    /// Calls are attempted normally.
    Closed,
    /// Calls are short-circuited without invoking the step.
    Open,
    /// A single probe call is allowed to test recovery.
    HalfOpen,
}

/// The stored phase. HalfOpen is not stored on its own: an `Open` whose
/// `open_duration` has elapsed is reported as HalfOpen with no probe
/// claimed yet, and `Probing` is HalfOpen with the single probe in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredPhase {
    Closed,
    Open { opened_at_ms: u64 },
    Probing { opened_at_ms: u64 },
}

/// Every piece of mutable breaker state, guarded together by one mutex so
/// phase, probe ownership, the failure count and every transition form a
/// single state machine. `generation` increments on every phase transition;
/// each admitted call remembers the generation it was admitted in, and its
/// outcome is applied only if the breaker is still in that generation.
#[derive(Debug)]
struct BreakerState {
    phase: StoredPhase,
    consecutive_failures: u32,
    generation: u64,
}

/// How a call was admitted by [`CircuitBreaker::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// A normal call admitted while Closed, in `generation`.
    Closed { generation: u64 },
    /// The single HalfOpen probe, which moved the breaker into `generation`.
    Probe { generation: u64 },
}

/// A circuit breaker composing michi-resilience's retry math with
/// tokio-clock-based async waiting, per-attempt timeout, and
/// consecutive-failure circuit-opening. All mutable state lives behind one
/// `std::sync::Mutex` that is never held across an `.await` (or while a
/// step runs), so [`CircuitBreaker::phase`] stays synchronous and every
/// admission decision and transition is atomic with respect to the others.
pub struct CircuitBreaker {
    retry_config: michi_core::resilience::RetryConfig,
    step_timeout: Duration,
    failure_threshold: u32,
    open_duration: Duration,
    state: std::sync::Mutex<BreakerState>,
    epoch: tokio::time::Instant,
}

impl CircuitBreaker {
    /// Infallible, normalizing constructor. `failure_threshold == 0` is
    /// floored to 1; `step_timeout`/`open_duration` of `Duration::ZERO` are
    /// each independently floored to `Duration::from_millis(1)`.
    pub fn new(
        retry_config: michi_core::resilience::RetryConfig,
        step_timeout: Duration,
        failure_threshold: u32,
        open_duration: Duration,
    ) -> Self {
        let step_timeout = if step_timeout.is_zero() { Duration::from_millis(1) } else { step_timeout };
        let open_duration = if open_duration.is_zero() { Duration::from_millis(1) } else { open_duration };
        let failure_threshold = if failure_threshold == 0 { 1 } else { failure_threshold };
        Self {
            retry_config,
            step_timeout,
            failure_threshold,
            open_duration,
            state: std::sync::Mutex::new(BreakerState {
                phase: StoredPhase::Closed,
                consecutive_failures: 0,
                generation: 0,
            }),
            epoch: tokio::time::Instant::now(),
        }
    }

    /// Current phase. Open is derived dynamically: once at least
    /// `open_duration` of tokio virtual time has elapsed since the breaker
    /// most recently opened, this reports `HalfOpen` even before any caller
    /// has claimed the probe.
    pub fn phase(&self) -> BreakerPhase {
        let now_ms = self.now_ms();
        let state = self.lock_state();
        match state.phase {
            StoredPhase::Closed => BreakerPhase::Closed,
            StoredPhase::Open { opened_at_ms } => {
                if now_ms.saturating_sub(opened_at_ms) >= self.open_duration_ms() {
                    BreakerPhase::HalfOpen
                } else {
                    BreakerPhase::Open
                }
            }
            StoredPhase::Probing { .. } => BreakerPhase::HalfOpen,
        }
    }
}

impl CircuitBreaker {
    /// Locks the state. The lock is only ever held for a few non-panicking
    /// field updates, so a poisoned lock still holds consistent state and is
    /// recovered rather than propagated.
    fn lock_state(&self) -> std::sync::MutexGuard<'_, BreakerState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn open_duration_ms(&self) -> u64 {
        u64::try_from(self.open_duration.as_millis()).unwrap_or(u64::MAX)
    }

    /// Decides, atomically, whether a call may run and in which role.
    fn admit(&self) -> Result<Admission, ExecutionError> {
        let now_ms = self.now_ms();
        let open_duration_ms = self.open_duration_ms();
        let mut state = self.lock_state();
        match state.phase {
            StoredPhase::Closed => Ok(Admission::Closed { generation: state.generation }),
            StoredPhase::Open { opened_at_ms } => {
                let since_opened_ms = now_ms.saturating_sub(opened_at_ms);
                if since_opened_ms >= open_duration_ms {
                    state.phase = StoredPhase::Probing { opened_at_ms };
                    state.generation = state.generation.wrapping_add(1);
                    Ok(Admission::Probe { generation: state.generation })
                } else {
                    Err(ExecutionError::CircuitOpen { retry_after_ms: open_duration_ms - since_opened_ms })
                }
            }
            StoredPhase::Probing { .. } => Err(ExecutionError::CircuitOpen { retry_after_ms: open_duration_ms }),
        }
    }

    /// Applies an admitted call's call-level outcome. An outcome from a
    /// generation the breaker has since left is stale and ignored, so it can
    /// never transition the current state.
    fn complete(&self, admission: Admission, succeeded: bool) {
        let now_ms = self.now_ms();
        let mut state = self.lock_state();
        match admission {
            Admission::Closed { generation } => {
                if state.generation != generation || state.phase != StoredPhase::Closed {
                    return;
                }
                if succeeded {
                    state.consecutive_failures = 0;
                } else {
                    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                    if state.consecutive_failures >= self.failure_threshold {
                        state.phase = StoredPhase::Open { opened_at_ms: now_ms };
                        state.generation = state.generation.wrapping_add(1);
                    }
                }
            }
            Admission::Probe { generation } => {
                if state.generation != generation || !matches!(state.phase, StoredPhase::Probing { .. }) {
                    return;
                }
                if succeeded {
                    state.phase = StoredPhase::Closed;
                    state.consecutive_failures = 0;
                } else {
                    // Reopens unconditionally, bypassing failure_threshold
                    // and leaving the counter untouched.
                    state.phase = StoredPhase::Open { opened_at_ms: now_ms };
                }
                state.generation = state.generation.wrapping_add(1);
            }
        }
    }

    /// Releases a probe that ended without an outcome (its step panicked or
    /// the call future was dropped): the breaker returns to HalfOpen with no
    /// probe claimed, so the next caller may probe.
    fn abandon_probe(&self, generation: u64) {
        let mut state = self.lock_state();
        if state.generation != generation {
            return;
        }
        if let StoredPhase::Probing { opened_at_ms } = state.phase {
            state.phase = StoredPhase::Open { opened_at_ms };
            state.generation = state.generation.wrapping_add(1);
        }
    }
}

/// Releases probe ownership if the probe call ends without reporting an
/// outcome (panic or cancellation). Disarmed once the outcome is recorded.
struct ProbeGuard<'a> {
    breaker: &'a CircuitBreaker,
    generation: u64,
    armed: bool,
}

impl Drop for ProbeGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.breaker.abandon_probe(self.generation);
        }
    }
}

impl CircuitBreaker {
    /// Executes `step`, with behavior depending on [`CircuitBreaker::phase`].
    /// The phase decision, probe claim, failure count and every transition
    /// are one atomic state machine; a call's outcome is applied only if the
    /// breaker has not transitioned since that call was admitted (a call
    /// admitted while Closed that finishes after the breaker opened, or a
    /// probe from an earlier HalfOpen window, is ignored).
    ///
    /// While `Open`, returns `Err(ExecutionError::CircuitOpen { retry_after_ms })`
    /// immediately without invoking `step` at all, where `retry_after_ms` is
    /// the time remaining until `open_duration` has elapsed since the
    /// breaker most recently opened.
    ///
    /// While `Closed`, retries retryable failures per `retry_config` with
    /// `next_retry_delay`'s backoff, threading `ExecutionError::Http`'s
    /// `retry_after` or `ExecutionError::CircuitOpen`'s `retry_after_ms`
    /// through when present -- including when either is wrapped inside
    /// `ExecutionError::StepFailed`'s `source`, which is unwrapped
    /// recursively. A timed-out attempt is retried only if
    /// [`Step::is_idempotent`] returns `true` (see [`Step`]'s "Retry
    /// eligibility"). Returns immediately on success or
    /// on a non-retryable error, and returns the last error once
    /// `next_retry_delay` reports exhaustion. On success, resets the
    /// consecutive-failure counter; on a call-level failure (after retries
    /// are exhausted or the error is non-retryable), increments the counter
    /// once and opens the circuit at `failure_threshold`.
    ///
    /// While `HalfOpen`, runs `step` exactly once with no retries: success
    /// closes the circuit and resets the counter, and any failure reopens
    /// the circuit unconditionally, bypassing `failure_threshold`.
    pub async fn call(&self, step: &dyn Step, jitter_seed: f64) -> Result<(), ExecutionError> {
        let admission = self.admit()?;
        if let Admission::Probe { generation } = admission {
            let mut guard = ProbeGuard { breaker: self, generation, armed: true };
            let attempt_result = match tokio::time::timeout(self.step_timeout, step.run(0)).await {
                Ok(result) => result,
                Err(_) => Err(ExecutionError::Timeout {
                    elapsed_ms: u64::try_from(self.step_timeout.as_millis()).unwrap_or(u64::MAX),
                }),
            };
            guard.armed = false;
            self.complete(admission, attempt_result.is_ok());
            return attempt_result;
        }
        let mut attempt = 0u32;
        loop {
            let attempt_result = match tokio::time::timeout(self.step_timeout, step.run(attempt)).await {
                Ok(result) => result,
                Err(_) => Err(ExecutionError::Timeout {
                    elapsed_ms: u64::try_from(self.step_timeout.as_millis()).unwrap_or(u64::MAX),
                }),
            };
            match attempt_result {
                Ok(()) => {
                    self.complete(admission, true);
                    return Ok(());
                }
                Err(err) => {
                    let may_reinvoke = err.is_retryable() && (step.is_idempotent() || !is_ambiguous_outcome(&err));
                    if !may_reinvoke {
                        self.complete(admission, false);
                        return Err(err);
                    }
                    let retry_after = retry_after_for(&err);
                    match michi_core::resilience::next_retry_delay(
                        &self.retry_config,
                        attempt,
                        jitter_seed,
                        retry_after,
                    ) {
                        Some(delay) => {
                            tokio::time::sleep(delay).await;
                            attempt += 1;
                        }
                        None => {
                            self.complete(admission, false);
                            return Err(err);
                        }
                    }
                }
            }
        }
    }
}

/// Runs the steps of `pipeline` sequentially in declaration order through
/// `breaker`, writing each step's real outcome into `PipelineStep.status`.
pub async fn execute_pipeline(
    pipeline: &mut michi_core::pipeline::Pipeline,
    runners: Vec<Box<dyn Step>>,
    breaker: &CircuitBreaker,
    jitter_seed: f64,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if runners.len() != pipeline.steps.len() {
        return Err(ExecutionError::StepCountMismatch { expected: pipeline.steps.len(), got: runners.len() });
    }

    if cancellation.is_cancelled() {
        for step in pipeline.steps.iter_mut() {
            step.status = michi_core::pipeline::StepStatus::Skipped;
        }
        return Err(ExecutionError::Cancelled);
    }

    for (idx, runner) in runners.iter().enumerate() {
        if idx > 0 && cancellation.is_cancelled() {
            for step in pipeline.steps[idx..].iter_mut() {
                step.status = michi_core::pipeline::StepStatus::Skipped;
            }
            return Err(ExecutionError::Cancelled);
        }
        let outcome = breaker.call(runner.as_ref(), jitter_seed).await;
        let step = &mut pipeline.steps[idx];
        match outcome {
            Ok(()) => {
                step.status = michi_core::pipeline::StepStatus::Completed;
            }
            Err(err) => {
                step.status = michi_core::pipeline::StepStatus::Failed;
                return Err(ExecutionError::StepFailed {
                    step_id: step.id.clone(),
                    step_name: step.name.clone(),
                    source: Box::new(err),
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum StepRunState {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

fn spawn_ready_steps(
    state: &mut [StepRunState],
    remaining_prereqs: &[usize],
    runners: &[std::sync::Arc<dyn Step>],
    breaker: &std::sync::Arc<CircuitBreaker>,
    jitter_seed: f64,
    join_set: &mut tokio::task::JoinSet<(usize, Result<(), ExecutionError>)>,
) {
    for i in 0..state.len() {
        if state[i] == StepRunState::Pending && remaining_prereqs[i] == 0 {
            state[i] = StepRunState::Running;
            let runner = std::sync::Arc::clone(&runners[i]);
            let breaker = std::sync::Arc::clone(breaker);
            join_set.spawn(async move {
                let result = breaker.call(runner.as_ref(), jitter_seed).await;
                (i, result)
            });
        }
    }
}

fn propagate_skip(idx: usize, state: &mut [StepRunState], dependents_of: &[Vec<usize>]) {
    let mut stack = vec![idx];
    while let Some(i) = stack.pop() {
        for &d in &dependents_of[i] {
            if state[d] == StepRunState::Pending {
                state[d] = StepRunState::Skipped;
                stack.push(d);
            }
        }
    }
}

/// Executes `pipeline.steps` concurrently, honoring the dependency edges in
/// `deps`. Steps with no unmet prerequisite run as soon as they become
/// eligible, polled concurrently via an internal `JoinSet`; a step only
/// starts once every step it depends on has reached a terminal status.
///
/// Every acceptance check runs in a fixed order before any step is
/// attempted: step-count mismatch, then a duplicate pipeline id (any
/// repeated `PipelineStep.id`, whether or not an edge mentions it), then an
/// id-set mismatch or unknown edge endpoint, then a cyclic dependency
/// (including a 1-node self-dependency).
///
/// On completion, each `PipelineStep.status` is written to `Completed`,
/// `Failed` (including a circuit-open short-circuit that never invokes
/// `Step::run`), or `Skipped` (every transitive dependent of a failed
/// step). The function returns `Ok(())` only if every step completed; on
/// any failure it returns `Err(ExecutionError::StepFailed)` identifying
/// one of the (possibly several) steps that failed, while every unrelated
/// in-flight step still runs to its own real terminal status rather than
/// being abandoned.
///
/// `breaker` is `Arc<CircuitBreaker>` rather than a plain reference,
/// unlike the unmodified, strictly sequential [`execute_pipeline`]: each
/// spawned step's task must independently hold a handle to the shared
/// breaker across the `JoinSet`'s `Send + 'static` boundary, which a
/// borrowed reference cannot satisfy.
pub async fn execute_pipeline_parallel(
    pipeline: &mut michi_core::pipeline::Pipeline,
    deps: &StepDependencies,
    runners: Vec<Box<dyn Step>>,
    breaker: std::sync::Arc<CircuitBreaker>,
    jitter_seed: f64,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if runners.len() != pipeline.steps.len() {
        return Err(ExecutionError::StepCountMismatch { expected: pipeline.steps.len(), got: runners.len() });
    }

    // Every pipeline id must be unique, whether or not an edge mentions it:
    // ids are the keys dependency resolution and status write-back index by.
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for step in &pipeline.steps {
        if !seen_ids.insert(step.id.as_str()) {
            return Err(ExecutionError::DuplicateStepId { step_id: step.id.clone() });
        }
    }

    let pipeline_ids: HashSet<&str> = pipeline.steps.iter().map(|s| s.id.as_str()).collect();
    let graph_ids: HashSet<&str> = deps.ids.iter().map(|s| s.as_str()).collect();
    for id in &pipeline_ids {
        if !graph_ids.contains(id) {
            return Err(ExecutionError::UnknownStepId { step_id: (*id).to_string() });
        }
    }
    for id in &graph_ids {
        if !pipeline_ids.contains(id) {
            return Err(ExecutionError::UnknownStepId { step_id: (*id).to_string() });
        }
    }
    for (dependent, prereqs) in &deps.edges {
        if !pipeline_ids.contains(dependent.as_str()) {
            return Err(ExecutionError::UnknownStepId { step_id: dependent.clone() });
        }
        for p in prereqs {
            if !pipeline_ids.contains(p.as_str()) {
                return Err(ExecutionError::UnknownStepId { step_id: p.clone() });
            }
        }
    }

    if let Some(step_id) = detect_cycle(deps) {
        return Err(ExecutionError::CyclicDependency { step_id });
    }

    let id_at: Vec<String> = pipeline.steps.iter().map(|s| s.id.clone()).collect();
    #[allow(clippy::disallowed_types, reason = "internal id-to-index lookup, never rendered as output")]
    let index_of: HashMap<&str, usize> = id_at.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    let mut prereqs_of: Vec<Vec<usize>> = vec![Vec::new(); id_at.len()];
    let mut dependents_of: Vec<Vec<usize>> = vec![Vec::new(); id_at.len()];
    for (dependent, prereqs) in &deps.edges {
        let d_idx = index_of[dependent.as_str()];
        for p in prereqs {
            let p_idx = index_of[p.as_str()];
            prereqs_of[d_idx].push(p_idx);
            dependents_of[p_idx].push(d_idx);
        }
    }

    let mut state: Vec<StepRunState> = vec![StepRunState::Pending; id_at.len()];
    let mut remaining_prereqs: Vec<usize> = prereqs_of.iter().map(Vec::len).collect();
    let runners: Vec<std::sync::Arc<dyn Step>> = runners.into_iter().map(std::sync::Arc::from).collect();

    let mut join_set: tokio::task::JoinSet<(usize, Result<(), ExecutionError>)> = tokio::task::JoinSet::new();
    let mut first_failure: Option<(usize, ExecutionError)> = None;
    let mut cancelled = false;

    if cancellation.is_cancelled() {
        cancelled = true;
        for st in state.iter_mut() {
            if *st == StepRunState::Pending {
                *st = StepRunState::Skipped;
            }
        }
    } else {
        spawn_ready_steps(&mut state, &remaining_prereqs, &runners, &breaker, jitter_seed, &mut join_set);
    }

    while let Some(joined) = join_set.join_next().await {
        let (i, result) = match joined {
            Ok(pair) => pair,
            Err(join_err) => std::panic::resume_unwind(join_err.into_panic()),
        };
        match result {
            Ok(()) => {
                state[i] = StepRunState::Completed;
                pipeline.steps[i].status = michi_core::pipeline::StepStatus::Completed;
                for &d in &dependents_of[i] {
                    if state[d] == StepRunState::Pending {
                        remaining_prereqs[d] = remaining_prereqs[d].saturating_sub(1);
                    }
                }
            }
            Err(err) => {
                state[i] = StepRunState::Failed;
                pipeline.steps[i].status = michi_core::pipeline::StepStatus::Failed;
                if first_failure.is_none() {
                    first_failure = Some((i, err));
                }
                propagate_skip(i, &mut state, &dependents_of);
            }
        }
        if cancellation.is_cancelled() && state.contains(&StepRunState::Pending) {
            cancelled = true;
            for st in state.iter_mut() {
                if *st == StepRunState::Pending {
                    *st = StepRunState::Skipped;
                }
            }
        } else {
            spawn_ready_steps(&mut state, &remaining_prereqs, &runners, &breaker, jitter_seed, &mut join_set);
        }
    }

    for (i, st) in state.iter().enumerate() {
        if *st == StepRunState::Skipped {
            pipeline.steps[i].status = michi_core::pipeline::StepStatus::Skipped;
        }
    }

    match first_failure {
        Some((i, err)) => Err(ExecutionError::StepFailed {
            step_id: pipeline.steps[i].id.clone(),
            step_name: pipeline.steps[i].name.clone(),
            source: Box::new(err),
        }),
        None if cancelled => Err(ExecutionError::Cancelled),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn cancellation_token_new_starts_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_token_cancel_flips_is_cancelled_to_true() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancellation_token_cancel_is_idempotent() {
        let token = CancellationToken::new();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancellation_token_clone_shares_state_bidirectionally() {
        let original = CancellationToken::new();
        let clone = original.clone();
        clone.cancel();
        assert!(original.is_cancelled(), "cancelling the clone must be observed on the original");

        let original2 = CancellationToken::new();
        let clone2 = original2.clone();
        original2.cancel();
        assert!(clone2.is_cancelled(), "cancelling the original must be observed on the clone");
    }

    #[test]
    fn cancellation_token_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CancellationToken>();
    }

    #[test]
    fn cancelled_variant_is_not_retryable_and_maps_to_unavailable_not_retryable() {
        assert!(!ExecutionError::Cancelled.is_retryable());
        let d: michi_core::DomainError = ExecutionError::Cancelled.into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        assert_eq!(d.message, "pipeline execution cancelled");
        assert!(!d.retryable);
    }

    #[test]
    fn execution_error_has_six_variants_and_impls_std_error() {
        fn assert_std_error<E: std::error::Error>() {}
        assert_std_error::<ExecutionError>();

        let _http = ExecutionError::Http { status: 500, message: "x".into(), retry_after: None };
        let _timeout = ExecutionError::Timeout { elapsed_ms: 1 };
        let _failed = ExecutionError::Failed { message: "x".into(), retryable: true };
        let _circuit_open = ExecutionError::CircuitOpen { retry_after_ms: 1 };
        let _step_failed = ExecutionError::StepFailed {
            step_id: "s".into(),
            step_name: "S".into(),
            source: Box::new(ExecutionError::Timeout { elapsed_ms: 1 }),
        };
        let _mismatch = ExecutionError::StepCountMismatch { expected: 1, got: 2 };
    }

    struct AlwaysOk;
    impl Step for AlwaysOk {
        fn run<'a>(
            &'a self,
            _attempt: u32,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn step_is_object_safe() {
        let steps: Vec<Box<dyn Step>> = vec![Box::new(AlwaysOk)];
        assert_eq!(steps.len(), 1);
    }

    #[tokio::test]
    async fn step_fn_adapts_closure_into_step() {
        let steps: Vec<Box<dyn Step>> = vec![Box::new(step_fn(|_attempt: u32| async { Ok(()) }))];
        let result = steps[0].run(0).await;
        assert!(result.is_ok());
    }

    #[test]
    fn is_retryable_per_variant() {
        assert_eq!(
            ExecutionError::Http { status: 429, message: "x".into(), retry_after: None }.is_retryable(),
            michi_core::resilience::is_retryable_status(429)
        );
        assert_eq!(
            ExecutionError::Http { status: 400, message: "x".into(), retry_after: None }.is_retryable(),
            michi_core::resilience::is_retryable_status(400)
        );
        assert!(ExecutionError::Timeout { elapsed_ms: 1 }.is_retryable());
        assert!(ExecutionError::Failed { message: "x".into(), retryable: true }.is_retryable());
        assert!(!ExecutionError::Failed { message: "x".into(), retryable: false }.is_retryable());
        assert!(ExecutionError::CircuitOpen { retry_after_ms: 1 }.is_retryable());
        assert!(ExecutionError::StepFailed {
            step_id: "s".into(),
            step_name: "S".into(),
            source: Box::new(ExecutionError::Timeout { elapsed_ms: 1 }),
        }
        .is_retryable());
        assert!(!ExecutionError::StepFailed {
            step_id: "s".into(),
            step_name: "S".into(),
            source: Box::new(ExecutionError::Failed { message: "x".into(), retryable: false }),
        }
        .is_retryable());
        assert!(!ExecutionError::StepCountMismatch { expected: 1, got: 2 }.is_retryable());
    }

    #[test]
    fn http_conversion_maps_status_groups() {
        let d: michi_core::DomainError = ExecutionError::Http {
            status: 429,
            message: "rate limited".into(),
            retry_after: Some(Duration::from_secs(2)),
        }
        .into();
        assert_eq!(d.code, michi_core::ErrorCode::RateLimited);
        assert!(d.retryable);
        assert_eq!(d.retry_after, Some(Duration::from_secs(2)));

        let d: michi_core::DomainError =
            ExecutionError::Http { status: 503, message: "unavailable".into(), retry_after: None }.into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        assert!(d.retryable);
        assert_eq!(d.retry_after, None);

        // Range boundaries: 502..=504 is inclusive on both ends, chosen over an
        // OR-pattern to satisfy clippy::manual_range_patterns -- assert both
        // edges so an accidental exclusive range (dropping 504) would be caught.
        let d: michi_core::DomainError =
            ExecutionError::Http { status: 502, message: "bad gateway".into(), retry_after: None }.into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        let d: michi_core::DomainError =
            ExecutionError::Http { status: 504, message: "gateway timeout".into(), retry_after: None }.into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);

        let d: michi_core::DomainError =
            ExecutionError::Http { status: 400, message: "bad request".into(), retry_after: None }.into();
        assert_eq!(d.code, michi_core::ErrorCode::ExternalFailure);
        assert!(!d.retryable);
    }

    #[test]
    fn timeout_circuit_open_and_mismatch_conversions() {
        let d: michi_core::DomainError = ExecutionError::Timeout { elapsed_ms: 250 }.into();
        assert_eq!(d.code, michi_core::ErrorCode::Timeout);
        assert!(d.retryable);

        let d: michi_core::DomainError = ExecutionError::CircuitOpen { retry_after_ms: 750 }.into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        assert!(d.retryable);
        assert_eq!(d.retry_after, Some(std::time::Duration::from_millis(750)));

        let d: michi_core::DomainError = ExecutionError::StepCountMismatch { expected: 3, got: 1 }.into();
        assert_eq!(d.code, michi_core::ErrorCode::InvalidInput);
        assert!(!d.retryable);
    }

    #[test]
    fn failed_conversion_preserves_retryable_flag() {
        let d: michi_core::DomainError = ExecutionError::Failed { message: "boom".into(), retryable: true }.into();
        assert_eq!(d.code, michi_core::ErrorCode::ExternalFailure);
        assert!(d.retryable);

        let d: michi_core::DomainError = ExecutionError::Failed { message: "boom".into(), retryable: false }.into();
        assert_eq!(d.code, michi_core::ErrorCode::ExternalFailure);
        assert!(!d.retryable);
    }

    #[test]
    fn step_failed_conversion_recurses_and_prefixes_message() {
        let source = ExecutionError::Failed { message: "disk full".into(), retryable: false };
        let err = ExecutionError::StepFailed {
            step_id: "upload".into(),
            step_name: "Upload".into(),
            source: Box::new(source),
        };
        let d: michi_core::DomainError = err.into();
        assert_eq!(d.code, michi_core::ErrorCode::ExternalFailure);
        assert!(!d.retryable);
        assert_eq!(d.message, "step upload (Upload) failed: disk full");
    }

    #[test]
    fn step_failed_conversion_preserves_inner_error_code() {
        // Uses a source variant whose code differs from ExternalFailure so this
        // actually exercises the recursive `inner.code` read, rather than being
        // indistinguishable from a hardcoded ExternalFailure (as Failed's own
        // ExternalFailure code would be) -- a mutation-testing checkpoint found
        // the existing StepFailed test alone couldn't catch a hardcode regression.
        let source = ExecutionError::Timeout { elapsed_ms: 5 };
        let err = ExecutionError::StepFailed { step_id: "s".into(), step_name: "S".into(), source: Box::new(source) };
        let d: michi_core::DomainError = err.into();
        assert_eq!(d.code, michi_core::ErrorCode::Timeout);
    }

    #[test]
    fn step_failed_conversion_inherits_inner_retry_after() {
        // Neither of the two tests above ever produces a Some(retry_after) on
        // the inner DomainError, so the `if let Some(retry_after) = ...`
        // branch in the StepFailed arm was previously untested -- a review
        // found this gap. CircuitOpen always sets retry_after, so it's used
        // here specifically to exercise that branch.
        let source = ExecutionError::CircuitOpen { retry_after_ms: 500 };
        let err = ExecutionError::StepFailed { step_id: "s".into(), step_name: "S".into(), source: Box::new(source) };
        let d: michi_core::DomainError = err.into();
        assert_eq!(d.retry_after, Some(std::time::Duration::from_millis(500)));
    }

    #[test]
    fn new_is_infallible_and_fresh_breaker_is_closed() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            std::time::Duration::ZERO,
            u32::MAX,
            std::time::Duration::ZERO,
        );
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
    }

    struct CountingStep {
        calls: std::sync::atomic::AtomicU32,
    }
    impl Step for CountingStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn call_succeeds_in_one_attempt_with_zero_virtual_time() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(1),
            u32::MAX,
            Duration::from_secs(60),
        );
        let step = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        assert_eq!(step.calls.load(Ordering::SeqCst), 1);
        assert_eq!(elapsed, Duration::ZERO);
    }

    struct FlakyStep {
        fail_until: u32,
        calls: std::sync::atomic::AtomicU32,
    }
    impl Step for FlakyStep {
        fn run<'a>(&'a self, attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt < self.fail_until {
                    Err(ExecutionError::Failed { message: "flaky".into(), retryable: true })
                } else {
                    Ok(())
                }
            })
        }
    }

    struct AlwaysFailStep {
        retryable: bool,
        calls: std::sync::atomic::AtomicU32,
    }
    impl Step for AlwaysFailStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let retryable = self.retryable;
            Box::pin(async move { Err(ExecutionError::Failed { message: "always".into(), retryable }) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retries_until_success_and_sums_delay() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(1), 0.0);
        let breaker =
            CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let step = FlakyStep { fail_until: 2, calls: std::sync::atomic::AtomicU32::new(0) };
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        assert_eq!(step.calls.load(Ordering::SeqCst), 3);
        let expected: Duration =
            (0..2).filter_map(|a| michi_core::resilience::next_retry_delay(&retry_config, a, 0.0, None)).sum();
        assert_eq!(elapsed, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_thread_http_retry_after() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(1), 0.0);
        let breaker =
            CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let ra = Duration::from_millis(300);
        struct HttpThenOk(std::sync::atomic::AtomicU32);
        impl Step for HttpThenOk {
            fn run<'a>(
                &'a self,
                attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ExecutionError::Http {
                            status: 503,
                            message: "x".into(),
                            retry_after: Some(Duration::from_millis(300)),
                        })
                    } else {
                        Ok(())
                    }
                })
            }
        }
        let step = HttpThenOk(std::sync::atomic::AtomicU32::new(0));
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        let expected = michi_core::resilience::next_retry_delay(&retry_config, 0, 0.0, Some(ra)).unwrap();
        assert_eq!(elapsed, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn retries_thread_circuit_open_retry_after() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(60), 0.0);
        let breaker =
            CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let ra = Duration::from_millis(30_000);
        struct CircuitOpenThenOk(std::sync::atomic::AtomicU32);
        impl Step for CircuitOpenThenOk {
            fn run<'a>(
                &'a self,
                attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ExecutionError::CircuitOpen { retry_after_ms: 30_000 })
                    } else {
                        Ok(())
                    }
                })
            }
        }
        let step = CircuitOpenThenOk(std::sync::atomic::AtomicU32::new(0));
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        let expected = michi_core::resilience::next_retry_delay(&retry_config, 0, 0.0, Some(ra)).unwrap();
        assert_eq!(elapsed, expected);
        assert!(elapsed >= ra, "must wait at least the circuit's stated {ra:?} cooldown, waited {elapsed:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn retries_thread_step_failed_wrapping_http_retry_after() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(60), 0.0);
        let breaker =
            CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let ra = Duration::from_secs(30);
        struct StepFailedHttpThenOk(std::sync::atomic::AtomicU32);
        impl Step for StepFailedHttpThenOk {
            fn run<'a>(
                &'a self,
                attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ExecutionError::StepFailed {
                            step_id: "s".into(),
                            step_name: "S".into(),
                            source: Box::new(ExecutionError::Http {
                                status: 503,
                                message: "x".into(),
                                retry_after: Some(Duration::from_secs(30)),
                            }),
                        })
                    } else {
                        Ok(())
                    }
                })
            }
        }
        let step = StepFailedHttpThenOk(std::sync::atomic::AtomicU32::new(0));
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        let expected = michi_core::resilience::next_retry_delay(&retry_config, 0, 0.0, Some(ra)).unwrap();
        assert_eq!(elapsed, expected);
        assert!(elapsed >= ra, "must wait at least the wrapped Http's stated {ra:?} retry_after, waited {elapsed:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn retries_thread_step_failed_wrapping_circuit_open_retry_after() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(60), 0.0);
        let breaker =
            CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let ra = Duration::from_millis(30_000);
        struct StepFailedCircuitOpenThenOk(std::sync::atomic::AtomicU32);
        impl Step for StepFailedCircuitOpenThenOk {
            fn run<'a>(
                &'a self,
                attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ExecutionError::StepFailed {
                            step_id: "s".into(),
                            step_name: "S".into(),
                            source: Box::new(ExecutionError::CircuitOpen { retry_after_ms: 30_000 }),
                        })
                    } else {
                        Ok(())
                    }
                })
            }
        }
        let step = StepFailedCircuitOpenThenOk(std::sync::atomic::AtomicU32::new(0));
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());
        let expected = michi_core::resilience::next_retry_delay(&retry_config, 0, 0.0, Some(ra)).unwrap();
        assert_eq!(elapsed, expected);
        assert!(
            elapsed >= ra,
            "must wait at least the wrapped CircuitOpen's stated {ra:?} cooldown, waited {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn exhausts_retries_then_fails() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(3, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let step = AlwaysFailStep { retryable: true, calls: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&step, 0.0).await;
        assert!(result.is_err());
        assert_eq!(step.calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn non_retryable_failure_returns_after_one_attempt_zero_time() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        );
        let step = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        let before = tokio::time::Instant::now();
        let result = breaker.call(&step, 0.0).await;
        let elapsed = before.elapsed();
        assert!(result.is_err());
        assert_eq!(step.calls.load(Ordering::SeqCst), 1);
        assert_eq!(elapsed, Duration::ZERO);
    }

    struct NeverCompletes;
    impl Step for NeverCompletes {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn attempt_exceeding_step_timeout_fails_with_timeout_elapsed_ms() {
        let retry_config =
            michi_core::resilience::RetryConfig::try_new(0, Duration::from_millis(1), Duration::from_millis(1), 0.0)
                .unwrap();
        let breaker = CircuitBreaker::new(retry_config, Duration::from_millis(500), u32::MAX, Duration::from_secs(60));
        let result = breaker.call(&NeverCompletes, 0.0).await;
        match result {
            Err(ExecutionError::Timeout { elapsed_ms }) => assert_eq!(elapsed_ms, 500),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn zero_step_timeout_floors_to_one_millisecond() {
        let retry_config =
            michi_core::resilience::RetryConfig::try_new(0, Duration::from_millis(1), Duration::from_millis(1), 0.0)
                .unwrap();
        let breaker = CircuitBreaker::new(retry_config, Duration::ZERO, u32::MAX, Duration::from_secs(60));
        let result = breaker.call(&NeverCompletes, 0.0).await;
        match result {
            Err(ExecutionError::Timeout { elapsed_ms }) => assert_eq!(elapsed_ms, 1),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn jitter_seed_forwarded_unmodified_for_non_finite_values() {
        for seed in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 7.5, -3.0] {
            let retry_config =
                michi_core::resilience::RetryConfig::new(2, Duration::from_millis(10), Duration::from_secs(1), 0.5);
            let breaker =
                CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
            let step = FlakyStep { fail_until: 1, calls: std::sync::atomic::AtomicU32::new(0) };
            let before = tokio::time::Instant::now();
            let result = breaker.call(&step, seed).await;
            let elapsed = before.elapsed();
            assert!(result.is_ok(), "seed {seed} should not panic or fail construction");
            let expected = michi_core::resilience::next_retry_delay(&retry_config, 0, seed, None).unwrap();
            assert_eq!(elapsed, expected, "seed {seed} elapsed mismatch");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn threshold_consecutive_failures_opens_circuit_once_per_call_not_per_attempt() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), 2, Duration::from_secs(60));
        let step = AlwaysFailStep { retryable: true, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&step, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Closed, "one failing call must not open a threshold-2 breaker");
        assert_eq!(
            step.calls.load(Ordering::SeqCst),
            6,
            "first call() must still exhaust all 6 attempts (max_retries=5)"
        );
        assert!(breaker.call(&step, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open, "second failing call() must open the threshold-2 breaker");
    }

    #[tokio::test(start_paused = true)]
    async fn zero_failure_threshold_behaves_like_one() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            0,
            Duration::from_secs(60),
        );
        let step = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&step, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn success_resets_counter_so_four_non_consecutive_failures_never_open_a_threshold_three_breaker() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            3,
            Duration::from_secs(60),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        let ok = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert!(breaker.call(&ok, 0.0).await.is_ok());
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(
            breaker.phase(),
            BreakerPhase::Closed,
            "4 total failures but never 3 consecutive - must stay Closed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn open_circuit_short_circuits_with_exact_retry_after_ms() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(10),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);

        tokio::time::advance(Duration::from_secs(4)).await;

        let probe = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&probe, 0.0).await;
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0, "step.run must not be invoked while Open");
        match result {
            Err(ExecutionError::CircuitOpen { retry_after_ms }) => assert_eq!(retry_after_ms, 6_000),
            other => panic!("expected CircuitOpen{{retry_after_ms: 6000}}, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn phase_transitions_to_half_open_once_open_duration_elapses() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(10),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);

        let start = tokio::time::Instant::now();
        tokio::time::advance(Duration::from_secs(9)).await;
        assert_eq!(breaker.phase(), BreakerPhase::Open, "9s < 10s open_duration");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen, "10s >= 10s open_duration");
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn zero_open_duration_floors_to_one_millisecond() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::ZERO,
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open, "0ms elapsed since opening, floored open_duration is 1ms");
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);
    }

    #[tokio::test(start_paused = true)]
    async fn half_open_success_closes_circuit_and_resets_counter() {
        // Uses failure_threshold=2, not 1: a review found the original
        // threshold=1 version made this test's reset-proof vacuous, since
        // count>=1 holds after a single post-recovery failure whether or not
        // the counter was actually reset (reset-then-+1=1 and
        // leftover-then-+1=2 both satisfy >=1). Verified by mutation testing:
        // deleting record_success() from the HalfOpen-success branch left the
        // old version's assertions unchanged. With threshold=2, a single
        // post-recovery failure only stays Closed if the counter was
        // genuinely reset to 0.
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            2,
            Duration::from_secs(10),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);

        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let ok = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&ok, 0.0).await.is_ok());
        assert_eq!(breaker.phase(), BreakerPhase::Closed);

        // Discriminating assertion: if the counter had NOT been reset, it
        // would already be at 2 from the failures above, and this one more
        // failure would push it to 3 >= 2, reopening the circuit.
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(
            breaker.phase(),
            BreakerPhase::Closed,
            "one post-recovery failure must not reopen a threshold-2 breaker if the counter was genuinely reset"
        );

        // A second post-recovery failure, from the correctly-reset baseline,
        // does reach the threshold -- confirming the counter still counts
        // correctly from 0, not stuck.
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn half_open_failure_reopens_circuit_with_exactly_one_attempt_no_retries() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), 1, Duration::from_secs(10));
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let step = FlakyStep { fail_until: 1, calls: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&step, 0.0).await;
        assert!(result.is_err(), "HalfOpen probe must not retry into the eventual success");
        assert_eq!(step.calls.load(Ordering::SeqCst), 1, "exactly one attempt during HalfOpen");
        assert_eq!(breaker.phase(), BreakerPhase::Open);
    }

    struct GatedProbe {
        release: std::sync::Arc<tokio::sync::Notify>,
        calls: std::sync::atomic::AtomicU32,
    }
    impl Step for GatedProbe {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let release = std::sync::Arc::clone(&self.release);
            Box::pin(async move {
                release.notified().await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn only_one_concurrent_half_open_caller_invokes_the_probe() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(10),
        ));
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let probe = std::sync::Arc::new(GatedProbe {
            release: std::sync::Arc::clone(&release),
            calls: std::sync::atomic::AtomicU32::new(0),
        });

        let b1 = std::sync::Arc::clone(&breaker);
        let p1 = std::sync::Arc::clone(&probe);
        let winner = tokio::spawn(async move { b1.call(p1.as_ref(), 0.0).await });
        tokio::task::yield_now().await;

        let loser_result = breaker.call(probe.as_ref(), 0.0).await;
        match loser_result {
            Err(ExecutionError::CircuitOpen { retry_after_ms }) => assert_eq!(retry_after_ms, 10_000),
            other => panic!(
                "expected the losing concurrent caller to get CircuitOpen{{retry_after_ms: 10000}} immediately, got {other:?}"
            ),
        }
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1, "only the winner may have invoked Step::run");

        release.notify_one();
        assert!(winner.await.unwrap().is_ok());
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
    }

    struct PanicsStep;
    impl Step for PanicsStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async { panic!("deliberate test panic") })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn panicking_step_leaves_breaker_state_untouched() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            2,
            Duration::from_secs(60),
        ));
        let phase_before = breaker.phase();

        let handle = {
            let breaker = std::sync::Arc::clone(&breaker);
            tokio::spawn(async move { breaker.call(&PanicsStep, 0.0).await })
        };
        let join_result = handle.await;
        assert!(
            join_result.is_err(),
            "the panic must propagate out of call() as a JoinError, proving it is not caught internally"
        );
        assert_eq!(breaker.phase(), phase_before, "phase must be unchanged after the panic");

        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(
            breaker.call(&fail, 0.0).await.is_err(),
            "one real failing call, to prove the panic did not pre-contribute to the failure count"
        );
        assert_eq!(breaker.phase(), BreakerPhase::Closed, "panic must not have contributed a failure count");

        let ok = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&ok, 0.0).await.is_ok(), "the breaker must remain usable after a panicking call");
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn half_open_failure_bypasses_failure_counter_unconditionally() {
        // A mutation-testing checkpoint found that swapping the HalfOpen-failure
        // branch's open_circuit() for record_failure() survives every black-box
        // test: by the time this branch is ever reached, consecutive_failures is
        // already >= failure_threshold (it can only get here via a prior Closed
        // -> Open transition), so record_failure()'s own threshold check would be
        // trivially true too -- the two calls are behaviorally indistinguishable
        // through call()/phase() alone. This white-box test locks in the actual
        // contract (unconditional reopen, no counter dependency) by reading the
        // private counter directly, documenting that open_circuit() is the
        // intended call, not an accident that happens to also satisfy the tests.
        // (Since the state-machine rewrite, the probe-failure arm of
        // CircuitBreaker::complete plays open_circuit()'s role and the
        // Closed-failure arm plays record_failure()'s; the contract is the same.)
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(10),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let before = breaker.lock_state().consecutive_failures;
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(
            breaker.lock_state().consecutive_failures,
            before,
            "HalfOpen failure must reopen unconditionally via open_circuit(), not via record_failure()'s threshold check"
        );
        assert_eq!(breaker.phase(), BreakerPhase::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn gate_admits_a_new_probe_in_each_subsequent_half_open_window() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(10),
        );
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);
        let probe1_fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&probe1_fail, 0.0).await.is_err());
        assert_eq!(probe1_fail.calls.load(Ordering::SeqCst), 1, "exactly one probe attempt in cycle 1");
        assert_eq!(breaker.phase(), BreakerPhase::Open, "a failing probe must reopen the circuit");

        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);
        let probe2 = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&probe2, 0.0).await;
        assert!(result.is_ok(), "a fresh probe in a new HalfOpen window must be admitted, got {result:?}");
        assert_eq!(probe2.calls.load(Ordering::SeqCst), 1, "the cycle-2 probe must actually have been invoked");
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_loop_terminates_within_bounded_attempts() {
        // A mutation-testing checkpoint found that a regression removing
        // `attempt += 1` from the retry loop doesn't fail cleanly -- it hangs
        // the test runner indefinitely under start_paused=true, since
        // next_retry_delay(attempt=0, ...) never reaches the max_retries
        // exhaustion check. Wrapping the call in an explicit outer timeout
        // converts that failure mode into a clean, fast assertion failure
        // instead of a hung CI job.
        let retry_config =
            michi_core::resilience::RetryConfig::new(3, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let step = AlwaysFailStep { retryable: true, calls: std::sync::atomic::AtomicU32::new(0) };
        let result = tokio::time::timeout(Duration::from_secs(5), breaker.call(&step, 0.0)).await;
        assert!(result.is_ok(), "retry loop must terminate (exhaust retries) well within a 5s bound, not hang");
        assert!(result.unwrap().is_err());
        assert_eq!(step.calls.load(Ordering::SeqCst), 4, "max_retries=3 -> 4 total attempts");
    }

    #[test]
    fn phase_reports_half_open_while_the_probe_is_claimed_regardless_of_elapsed_time() {
        // The stored `Probing` phase (the HalfOpen probe is in flight) must
        // report HalfOpen even when `opened_at_ms` is recent enough that an
        // unclaimed `Open` would still report Open -- i.e. phase() must not
        // re-derive HalfOpen from elapsed time for that arm. Set directly so
        // the arm is covered without an in-flight probe.
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(10),
        );
        breaker.lock_state().phase = StoredPhase::Probing { opened_at_ms: 0 };
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);
    }

    fn make_pipeline(ids: &[&str]) -> michi_core::pipeline::Pipeline {
        michi_core::pipeline::Pipeline {
            id: "p".into(),
            steps: ids
                .iter()
                .map(|id| michi_core::pipeline::PipelineStep {
                    id: (*id).into(),
                    name: (*id).into(),
                    status: michi_core::pipeline::StepStatus::Pending,
                })
                .collect(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn runner_count_greater_than_steps_returns_mismatch_without_mutating_statuses() {
        let mut pipeline = make_pipeline(&["a"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepCountMismatch { expected, got }) => {
                assert_eq!(expected, 1);
                assert_eq!(got, 2);
            }
            other => panic!("expected StepCountMismatch{{expected: 1, got: 2}}, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn runner_count_less_than_steps_returns_mismatch_without_mutating_statuses() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) })];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepCountMismatch { expected, got }) => {
                assert_eq!(expected, 2);
                assert_eq!(got, 1);
            }
            other => panic!("expected StepCountMismatch{{expected: 2, got: 1}}, got {other:?}"),
        }
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn empty_pipeline_and_empty_runners_returns_ok() {
        let mut pipeline = make_pipeline(&[]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = Vec::new();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        assert!(pipeline.steps.is_empty());
    }

    struct CancelsTokenThenOk {
        token: CancellationToken,
    }
    impl Step for CancelsTokenThenOk {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.token.cancel();
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_observed_mid_run_skips_remaining_steps_and_preserves_earlier_statuses() {
        let mut pipeline = make_pipeline(&["a", "b", "c", "d"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let token = CancellationToken::new();
        let calls_b = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let calls_c = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let calls_d = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CancelsTokenThenOk { token: token.clone() }),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_b))),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_c))),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_d))),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        assert!(
            matches!(result, Err(ExecutionError::Cancelled)),
            "expected Err(Cancelled) even though a's own Step::run resolved Ok, got {result:?}"
        );
        assert_eq!(
            pipeline.steps[0].status,
            michi_core::pipeline::StepStatus::Completed,
            "a's already-terminal status must be preserved"
        );
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Skipped, "b");
        assert_eq!(pipeline.steps[2].status, michi_core::pipeline::StepStatus::Skipped, "c");
        assert_eq!(pipeline.steps[3].status, michi_core::pipeline::StepStatus::Skipped, "d");
        assert_eq!(calls_b.load(Ordering::SeqCst), 0, "b's Step::run must never be invoked once cancelled");
        assert_eq!(calls_c.load(Ordering::SeqCst), 0, "c's Step::run must never be invoked once cancelled");
        assert_eq!(calls_d.load(Ordering::SeqCst), 0, "d's Step::run must never be invoked once cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_before_any_step_skips_everything_with_zero_invocations() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let calls_a = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let calls_b = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(SpyStep(std::sync::Arc::clone(&calls_a))),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_b))),
        ];
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        assert!(matches!(result, Err(ExecutionError::Cancelled)), "expected Err(Cancelled), got {result:?}");
        assert_eq!(calls_a.load(Ordering::SeqCst), 0);
        assert_eq!(calls_b.load(Ordering::SeqCst), 0);
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Skipped);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_requested_after_the_last_checkpoint_has_passed_does_not_flip_ok_to_cancelled() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let token = CancellationToken::new();
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CancelsTokenThenOk { token: token.clone() }),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        assert!(result.is_ok(), "a too-late cancellation request must not retroactively turn an otherwise-successful run into Err(Cancelled), got {result:?}");
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Completed);
        }
        assert!(token.is_cancelled(), "sanity: the token really was cancelled by b's own run");
    }

    #[tokio::test(start_paused = true)]
    async fn zero_step_pipeline_with_precancelled_token_still_returns_cancelled() {
        let mut pipeline = make_pipeline(&[]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = Vec::new();
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        assert!(matches!(result, Err(ExecutionError::Cancelled)), "expected Err(Cancelled), got {result:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_does_not_preempt_step_count_mismatch() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) })];
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        match result {
            Err(ExecutionError::StepCountMismatch { expected, got }) => {
                assert_eq!(expected, 2);
                assert_eq!(got, 1);
            }
            other => panic!("expected StepCountMismatch to take precedence over Cancelled, got {other:?}"),
        }
    }

    struct GatedThenOk(std::sync::Arc<tokio::sync::Notify>);
    impl Step for GatedThenOk {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            let notify = std::sync::Arc::clone(&self.0);
            Box::pin(async move {
                notify.notified().await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_during_in_flight_step_does_not_interrupt_it() {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut pipeline = make_pipeline(&["a"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(3600),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![Box::new(GatedThenOk(std::sync::Arc::clone(&notify)))];
        let token = CancellationToken::new();

        let result = {
            let fut = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token);
            tokio::pin!(fut);
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);
            assert!(
                matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending),
                "the run must be blocked awaiting the gate"
            );

            token.cancel();
            assert!(
                matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending),
                "cancelling must not interrupt the already-in-flight step"
            );

            notify.notify_one();
            fut.await
        };
        assert!(result.is_ok(), "the in-flight step must still resolve to its own real Ok outcome, got {result:?}");
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_after_execute_pipeline_returns_does_not_panic_or_mutate() {
        let mut pipeline = make_pipeline(&["a"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) })];
        let token = CancellationToken::new();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &token).await;
        assert!(result.is_ok());
        let status_before = pipeline.steps[0].status;

        token.cancel();

        assert_eq!(
            pipeline.steps[0].status, status_before,
            "cancel() after execute_pipeline has already returned must not mutate a finished pipeline's status"
        );
    }

    fn split_toon_row(row: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut chars = row.chars().peekable();
        while chars.peek().is_some() {
            let mut field = String::new();
            if chars.peek() == Some(&'"') {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            field.push(escaped);
                        }
                    } else if c == '"' {
                        break;
                    } else {
                        field.push(c);
                    }
                }
            } else {
                while let Some(&c) = chars.peek() {
                    if c == ',' {
                        break;
                    }
                    field.push(c);
                    chars.next();
                }
            }
            fields.push(field);
            if chars.peek() == Some(&',') {
                chars.next();
            }
        }
        fields
    }

    #[tokio::test(start_paused = true)]
    async fn all_success_pipeline_reports_completed_in_every_render_row() {
        let mut pipeline = make_pipeline(&["fetch", "upload", "notify"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = (0..3)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Completed);
        }

        let rendered = pipeline.render();
        let mut lines = rendered.lines();
        let _header = lines.next().unwrap();
        let mut row_count = 0;
        for line in lines {
            let row = line.strip_prefix("  ").unwrap_or(line);
            let fields = split_toon_row(row);
            assert_eq!(fields.len(), 3, "row {row:?} must have exactly 3 fields");
            assert_eq!(fields[2], "completed", "row {row:?} third field must be exactly 'completed'");
            row_count += 1;
        }
        assert_eq!(row_count, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn render_field_exact_parsing_is_not_fooled_by_completed_in_step_name() {
        // The all-success fixture above never creates the condition AC-038's
        // field-exact parsing exists to guard: a row whose id/name literally
        // contains the substring "completed" but whose real status is NOT
        // completed. A naive `rendered.contains("completed")` check would be
        // fooled by this row; field-exact parsing (checking fields[2]
        // specifically) correctly reports it as failed.
        let mut pipeline = make_pipeline(&["get-completed-orders"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> =
            vec![Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) })];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_err());
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);

        let rendered = pipeline.render();
        assert!(
            rendered.contains("completed"),
            "sanity check: the row's name must literally contain the substring 'completed' for this test to exercise anything"
        );

        let mut lines = rendered.lines();
        let _header = lines.next().unwrap();
        let row = lines.next().unwrap();
        let row = row.strip_prefix("  ").unwrap_or(row);
        let fields = split_toon_row(row);
        assert_eq!(fields.len(), 3);
        assert_ne!(
            fields[2], "completed",
            "field-exact parsing must report this row's real status, not be fooled by 'completed' appearing in the name field"
        );
        assert_eq!(fields[2], "failed");
    }

    #[tokio::test(start_paused = true)]
    async fn pre_set_non_pending_status_does_not_skip_the_runner() {
        // Uses Failed, not Completed, as the pre-set entry status: a review
        // found that pre-setting Completed made the final status assertion
        // vacuous, since a genuinely successful run also produces Completed
        // -- indistinguishable from a bug that invokes the runner but skips
        // the status WRITE. Failed->Completed is a real, observable
        // transition that only a genuine overwrite produces.
        let mut pipeline = make_pipeline(&["a", "b"]);
        pipeline.steps[0].status = michi_core::pipeline::StepStatus::Failed;
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runner0 = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(SpyStep(std::sync::Arc::clone(&runner0))),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        assert_eq!(
            runner0.load(Ordering::SeqCst),
            1,
            "the pre-Completed step's runner must still be invoked exactly once"
        );
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn nth_step_failure_is_fail_fast_with_exact_source_and_statuses() {
        let mut pipeline = make_pipeline(&["a", "b", "c"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(10),
        );
        let after_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        struct AlwaysFailsNonRetryableStep;
        impl Step for AlwaysFailsNonRetryableStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                Box::pin(async { Err(ExecutionError::Failed { message: "boom".into(), retryable: false }) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(AlwaysFailsNonRetryableStep),
            Box::new(SpyStep(std::sync::Arc::clone(&after_calls))),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert_eq!(after_calls.load(Ordering::SeqCst), 0, "no runner after the failing step may be invoked");
        match result {
            Err(ExecutionError::StepFailed { step_id, step_name, source }) => {
                assert_eq!(step_id, "b");
                assert_eq!(step_name, "b");
                match *source {
                    ExecutionError::Failed { ref message, retryable } => {
                        assert_eq!(message, "boom");
                        assert!(!retryable);
                    }
                    other => panic!("expected the exact Failed source, got {other:?}"),
                }
            }
            other => panic!("expected StepFailed, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Failed);
        assert_eq!(pipeline.steps[2].status, michi_core::pipeline::StepStatus::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn step_failed_reports_distinct_id_and_name_in_correct_fields() {
        // A mutation-testing checkpoint found that every test in this module
        // uses make_pipeline, whose fixture always sets id and name to the
        // same string -- so a bug that swaps step_id/step_name when
        // constructing StepFailed would go completely undetected. This test
        // uses genuinely distinct id/name values to close that gap.
        let mut pipeline = michi_core::pipeline::Pipeline {
            id: "p".into(),
            steps: vec![michi_core::pipeline::PipelineStep {
                id: "step-id-xyz".into(),
                name: "Human Readable Name".into(),
                status: michi_core::pipeline::StepStatus::Pending,
            }],
        };
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> =
            vec![Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) })];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, step_name, .. }) => {
                assert_eq!(step_id, "step-id-xyz", "step_id must come from step.id, not step.name");
                assert_eq!(step_name, "Human Readable Name", "step_name must come from step.name, not step.id");
            }
            other => panic!("expected StepFailed, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn execute_pipeline_never_writes_skipped() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(10),
        );
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let _ = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        for step in &pipeline.steps {
            assert_ne!(step.status, michi_core::pipeline::StepStatus::Skipped);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn steps_execute_strictly_sequentially_with_a_sleep_window() {
        let mut pipeline = make_pipeline(&["s0", "s1", "s2"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let log: std::sync::Arc<std::sync::Mutex<Vec<(&'static str, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        struct MarkerStep {
            id: String,
            log: std::sync::Arc<std::sync::Mutex<Vec<(&'static str, String)>>>,
            sleep: Option<Duration>,
        }
        impl Step for MarkerStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                Box::pin(async move {
                    self.log.lock().unwrap_or_else(|e| e.into_inner()).push(("start", self.id.clone()));
                    if let Some(sleep) = self.sleep {
                        tokio::time::sleep(sleep).await;
                    }
                    self.log.lock().unwrap_or_else(|e| e.into_inner()).push(("end", self.id.clone()));
                    Ok(())
                })
            }
        }

        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(MarkerStep {
                id: "s0".into(),
                log: std::sync::Arc::clone(&log),
                sleep: Some(Duration::from_millis(50)),
            }),
            Box::new(MarkerStep { id: "s1".into(), log: std::sync::Arc::clone(&log), sleep: None }),
            Box::new(MarkerStep { id: "s2".into(), log: std::sync::Arc::clone(&log), sleep: None }),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        let recorded = log.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(
            recorded,
            vec![
                ("start", "s0".to_string()),
                ("end", "s0".to_string()),
                ("start", "s1".to_string()),
                ("end", "s1".to_string()),
                ("start", "s2".to_string()),
                ("end", "s2".to_string()),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn jitter_seed_is_identical_across_every_step() {
        let mut pipeline = make_pipeline(&["a", "b", "c"]);
        let retry_config =
            michi_core::resilience::RetryConfig::new(1, Duration::from_millis(10), Duration::from_millis(100), 0.5);
        let breaker = CircuitBreaker::new(retry_config.clone(), Duration::from_secs(5), 10, Duration::from_secs(60));

        struct SeedSpyStep;
        impl Step for SeedSpyStep {
            fn run<'a>(
                &'a self,
                attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ExecutionError::Failed { message: "retry once".into(), retryable: true })
                    } else {
                        Ok(())
                    }
                })
            }
        }

        let runners: Vec<Box<dyn Step>> = (0..3).map(|_| Box::new(SeedSpyStep) as Box<dyn Step>).collect();

        let call_seed = 0.42_f64;
        let before = tokio::time::Instant::now();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, call_seed, &CancellationToken::new()).await;
        let elapsed = before.elapsed();
        assert!(result.is_ok());

        let per_step_delay = michi_core::resilience::next_retry_delay(&retry_config, 0, call_seed, None).unwrap();
        assert_eq!(
            elapsed,
            per_step_delay * 3,
            "all 3 steps must have used the same jitter_seed, producing 3 identical delays"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn panicking_kth_step_propagates_and_leaves_correct_partial_state() {
        let pipeline = std::sync::Arc::new(tokio::sync::Mutex::new(make_pipeline(&["a", "b", "c"])));
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            10,
            Duration::from_secs(60),
        ));

        let handle = {
            let pipeline = std::sync::Arc::clone(&pipeline);
            let breaker = std::sync::Arc::clone(&breaker);
            tokio::spawn(async move {
                let mut guard = pipeline.lock().await;
                let runners: Vec<Box<dyn Step>> = vec![
                    Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
                    Box::new(PanicsStep),
                    Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
                ];
                let _ = execute_pipeline(&mut guard, runners, &breaker, 0.0, &CancellationToken::new()).await;
            })
        };
        let join_result = handle.await;
        assert!(join_result.is_err(), "the panic must propagate out of execute_pipeline uncaught");

        let guard = pipeline.lock().await;
        assert_eq!(guard.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(
            guard.steps[1].status,
            michi_core::pipeline::StepStatus::Pending,
            "step K's status must be untouched, not Failed"
        );
        assert_eq!(guard.steps[2].status, michi_core::pipeline::StepStatus::Pending);
    }

    #[test]
    fn new_variants_are_not_retryable_and_map_to_invalid_input() {
        let cases = [
            ExecutionError::UnknownStepId { step_id: "x".into() },
            ExecutionError::DuplicateStepId { step_id: "x".into() },
            ExecutionError::CyclicDependency { step_id: "x".into() },
        ];
        for case in cases {
            assert!(!case.is_retryable());
        }
        let d: michi_core::DomainError = ExecutionError::UnknownStepId { step_id: "a".into() }.into();
        assert_eq!(d.code, michi_core::ErrorCode::InvalidInput);
        assert!(!d.retryable);
        let d: michi_core::DomainError = ExecutionError::DuplicateStepId { step_id: "b".into() }.into();
        assert_eq!(d.code, michi_core::ErrorCode::InvalidInput);
        assert!(!d.retryable);
        let d: michi_core::DomainError = ExecutionError::CyclicDependency { step_id: "c".into() }.into();
        assert_eq!(d.code, michi_core::ErrorCode::InvalidInput);
        assert!(!d.retryable);
    }

    #[test]
    fn step_dependencies_new_rejects_duplicate_ids() {
        let ids = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        match StepDependencies::new(&ids) {
            Err(ExecutionError::DuplicateStepId { step_id }) => assert_eq!(step_id, "a"),
            other => panic!("expected DuplicateStepId, got {other:?}"),
        }
    }

    #[test]
    fn step_dependencies_new_accepts_distinct_ids_with_zero_edges() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let deps = StepDependencies::new(&ids).expect("distinct ids must construct");
        assert!(deps.edges.is_empty());
        assert_eq!(deps.ids.len(), 2);
    }

    #[test]
    fn depends_on_accumulates_multiple_edges_for_the_same_dependent() {
        let ids = vec!["a1".to_string(), "a2".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a1");
        deps.depends_on("b", "a2");
        let edges = deps.edges.get("b").expect("b must have edges");
        assert_eq!(edges.len(), 2, "the second call must add a second edge, not overwrite the first");
        assert!(edges.contains(&"a1".to_string()));
        assert!(edges.contains(&"a2".to_string()));
    }

    #[test]
    fn detect_cycle_finds_a_two_node_cycle() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "b");
        deps.depends_on("b", "a");
        let found = detect_cycle(&deps);
        assert!(
            found == Some("a".to_string()) || found == Some("b".to_string()),
            "expected one of the cyclic ids, got {found:?}"
        );
    }

    #[test]
    fn detect_cycle_finds_a_self_dependency_as_a_one_node_cycle() {
        let ids = vec!["a".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "a");
        assert_eq!(detect_cycle(&deps), Some("a".to_string()));
    }

    #[test]
    fn detect_cycle_reports_none_for_an_acyclic_graph() {
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        deps.depends_on("c", "b");
        assert_eq!(detect_cycle(&deps), None);
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_runner_count_mismatch_returns_error_without_mutating_statuses() {
        let mut pipeline = make_pipeline(&["a"]);
        let deps = StepDependencies::new(&["a".to_string()]).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepCountMismatch { expected, got }) => {
                assert_eq!(expected, 1);
                assert_eq!(got, 0);
            }
            other => panic!("expected StepCountMismatch, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_empty_pipeline_returns_ok_immediately() {
        let mut pipeline = make_pipeline(&[]);
        let deps = StepDependencies::new(&[]).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn duplicate_pipeline_id_referenced_by_an_edge_is_rejected() {
        let mut pipeline = make_pipeline(&["a", "a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..3)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::DuplicateStepId { step_id }) => assert_eq!(step_id, "a"),
            other => panic!("expected DuplicateStepId, got {other:?}"),
        }
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn id_set_mismatch_between_graph_and_pipeline_is_rejected() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "c".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(matches!(result, Err(ExecutionError::UnknownStepId { .. })), "expected UnknownStepId, got {result:?}");
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn edge_endpoint_absent_from_pipeline_is_rejected() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "ghost");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> =
            (0..2).map(|_| Box::new(SpyStep(std::sync::Arc::clone(&calls))) as Box<dyn Step>).collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::UnknownStepId { step_id }) => assert_eq!(step_id, "ghost"),
            other => panic!("expected UnknownStepId{{step_id: ghost}}, got {other:?}"),
        }
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no Step::run may be invoked before UnknownStepId validation returns"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pipeline_superset_step_absent_from_graph_is_rejected() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> =
            (0..2).map(|_| Box::new(SpyStep(std::sync::Arc::clone(&calls))) as Box<dyn Step>).collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::UnknownStepId { step_id }) => assert_eq!(step_id, "b"),
            other => panic!("expected UnknownStepId{{step_id: b}}, got {other:?}"),
        }
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no Step::run may be invoked before UnknownStepId validation returns, including the pipeline-superset direction"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cyclic_dependency_is_rejected() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "b");
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(
            matches!(result, Err(ExecutionError::CyclicDependency { .. })),
            "expected CyclicDependency, got {result:?}"
        );
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_self_dependency_is_rejected_as_a_one_node_cycle() {
        let mut pipeline = make_pipeline(&["a"]);
        let ids = vec!["a".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![Box::new(SpyStep(std::sync::Arc::clone(&calls)))];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::CyclicDependency { step_id }) => assert_eq!(step_id, "a"),
            other => panic!("expected CyclicDependency{{step_id: a}}, got {other:?}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a's runner must never be invoked once its own self-dependency is rejected as a cycle"
        );
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Pending);
    }

    #[tokio::test(start_paused = true)]
    async fn simultaneous_violations_resolve_in_fixed_order() {
        let mut pipeline = make_pipeline(&["a", "a"]);
        let ids = vec!["a".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) })];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(
            matches!(result, Err(ExecutionError::StepCountMismatch { .. })),
            "StepCountMismatch must be checked first, got {result:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn duplicate_beats_unknown_step_id() {
        let mut pipeline = make_pipeline(&["a", "a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..3)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::DuplicateStepId { step_id }) => assert_eq!(step_id, "a"),
            other => panic!(
                "expected DuplicateStepId to win over the simultaneous UnknownStepId(\"c\") violation, got {other:?}"
            ),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn unknown_beats_cyclic_dependency() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "b");
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(
            matches!(result, Err(ExecutionError::UnknownStepId { .. })),
            "expected UnknownStepId to win over the simultaneous CyclicDependency violation (deps' id set {{a,b,c}} != pipeline's {{a,b}}, and a/b also cyclically depend on each other), got {result:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn independent_steps_all_succeed_returns_ok_and_all_completed() {
        let mut pipeline = make_pipeline(&["a", "b", "c"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..3)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok(), "expected Ok(()), got {result:?}");
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Completed);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn single_independent_step_failure_returns_step_failed_identifying_it() {
        let mut pipeline = make_pipeline(&["a"]);
        let ids = vec!["a".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> =
            vec![Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) })];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, .. }) => assert_eq!(step_id, "a"),
            other => panic!("expected StepFailed{{step_id: a}}, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_step_skips_its_direct_dependent() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(SpyStep(std::sync::Arc::clone(&calls))),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, .. }) => assert_eq!(step_id, "a"),
            other => panic!("expected StepFailed{{step_id: a}}, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Skipped);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "b's runner must never be invoked once its prerequisite a has failed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failed_step_transitively_skips_a_multi_level_dependency_chain() {
        let mut pipeline = make_pipeline(&["a", "b", "c"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        deps.depends_on("c", "b");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let calls_b = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let calls_c = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_b))),
            Box::new(SpyStep(std::sync::Arc::clone(&calls_c))),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, .. }) => assert_eq!(step_id, "a"),
            other => panic!("expected StepFailed{{step_id: a}}, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Skipped);
        assert_eq!(pipeline.steps[2].status, michi_core::pipeline::StepStatus::Skipped);
        assert_eq!(
            calls_b.load(Ordering::SeqCst),
            0,
            "b's runner must never be invoked once its prerequisite a has failed"
        );
        assert_eq!(
            calls_c.load(Ordering::SeqCst),
            0,
            "c's runner must never be invoked once its transitive prerequisite a has failed via b"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn dependent_step_runs_only_after_its_prerequisite_completes() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok(), "expected Ok(()), got {result:?}");
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Completed);
    }

    struct RendezvousX(tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>);
    impl Step for RendezvousX {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async move {
                let rx = self.0.lock().await.take().expect("run must only be invoked once");
                rx.await.map_err(|_| ExecutionError::Failed { message: "sender dropped".into(), retryable: false })?;
                Ok(())
            })
        }
    }

    struct RendezvousY(tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>);
    impl Step for RendezvousY {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async move {
                let tx = self.0.lock().await.take().expect("run must only be invoked once");
                let _ = tx.send(());
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn independent_steps_are_polled_concurrently_not_sequentially() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut pipeline = make_pipeline(&["x", "y"]);
        let ids = vec!["x".to_string(), "y".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(RendezvousX(tokio::sync::Mutex::new(Some(rx)))),
            Box::new(RendezvousY(tokio::sync::Mutex::new(Some(tx)))),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok(), "X's rx must resolve because Y is polled concurrently, got {result:?}");
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn sequential_execute_pipeline_times_out_on_the_equivalent_input() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut pipeline = make_pipeline(&["x", "y"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(RendezvousX(tokio::sync::Mutex::new(Some(rx)))),
            Box::new(RendezvousY(tokio::sync::Mutex::new(Some(tx)))),
        ];
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { source, .. }) => {
                assert!(matches!(*source, ExecutionError::Timeout { .. }), "expected Timeout, got {source:?}");
            }
            other => panic!("expected StepFailed{{source: Timeout}}, got {other:?}"),
        }
    }

    struct SetsFlagAfterYield {
        flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Step for SetsFlagAfterYield {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            let flag = std::sync::Arc::clone(&self.flag);
            Box::pin(async move {
                tokio::task::yield_now().await;
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    struct RecordsFlagOnInvocation {
        flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        observed_flag_at_invocation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Step for RecordsFlagOnInvocation {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.observed_flag_at_invocation.store(self.flag.load(Ordering::SeqCst), Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dependent_step_is_not_invoked_until_its_dependency_completes() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let a_completed_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let b_observed_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(SetsFlagAfterYield { flag: std::sync::Arc::clone(&a_completed_flag) }),
            Box::new(RecordsFlagOnInvocation {
                flag: std::sync::Arc::clone(&a_completed_flag),
                observed_flag_at_invocation: std::sync::Arc::clone(&b_observed_flag),
            }),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        assert!(
            b_observed_flag.load(Ordering::SeqCst),
            "B's Step::run must not be invoked until A's flag was set, i.e. until A reached Completed"
        );
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn dependent_waits_for_both_declared_prerequisites_to_complete() {
        let mut pipeline = make_pipeline(&["a1", "a2", "b"]);
        let ids = vec!["a1".to_string(), "a2".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a1");
        deps.depends_on("b", "a2");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..3)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok());
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Completed);
        }
    }

    struct GatedNeverResolves(std::sync::Arc<tokio::sync::Notify>);
    impl Step for GatedNeverResolves {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            let notify = std::sync::Arc::clone(&self.0);
            Box::pin(async move {
                notify.notified().await;
                Ok(())
            })
        }
    }

    struct CountingSkippableStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
    impl Step for CountingSkippableStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dependent_is_skipped_as_soon_as_either_prerequisite_fails_without_waiting_for_the_other() {
        let mut pipeline = make_pipeline(&["a1", "a2", "b"]);
        let ids = vec!["a1".to_string(), "a2".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a1");
        deps.depends_on("b", "a2");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(3600),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let a2_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let b_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(GatedNeverResolves(std::sync::Arc::clone(&a2_notify))),
            Box::new(CountingSkippableStep(std::sync::Arc::clone(&b_calls))),
        ];

        let result = {
            let cancellation_scratch = CancellationToken::new();
            let fut = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &cancellation_scratch);
            tokio::pin!(fut);
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);

            for _ in 0..8 {
                let poll = fut.as_mut().poll(&mut cx);
                assert!(matches!(poll, std::task::Poll::Pending), "the run must not resolve before a2 is notified");
                tokio::task::yield_now().await;
            }
            assert_eq!(
                b_calls.load(Ordering::SeqCst),
                0,
                "b must not be invoked while a1 has failed but a2 has not yet been released - this is the witness that b's skip does not wait on a2"
            );

            a2_notify.notify_one();
            fut.await
        };

        assert!(matches!(result, Err(ExecutionError::StepFailed { .. })));
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
        assert_eq!(pipeline.steps[2].status, michi_core::pipeline::StepStatus::Skipped);
        assert_eq!(
            b_calls.load(Ordering::SeqCst),
            0,
            "b's runner must never have been invoked at all, even after the run completed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failure_skips_direct_and_transitive_dependents_leaving_no_step_pending() {
        let mut pipeline = make_pipeline(&["a", "b", "c", "d"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        deps.depends_on("c", "b");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        assert!(matches!(result, Err(ExecutionError::StepFailed { .. })));
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed, "a");
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Skipped, "b (direct dependent)");
        assert_eq!(pipeline.steps[2].status, michi_core::pipeline::StepStatus::Skipped, "c (transitive dependent)");
        assert_eq!(pipeline.steps[3].status, michi_core::pipeline::StepStatus::Completed, "d (unrelated)");
        for step in &pipeline.steps {
            assert_ne!(
                step.status,
                michi_core::pipeline::StepStatus::Pending,
                "no step may remain Pending on a live Err(StepFailed) return"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn two_independent_failures_yield_exactly_one_step_failed_identifying_one_of_them() {
        let mut pipeline = michi_core::pipeline::Pipeline {
            id: "p".into(),
            steps: vec![
                michi_core::pipeline::PipelineStep {
                    id: "a".into(),
                    name: "Alpha".into(),
                    status: michi_core::pipeline::StepStatus::Pending,
                },
                michi_core::pipeline::PipelineStep {
                    id: "b".into(),
                    name: "Beta".into(),
                    status: michi_core::pipeline::StepStatus::Pending,
                },
            ],
        };
        let ids = vec!["a".to_string(), "b".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, step_name, .. }) => {
                assert!(
                    (step_id == "a" && step_name == "Alpha") || (step_id == "b" && step_name == "Beta"),
                    "expected one of the two failed steps' own id paired with its own name, got step_id={step_id:?} step_name={step_name:?}"
                );
            }
            other => panic!("expected exactly one StepFailed, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Failed);
    }

    struct SleepsThenOk;
    impl Step for SleepsThenOk {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn unrelated_in_flight_step_runs_to_completion_despite_an_earlier_failure() {
        let mut pipeline = make_pipeline(&["a", "d"]);
        let ids = vec!["a".to_string(), "d".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            2,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(SleepsThenOk),
        ];
        let result = execute_pipeline_parallel(
            &mut pipeline,
            &deps,
            runners,
            std::sync::Arc::clone(&breaker),
            0.0,
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(ExecutionError::StepFailed { .. })));
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed, "a");
        assert_eq!(
            pipeline.steps[1].status,
            michi_core::pipeline::StepStatus::Completed,
            "d must run to its own real terminal status"
        );
        assert_eq!(breaker.phase(), BreakerPhase::Closed, "a's single failure must not open a threshold-2 breaker");
    }

    #[tokio::test(start_paused = true)]
    async fn newly_eligible_unrelated_step_still_starts_after_failure_elsewhere() {
        let mut pipeline = make_pipeline(&["a", "c", "d"]);
        let ids = vec!["a".to_string(), "c".to_string(), "d".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("d", "c");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            2,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(SleepsThenOk),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result = execute_pipeline_parallel(
            &mut pipeline,
            &deps,
            runners,
            std::sync::Arc::clone(&breaker),
            0.0,
            &CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(ExecutionError::StepFailed { .. })));
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed, "a");
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Completed, "c");
        assert_eq!(
            pipeline.steps[2].status,
            michi_core::pipeline::StepStatus::Completed,
            "d must still be invoked once c completes, even though a already failed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_skips_every_step_before_first_spawn() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> =
            vec![Box::new(SpyStep(std::sync::Arc::clone(&calls))), Box::new(SpyStep(std::sync::Arc::clone(&calls)))];
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(matches!(result, Err(ExecutionError::Cancelled)), "expected Err(Cancelled), got {result:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no Step::run may be invoked once cancelled before the first spawn"
        );
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Skipped);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_at_a_subsequent_checkpoint_skips_the_pending_dependent_and_returns_cancelled() {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(3600),
            1,
            Duration::from_secs(60),
        ));
        let b_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(GatedThenOk(std::sync::Arc::clone(&notify))),
            Box::new(SpyStep(std::sync::Arc::clone(&b_calls))),
        ];
        let token = CancellationToken::new();

        let result = {
            let fut = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token);
            tokio::pin!(fut);
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);

            for _ in 0..8 {
                assert!(
                    matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending),
                    "a must still be in flight awaiting its gate"
                );
                tokio::task::yield_now().await;
            }

            token.cancel();
            notify.notify_one();
            fut.await
        };

        assert!(matches!(result, Err(ExecutionError::Cancelled)), "expected Err(Cancelled), got {result:?}");
        assert_eq!(
            pipeline.steps[0].status,
            michi_core::pipeline::StepStatus::Completed,
            "a resolved Ok before cancellation took effect"
        );
        assert_eq!(
            pipeline.steps[1].status,
            michi_core::pipeline::StepStatus::Skipped,
            "b, still Pending at the next checkpoint, must be skipped"
        );
        assert_eq!(b_calls.load(Ordering::SeqCst), 0, "b's Step::run must never be invoked once cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_after_every_step_is_already_running_returns_ok() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(3600),
            1,
            Duration::from_secs(60),
        ));
        let a_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(GatedThenOk(std::sync::Arc::clone(&a_notify))),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let token = CancellationToken::new();

        let result = {
            let fut = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token);
            tokio::pin!(fut);
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);
            assert!(matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending));

            token.cancel();
            a_notify.notify_one();
            fut.await
        };

        assert!(
            result.is_ok(),
            "a too-late cancellation (no Pending step remained at any later checkpoint) must not turn an all-Ok run into Err(Cancelled), got {result:?}"
        );
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Completed);
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_while_a_step_is_running_lets_it_finish_before_returning() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(3600),
            1,
            Duration::from_secs(60),
        ));
        let a_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(GatedThenOk(std::sync::Arc::clone(&a_notify))),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let token = CancellationToken::new();

        let result = {
            let fut = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token);
            tokio::pin!(fut);
            let waker = std::task::Waker::noop();
            let mut cx = std::task::Context::from_waker(waker);

            for _ in 0..8 {
                assert!(matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending), "a must still be running");
                tokio::task::yield_now().await;
            }

            token.cancel();

            assert!(
                matches!(fut.as_mut().poll(&mut cx), std::task::Poll::Pending),
                "must not return while a is still Running, even after cancellation is observed"
            );

            a_notify.notify_one();
            fut.await
        };
        assert!(result.is_ok());
        assert_eq!(
            pipeline.steps[0].status,
            michi_core::pipeline::StepStatus::Completed,
            "a must reach its own real terminal status, never overwritten to Skipped"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_does_not_preempt_step_count_mismatch_parallel() {
        let mut pipeline = make_pipeline(&["a"]);
        let ids = vec!["a".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![];
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(
            matches!(result, Err(ExecutionError::StepCountMismatch { .. })),
            "expected StepCountMismatch, got {result:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_does_not_preempt_duplicate_step_id() {
        let mut pipeline = make_pipeline(&["a", "a"]);
        let ids = vec!["a".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(
            matches!(result, Err(ExecutionError::DuplicateStepId { .. })),
            "expected DuplicateStepId, got {result:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_does_not_preempt_unknown_step_id() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "c".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(matches!(result, Err(ExecutionError::UnknownStepId { .. })), "expected UnknownStepId, got {result:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn precancelled_token_does_not_preempt_cyclic_dependency() {
        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("a", "b");
        deps.depends_on("b", "a");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let token = CancellationToken::new();
        token.cancel();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(
            matches!(result, Err(ExecutionError::CyclicDependency { .. })),
            "expected CyclicDependency, got {result:?}"
        );
    }

    struct FailsAndCancels {
        token: CancellationToken,
        calls: std::sync::atomic::AtomicU32,
    }
    impl Step for FailsAndCancels {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.token.cancel();
            Box::pin(async { Err(ExecutionError::Failed { message: "boom".into(), retryable: false }) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_failure_and_cancellation_in_the_same_run_returns_step_failed_not_cancelled() {
        let mut pipeline = make_pipeline(&["a", "b", "c"]);
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("c", "b");
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::new(0, Duration::from_millis(1), Duration::from_secs(1), 0.0),
            Duration::from_secs(3600),
            u32::MAX,
            Duration::from_secs(60),
        ));
        let token = CancellationToken::new();
        let c_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(FailsAndCancels { token: token.clone(), calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
            Box::new(SpyStep(std::sync::Arc::clone(&c_calls))),
        ];
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        match result {
            Err(ExecutionError::StepFailed { step_id, .. }) => assert_eq!(step_id, "a"),
            other => panic!("expected StepFailed to take precedence over Cancelled, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_after_execute_pipeline_parallel_returns_does_not_panic_or_mutate() {
        let mut pipeline = make_pipeline(&["a"]);
        let ids = vec!["a".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let runners: Vec<Box<dyn Step>> = vec![Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) })];
        let token = CancellationToken::new();
        let result = execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &token).await;
        assert!(result.is_ok());
        let status_before = pipeline.steps[0].status;

        token.cancel();

        assert_eq!(
            pipeline.steps[0].status, status_before,
            "cancel() after execute_pipeline_parallel has already returned must not mutate a finished pipeline's status"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn circuit_open_short_circuit_writes_failed_and_skips_dependent() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(3600),
        ));
        let priming_fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&priming_fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);

        let mut pipeline = make_pipeline(&["a", "b"]);
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut deps = StepDependencies::new(&ids).unwrap();
        deps.depends_on("b", "a");
        let a_calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct SpyStep(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for SpyStep {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> = vec![
            Box::new(SpyStep(std::sync::Arc::clone(&a_calls))),
            Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }),
        ];
        let result = execute_pipeline_parallel(
            &mut pipeline,
            &deps,
            runners,
            std::sync::Arc::clone(&breaker),
            0.0,
            &CancellationToken::new(),
        )
        .await;
        match result {
            Err(ExecutionError::StepFailed { source, .. }) => {
                assert!(
                    matches!(*source, ExecutionError::CircuitOpen { .. }),
                    "expected CircuitOpen source, got {source:?}"
                );
            }
            other => panic!("expected StepFailed{{source: CircuitOpen}}, got {other:?}"),
        }
        assert_eq!(pipeline.steps[0].status, michi_core::pipeline::StepStatus::Failed, "a");
        assert_eq!(pipeline.steps[1].status, michi_core::pipeline::StepStatus::Skipped, "b, a's dependent");
        assert_eq!(
            a_calls.load(Ordering::SeqCst),
            0,
            "a's Step::run must never be invoked once the circuit is already Open"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn half_open_gate_releases_after_probe_panics_and_admits_a_real_subsequent_probe() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(10),
        ));
        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let handle = {
            let breaker = std::sync::Arc::clone(&breaker);
            tokio::spawn(async move { breaker.call(&PanicsStep, 0.0).await })
        };
        assert!(handle.await.is_err(), "the panic must propagate out of call() as a JoinError");

        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let probe2 = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&probe2, 0.0).await;
        assert!(result.is_ok(), "a real subsequent probe must be admitted after the panic, got {result:?}");
        assert_eq!(
            probe2.calls.load(Ordering::SeqCst),
            1,
            "the subsequent probe's Step::run must actually have been invoked, not rejected as CircuitOpen"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn execute_pipeline_still_executes_duplicate_ids_without_validation() {
        let mut pipeline = make_pipeline(&["dup", "dup"]);
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        );
        let runners: Vec<Box<dyn Step>> = (0..2)
            .map(|_| Box::new(CountingStep { calls: std::sync::atomic::AtomicU32::new(0) }) as Box<dyn Step>)
            .collect();
        let result = execute_pipeline(&mut pipeline, runners, &breaker, 0.0, &CancellationToken::new()).await;
        assert!(result.is_ok(), "execute_pipeline must still accept duplicate ids with no validation, got {result:?}");
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Completed);
        }
    }

    #[test]
    fn execute_pipeline_signature_includes_trailing_cancellation_token() {
        #[allow(dead_code)]
        async fn _sig(
            p: &mut michi_core::pipeline::Pipeline,
            r: Vec<Box<dyn Step>>,
            b: &CircuitBreaker,
            j: f64,
            c: &CancellationToken,
        ) -> Result<(), ExecutionError> {
            execute_pipeline(p, r, b, j, c).await
        }
    }

    struct NeverResolvesStep;
    impl Step for NeverResolvesStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }

    struct DelayedPanicStep;
    impl Step for DelayedPanicStep {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                panic!("deliberate test panic");
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn panicking_step_contributes_neither_success_nor_failure_to_the_shared_breaker() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            2,
            Duration::from_secs(60),
        ));
        let pipeline = std::sync::Arc::new(tokio::sync::Mutex::new(make_pipeline(&["a", "b", "c"])));
        let ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let deps = StepDependencies::new(&ids).unwrap();

        let breaker_for_task = std::sync::Arc::clone(&breaker);
        let pipeline_for_task = std::sync::Arc::clone(&pipeline);
        let handle = tokio::spawn(async move {
            let mut guard = pipeline_for_task.lock().await;
            let runners: Vec<Box<dyn Step>> = vec![
                Box::new(DelayedPanicStep),
                Box::new(AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) }),
                Box::new(NeverResolvesStep),
            ];
            execute_pipeline_parallel(&mut guard, &deps, runners, breaker_for_task, 0.0, &CancellationToken::new())
                .await
        });
        let join_result = handle.await;
        assert!(
            join_result.is_err(),
            "the panic must propagate out of execute_pipeline_parallel uncaught, surfacing as a JoinError on this outer spawn"
        );
        assert_eq!(
            breaker.phase(),
            BreakerPhase::Closed,
            "b's single failure alone must not reach failure_threshold=2 - proving a's panic contributed no failure count of its own"
        );

        let guard = pipeline.lock().await;
        assert_eq!(
            guard.steps[0].status,
            michi_core::pipeline::StepStatus::Pending,
            "a, the panicking step's own status, must be left untouched"
        );
        assert_eq!(
            guard.steps[2].status,
            michi_core::pipeline::StepStatus::Pending,
            "c, a third step never drained before the panic was observed, must also be left untouched"
        );
    }

    // ---- Regression tests for the 2026-10 defect batch ----

    struct HangingCommit {
        commits: std::sync::atomic::AtomicU32,
    }
    impl Step for HangingCommit {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async move {
                self.commits.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_does_not_reinvoke_a_step_that_has_not_opted_in_to_retries() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(1, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_millis(500), u32::MAX, Duration::from_secs(60));
        let step = HangingCommit { commits: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&step, 0.0).await;
        assert!(matches!(result, Err(ExecutionError::Timeout { .. })), "expected Timeout, got {result:?}");
        assert_eq!(
            step.commits.load(Ordering::SeqCst),
            1,
            "a timed-out attempt may already have committed; a step that has not opted in must not be re-invoked"
        );
    }

    struct IdempotentHangingCommit {
        commits: std::sync::atomic::AtomicU32,
    }
    impl Step for IdempotentHangingCommit {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            Box::pin(async move {
                self.commits.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(())
            })
        }
        fn is_idempotent(&self) -> bool {
            true
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_still_retries_a_step_that_opted_in_as_idempotent() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(1, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_millis(500), u32::MAX, Duration::from_secs(60));
        let step = IdempotentHangingCommit { commits: std::sync::atomic::AtomicU32::new(0) };
        let result = breaker.call(&step, 0.0).await;
        assert!(matches!(result, Err(ExecutionError::Timeout { .. })), "expected Timeout, got {result:?}");
        assert_eq!(step.commits.load(Ordering::SeqCst), 2, "an idempotent step is retried after a timeout");
    }

    #[tokio::test(start_paused = true)]
    async fn step_returned_timeout_is_also_not_retried_unless_idempotent() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(1, Duration::from_millis(1), Duration::from_millis(10), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let make = |calls: std::sync::Arc<std::sync::atomic::AtomicU32>| {
            step_fn(move |_attempt: u32| {
                calls.fetch_add(1, Ordering::SeqCst);
                async {
                    Err(ExecutionError::StepFailed {
                        step_id: "inner".into(),
                        step_name: "inner".into(),
                        source: Box::new(ExecutionError::Timeout { elapsed_ms: 7 }),
                    })
                }
            })
        };
        assert!(!make(std::sync::Arc::clone(&calls)).is_idempotent(), "step_fn defaults to not idempotent");
        assert!(breaker.call(&make(std::sync::Arc::clone(&calls)), 0.0).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "wrapped Timeout from a non-idempotent step: one invocation");

        calls.store(0, Ordering::SeqCst);
        let opted_in = make(std::sync::Arc::clone(&calls)).idempotent();
        assert!(opted_in.is_idempotent());
        assert!(breaker.call(&opted_in, 0.0).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "FnStep::idempotent() opts the closure in to timeout retries");
    }

    #[tokio::test(start_paused = true)]
    async fn non_timeout_retryable_errors_still_retry_without_opt_in() {
        let retry_config =
            michi_core::resilience::RetryConfig::new(5, Duration::from_millis(10), Duration::from_secs(1), 0.0);
        let breaker = CircuitBreaker::new(retry_config, Duration::from_secs(5), u32::MAX, Duration::from_secs(60));
        let step = FlakyStep { fail_until: 2, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(!step.is_idempotent());
        assert!(breaker.call(&step, 0.0).await.is_ok());
        assert_eq!(step.calls.load(Ordering::SeqCst), 3, "step-classified retryable failures are unaffected");
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_rejects_duplicate_ids_even_when_no_edge_mentions_them() {
        let mut pipeline = make_pipeline(&["a", "a"]);
        let deps = StepDependencies::new(&["a".to_string()]).unwrap();
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(5),
            1,
            Duration::from_secs(60),
        ));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        struct Spy(std::sync::Arc<std::sync::atomic::AtomicU32>);
        impl Step for Spy {
            fn run<'a>(
                &'a self,
                _attempt: u32,
            ) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(()) })
            }
        }
        let runners: Vec<Box<dyn Step>> =
            vec![Box::new(Spy(std::sync::Arc::clone(&calls))), Box::new(Spy(std::sync::Arc::clone(&calls)))];
        let result =
            execute_pipeline_parallel(&mut pipeline, &deps, runners, breaker, 0.0, &CancellationToken::new()).await;
        match result {
            Err(ExecutionError::DuplicateStepId { step_id }) => assert_eq!(step_id, "a"),
            other => panic!("expected DuplicateStepId{{step_id: a}}, got {other:?}"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no step may run once a duplicate id is detected");
        for step in &pipeline.steps {
            assert_eq!(step.status, michi_core::pipeline::StepStatus::Pending);
        }
    }

    fn large_graph(n: usize, close_cycle: bool) -> StepDependencies {
        let ids: Vec<String> = (0..n).map(|i| format!("s{i}")).collect();
        let mut deps = StepDependencies::new(&ids).expect("distinct ids");
        for i in 1..n {
            deps.depends_on(&ids[i], &ids[i - 1]);
        }
        if close_cycle {
            deps.depends_on(&ids[0], &ids[n - 1]);
        }
        deps
    }

    #[test]
    fn detect_cycle_handles_a_100k_node_cycle_without_overflowing_the_stack() {
        let deps = large_graph(100_000, true);
        let found = detect_cycle(&deps).expect("a 100k-node ring must be reported as a cycle");
        assert!(deps.ids.contains(&found), "reported id {found} must be a step id in the cycle");
    }

    #[test]
    fn detect_cycle_accepts_a_100k_node_acyclic_chain() {
        let deps = large_graph(100_000, false);
        assert_eq!(detect_cycle(&deps), None);
    }

    struct GatedFailure {
        release: std::sync::Arc<tokio::sync::Notify>,
    }
    impl Step for GatedFailure {
        fn run<'a>(&'a self, _attempt: u32) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>> {
            let release = std::sync::Arc::clone(&self.release);
            Box::pin(async move {
                release.notified().await;
                Err(ExecutionError::Failed { message: "stale".into(), retryable: false })
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stale_failure_from_before_the_breaker_opened_does_not_reopen_a_recovered_breaker() {
        let breaker = std::sync::Arc::new(CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(10),
        ));
        // The stale call is admitted while Closed and stays in flight across
        // the whole Open -> HalfOpen -> Closed cycle below.
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let stale = {
            let breaker = std::sync::Arc::clone(&breaker);
            let step = GatedFailure { release: std::sync::Arc::clone(&release) };
            tokio::spawn(async move { breaker.call(&step, 0.0).await })
        };
        tokio::task::yield_now().await;

        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&fail, 0.0).await.is_err());
        assert_eq!(breaker.phase(), BreakerPhase::Open);
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = CountingStep { calls: std::sync::atomic::AtomicU32::new(0) };
        assert!(breaker.call(&probe, 0.0).await.is_ok());
        assert_eq!(breaker.phase(), BreakerPhase::Closed, "the probe succeeded, so the breaker recovered");

        release.notify_one();
        assert!(stale.await.expect("stale call must not panic").is_err());
        assert_eq!(
            breaker.phase(),
            BreakerPhase::Closed,
            "a result from a call admitted before the breaker opened must not transition the recovered breaker"
        );
    }

    /// Bounded stress test (multi-thread runtime, < 2 s wall clock) for the
    /// HalfOpen race: a caller whose HalfOpen decision was taken before a
    /// concurrent probe succeeded must not run a second probe and reopen the
    /// recovered breaker.
    #[test]
    fn half_open_race_stress_recovered_breaker_is_never_reopened_by_a_stale_half_open_decision() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_time()
            .build()
            .expect("multi-thread runtime must build");
        rt.block_on(async {
            let breaker = std::sync::Arc::new(CircuitBreaker::new(
                michi_core::resilience::RetryConfig::try_new(0, Duration::from_millis(1), Duration::from_millis(1), 0.0)
                    .expect("valid retry config"),
                Duration::from_secs(60),
                100,
                Duration::from_millis(1),
            ));
            std::thread::sleep(Duration::from_millis(3));
            let deadline = std::time::Instant::now() + Duration::from_millis(1000);
            let mut trial = 0u32;
            while std::time::Instant::now() < deadline {
                trial += 1;
                force_half_open_for_test(&breaker);
                let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(9));
                let a = {
                    let breaker = std::sync::Arc::clone(&breaker);
                    let barrier = std::sync::Arc::clone(&barrier);
                    tokio::spawn(async move {
                        barrier.wait().await;
                        breaker.call(&AlwaysOk, 0.0).await
                    })
                };
                let mut bs = Vec::new();
                for _ in 0..8 {
                    let breaker = std::sync::Arc::clone(&breaker);
                    let barrier = std::sync::Arc::clone(&barrier);
                    bs.push(tokio::spawn(async move {
                        barrier.wait().await;
                        let fail = AlwaysFailStep { retryable: false, calls: std::sync::atomic::AtomicU32::new(0) };
                        breaker.call(&fail, 0.0).await
                    }));
                }
                let a_result = a.await.expect("A must not panic");
                for b in bs {
                    let _ = b.await.expect("B must not panic");
                }
                if a_result.is_ok() {
                    assert_eq!(
                        breaker.phase(),
                        BreakerPhase::Closed,
                        "trial {trial}: A's probe succeeded and no B can reach failure_threshold=100, so the breaker must stay Closed"
                    );
                }
            }
        });
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_a_stale_half_open_decision_cannot_reopen_a_recovered_breaker() {
        // Deterministic replay of the reported race by driving admission and
        // completion directly, in the problematic order.
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            100,
            Duration::from_secs(10),
        );
        force_half_open_for_test(&breaker);
        tokio::time::advance(Duration::from_secs(10)).await;
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen);

        let a = breaker.admit().expect("first HalfOpen caller claims the probe");
        assert!(matches!(a, Admission::Probe { .. }));
        // B arrives while A's probe is in flight: the decision and the claim
        // are one critical section, so B cannot also become a probe.
        assert!(matches!(breaker.admit(), Err(ExecutionError::CircuitOpen { retry_after_ms: 10_000 })));

        breaker.complete(a, true);
        assert_eq!(breaker.phase(), BreakerPhase::Closed);

        // B retries after A's success: it is admitted as a normal Closed call,
        // never as a second probe, and its failure only counts toward 100.
        let b = breaker.admit().expect("Closed breaker admits");
        assert!(matches!(b, Admission::Closed { .. }), "no second probe after recovery, got {b:?}");
        breaker.complete(b, false);
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
        assert_eq!(breaker.lock_state().consecutive_failures, 1);

        // A probe outcome from the finished HalfOpen generation is stale and
        // must not transition the current state.
        breaker.complete(a, false);
        assert_eq!(breaker.phase(), BreakerPhase::Closed, "stale probe failure must be ignored");
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_stale_closed_outcomes_neither_extend_the_open_window_nor_reset_the_count() {
        // The failure-count/open interleaving: several calls admitted in the
        // same Closed generation finish after the breaker has already opened.
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            2,
            Duration::from_secs(10),
        );
        let c1 = breaker.admit().expect("closed");
        let c2 = breaker.admit().expect("closed");
        let c3 = breaker.admit().expect("closed");
        let c4 = breaker.admit().expect("closed");
        breaker.complete(c1, false);
        breaker.complete(c2, false);
        assert_eq!(breaker.phase(), BreakerPhase::Open, "threshold 2 reached");

        tokio::time::advance(Duration::from_secs(4)).await;
        breaker.complete(c3, false);
        breaker.complete(c4, true);
        match breaker.admit() {
            Err(ExecutionError::CircuitOpen { retry_after_ms }) => {
                assert_eq!(retry_after_ms, 6_000, "a stale failure must not re-stamp opened_at")
            }
            other => panic!("expected CircuitOpen, got {other:?}"),
        }
        assert_eq!(breaker.lock_state().consecutive_failures, 2, "a stale success must not reset the count");
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_abandoned_probe_only_releases_its_own_generation() {
        let breaker = CircuitBreaker::new(
            michi_core::resilience::RetryConfig::default(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(10),
        );
        force_half_open_for_test(&breaker);
        tokio::time::advance(Duration::from_secs(10)).await;
        let Ok(Admission::Probe { generation: g1 }) = breaker.admit() else {
            panic!("expected a probe admission");
        };
        breaker.abandon_probe(g1);
        assert_eq!(breaker.phase(), BreakerPhase::HalfOpen, "an abandoned probe frees the slot");
        let p2 = breaker.admit().expect("a new probe may be claimed");
        breaker.abandon_probe(g1);
        assert!(
            matches!(breaker.admit(), Err(ExecutionError::CircuitOpen { .. })),
            "releasing a stale generation must not free the current probe's slot"
        );
        breaker.complete(p2, true);
        assert_eq!(breaker.phase(), BreakerPhase::Closed);
    }

    /// Test-only: puts the breaker into the state it would be in after
    /// opening at its failure threshold at epoch time 0, so it reports HalfOpen
    /// once the breaker clock is at least `open_duration` past its epoch.
    fn force_half_open_for_test(breaker: &CircuitBreaker) {
        let mut state = breaker.lock_state();
        state.consecutive_failures = breaker.failure_threshold;
        state.phase = StoredPhase::Open { opened_at_ms: 0 };
        state.generation = state.generation.wrapping_add(1);
    }
}

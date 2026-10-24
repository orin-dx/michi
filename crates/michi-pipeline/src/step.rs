//! The `Step` trait, its context and the closure adapter.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::{CancellationToken, ExecutionError, RunId, StepId};

/// Whether running a step more than once has the same effect as running it once, or how to tell whether it already
/// ran.
///
/// Decides what happens after an attempt whose effect is unknown (timeout, HTTP 5xx, interruption,
/// [`ExecutionError::failed`]): an idempotent step is retried and, on resume, re-run; a prechecked step is retried
/// and re-run through its [`Step::precheck`]; any other step ends
/// [`michi_core::pipeline::StepStatus::OutcomeUnknown`] and is never replayed automatically.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Replay {
    /// Repeating the step could repeat its side effect. The default: a step must opt in to replay.
    #[default]
    NotIdempotent,
    /// Repeating the step is harmless.
    Idempotent,
    /// Repeating the step could repeat its side effect, but its [`Step::precheck`] reliably tells whether the effect
    /// is already in place. The precheck runs before every attempt, in a fresh run and on resume.
    Prechecked,
}

/// What a [`Step::precheck`] found in the outside world. Mirrors [`crate::Verification`]: precheck is verification
/// the step performs for itself.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precheck {
    /// The effect is already in place: the step is not run and ends `CompletedEarlier`.
    Applied,
    /// There is proof the effect is absent: the attempt runs. Return it only with proof; a read that may be stale
    /// (a cached registry document, say) is not proof.
    NotApplied,
    /// Can't tell (eventual consistency, a read that failed softly): the attempt does not run.
    Unknown,
}

/// What bounds one attempt of a step: the executor's attempt timeout, or the step's deadline.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttemptBound {
    /// Each attempt runs under the attempt timeout ([`crate::ExecOptions::attempt_timeout`] or
    /// [`crate::ExecStep::attempt_timeout`]), cut to the time left before the step's deadline. The default.
    #[default]
    Timeout,
    /// The attempt runs until the step's deadline (its own or its deadline group's, whichever is earlier), however
    /// long the attempt timeout is. For a step that waits by design, such as [`crate::poll_step`]. Without a deadline,
    /// the attempt timeout bounds the attempt as usual. The step sees its bound in
    /// [`StepContext::attempt_deadline`] and should finish before it: an attempt the executor has to cut ends with
    /// [`ExecutionError::Timeout`], whose effect is unknown.
    Deadline,
}

/// What a step attempt gets from the executor.
#[derive(Debug, Clone)]
pub struct StepContext {
    run_id: RunId,
    step_id: StepId,
    attempt: u32,
    cancellation: CancellationToken,
    deadline: Option<tokio::time::Instant>,
    attempt_deadline: Option<tokio::time::Instant>,
    attempt_timeout: Duration,
}

/// The attempt timeout a context made outside the executor reports: the executor's default.
const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

impl StepContext {
    /// A context for attempt 0 with a fresh token, no deadline and the default 30 s attempt timeout, for exercising
    /// a [`Step`] on its own (in tests, say). The executor builds its own.
    #[must_use]
    pub fn new(run_id: RunId, step_id: StepId) -> Self {
        Self {
            run_id,
            step_id,
            attempt: 0,
            cancellation: CancellationToken::new(),
            deadline: None,
            attempt_deadline: None,
            attempt_timeout: DEFAULT_ATTEMPT_TIMEOUT,
        }
    }

    pub(crate) fn for_attempt(
        run_id: RunId,
        step_id: StepId,
        attempt: u32,
        cancellation: CancellationToken,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        Self { attempt, cancellation, deadline, ..Self::new(run_id, step_id) }
    }

    /// The same context with another cancellation token.
    pub(crate) fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Sets when the executor cuts this attempt, and the step's configured attempt timeout.
    pub(crate) fn bounded(mut self, attempt_deadline: Option<tokio::time::Instant>, attempt_timeout: Duration) -> Self {
        self.attempt_deadline = attempt_deadline;
        self.attempt_timeout = attempt_timeout;
        self
    }

    /// The run this attempt belongs to.
    #[must_use]
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// The step being run.
    #[must_use]
    pub fn step_id(&self) -> &StepId {
        &self.step_id
    }

    /// 0-based attempt number within this run of the step. For logging and backoff only; never part of the
    /// idempotency key.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// This step's cancellation token, a child of the run's. Watch it to stop cooperatively; once it fires, the
    /// executor drops the attempt after the cancellation grace period.
    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// When the step's deadline expires, if one is set: its total deadline across all attempts, or its deadline
    /// group's window end ([`crate::ExecOptions::group_deadline`]) if that is earlier.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Time left until [`StepContext::deadline`], if one is set.
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(tokio::time::Instant::now()))
    }

    /// When the executor cuts this attempt (or this precheck): the attempt timeout from its start, or the step's
    /// deadline if that comes first or the step is [`AttemptBound::Deadline`]. `None` outside the executor. A step
    /// that wants to stop on its own terms, with its own error, finishes before it.
    #[must_use]
    pub fn attempt_deadline(&self) -> Option<tokio::time::Instant> {
        self.attempt_deadline
    }

    /// The attempt timeout configured for this step ([`crate::ExecStep::attempt_timeout`] or
    /// [`crate::ExecOptions::attempt_timeout`]). An [`AttemptBound::Deadline`] step is not cut by it, so it can apply
    /// it to each unit of its own work: [`crate::poll_step`] bounds each check with it.
    #[must_use]
    pub fn attempt_timeout(&self) -> Duration {
        self.attempt_timeout
    }

    /// `run_id:step_id`: the same on every attempt and on resume, unique across runs. Pass it to a downstream that
    /// deduplicates to make the step's effect apply once.
    #[must_use]
    pub fn idempotency_key(&self) -> michi_core::resilience::IdempotencyKey {
        let mut key = String::with_capacity(self.run_id.as_str().len() + 1 + self.step_id.as_str().len());
        key.push_str(self.run_id.as_str());
        key.push(':');
        key.push_str(self.step_id.as_str());
        michi_core::resilience::IdempotencyKey::new(key)
    }
}

/// The boxed future a [`Step`] attempt returns.
pub type StepFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>>;

/// The boxed future a [`Step::precheck`] returns.
pub type PrecheckFuture<'a> = Pin<Box<dyn Future<Output = Result<Precheck, ExecutionError>> + Send + 'a>>;

/// What a plan step runs. Object-safe, so steps of different types share one plan without an async-trait
/// dependency.
///
/// # Replay safety
///
/// The executor may run a step more than once: on retry within a run, and on resume. Every failed attempt carries two
/// facts: whether retrying could help ([`ExecutionError::is_retryable`]) and whether its effect may have landed
/// ([`ExecutionError::effect`]). From those and [`Step::replay`]:
///
/// - **Not applied** ([`crate::Effect::NotApplied`]): circuit open, `Cancelled`, a passed deadline
///   ([`ExecutionError::DeadlinePassed`]), HTTP 4xx and 503, [`ExecutionError::refused`] and
///   [`ExecutionError::rejected`]. Retried if retryable; otherwise the step ends `Failed` and resume runs it again.
/// - **Unknown** ([`crate::Effect::Unknown`]): a timeout, HTTP 5xx other than 503, an interruption, a panic,
///   [`ExecutionError::failed`] and [`ExecutionError::transient`]. Retried only if retryable and the step is
///   [`Replay::Idempotent`] or [`Replay::Prechecked`]. Otherwise the step ends `OutcomeUnknown` and resume holds it
///   until the caller verifies it (an idempotent step ends `Failed` and resume runs it; a prechecked one runs and its
///   precheck decides).
///
/// A step classifies its own outcome by the error it returns, so it must classify honestly: a step that may have
/// committed its effect must not report 429, 503, a 4xx, `refused`, `rejected` or `Cancelled`. It reports
/// [`ExecutionError::failed`] (or 500, 502, 504 or a timeout) instead.
pub trait Step: Send + Sync {
    /// Runs one attempt.
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a>;

    /// Whether the step may be replayed after an attempt whose effect is unknown. Defaults to
    /// [`Replay::NotIdempotent`].
    fn replay(&self) -> Replay {
        Replay::NotIdempotent
    }

    /// Looks at the outside world before an attempt. Only consulted for [`Replay::Prechecked`] steps, before every
    /// attempt (after breaker admission), under the attempt timeout. It is not reported to the breaker and does not
    /// draw from the retry budget.
    ///
    /// A timeout or an `Err` counts as [`Precheck::Unknown`], except [`ExecutionError::refused`] (not retryable, not
    /// applied), which fails the step: that is how a precheck reports a blocking precondition. The default answers
    /// `Unknown`, which never runs the step, so a prechecked step must implement this.
    fn precheck<'a>(&'a self, ctx: &'a StepContext) -> PrecheckFuture<'a> {
        let _ = ctx;
        Box::pin(async { Ok(Precheck::Unknown) })
    }

    /// What bounds each attempt. Defaults to [`AttemptBound::Timeout`]; a step that waits by design until its deadline
    /// returns [`AttemptBound::Deadline`]. A precheck always runs under the attempt timeout.
    fn attempt_bound(&self) -> AttemptBound {
        AttemptBound::Timeout
    }
}

/// [`Step`] over an async closure, built by [`step_fn`].
pub struct FnStep<F> {
    f: F,
    replay: Replay,
}

// Manual so `F` (usually a closure) need not be `Debug`.
impl<F> std::fmt::Debug for FnStep<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnStep").field("replay", &self.replay).finish_non_exhaustive()
    }
}

impl<F> FnStep<F> {
    /// Declares the step idempotent ([`Replay::Idempotent`]).
    #[must_use]
    pub fn idempotent(mut self) -> Self {
        self.replay = Replay::Idempotent;
        self
    }
}

/// Adapts an async closure into a [`Step`]. The closure receives an owned clone of the attempt's [`StepContext`].
/// The step is [`Replay::NotIdempotent`] unless [`FnStep::idempotent`] is called.
pub fn step_fn<F, Fut>(f: F) -> FnStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    FnStep { f, replay: Replay::NotIdempotent }
}

impl<F, Fut> Step for FnStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin((self.f)(ctx.clone()))
    }

    fn replay(&self) -> Replay {
        self.replay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> StepContext {
        StepContext::new(RunId::try_new("run-7").unwrap(), StepId::try_new("upload").unwrap())
    }

    #[test]
    fn idempotency_key_is_run_colon_step_and_ignores_the_attempt() {
        let first = ctx();
        let retry = StepContext::for_attempt(
            first.run_id().clone(),
            first.step_id().clone(),
            3,
            CancellationToken::new(),
            None,
        );
        assert_eq!(first.idempotency_key().as_str(), "run-7:upload");
        assert_eq!(retry.idempotency_key(), first.idempotency_key());
    }

    #[tokio::test(start_paused = true)]
    async fn step_fn_adapts_a_closure_and_defaults_to_not_idempotent() {
        let step = step_fn(|ctx: StepContext| async move {
            assert_eq!(ctx.step_id().as_str(), "upload");
            Ok(())
        });
        assert_eq!(step.replay(), Replay::NotIdempotent);
        assert_eq!(step.run(&ctx()).await, Ok(()));
        assert_eq!(step_fn(|_| async { Ok(()) }).idempotent().replay(), Replay::Idempotent);
    }

    #[test]
    fn step_is_object_safe() {
        let steps: Vec<Box<dyn Step>> = vec![Box::new(step_fn(|_| async { Ok(()) }))];
        assert_eq!(steps.len(), 1);
    }
}

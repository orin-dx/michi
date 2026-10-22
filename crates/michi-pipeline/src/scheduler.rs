//! The scheduler loop (one for both modes) and the per-step retry loop.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use michi_core::pipeline::{NotRunReason, SkipReason, StepStatus, UnknownCause};
use michi_core::resilience::{next_retry_delay, RetryBudget, RetryConfig};
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::breaker::AdmissionGuard;
use crate::error::millis;
use crate::exec::{ExecOptions, FailurePolicy, Mode, StepObserver};
use crate::plan::Fnv;
use crate::{
    AttemptRecord, CancellationToken, CircuitBreaker, ExecutionError, Plan, Replay, ReplayClass, RunId, RunRecord,
    Step, StepContext, StepId, StepRecord,
};

/// How a step enters the scheduler: to be run, or already terminal (carried over by resume).
pub(crate) enum Initial {
    Run,
    Done(Box<StepRecord>),
}

/// Attempts a running step has finished, shared with the scheduler so they survive a panic.
type Attempts = Arc<Mutex<Vec<AttemptRecord>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Everything one spawned step needs.
struct Job {
    runner: Arc<dyn Step>,
    replay: Replay,
    run_id: RunId,
    step_id: StepId,
    token: CancellationToken,
    retry: RetryConfig,
    attempt_timeout: Duration,
    deadline: Option<Duration>,
    jitter_seed: Option<f64>,
    breaker: Option<(String, Arc<CircuitBreaker>)>,
    budget: Option<Arc<Mutex<RetryBudget>>>,
    grace: Duration,
    attempts: Attempts,
}

/// A step's terminal result, before the scheduler adds timing and attempts.
struct Outcome {
    status: StepStatus,
    error: Option<ExecutionError>,
    reason: Option<String>,
}

/// A dispatched step the scheduler is waiting on.
struct Running {
    index: usize,
    dispatched: Instant,
    attempts: Attempts,
    replay: Replay,
}

/// Runs `plan` from `initial` and returns the record. Owns its `JoinSet`, so no step outlives the call.
pub(crate) async fn run(plan: &Plan, run_id: RunId, opts: &ExecOptions, initial: Vec<Initial>) -> RunRecord {
    let mut s = Scheduler::new(plan, run_id, opts);
    s.seed(initial);
    s.drive().await;
    s.finish()
}

struct Scheduler<'a> {
    plan: &'a Plan,
    opts: &'a ExecOptions,
    run_id: RunId,
    run_start: Instant,
    run_token: CancellationToken,
    budget: Option<Arc<Mutex<RetryBudget>>>,
    records: Vec<Option<StepRecord>>,
    remaining: Vec<usize>,
    ready: BTreeSet<usize>,
    completion_order: Vec<StepId>,
    stopping: bool,
    observer_panics: u32,
}

/// What woke the scheduler loop.
enum Event {
    Joined(Option<Result<(tokio::task::Id, Outcome), tokio::task::JoinError>>),
    Cancelled,
}

impl<'a> Scheduler<'a> {
    fn new(plan: &'a Plan, run_id: RunId, opts: &'a ExecOptions) -> Self {
        let n = plan.len();
        Self {
            plan,
            opts,
            run_id,
            run_start: Instant::now(),
            run_token: opts.cancellation.child_token(),
            budget: opts.retry_budget.clone().map(|b| Arc::new(Mutex::new(b))),
            records: vec![None; n],
            remaining: vec![0; n],
            ready: BTreeSet::new(),
            completion_order: Vec::new(),
            stopping: false,
            observer_panics: 0,
        }
    }

    /// Records carried-over steps, skips steps behind a carried-over failure, and fills the ready set.
    fn seed(&mut self, initial: Vec<Initial>) {
        for (i, init) in initial.into_iter().enumerate() {
            if let Initial::Done(record) = init {
                self.set_terminal(i, *record);
            }
        }
        for &i in self.plan.topological_order() {
            if self.records[i].is_some() {
                continue;
            }
            let mut waiting = 0;
            let mut blocked_by = None;
            for &p in self.plan.prerequisites_of(i) {
                match &self.records[p] {
                    Some(r) if r.status.is_success() => {}
                    Some(r) => {
                        blocked_by = Some(root_of(r));
                        break;
                    }
                    None => waiting += 1,
                }
            }
            if let Some(root) = blocked_by {
                let record = self.skipped_by_prerequisite(i, root);
                self.set_terminal(i, record);
            } else if waiting == 0 {
                self.ready.insert(i);
            } else {
                self.remaining[i] = waiting;
            }
        }
    }

    async fn drive(&mut self) {
        let cap = match self.opts.mode {
            Mode::Sequential => 1,
            Mode::Parallel { max_concurrency } => max_concurrency.get(),
        };
        let mut join_set: JoinSet<Outcome> = JoinSet::new();
        #[allow(clippy::disallowed_types, reason = "internal task-id lookup, never rendered")]
        let mut running: std::collections::HashMap<tokio::task::Id, Running> = std::collections::HashMap::new();
        loop {
            if self.run_token.is_cancelled() {
                self.stopping = true;
            }
            while !self.stopping && join_set.len() < cap {
                let Some(i) = self.ready.pop_first() else { break };
                let (job, entry) = self.job(i);
                let opts = self.opts;
                if observe(opts, |o| o.on_step_started(&self.run_id, &job.step_id)) {
                    self.observer_panics = self.observer_panics.saturating_add(1);
                }
                let handle = join_set.spawn(run_step(job));
                running.insert(handle.id(), entry);
            }
            if join_set.is_empty() {
                break;
            }
            let event = tokio::select! {
                biased;
                joined = join_set.join_next_with_id() => Event::Joined(joined),
                () = self.run_token.cancelled(), if !self.stopping => Event::Cancelled,
            };
            match event {
                Event::Cancelled => self.stopping = true,
                Event::Joined(None) => break,
                Event::Joined(Some(Ok((id, outcome)))) => {
                    if let Some(entry) = running.remove(&id) {
                        self.step_finished(entry, outcome);
                    }
                }
                Event::Joined(Some(Err(join_err))) => {
                    if let Some(entry) = running.remove(&join_err.id()) {
                        let outcome = harness_outcome(join_err, entry.replay);
                        self.step_finished(entry, outcome);
                    }
                }
            }
        }
    }

    /// Builds the job for step `i` and the scheduler's bookkeeping for it.
    fn job(&self, i: usize) -> (Job, Running) {
        let step = &self.plan.steps()[i];
        let attempts: Attempts = Arc::default();
        let replay = step.runner().replay();
        let breaker = self.opts.breakers.as_ref().map(|registry| {
            let key = step.effective_breaker_key().to_string();
            let breaker = registry.breaker(&key);
            (key, breaker)
        });
        let attempt_timeout = step.attempt_timeout_override().unwrap_or(self.opts.attempt_timeout);
        let job = Job {
            runner: Arc::clone(step.runner()),
            replay,
            run_id: self.run_id.clone(),
            step_id: step.id().clone(),
            token: self.run_token.child_token(),
            retry: step.retry_override().unwrap_or(&self.opts.retry).clone(),
            attempt_timeout: if attempt_timeout.is_zero() { Duration::from_millis(1) } else { attempt_timeout },
            deadline: step.deadline_override().or(self.opts.step_deadline),
            jitter_seed: self.opts.jitter_seed,
            breaker,
            budget: self.budget.clone(),
            grace: self.opts.cancel_grace,
            attempts: Arc::clone(&attempts),
        };
        (job, Running { index: i, dispatched: Instant::now(), attempts, replay })
    }

    fn step_finished(&mut self, entry: Running, outcome: Outcome) {
        let i = entry.index;
        let step = &self.plan.steps()[i];
        let mut record = StepRecord::new(step.id().clone(), step.name().to_string(), outcome.status);
        record.reason = outcome.reason;
        record.error = outcome.error;
        record.attempts = std::mem::take(&mut *lock(&entry.attempts));
        if outcome.status == StepStatus::Panicked {
            let attempt = u32::try_from(record.attempts.len()).unwrap_or(u32::MAX);
            record.attempts.push(AttemptRecord {
                attempt,
                class: Some(ReplayClass::Ambiguous),
                error: record.error.as_ref().map(ToString::to_string),
                elapsed: entry.dispatched.elapsed(),
            });
        }
        record.started_at = Some(entry.dispatched.duration_since(self.run_start));
        record.elapsed = Some(entry.dispatched.elapsed());

        if record.status.is_success() {
            self.completion_order.push(record.step_id.clone());
            self.set_terminal(i, record);
            for &d in self.plan.dependents_of(i) {
                if self.records[d].is_none() && self.remaining[d] > 0 {
                    self.remaining[d] -= 1;
                    if self.remaining[d] == 0 {
                        self.ready.insert(d);
                    }
                }
            }
            return;
        }
        self.set_terminal(i, record);
        if !self.run_token.is_cancelled() {
            self.skip_dependents(i);
        }
        if self.opts.failure_policy == FailurePolicy::FailFast {
            self.stopping = true;
            self.run_token.cancel();
        }
    }

    /// Marks every transitive dependent of `i` that has not started as skipped, naming `i`'s root cause.
    fn skip_dependents(&mut self, i: usize) {
        let Some(root) = self.records[i].as_ref().map(root_of) else { return };
        let mut stack = vec![i];
        while let Some(j) = stack.pop() {
            for &d in self.plan.dependents_of(j) {
                if self.records[d].is_none() {
                    self.ready.remove(&d);
                    let record = self.skipped_by_prerequisite(d, root.clone());
                    self.set_terminal(d, record);
                    stack.push(d);
                }
            }
        }
    }

    fn skipped_by_prerequisite(&self, i: usize, root: StepId) -> StepRecord {
        let step = &self.plan.steps()[i];
        let mut record = StepRecord::new(
            step.id().clone(),
            step.name().to_string(),
            StepStatus::Skipped(SkipReason::PrerequisiteFailed),
        );
        record.reason = Some(format!("prerequisite {root} did not complete"));
        record.caused_by = Some(root);
        record
    }

    fn set_terminal(&mut self, i: usize, record: StepRecord) {
        let opts = self.opts;
        if observe(opts, |o| o.on_step_finished(&self.run_id, &record)) {
            self.observer_panics = self.observer_panics.saturating_add(1);
        }
        self.records[i] = Some(record);
    }

    fn finish(mut self) -> RunRecord {
        for i in 0..self.records.len() {
            if self.records[i].is_none() {
                let step = &self.plan.steps()[i];
                let mut record = StepRecord::new(
                    step.id().clone(),
                    step.name().to_string(),
                    StepStatus::Skipped(SkipReason::Cancelled),
                );
                record.reason = Some("run cancelled before the step started".to_string());
                self.set_terminal(i, record);
            }
        }
        RunRecord {
            run_id: self.run_id,
            plan_fingerprint: self.plan.fingerprint(),
            plan_steps: self.plan.len(),
            steps: self.records.into_iter().flatten().collect(),
            completion_order: self.completion_order,
            cancelled: self.opts.cancellation.is_cancelled(),
            elapsed: self.run_start.elapsed(),
            observer_panics: self.observer_panics,
        }
    }
}

/// Calls the run's observer, if any, and reports whether the call panicked. The observer is caller code: a panic in
/// it is caught so it can neither unwind the scheduler (losing the record and dropping running steps) nor stop the
/// run. The caller counts it in [`RunRecord::observer_panics`].
fn observe(opts: &ExecOptions, call: impl FnOnce(&dyn StepObserver)) -> bool {
    let Some(observer) = opts.observer.as_deref() else { return false };
    // AssertUnwindSafe: after a caught panic the scheduler only reads its own state, which the observer cannot
    // touch; whatever state the observer itself left half-updated is its own.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(observer))).is_err()
}

/// The step a failure is attributed to: a skipped step's root cause, or the step itself.
fn root_of(record: &StepRecord) -> StepId {
    record.caused_by.clone().unwrap_or_else(|| record.step_id.clone())
}

/// The outcome of a task that did not return: it panicked, or (only if the set was dropped) it was aborted.
fn harness_outcome(join_err: tokio::task::JoinError, replay: Replay) -> Outcome {
    if join_err.is_panic() {
        let payload = join_err.into_panic();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        let error = ExecutionError::Panicked { message };
        return Outcome { status: StepStatus::Panicked, reason: Some(error.to_string()), error: Some(error) };
    }
    classify(ExecutionError::Interrupted { elapsed_ms: 0 }, replay, "task aborted", true)
}

/// Whether the executor may run the step again after `err`: the error must be retryable, and an ambiguous one only
/// for an idempotent step.
fn may_reinvoke(replay: Replay, err: &ExecutionError) -> bool {
    err.is_retryable() && (replay == Replay::Idempotent || err.replay_class() != ReplayClass::Ambiguous)
}

/// One attempt under a timeout, racing cancellation plus the grace period.
async fn attempt_once(
    step: &dyn Step,
    ctx: &StepContext,
    limit: Duration,
    token: &CancellationToken,
    grace: Duration,
) -> Result<(), ExecutionError> {
    let start = Instant::now();
    let attempt = tokio::time::timeout(limit, step.run(ctx));
    let interrupted = async {
        token.cancelled().await;
        tokio::time::sleep(grace).await;
    };
    tokio::select! {
        biased;
        result = attempt => result.unwrap_or(Err(ExecutionError::Timeout { elapsed_ms: millis(limit) })),
        () = interrupted => Err(ExecutionError::Interrupted { elapsed_ms: millis(start.elapsed()) }),
    }
}

/// Admission, the retry loop and classification for one step.
async fn run_step(job: Job) -> Outcome {
    if job.token.is_cancelled() {
        return Outcome {
            status: StepStatus::Skipped(SkipReason::Cancelled),
            error: None,
            reason: Some("run cancelled before the step started".to_string()),
        };
    }
    let guard = match &job.breaker {
        Some((key, breaker)) => match breaker.admit() {
            Ok(admission) => Some(AdmissionGuard::new(Arc::clone(breaker), admission)),
            Err(err) => {
                return Outcome {
                    status: StepStatus::NotRun(NotRunReason::CircuitOpen),
                    reason: Some(format!("circuit breaker {key}: {err}")),
                    error: Some(err),
                }
            }
        },
        None => None,
    };
    let probe = guard.as_ref().is_some_and(AdmissionGuard::is_probe);
    let started = Instant::now();
    // A deadline too far away for the clock to represent is no deadline, rather than an overflow panic.
    let deadline = job.deadline.and_then(|d| started.checked_add(d));
    let mut attempt = 0u32;
    let (err, stop) = loop {
        let ctx =
            StepContext::for_attempt(job.run_id.clone(), job.step_id.clone(), attempt, job.token.clone(), deadline);
        let attempt_start = Instant::now();
        let limit = match deadline {
            Some(d) => {
                job.attempt_timeout.min(d.saturating_duration_since(attempt_start)).max(Duration::from_millis(1))
            }
            None => job.attempt_timeout,
        };
        let result = attempt_once(job.runner.as_ref(), &ctx, limit, &job.token, job.grace).await;
        lock(&job.attempts).push(AttemptRecord {
            attempt,
            class: result.as_ref().err().map(ExecutionError::replay_class),
            error: result.as_ref().err().map(ToString::to_string),
            elapsed: attempt_start.elapsed(),
        });
        let err = match result {
            Ok(()) => {
                if let Some(budget) = &job.budget {
                    lock(budget).deposit();
                }
                if let Some(guard) = guard {
                    guard.complete(true);
                }
                return Outcome { status: StepStatus::Completed, error: None, reason: None };
            }
            Err(err) => err,
        };
        if job.token.is_cancelled() {
            break (err, "run cancelled");
        }
        if !may_reinvoke(job.replay, &err) {
            break (err, "");
        }
        if probe {
            break (err, "a half-open breaker allows one probe attempt");
        }
        let seed = job.jitter_seed.unwrap_or_else(|| derived_jitter_seed(&job.run_id, &job.step_id, attempt));
        let Some(delay) = retry_delay(&job.retry, attempt, seed, err.retry_after()) else {
            break (err, "retries exhausted");
        };
        if deadline.is_some_and(|d| Instant::now().checked_add(delay).is_none_or(|wake| wake >= d)) {
            break (err, "step deadline reached");
        }
        if let Some(budget) = &job.budget {
            if !lock(budget).try_withdraw() {
                break (err, "retry budget exhausted");
            }
        }
        let cancelled = tokio::select! {
            () = tokio::time::sleep(delay) => false,
            () = job.token.cancelled() => true,
        };
        if cancelled {
            break (err, "run cancelled during backoff");
        }
        attempt += 1;
    };
    if let Some(guard) = guard {
        match err.innermost() {
            // Cancellation says nothing about the downstream's health: release a probe, report nothing.
            ExecutionError::Cancelled | ExecutionError::Interrupted { .. } => {}
            // The downstream answered and rejected the request itself: it is up, so this is not a failure.
            ExecutionError::Http { status, .. } if is_client_error(*status) => guard.complete(true),
            _ => guard.complete(false),
        }
    }
    classify(err, job.replay, stop, job.token.is_cancelled())
}

/// Whether an HTTP status is the caller's own mistake (4xx other than 408 Request Timeout and 429 Too Many Requests):
/// proof the downstream is up and answering, so it never counts as a breaker failure. 408 and 429 say the downstream
/// is slow or overloaded, and 5xx says it failed, so those do count.
fn is_client_error(status: u16) -> bool {
    (400..500).contains(&status) && status != 408 && status != 429
}

/// The backoff before retry `attempt + 1`: michi-core's [`next_retry_delay`], except that a server's `retry_after` is
/// honoured in full even past `RetryConfig::max_delay`, which caps only the exponential backoff. The step deadline
/// still bounds it: the caller stops retrying if the wait would end past the deadline.
fn retry_delay(retry: &RetryConfig, attempt: u32, seed: f64, retry_after: Option<Duration>) -> Option<Duration> {
    let backoff = next_retry_delay(retry, attempt, seed, retry_after)?;
    Some(retry_after.map_or(backoff, |hint| backoff.max(ceil_to_millis(hint))))
}

/// `d` rounded up to whole milliseconds, as `next_retry_delay` rounds a hint, so a hint is never shortened.
fn ceil_to_millis(d: Duration) -> Duration {
    let whole = Duration::from_millis(millis(d));
    if whole < d {
        whole.saturating_add(Duration::from_millis(1))
    } else {
        whole
    }
}

/// A jitter seed in `[0, 1)` for one retry, derived from the run id, step id and attempt: steps that fail together
/// back off by different amounts, and the same run always waits the same delays.
fn derived_jitter_seed(run_id: &RunId, step_id: &StepId, attempt: u32) -> f64 {
    let mut h = Fnv::new();
    h.write(run_id.as_str().as_bytes());
    h.write(b":");
    h.write(step_id.as_str().as_bytes());
    h.write(b":");
    h.write(&attempt.to_le_bytes());
    // FNV-1a mixes its last bytes poorly; the splitmix64 finalizer spreads them over every bit.
    let mut z = h.finish();
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    // The top 53 bits as a fraction of 2^53, both exact in an f64.
    #[allow(clippy::cast_precision_loss, reason = "53-bit values fit an f64 mantissa exactly")]
    let fraction = (z >> 11) as f64 / (1u64 << 53) as f64;
    fraction
}

/// The terminal status and final error for a step whose last attempt failed with `err`. `cancelled` says whether the
/// step's cancellation token had fired. A step that returns [`ExecutionError::Cancelled`] while it had not stopped on
/// its own, so it ends `Failed`: a root cause that resume runs again, not a `Skipped(Cancelled)` consequence.
fn classify(err: ExecutionError, replay: Replay, stop: &str, cancelled: bool) -> Outcome {
    let class = err.replay_class();
    let mut stop = stop;
    let (status, error) = match class {
        ReplayClass::Ambiguous if replay == Replay::NotIdempotent => {
            let cause = match err.innermost() {
                ExecutionError::Http { status: 502 | 504, .. } => UnknownCause::Gateway,
                ExecutionError::Interrupted { .. } => UnknownCause::Cancelled,
                ExecutionError::Panicked { .. } => UnknownCause::Interrupted,
                _ => UnknownCause::Timeout,
            };
            let error =
                if err.is_retryable() { ExecutionError::AmbiguousOutcome { source: Box::new(err) } } else { err };
            (StepStatus::OutcomeUnknown(cause), error)
        }
        ReplayClass::NotExecuted => match err.innermost() {
            ExecutionError::CircuitOpen { .. } => (StepStatus::NotRun(NotRunReason::CircuitOpen), err),
            _ if cancelled => (StepStatus::Skipped(SkipReason::Cancelled), err),
            _ => {
                if stop.is_empty() {
                    stop = "the step cancelled itself although the run was not cancelled";
                }
                (StepStatus::Failed, err)
            }
        },
        _ => (StepStatus::Failed, err),
    };
    let reason = if stop.is_empty() { error.to_string() } else { format!("{error}; {stop}") };
    Outcome { status, error: Some(error), reason: Some(reason) }
}

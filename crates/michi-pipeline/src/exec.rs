//! Options, the observer, and the `execute`/`resume` entry points.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use michi_core::pipeline::{StepStatus, UnknownCause};
use michi_core::resilience::{RetryBudget, RetryConfig};

use crate::scheduler::{self, Initial};
use crate::{
    BreakerRegistry, CancellationToken, ExecutionError, Plan, PlanFingerprint, Replay, RunId, RunRecord, StepId,
    StepRecord,
};

/// How many steps run at once.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// One step at a time, in [`Plan::execution_order`]. Deterministic.
    #[default]
    Sequential,
    /// Up to `max_concurrency` steps at once. Ready steps are dispatched in declaration order while fewer than the
    /// cap are running.
    Parallel {
        /// The most steps running at once.
        max_concurrency: NonZeroUsize,
    },
}

/// What a step failure does to the rest of the run.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    /// Skip the failed step's transitive dependents; independent branches keep running.
    #[default]
    SkipDependents,
    /// Stop dispatching and cancel running steps (after the cancellation grace period, their attempts are dropped).
    FailFast,
}

/// Called synchronously from the scheduler loop as steps start and finish, so callers can persist progress and
/// rebuild the run with [`RunRecord::from_parts`] after a crash. A closure `Fn(&RunId, &StepRecord)` implements it
/// (finish events only). Keep it quick: it runs between dispatches.
///
/// A panic in an observer method is caught: the run continues, later events are still delivered, and the panic is
/// counted in [`RunRecord::observer_panics`]. (The process's panic hook still runs, so the default hook prints it.)
pub trait StepObserver: Send + Sync {
    /// A step is about to be dispatched. Persist [`StepRecord::in_flight`] here to make a crash mid-step visible.
    ///
    /// The step is dispatched whatever happens here, even if this panics. If saving the marker can fail and the step
    /// must not run without it, implement [`DurableObserver`] and pass it to [`ExecOptions::durable_starts`].
    fn on_step_started(&self, run_id: &RunId, step_id: &StepId) {
        let _ = (run_id, step_id);
    }

    /// A step reached its terminal status in this run: ran, was skipped, or (on resume) was carried over.
    fn on_step_finished(&self, run_id: &RunId, record: &StepRecord);
}

impl<F> StepObserver for F
where
    F: Fn(&RunId, &StepRecord) + Send + Sync,
{
    fn on_step_finished(&self, run_id: &RunId, record: &StepRecord) {
        self(run_id, record);
    }
}

/// An observer that saves each step's in-flight marker before the step may run. Pass it to
/// [`ExecOptions::durable_starts`]: a step then runs only once [`DurableObserver::record_start`] returns `Ok`, so a
/// record rebuilt after a crash never shows a step that ran as `Pending`.
///
/// A closure observer cannot be one: it sees finish events only, so durable starts with it do not compile.
///
/// ```compile_fail
/// use std::sync::Arc;
/// use michi_pipeline::{ExecOptions, RunId, StepRecord};
///
/// let finish_only = |_: &RunId, _: &StepRecord| {};
/// let _ = ExecOptions::new().durable_starts(Arc::new(finish_only));
/// ```
pub trait DurableObserver: StepObserver {
    /// Persist the in-flight marker ([`StepRecord::in_flight`]) and return `Ok` only once it is saved. Called instead
    /// of [`StepObserver::on_step_started`]. `Err(reason)` or a panic stops the step: it ends
    /// `NotRun(StartNotRecorded)` without being invoked, and resume runs it again.
    ///
    /// # Errors
    ///
    /// Why the marker could not be saved.
    fn record_start(&self, run_id: &RunId, step_id: &StepId) -> Result<(), String>;
}

/// Options for [`execute`]. Every field has a default; builder methods override them.
#[derive(Clone)]
pub struct ExecOptions {
    pub(crate) mode: Mode,
    pub(crate) failure_policy: FailurePolicy,
    pub(crate) retry: RetryConfig,
    pub(crate) attempt_timeout: Duration,
    pub(crate) step_deadline: Option<Duration>,
    pub(crate) group_deadlines: BTreeMap<String, Duration>,
    pub(crate) jitter_seed: Option<f64>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) cancel_grace: Duration,
    pub(crate) retry_budget: Option<RetryBudget>,
    pub(crate) breakers: Option<Arc<BreakerRegistry>>,
    pub(crate) concurrency_limits: BTreeMap<String, NonZeroUsize>,
    pub(crate) observer: Option<Arc<dyn StepObserver>>,
    /// Set with the observer by `durable_starts`: the same observer, which records starts.
    pub(crate) durable: Option<Arc<dyn DurableObserver>>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Sequential,
            failure_policy: FailurePolicy::SkipDependents,
            retry: RetryConfig::default(),
            attempt_timeout: Duration::from_secs(30),
            step_deadline: None,
            group_deadlines: BTreeMap::new(),
            jitter_seed: None,
            cancellation: CancellationToken::new(),
            cancel_grace: Duration::from_secs(5),
            retry_budget: Some(RetryBudget::default()),
            breakers: None,
            concurrency_limits: BTreeMap::new(),
            observer: None,
            durable: None,
        }
    }
}

impl std::fmt::Debug for ExecOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecOptions")
            .field("mode", &self.mode)
            .field("failure_policy", &self.failure_policy)
            .field("retry", &self.retry)
            .field("attempt_timeout", &self.attempt_timeout)
            .field("step_deadline", &self.step_deadline)
            .field("group_deadlines", &self.group_deadlines)
            .field("jitter_seed", &self.jitter_seed)
            .field("cancel_grace", &self.cancel_grace)
            .field("retry_budget", &self.retry_budget)
            .field("breakers", &self.breakers.is_some())
            .field("concurrency_limits", &self.concurrency_limits)
            .field("observer", &self.observer.is_some())
            .field("durable_starts", &self.durable.is_some())
            .finish_non_exhaustive()
    }
}

impl ExecOptions {
    /// The defaults: sequential, skip dependents, `RetryConfig::default()`, 30 s per attempt, no step or group
    /// deadlines, jitter seeded per run, step and attempt, 5 s cancellation grace, `RetryBudget::default()`, no breakers, no
    /// concurrency limits, no observer, no durable starts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sequential or parallel scheduling.
    #[must_use]
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// What a failure does to the rest of the run.
    #[must_use]
    pub fn failure_policy(mut self, policy: FailurePolicy) -> Self {
        self.failure_policy = policy;
        self
    }

    /// The retry policy for steps without their own.
    #[must_use]
    pub fn retry(mut self, retry: RetryConfig) -> Self {
        self.retry = retry;
        self
    }

    /// The per-attempt timeout for steps without their own. Zero is floored to 1 ms.
    #[must_use]
    pub fn attempt_timeout(mut self, timeout: Duration) -> Self {
        self.attempt_timeout = timeout;
        self
    }

    /// A total time bound per step across all attempts and backoff, for steps without their own. A deadline too far
    /// away for the clock to represent (such as `Duration::MAX`) means no deadline.
    #[must_use]
    pub fn step_deadline(mut self, deadline: Duration) -> Self {
        self.step_deadline = Some(deadline);
        self
    }

    /// The window shared by every step whose [`crate::ExecStep::deadline_group`] is `group`. It opens when the
    /// group's first step is dispatched in this run, so steps that start at different times still end together. A
    /// later call for the same group replaces the window, and a window for a group no step uses is ignored.
    ///
    /// Within the window, a step's deadline is the earlier of its own and the window's end: no attempt or backoff runs
    /// past it. A step of the group that would be dispatched once the window has ended is not run: it ends `Failed`
    /// with [`ExecutionError::DeadlinePassed`], which is not applied, so resume runs it again in a new window. A
    /// window too long for the clock to represent (such as `Duration::MAX`) means no deadline. A zero window behaves
    /// like a zero step deadline: the first step gets a 1 ms attempt and the others do not run.
    #[must_use]
    pub fn group_deadline(mut self, group: impl Into<String>, window: Duration) -> Self {
        self.group_deadlines.insert(group.into(), window);
        self
    }

    /// A fixed jitter seed in `[0, 1]`, passed to `next_retry_delay` for every retry of every step.
    ///
    /// Without one (the default), each retry gets its own seed derived from the run id, step id and attempt number,
    /// so steps that fail together do not retry in lockstep, yet the same run and plan always wait the same delays.
    /// A fixed seed makes every step's backoff identical; use it only when that is what you want (tests, say).
    #[must_use]
    pub fn jitter_seed(mut self, seed: f64) -> Self {
        self.jitter_seed = Some(seed);
        self
    }

    /// The caller's cancellation token. The run uses a child of it.
    #[must_use]
    pub fn cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation = token;
        self
    }

    /// How long a running attempt may keep going after cancellation before it is dropped.
    #[must_use]
    pub fn cancel_grace(mut self, grace: Duration) -> Self {
        self.cancel_grace = grace;
        self
    }

    /// The per-run retry budget's starting state.
    #[must_use]
    pub fn retry_budget(mut self, budget: RetryBudget) -> Self {
        self.retry_budget = Some(budget);
        self
    }

    /// Retries are limited only by each step's retry policy.
    #[must_use]
    pub fn no_retry_budget(mut self) -> Self {
        self.retry_budget = None;
        self
    }

    /// The breaker registry. Share one across runs so samples accumulate. Without one, no breaker applies.
    #[must_use]
    pub fn breakers(mut self, registry: Arc<BreakerRegistry>) -> Self {
        self.breakers = Some(registry);
        self
    }

    /// At most `limit` steps whose [`crate::ExecStep::concurrency_key`] is `key` run at once, within the mode's
    /// overall cap. A later call for the same key replaces the limit. A limit for a key no step uses is ignored, and a
    /// key without a limit is unlimited.
    ///
    /// A step whose key is at its limit waits without holding back other ready steps. When a slot frees, the waiting
    /// steps compete with every other ready step by declaration order, so the earliest-declared dispatchable step goes
    /// first and dispatch stays deterministic. A step holds its slot from dispatch until it ends, including precheck,
    /// retries and the backoff between them. Limits apply within one run, not across runs.
    #[must_use]
    pub fn concurrency_limit(mut self, key: impl Into<String>, limit: NonZeroUsize) -> Self {
        self.concurrency_limits.insert(key.into(), limit);
        self
    }

    /// The step observer. Replaces any observer set before, including one from [`ExecOptions::durable_starts`], which
    /// turns durable starts off.
    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn StepObserver>) -> Self {
        self.observer = Some(observer);
        self.durable = None;
        self
    }

    /// Makes `observer` the step observer and runs a step only once its [`DurableObserver::record_start`] has saved
    /// the in-flight marker. If that returns `Err` or panics, the step ends `NotRun(StartNotRecorded)` without being
    /// invoked, its dependents are skipped and other branches carry on; resume runs it again. Replaces any observer
    /// set before.
    ///
    /// This closes the crash gap: a step that ran always has a saved marker, so a record rebuilt with
    /// [`RunRecord::from_parts`] after a crash shows it `Running` (verify, then resume), never `Pending`.
    #[must_use]
    pub fn durable_starts(mut self, observer: Arc<dyn DurableObserver>) -> Self {
        self.observer = Some(Arc::clone(&observer) as Arc<dyn StepObserver>);
        self.durable = Some(observer);
        self
    }
}

/// What the caller found when checking a step whose outcome was unknown.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// The effect happened: resume treats the step as `CompletedEarlier`.
    Applied,
    /// The effect did not happen: resume runs the step again.
    NotApplied,
}

/// Options for [`resume`]: the execution options plus the caller's verdicts on steps whose outcome was unknown.
#[derive(Debug, Clone, Default)]
pub struct ResumeOptions {
    pub(crate) exec: ExecOptions,
    pub(crate) verified: BTreeMap<StepId, Verification>,
}

impl ResumeOptions {
    /// Resume with these execution options and no verdicts.
    #[must_use]
    pub fn new(exec: ExecOptions) -> Self {
        Self { exec, verified: BTreeMap::new() }
    }

    /// Records what the caller found for a step awaiting verification in the prior record
    /// ([`StepRecord::awaits_verification`]): `OutcomeUnknown`, `Panicked`, `Running`, or `Failed` with an error whose
    /// effect is unknown. A verdict takes precedence over the step's own precheck.
    #[must_use]
    pub fn verified(mut self, step_id: StepId, verification: Verification) -> Self {
        self.verified.insert(step_id, verification);
        self
    }
}

/// Why [`resume`] refused to run, or [`RunRecord::from_parts`] refused to rebuild a record.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResumeError {
    /// The plan's structure differs from the plan the record was made with. Start a new run instead.
    #[error("the plan changed since the run started: record fingerprint {record}, plan fingerprint {plan}")]
    FingerprintMismatch {
        /// The record's fingerprint.
        record: PlanFingerprint,
        /// The plan's fingerprint.
        plan: PlanFingerprint,
    },
    /// The record has a step the plan does not.
    #[error("the record has step {step_id}, which the plan does not")]
    UnknownStep {
        /// The step id.
        step_id: StepId,
    },
    /// A verdict was given for a step whose outcome is not in doubt.
    #[error("step {step_id} is {status}, not awaiting verification")]
    NotAwaitingVerification {
        /// The step id.
        step_id: StepId,
        /// Its status label in the prior record, or `missing`.
        status: &'static str,
    },
}

impl From<ResumeError> for michi_core::DomainError {
    fn from(err: ResumeError) -> Self {
        let code = match err {
            ResumeError::FingerprintMismatch { .. } => michi_core::ErrorCode::Conflict,
            ResumeError::UnknownStep { .. } | ResumeError::NotAwaitingVerification { .. } => {
                michi_core::ErrorCode::InvalidInput
            }
        };
        michi_core::DomainError::new(code, err.to_string()).retryable(false)
    }
}

/// Runs `plan` under `run_id` and returns what happened to every step. Never fails: a run that happened is a record.
///
/// Steps run on a `JoinSet` owned by this call, so none outlives it; dropping the returned future aborts them.
pub async fn execute(plan: &Plan, run_id: RunId, opts: &ExecOptions) -> RunRecord {
    let initial = (0..plan.len()).map(|_| Initial::Run).collect();
    scheduler::run(plan, run_id, opts, initial).await
}

/// Continues the run in `prior` under its run id, so idempotency keys stay the same.
///
/// Refuses, before running anything, a plan whose fingerprint differs from the record's, a record naming a step the
/// plan lacks, and a verdict for a step that is not awaiting verification. Then, per step:
///
/// - `Completed` and `CompletedEarlier` steps become `CompletedEarlier` and are not run.
/// - `Skipped`, `NotRun`, `Pending` and missing steps run again: they never took effect.
/// - `Failed` steps run again if their final error is [`crate::Effect::NotApplied`] or the step is
///   [`crate::Replay::Idempotent`]. Otherwise (a `Failed` whose effect is unknown, only in records saved before 0.5)
///   they are held like `OutcomeUnknown`.
/// - `OutcomeUnknown`, `Panicked` and `Running` steps, and held `Failed` ones: a caller's verdict decides
///   ([`Verification::Applied`] makes them `CompletedEarlier`, [`Verification::NotApplied`] runs them). Without one,
///   an idempotent step runs again, a [`crate::Replay::Prechecked`] step runs and its precheck decides, and any other
///   step keeps its prior record (a `Running` one becomes `OutcomeUnknown(Interrupted)`, a held `Failed` one
///   `OutcomeUnknown(StepFailed)`) and its dependents are skipped.
///
/// At least once per step: a step can run again if the process stopped after its effect but before its record was
/// saved, unless the run used [`ExecOptions::durable_starts`]. Not incremental recomputation: a completed step is not
/// re-run because its inputs changed.
///
/// # Errors
///
/// [`ResumeError::FingerprintMismatch`], [`ResumeError::UnknownStep`] or [`ResumeError::NotAwaitingVerification`],
/// returned before any step runs.
pub async fn resume(prior: &RunRecord, plan: &Plan, opts: &ResumeOptions) -> Result<RunRecord, ResumeError> {
    if prior.plan_fingerprint() != plan.fingerprint() {
        return Err(ResumeError::FingerprintMismatch { record: prior.plan_fingerprint(), plan: plan.fingerprint() });
    }
    if let Some(unknown) = prior.steps().iter().find(|s| plan.index_of(s.step_id()).is_none()) {
        return Err(ResumeError::UnknownStep { step_id: unknown.step_id().clone() });
    }
    // One index instead of a linear `prior.step()` per plan step and per verdict, which made resume O(N²). Every way
    // to build a record (execute, from_parts, deserializing) lists each step once.
    let mut by_id: BTreeMap<&StepId, &StepRecord> = BTreeMap::new();
    for record in prior.steps() {
        by_id.entry(record.step_id()).or_insert(record);
    }
    for step_id in opts.verified.keys() {
        let prev = by_id.get(step_id);
        if !prev.is_some_and(|r| r.awaits_verification()) {
            return Err(ResumeError::NotAwaitingVerification {
                step_id: step_id.clone(),
                status: prev.map_or("missing", |r| r.status().label()),
            });
        }
    }
    let initial = plan
        .steps()
        .iter()
        .map(|step| {
            let Some(&prev) = by_id.get(step.id()) else { return Initial::Run };
            let replay = step.runner().replay();
            match prev.status() {
                StepStatus::Completed | StepStatus::CompletedEarlier => {
                    Initial::Done(Box::new(carried_over(prev, "completed earlier in this run")))
                }
                StepStatus::Failed if !prev.awaits_verification() => Initial::Run,
                StepStatus::Failed | StepStatus::OutcomeUnknown(_) | StepStatus::Panicked | StepStatus::Running => {
                    match opts.verified.get(step.id()) {
                        Some(Verification::Applied) => {
                            Initial::Done(Box::new(carried_over(prev, "verified as applied by the caller")))
                        }
                        Some(Verification::NotApplied) => Initial::Run,
                        None if matches!(replay, Replay::Idempotent | Replay::Prechecked) => Initial::Run,
                        None => Initial::Done(Box::new(kept_unresolved(prev))),
                    }
                }
                // Never invoked, or never took effect.
                StepStatus::Skipped(_) | StepStatus::NotRun(_) | StepStatus::Pending => Initial::Run,
                // A status this version doesn't know (`StepStatus` is non-exhaustive): don't guess it is safe to run.
                _ => Initial::Done(Box::new(kept_unresolved(prev))),
            }
        })
        .collect();
    Ok(scheduler::run(plan, prior.run_id().clone(), &opts.exec, initial).await)
}

/// A prior success, carried into the new record as `CompletedEarlier` with its attempts.
fn carried_over(prev: &StepRecord, reason: &str) -> StepRecord {
    let mut record = StepRecord::new(prev.step_id().clone(), prev.name().to_string(), StepStatus::CompletedEarlier);
    record.reason = Some(reason.to_string());
    record.attempts = prev.attempts().to_vec();
    record
}

/// A prior outcome still in doubt, kept as it was (timing cleared). A step left `Running` by a crash becomes
/// `OutcomeUnknown(Interrupted)`, and a held `Failed` step `OutcomeUnknown(StepFailed)`, so the new record names the
/// right recovery (verify, then resume) instead of one resume would not follow.
fn kept_unresolved(prev: &StepRecord) -> StepRecord {
    let mut record = prev.clone();
    record.started_at = None;
    record.elapsed = None;
    match record.status {
        StepStatus::Running => {
            record.status = StepStatus::OutcomeUnknown(UnknownCause::Interrupted);
            record.error = Some(ExecutionError::AmbiguousOutcome {
                source: Box::new(ExecutionError::Interrupted { elapsed_ms: 0 }),
            });
            record.reason = Some(
                "in flight when the previous process stopped; verify whether it took effect, then resume".to_string(),
            );
        }
        StepStatus::Failed => {
            record.status = StepStatus::OutcomeUnknown(UnknownCause::StepFailed);
            let source = record.error.take().unwrap_or_else(|| ExecutionError::failed("the step failed"));
            record.reason = Some(format!(
                "{source}; the failure does not say whether the effect landed, so verify it, then resume"
            ));
            record.error = Some(ExecutionError::AmbiguousOutcome { source: Box::new(source) });
        }
        _ => {}
    }
    record
}

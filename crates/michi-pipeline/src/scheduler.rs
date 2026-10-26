//! The scheduler loop (one for both modes) and the per-step retry loop.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use michi_core::pipeline::{NotRunReason, SkipReason, StepStatus, UnknownCause};
use michi_core::resilience::{next_retry_delay, RetryBudget, RetryConfig};
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::breaker::AdmissionGuard;
use crate::breaker::CircuitBreaker;
use crate::error::millis;
use crate::exec::{ExecOptions, FailurePolicy, Mode, StepObserver};
use crate::plan::Fnv;
use crate::{
    AttemptBound, AttemptRecord, CancellationToken, Effect, ExecutionError, Plan, Precheck, Replay, ReplayClass, RunId,
    RunRecord, Step, StepContext, StepId, StepRecord,
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
    bound: AttemptBound,
    run_id: RunId,
    step_id: StepId,
    token: CancellationToken,
    retry: RetryConfig,
    retry_after_cap: Duration,
    attempt_timeout: Duration,
    deadline: Option<Duration>,
    /// When the step's deadline group's window ends, and the group's key.
    group_deadline: Option<(Instant, String)>,
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
    /// The concurrency slot it holds, released when it ends.
    slot: Option<usize>,
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
    /// Per step: the concurrency slot of its key, if that key has a limit.
    slot_of: Vec<Option<usize>>,
    /// One per limited key that some step uses.
    slots: Vec<Slot>,
    /// Per step: its deadline group's window, if that group has one configured.
    window_of: Vec<Option<usize>>,
    /// One per deadline group with a window that some step uses.
    windows: Vec<Window>,
    completion_order: Vec<StepId>,
    stopping: bool,
    observer_panics: u32,
}

/// A concurrency key with a limit: how many of its steps run, and which ready ones wait for room, by declaration
/// index.
struct Slot {
    limit: usize,
    in_flight: usize,
    waiting: BTreeSet<usize>,
}

/// A deadline group's window. It opens when the group's first step is dispatched; `ends` is `None` until then, or
/// when its end is too far away for the clock to represent.
struct Window {
    key: String,
    length: Duration,
    opened: bool,
    ends: Option<Instant>,
}

impl Window {
    /// Whether the window opened and has already ended at `now`.
    fn closed_at(&self, now: Instant) -> bool {
        self.ends.is_some_and(|end| now >= end)
    }
}

/// Resolves each step's key (picked by `key_of`) against the options' per-key settings in `config`, making one entry
/// per key that both a step and the settings name. Steps whose key has no setting get `None`, and with no settings
/// at all no step does, so the scheduler takes its plain path.
fn resolve_keys<V, T>(
    plan: &Plan,
    config: &std::collections::BTreeMap<String, V>,
    key_of: impl Fn(&crate::ExecStep) -> Option<&str>,
    mut make: impl FnMut(&str, &V) -> T,
) -> (Vec<Option<usize>>, Vec<T>) {
    let mut entries = Vec::new();
    if config.is_empty() {
        return (vec![None; plan.len()], entries);
    }
    let mut index: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let of = plan
        .steps()
        .iter()
        .map(|step| {
            let key = key_of(step)?;
            let value = config.get(key)?;
            Some(*index.entry(key).or_insert_with(|| {
                entries.push(make(key, value));
                entries.len() - 1
            }))
        })
        .collect();
    (of, entries)
}

/// What woke the scheduler loop.
enum Event {
    Joined(Option<Result<(tokio::task::Id, Outcome), tokio::task::JoinError>>),
    Cancelled,
}

impl<'a> Scheduler<'a> {
    fn new(plan: &'a Plan, run_id: RunId, opts: &'a ExecOptions) -> Self {
        let n = plan.len();
        let (slot_of, slots) =
            resolve_keys(plan, &opts.concurrency_limits, crate::ExecStep::concurrency_key_of, |_, limit| Slot {
                limit: limit.get(),
                in_flight: 0,
                waiting: BTreeSet::new(),
            });
        let (window_of, windows) =
            resolve_keys(plan, &opts.group_deadlines, crate::ExecStep::deadline_group_of, |key, length| Window {
                key: key.to_string(),
                length: *length,
                opened: false,
                ends: None,
            });
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
            slot_of,
            slots,
            window_of,
            windows,
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
        for i in self.plan.topological_order() {
            if self.records[i].is_some() {
                continue;
            }
            let mut waiting = 0;
            let mut blocked_by = None;
            for p in self.plan.prerequisites_of(i) {
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
                let Some(i) = self.next_dispatchable() else { break };
                if self.window_closed(i) {
                    self.deadline_passed(i);
                    continue;
                }
                if let Some(durable) = self.opts.durable.as_deref() {
                    if let Err(reason) = self.record_start(durable, i) {
                        self.start_not_recorded(i, &reason);
                        continue;
                    }
                } else {
                    let opts = self.opts;
                    let step_id = self.plan.steps()[i].id();
                    if observe(opts, |o| o.on_step_started(&self.run_id, step_id)) {
                        self.observer_panics = self.observer_panics.saturating_add(1);
                    }
                }
                if let Some(slot) = self.slot_of[i] {
                    self.slots[slot].in_flight += 1;
                }
                if let Some(w) = self.window_of[i] {
                    let window = &mut self.windows[w];
                    if !window.opened {
                        window.opened = true;
                        // A window too long for the clock to represent is no deadline, rather than an overflow panic.
                        window.ends = Instant::now().checked_add(window.length);
                    }
                }
                let (job, entry) = self.job(i);
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
                        self.release(entry.slot);
                        self.step_finished(entry, outcome);
                    }
                }
                Event::Joined(Some(Err(join_err))) => {
                    if let Some(entry) = running.remove(&join_err.id()) {
                        self.release(entry.slot);
                        let outcome = harness_outcome(join_err, entry.replay);
                        self.step_finished(entry, outcome);
                    }
                }
            }
        }
    }

    /// The next step to dispatch: the earliest-declared ready step whose concurrency key has room. A ready step whose
    /// key is full moves to its key's waiting set, so it holds back neither the global cap nor other keys; it comes
    /// back into contention, still by declaration index, once its key has room. With no limited keys this is the
    /// first ready step.
    fn next_dispatchable(&mut self) -> Option<usize> {
        loop {
            // The earliest ready step, or a waiting step of a key with room if one was declared earlier.
            let mut best = self.ready.first().map(|&i| (i, None));
            for (s, slot) in self.slots.iter().enumerate() {
                if slot.in_flight < slot.limit {
                    if let Some(&w) = slot.waiting.first() {
                        if best.is_none_or(|(b, _)| w < b) {
                            best = Some((w, Some(s)));
                        }
                    }
                }
            }
            let (i, waiting_in) = best?;
            if let Some(s) = waiting_in {
                self.slots[s].waiting.remove(&i);
                return Some(i);
            }
            self.ready.remove(&i);
            match self.slot_of[i] {
                // A step whose window has closed is returned rather than parked, so it fails now.
                Some(s) if self.slots[s].in_flight >= self.slots[s].limit && !self.window_closed(i) => {
                    self.slots[s].waiting.insert(i);
                }
                _ => return Some(i),
            }
        }
    }

    /// Whether step `i`'s deadline group window has opened and ended.
    fn window_closed(&self, i: usize) -> bool {
        self.window_of[i].is_some_and(|w| self.windows[w].closed_at(Instant::now()))
    }

    /// Step `i`'s deadline group window ended before it could be dispatched: it ends `Failed` with
    /// [`ExecutionError::DeadlinePassed`] without being invoked, and counts as a failure (its dependents are skipped,
    /// and fail-fast stops the run).
    fn deadline_passed(&mut self, i: usize) {
        let Some(w) = self.window_of[i] else { return };
        let window = &self.windows[w];
        let message = format!(
            "the {}ms window of deadline group {} ended before the step started",
            millis(window.length),
            window.key
        );
        let step = &self.plan.steps()[i];
        let mut record = StepRecord::new(step.id().clone(), step.name().to_string(), StepStatus::Failed);
        record.reason = Some(format!("{message}, so it was not run"));
        record.error = Some(ExecutionError::DeadlinePassed { message });
        self.failed(i, record);
    }

    /// Gives back a finished step's concurrency slot.
    fn release(&mut self, slot: Option<usize>) {
        if let Some(s) = slot {
            self.slots[s].in_flight = self.slots[s].in_flight.saturating_sub(1);
        }
    }

    /// With durable starts: asks the observer to save step `i`'s in-flight marker. `Err` says why the step must not
    /// run: the observer refused or panicked.
    fn record_start(&mut self, observer: &dyn crate::DurableObserver, i: usize) -> Result<(), String> {
        let step_id = self.plan.steps()[i].id();
        // AssertUnwindSafe: as in `observe`, the scheduler's own state is untouched by the observer.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.record_start(&self.run_id, step_id))) {
            Ok(result) => result,
            Err(payload) => {
                self.observer_panics = self.observer_panics.saturating_add(1);
                Err(format!("record_start panicked: {}", panic_message(payload.as_ref())))
            }
        }
    }

    /// Step `i`'s start could not be recorded, so it was not dispatched: it ends `NotRun(StartNotRecorded)` and
    /// counts as a failure (its dependents are skipped, and fail-fast stops the run).
    fn start_not_recorded(&mut self, i: usize, reason: &str) {
        let step = &self.plan.steps()[i];
        let mut record = StepRecord::new(
            step.id().clone(),
            step.name().to_string(),
            StepStatus::NotRun(NotRunReason::StartNotRecorded),
        );
        record.reason = Some(format!("its start could not be recorded, so it was not run: {reason}"));
        self.failed(i, record);
    }

    /// Builds the job for step `i` and the scheduler's bookkeeping for it.
    fn job(&self, i: usize) -> (Job, Running) {
        let step = &self.plan.steps()[i];
        let attempts: Attempts = Arc::default();
        let replay = step.replay();
        let breaker = self.opts.breakers.as_ref().map(|registry| {
            let key = step.effective_breaker_key().to_string();
            let breaker = registry.breaker(&key);
            (key, breaker)
        });
        let attempt_timeout = step.attempt_timeout_override().unwrap_or(self.opts.attempt_timeout);
        let job = Job {
            runner: Arc::clone(step.runner()),
            replay,
            bound: step.attempt_bound(),
            run_id: self.run_id.clone(),
            step_id: step.id().clone(),
            token: self.run_token.child_token(),
            retry: step.retry_override().unwrap_or(&self.opts.retry).clone(),
            retry_after_cap: self.opts.retry_after_cap,
            attempt_timeout: if attempt_timeout.is_zero() { Duration::from_millis(1) } else { attempt_timeout },
            deadline: step.deadline_override().or(self.opts.step_deadline),
            group_deadline: self.window_of[i].and_then(|w| {
                let window = &self.windows[w];
                window.ends.map(|end| (end, window.key.clone()))
            }),
            jitter_seed: self.opts.jitter_seed,
            breaker,
            budget: self.budget.clone(),
            grace: self.opts.cancel_grace,
            attempts: Arc::clone(&attempts),
        };
        (job, Running { index: i, slot: self.slot_of[i], dispatched: Instant::now(), attempts, replay })
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
            for d in self.plan.dependents_of(i) {
                if self.records[d].is_none() && self.remaining[d] > 0 {
                    self.remaining[d] -= 1;
                    if self.remaining[d] == 0 {
                        self.ready.insert(d);
                    }
                }
            }
            return;
        }
        self.failed(i, record);
    }

    /// Records step `i`'s unsuccessful terminal record, skips its dependents and applies the failure policy.
    fn failed(&mut self, i: usize, record: StepRecord) {
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
            for d in self.plan.dependents_of(j) {
                if self.records[d].is_none() {
                    self.ready.remove(&d);
                    if let Some(s) = self.slot_of[d] {
                        self.slots[s].waiting.remove(&d);
                    }
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

    fn set_terminal(&mut self, i: usize, mut record: StepRecord) {
        record.replay = self.plan.steps().get(i).map(|s| s.replay());
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
        let error = ExecutionError::Panicked { message: panic_message(payload.as_ref()) };
        return Outcome { status: StepStatus::Panicked, reason: Some(error.to_string()), error: Some(error) };
    }
    classify(ExecutionError::Interrupted { elapsed_ms: 0 }, replay, "task aborted", true)
}

/// A panic payload as text, if it was a string.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

/// Whether the executor may run the step again after `err`: the error must be retryable, and one whose effect is
/// unknown only for a step that is idempotent or prechecked (its precheck runs before the retry).
fn may_reinvoke(replay: Replay, err: &ExecutionError) -> bool {
    err.is_retryable()
        && (err.effect() == Effect::NotApplied || matches!(replay, Replay::Idempotent | Replay::Prechecked))
}

/// What a precheck decided, with timeouts and errors folded in.
enum Checked {
    Applied,
    NotApplied,
    /// Can't tell, and why.
    Unknown(String),
    /// The precheck returned [`ExecutionError::refused`]: a blocking precondition that fails the step.
    Refused(ExecutionError),
}

/// One precheck under the attempt's time limit, racing cancellation plus the grace period like an attempt.
async fn precheck_once(
    step: &dyn Step,
    ctx: &StepContext,
    limit: Duration,
    token: &CancellationToken,
    grace: Duration,
) -> Checked {
    let check = tokio::time::timeout(limit, step.precheck(ctx));
    let interrupted = async {
        token.cancelled().await;
        tokio::time::sleep(grace).await;
    };
    let result = tokio::select! {
        biased;
        result = check => result.unwrap_or(Err(ExecutionError::Timeout { elapsed_ms: millis(limit) })),
        () = interrupted => return Checked::Unknown("the precheck was still running after cancellation".to_string()),
    };
    match result {
        Ok(Precheck::Applied) => Checked::Applied,
        Ok(Precheck::NotApplied) => Checked::NotApplied,
        Ok(_) => Checked::Unknown("the precheck answered unknown".to_string()),
        Err(err) if err.replay_class() == ReplayClass::Refused => Checked::Refused(err),
        Err(err) => Checked::Unknown(format!("the precheck failed: {err}")),
    }
}

/// Reports a failed step's last error to its breaker, if it holds an admission.
///
/// Healthy: a client error (the downstream answered and rejected this request, so it is up) and
/// [`ExecutionError::refused`] (the step rejected the work before any call, so it says nothing about the downstream).
/// Neither: cancellation, interruption and a passed deadline. Failure: everything else, including
/// [`ExecutionError::rejected`] (a dependency turned the request away) and [`ExecutionError::failed`] (the step can't
/// say what happened).
fn report_failure(guard: Option<AdmissionGuard>, err: &ExecutionError) {
    if let Some(guard) = guard {
        match err.innermost() {
            // Cancellation and a passed deadline say nothing about the downstream's health: release a probe, report
            // nothing.
            ExecutionError::Cancelled | ExecutionError::Interrupted { .. } | ExecutionError::DeadlinePassed { .. } => {}
            // The downstream answered and rejected the request itself: it is up, so this is not a failure.
            ExecutionError::Http { status, .. } if is_client_error(*status) => guard.complete(true),
            // The step refused the work itself, before reaching the downstream.
            ExecutionError::Failed { retryable: false, effect: Effect::NotApplied, .. } => guard.complete(true),
            _ => guard.complete(false),
        }
    }
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
    // A deadline too far away for the clock to represent is no deadline, rather than an overflow panic. With a
    // deadline group as well, the earlier end applies, and the reason names the one that did.
    let own = job.deadline.and_then(|d| started.checked_add(d));
    let (deadline, deadline_name) = match (own, &job.group_deadline) {
        (Some(own), Some((group, _))) if own <= *group => (Some(own), "step deadline".to_string()),
        (_, Some((group, key))) => (Some(*group), format!("deadline group {key}")),
        (own, None) => (own, "step deadline".to_string()),
    };
    let limit_from = |now: Instant| match deadline {
        Some(d) => job.attempt_timeout.min(d.saturating_duration_since(now)).max(Duration::from_millis(1)),
        None => job.attempt_timeout,
    };
    // A deadline-bound step's attempt runs until its deadline, not the attempt timeout; without a deadline it falls
    // back to the timeout.
    let attempt_limit_from = |now: Instant| match (job.bound, deadline) {
        (AttemptBound::Deadline, Some(d)) => d.saturating_duration_since(now).max(Duration::from_millis(1)),
        _ => limit_from(now),
    };
    let mut attempt = 0u32;
    // The last failed attempt's error, which decides the outcome when a later precheck cannot run the step again.
    let mut prior: Option<ExecutionError> = None;
    let (err, stop) = loop {
        let ctx =
            StepContext::for_attempt(job.run_id.clone(), job.step_id.clone(), attempt, job.token.clone(), deadline);
        if job.replay == Replay::Prechecked {
            let now = Instant::now();
            let limit = limit_from(now);
            let ctx = ctx.clone().bounded(now.checked_add(limit), job.attempt_timeout);
            let checked = precheck_once(job.runner.as_ref(), &ctx, limit, &job.token, job.grace).await;
            let not_runnable = match checked {
                Checked::NotApplied if !job.token.is_cancelled() => None,
                Checked::Applied => {
                    if let Some(err) = &prior {
                        report_failure(guard, err);
                    }
                    return Outcome {
                        status: StepStatus::CompletedEarlier,
                        error: None,
                        reason: Some("already applied (precheck)".to_string()),
                    };
                }
                Checked::NotApplied => Some("run cancelled".to_string()),
                Checked::Unknown(why) => Some(why),
                Checked::Refused(refusal) => match prior {
                    // An earlier attempt may have taken effect: the refusal cannot clear that doubt.
                    Some(err) if err.effect() == Effect::Unknown => {
                        break (err, format!("the precheck refused to continue: {refusal}"));
                    }
                    _ => {
                        let reason = format!("precheck: {refusal}");
                        return Outcome { status: StepStatus::Failed, error: Some(refusal), reason: Some(reason) };
                    }
                },
            };
            if let Some(why) = not_runnable {
                match prior {
                    Some(err) => break (err, format!("{why}, so the step was not tried again after this attempt")),
                    None if job.token.is_cancelled() => {
                        return Outcome {
                            status: StepStatus::Skipped(SkipReason::Cancelled),
                            error: None,
                            reason: Some("run cancelled before the step started".to_string()),
                        };
                    }
                    None => {
                        return Outcome {
                            status: StepStatus::NotRun(NotRunReason::Unverified),
                            error: None,
                            reason: Some(format!(
                                "{why}: whether the effect is already in place is unknown, so the step was not run"
                            )),
                        };
                    }
                }
            }
        }
        let attempt_start = Instant::now();
        let limit = attempt_limit_from(attempt_start);
        let ctx = ctx.bounded(attempt_start.checked_add(limit), job.attempt_timeout);
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
            break (err, "run cancelled".to_string());
        }
        if !may_reinvoke(job.replay, &err) {
            break (err, String::new());
        }
        if probe {
            break (err, "a half-open breaker allows one probe attempt".to_string());
        }
        // A server's hint is honoured past `max_delay` but not without limit: a day-long `Retry-After` would park the
        // run. The step ends with this error as it is; resume runs it again if it is safe to.
        if let Some(hint) = err.retry_after().filter(|hint| *hint > job.retry_after_cap) {
            let stop = format!(
                "the server's retry hint of {}ms exceeded the retry-after cap of {}ms, so it was not waited for",
                millis(hint),
                millis(job.retry_after_cap)
            );
            break (err, stop);
        }
        let seed = job.jitter_seed.unwrap_or_else(|| derived_jitter_seed(&job.run_id, &job.step_id, attempt));
        let Some(delay) = retry_delay(&job.retry, attempt, seed, err.retry_after()) else {
            break (err, "retries exhausted".to_string());
        };
        if deadline.is_some_and(|d| Instant::now().checked_add(delay).is_none_or(|wake| wake >= d)) {
            break (err, format!("{deadline_name} reached"));
        }
        if let Some(budget) = &job.budget {
            if !lock(budget).try_withdraw() {
                break (err, "retry budget exhausted".to_string());
            }
        }
        let cancelled = tokio::select! {
            () = tokio::time::sleep(delay) => false,
            () = job.token.cancelled() => true,
        };
        if cancelled {
            break (err, "run cancelled during backoff".to_string());
        }
        attempt += 1;
        prior = Some(err);
    };
    report_failure(guard, &err);
    classify(err, job.replay, &stop, job.token.is_cancelled())
}

/// Whether an HTTP status is the caller's own mistake (4xx other than 408 Request Timeout and 429 Too Many Requests):
/// proof the downstream is up and answering, so it never counts as a breaker failure. 408 and 429 say the downstream
/// is slow or overloaded, and 5xx says it failed, so those do count.
fn is_client_error(status: u16) -> bool {
    (400..500).contains(&status) && status != 408 && status != 429
}

/// The backoff before retry `attempt + 1`: michi-core's [`next_retry_delay`], except that a server's `retry_after` is
/// honoured in full even past `RetryConfig::max_delay`, which caps only the exponential backoff. The step's deadline
/// (its own or its group's) still bounds it: the caller stops retrying if the wait would end past the deadline.
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
/// step's cancellation token had fired. A step that returns [`ExecutionError::Cancelled`] while the token had not
/// fired stopped on its own, so it ends `Failed`: a root cause that resume runs again, not a `Skipped(Cancelled)`
/// consequence.
///
/// An error whose effect is unknown ends a step that is not idempotent `OutcomeUnknown`, wrapped in
/// [`ExecutionError::AmbiguousOutcome`], so resume holds it (or, for a prechecked step, runs its precheck) instead of
/// repeating an effect that may have landed. An idempotent step ends `Failed` with the error as it is.
fn classify(err: ExecutionError, replay: Replay, stop: &str, cancelled: bool) -> Outcome {
    let mut stop = stop;
    let (status, error) = match err.replay_class() {
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
        _ if err.effect() == Effect::Unknown && replay != Replay::Idempotent => {
            let cause = match err.innermost() {
                ExecutionError::Http { status: 502 | 504, .. } => UnknownCause::Gateway,
                ExecutionError::Interrupted { .. } => UnknownCause::Cancelled,
                ExecutionError::Panicked { .. } => UnknownCause::Interrupted,
                ExecutionError::Timeout { .. } => UnknownCause::Timeout,
                _ => UnknownCause::StepFailed,
            };
            let error = match err {
                already @ ExecutionError::AmbiguousOutcome { .. } => already,
                panicked @ ExecutionError::Panicked { .. } => panicked,
                other => ExecutionError::AmbiguousOutcome { source: Box::new(other) },
            };
            (StepStatus::OutcomeUnknown(cause), error)
        }
        _ => (StepStatus::Failed, err),
    };
    let reason = if stop.is_empty() { error.to_string() } else { format!("{error}; {stop}") };
    Outcome { status, error: Some(error), reason: Some(reason) }
}

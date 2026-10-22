//! Shared helpers for the integration tests: ids, a scripted step and an event log on Tokio's paused clock.

#![allow(dead_code)] // each test binary uses a different subset

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use michi_core::resilience::RetryConfig;
use michi_pipeline::{
    ExecStep, ExecutionError, Plan, Precheck, PrecheckFuture, Replay, RunId, RunRecord, Step, StepContext, StepFuture,
    StepId, StepStatus,
};

pub fn sid(s: &str) -> StepId {
    StepId::try_new(s).unwrap()
}

pub fn rid(s: &str) -> RunId {
    RunId::try_new(s).unwrap()
}

pub fn http(status: u16) -> ExecutionError {
    ExecutionError::Http { status, message: format!("status {status}"), retry_after: None }
}

/// A failure that certainly had no effect (`refused`): the step ends `Failed` and resume runs it again.
pub fn fatal(msg: &str) -> ExecutionError {
    ExecutionError::refused(msg)
}

/// A downstream failure the breaker counts (`failed`: not retryable, effect unknown). A step that is not idempotent
/// ends `OutcomeUnknown` with it.
pub fn down(msg: &str) -> ExecutionError {
    ExecutionError::failed(msg)
}

/// `max_retries` retries with a fixed `delay_ms` and no jitter.
pub fn retries(max_retries: u32, delay_ms: u64) -> RetryConfig {
    RetryConfig::try_new(max_retries, Duration::from_millis(delay_ms), Duration::from_millis(delay_ms.max(1)), 0.0)
        .unwrap()
}

/// Events with their virtual time in ms since the log was created, plus an in-flight high-water mark.
#[derive(Clone)]
pub struct Log {
    start: tokio::time::Instant,
    events: Arc<Mutex<Vec<(u64, String)>>>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
}

impl Log {
    pub fn new() -> Self {
        Self {
            start: tokio::time::Instant::now(),
            events: Arc::default(),
            in_flight: Arc::default(),
            max_in_flight: Arc::default(),
        }
    }

    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap()
    }

    pub fn push(&self, event: impl Into<String>) {
        let at = self.now_ms();
        self.events.lock().unwrap().push((at, event.into()));
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().iter().map(|(_, e)| e.clone()).collect()
    }

    pub fn timed(&self) -> Vec<(u64, String)> {
        self.events.lock().unwrap().clone()
    }

    /// Step ids in the order their attempts started.
    pub fn starts(&self) -> Vec<String> {
        self.events().into_iter().filter_map(|e| e.strip_prefix("start:").map(str::to_string)).collect()
    }

    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }

    fn enter(&self) {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
    }

    fn exit(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What one attempt does.
#[derive(Clone, Debug)]
pub enum Act {
    Ok,
    Err(ExecutionError),
    Panic,
    /// Never finishes on its own and ignores cancellation.
    Hang,
    /// Waits for the step's cancellation token, then returns `Cancelled`.
    Cooperative,
    /// Sleeps, then does the inner act.
    After(u64, Box<Act>),
}

pub fn after(ms: u64, act: Act) -> Act {
    Act::After(ms, Box::new(act))
}

/// A step that performs `acts[attempt]` (the last act repeats), logging `start:<id>` and `end:<id>`, counting calls
/// and capturing every idempotency key it saw.
pub struct Scripted {
    id: String,
    acts: Vec<Act>,
    replay: Replay,
    log: Log,
    checks: Vec<Result<Precheck, ExecutionError>>,
    pub calls: Arc<AtomicU32>,
    pub check_calls: Arc<AtomicU32>,
    pub keys: Arc<Mutex<Vec<String>>>,
}

impl Scripted {
    pub fn new(log: &Log, id: &str, acts: Vec<Act>) -> Self {
        Self {
            id: id.to_string(),
            acts,
            replay: Replay::NotIdempotent,
            log: log.clone(),
            checks: Vec::new(),
            calls: Arc::default(),
            check_calls: Arc::default(),
            keys: Arc::default(),
        }
    }

    pub fn idempotent(mut self) -> Self {
        self.replay = Replay::Idempotent;
        self
    }

    /// Makes the step `Replay::Prechecked`; its precheck answers `checks[n]` on its n-th call (the last repeats) and
    /// logs `check:<id>`.
    pub fn prechecked(mut self, checks: Vec<Result<Precheck, ExecutionError>>) -> Self {
        self.replay = Replay::Prechecked;
        self.checks = checks;
        self
    }
}

struct InFlight(Log);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.exit();
    }
}

fn perform(act: Act, ctx: StepContext) -> StepFuture<'static> {
    Box::pin(async move {
        match act {
            Act::Ok => Ok(()),
            Act::Err(e) => Err(e),
            Act::Panic => panic!("scripted panic"),
            Act::Hang => std::future::pending().await,
            Act::Cooperative => {
                ctx.cancellation().cancelled().await;
                Err(ExecutionError::Cancelled)
            }
            Act::After(ms, inner) => {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                perform(*inner, ctx).await
            }
        }
    })
}

impl Step for Scripted {
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
        self.keys.lock().unwrap().push(ctx.idempotency_key().as_str().to_string());
        let act = self.acts[n.min(self.acts.len() - 1)].clone();
        let log = self.log.clone();
        let id = self.id.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            log.push(format!("start:{id}"));
            log.enter();
            let guard = InFlight(log.clone());
            let result = perform(act, ctx).await;
            drop(guard);
            log.push(format!("end:{id}"));
            result
        })
    }

    fn replay(&self) -> Replay {
        self.replay
    }

    fn precheck<'a>(&'a self, _ctx: &'a StepContext) -> PrecheckFuture<'a> {
        let n = self.check_calls.fetch_add(1, Ordering::SeqCst) as usize;
        self.log.push(format!("check:{}", self.id));
        let answer = self.checks[n.min(self.checks.len() - 1)].clone();
        Box::pin(async move { answer })
    }
}

/// An `ExecStep` running `runner` after `deps`.
pub fn exec(id: &str, deps: &[&str], runner: impl Step + 'static) -> ExecStep {
    deps.iter().fold(ExecStep::new(sid(id), format!("Step {id}"), runner), |s, d| s.depends_on(sid(d)))
}

/// Shorthand: a scripted step with a single act, after `deps`.
pub fn act(log: &Log, id: &str, deps: &[&str], act: Act) -> ExecStep {
    exec(id, deps, Scripted::new(log, id, vec![act]))
}

pub fn plan(steps: Vec<ExecStep>) -> Plan {
    Plan::new(steps).unwrap()
}

pub fn status(record: &RunRecord, id: &str) -> StepStatus {
    record.step(&sid(id)).unwrap_or_else(|| panic!("no step {id}")).status()
}

pub fn statuses(record: &RunRecord) -> Vec<(String, StepStatus)> {
    record.steps().iter().map(|s| (s.step_id().to_string(), s.status())).collect()
}

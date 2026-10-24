//! A step's `replay()` and `attempt_bound()` are caller code. They are read once when the `ExecStep` is built, so a
//! panic in them happens at plan construction, before anything runs, and never mid-run where it would lose the record
//! of an effect that already happened.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use common::*;
use michi_pipeline::{
    execute, AttemptBound, ExecOptions, ExecStep, Replay, RunOutcome, Step, StepContext, StepFuture, StepStatus,
};

/// A step whose `replay` and `attempt_bound` answer once and then panic.
struct Flaky {
    replay_calls: Arc<AtomicU32>,
    bound_calls: Arc<AtomicU32>,
    runs: Arc<AtomicU32>,
}

impl Flaky {
    fn new() -> Self {
        Self { replay_calls: Arc::default(), bound_calls: Arc::default(), runs: Arc::default() }
    }
}

impl Step for Flaky {
    fn run<'a>(&'a self, _ctx: &'a StepContext) -> StepFuture<'a> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }

    fn replay(&self) -> Replay {
        assert!(self.replay_calls.fetch_add(1, Ordering::SeqCst) == 0, "replay() read again after construction");
        Replay::Idempotent
    }

    fn attempt_bound(&self) -> AttemptBound {
        assert!(self.bound_calls.fetch_add(1, Ordering::SeqCst) == 0, "attempt_bound() read again after construction");
        AttemptBound::Timeout
    }
}

/// A step whose `replay` always panics.
struct BrokenReplay;

impl Step for BrokenReplay {
    fn run<'a>(&'a self, _ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn replay(&self) -> Replay {
        panic!("replay() is broken")
    }
}

#[tokio::test(start_paused = true)]
async fn a_step_whose_replay_panics_fails_at_construction_not_mid_run() {
    let built = catch_unwind(AssertUnwindSafe(|| ExecStep::new(sid("a"), "A", BrokenReplay)));
    assert!(built.is_err(), "the panic belongs to the caller's plan-building code, before anything runs");
    let built = catch_unwind(AssertUnwindSafe(|| ExecStep::from_arc(sid("a"), "A", Arc::new(BrokenReplay))));
    assert!(built.is_err());
}

#[tokio::test(start_paused = true)]
async fn replay_and_attempt_bound_are_read_once_so_a_run_never_calls_them_again() {
    let flaky = Flaky::new();
    let (replay_calls, bound_calls, runs) =
        (Arc::clone(&flaky.replay_calls), Arc::clone(&flaky.bound_calls), Arc::clone(&flaky.runs));
    let p = plan(vec![exec("a", &[], flaky)]);
    assert_eq!(replay_calls.load(Ordering::SeqCst), 1);
    assert_eq!(bound_calls.load(Ordering::SeqCst), 1);

    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&record, "a"), StepStatus::Completed);
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(replay_calls.load(Ordering::SeqCst), 1, "the run used the value read at construction");
    assert_eq!(bound_calls.load(Ordering::SeqCst), 1);
    assert_eq!(record.step(&sid("a")).unwrap().replay(), Some(Replay::Idempotent));
}

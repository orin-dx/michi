//! What happens to a step while its start marker is being saved.
//!
//! The save runs on a blocking thread at the top of the step's own task, so the run can change under it: another step
//! can fail (fail-fast), the caller can cancel, a deadline group's window can end. In every case the step must end the
//! way it would have if the change had landed just before its dispatch, and `Step::run` must not be called.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use michi_pipeline::{
    execute, step_fn, CancellationToken, DurableObserver, ExecOptions, ExecStep, ExecutionError, FailurePolicy, Mode,
    Plan, RunId, RunRecord, SkipReason, StepId, StepObserver, StepRecord, StepStatus,
};

const SLOW_SAVE: Duration = Duration::from_millis(200);

fn id(s: &str) -> StepId {
    StepId::try_new(s).expect("valid id")
}

/// Saves instantly, except for the step named `slow`, whose save blocks for `SLOW_SAVE`.
struct Store {
    slow: &'static str,
}

impl StepObserver for Store {
    fn on_step_finished(&self, _: &RunId, _: &StepRecord) {}
}

impl DurableObserver for Store {
    fn record_start(&self, _: &RunId, step_id: &StepId) -> Result<(), String> {
        if step_id.as_str() == self.slow {
            std::thread::sleep(SLOW_SAVE);
        }
        Ok(())
    }
}

/// A step that notes in `ran` that it was invoked.
fn probe(name: &str, ran: &Arc<AtomicBool>) -> ExecStep {
    let ran = Arc::clone(ran);
    ExecStep::new(
        id(name),
        "step",
        step_fn(move |_| {
            ran.store(true, Ordering::SeqCst);
            async { Ok(()) }
        }),
    )
}

fn options(slow: &'static str) -> ExecOptions {
    ExecOptions::new()
        .mode(Mode::Parallel { max_concurrency: NonZeroUsize::new(4).expect("nonzero") })
        .durable_starts(Arc::new(Store { slow }))
}

fn run(plan: &Plan, opts: &ExecOptions) -> RunRecord {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
    rt.block_on(execute(plan, RunId::try_new("races").expect("valid id"), opts))
}

fn status(record: &RunRecord, name: &str) -> StepStatus {
    record.steps().iter().find(|s| s.step_id().as_str() == name).map(StepRecord::status).expect("step in record")
}

#[test]
fn fail_fast_during_the_save_skips_the_step_without_running_it() {
    let ran = Arc::new(AtomicBool::new(false));
    let bad = ExecStep::new(id("bad"), "step", step_fn(|_| async { Err(ExecutionError::refused("no")) }));
    let plan = Plan::new(vec![bad, probe("victim", &ran)]).expect("plan");
    let record = run(&plan, &options("victim").failure_policy(FailurePolicy::FailFast));

    assert_eq!(status(&record, "bad"), StepStatus::Failed);
    assert_eq!(status(&record, "victim"), StepStatus::Skipped(SkipReason::Cancelled));
    assert!(!ran.load(Ordering::SeqCst), "a step whose run was cancelled during its save was still invoked");
}

#[test]
fn an_outside_cancel_during_the_save_skips_the_step_without_running_it() {
    let ran = Arc::new(AtomicBool::new(false));
    let plan = Plan::new(vec![probe("victim", &ran)]).expect("plan");
    let token = CancellationToken::new();
    let opts = options("victim").cancellation(token.clone());
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
    let record = rt.block_on(async {
        // Lands well inside the 200 ms save.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token.cancel();
        });
        execute(&plan, RunId::try_new("races").expect("valid id"), &opts).await
    });

    assert_eq!(status(&record, "victim"), StepStatus::Skipped(SkipReason::Cancelled));
    assert!(!ran.load(Ordering::SeqCst), "a step whose run was cancelled during its save was still invoked");
}

#[test]
fn a_group_window_that_ends_during_the_save_fails_the_step_without_running_it() {
    // The window opens at dispatch, so the save counts against it: a 200 ms save under a 100 ms window leaves nothing.
    let ran = Arc::new(AtomicBool::new(false));
    let plan = Plan::new(vec![probe("victim", &ran).deadline_group("g")]).expect("plan");
    let record = run(&plan, &options("victim").group_deadline("g", Duration::from_millis(100)));

    assert_eq!(status(&record, "victim"), StepStatus::Failed);
    let step = record.steps().iter().find(|s| s.step_id().as_str() == "victim").expect("step");
    assert!(matches!(step.error(), Some(ExecutionError::DeadlinePassed { .. })), "got {:?}", step.error());
    assert!(!ran.load(Ordering::SeqCst), "a step whose window ended during its save was still invoked");
}

#[test]
fn a_group_window_that_outlasts_the_save_still_runs_the_step() {
    let ran = Arc::new(AtomicBool::new(false));
    let plan = Plan::new(vec![probe("victim", &ran).deadline_group("g")]).expect("plan");
    let record = run(&plan, &options("victim").group_deadline("g", Duration::from_secs(30)));

    assert_eq!(status(&record, "victim"), StepStatus::Completed);
    assert!(ran.load(Ordering::SeqCst));
}

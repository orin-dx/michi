//! Shared deadlines: `ExecStep::deadline_group` with `ExecOptions::group_deadline`. All on Tokio's paused clock.

mod common;

use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, resume, step_fn, Effect, ExecOptions, ExecStep, ExecutionError, FailurePolicy, Mode, ReplayClass,
    ResumeOptions, RunOutcome, SkipReason, StepStatus,
};

fn parallel(cap: usize) -> Mode {
    Mode::Parallel { max_concurrency: NonZeroUsize::new(cap).unwrap() }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn started(record: &michi_pipeline::RunRecord, id: &str) -> Option<u128> {
    record.step(&sid(id)).unwrap().started_at().map(|d| d.as_millis())
}

fn elapsed(record: &michi_pipeline::RunRecord, id: &str) -> Option<u128> {
    record.step(&sid(id)).unwrap().elapsed().map(|d| d.as_millis())
}

/// An idempotent scripted step in deadline group `group`.
fn member(log: &Log, id: &str, deps: &[&str], group: &str, act: Act) -> ExecStep {
    exec(id, deps, Scripted::new(log, id, vec![act]).idempotent()).deadline_group(group)
}

#[tokio::test(start_paused = true)]
async fn members_started_at_different_times_expire_together() {
    // Three publishes finish at 10, 50 and 100 ms; each verify waits forever. The window opens when the first verify
    // is dispatched (10 ms), so all three verifies are cut at 310 ms, whatever time each started.
    let log = Log::new();
    let p = plan(vec![
        act(&log, "p1", &[], after(10, Act::Ok)),
        act(&log, "p2", &[], after(50, Act::Ok)),
        act(&log, "p3", &[], after(100, Act::Ok)),
        member(&log, "v1", &["p1"], "verify", Act::Hang),
        member(&log, "v2", &["p2"], "verify", Act::Hang),
        member(&log, "v3", &["p3"], "verify", Act::Hang),
    ]);
    let opts = ExecOptions::new().mode(parallel(8)).group_deadline("verify", ms(300)).no_retry_budget();
    let record = execute(&p, rid("r"), &opts).await;
    for (id, start) in [("v1", 10), ("v2", 50), ("v3", 100)] {
        assert_eq!(started(&record, id), Some(start), "{id}");
        assert_eq!(elapsed(&record, id), Some(310 - start), "{id} ends with the shared window");
        assert_eq!(status(&record, id), StepStatus::Failed, "{id}: an idempotent step cut by a deadline fails");
    }
    assert_eq!(record.elapsed(), ms(310));
}

#[tokio::test(start_paused = true)]
async fn a_member_ready_after_the_window_closed_is_not_started() {
    let log = Log::new();
    let late = Scripted::new(&log, "late", vec![Act::Ok]);
    let late_calls = Arc::clone(&late.calls);
    let p = plan(vec![
        member(&log, "early", &[], "verify", after(10, Act::Ok)),
        act(&log, "slow", &[], after(200, Act::Ok)),
        exec("late", &["slow"], late).deadline_group("verify"),
        act(&log, "after-late", &["late"], Act::Ok),
    ]);
    let opts = ExecOptions::new().mode(parallel(4)).group_deadline("verify", ms(100));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "early"), StepStatus::Completed);
    assert_eq!(status(&record, "late"), StepStatus::Failed);
    assert_eq!(late_calls.load(Ordering::SeqCst), 0, "late was never invoked");
    assert!(!log.starts().contains(&"late".to_string()));
    let late_record = record.step(&sid("late")).unwrap();
    assert!(late_record.attempts().is_empty());
    assert_eq!(late_record.started_at(), None);
    let err = late_record.error().unwrap();
    assert!(matches!(err, ExecutionError::DeadlinePassed { .. }), "{err:?}");
    assert_eq!(err.effect(), Effect::NotApplied, "nothing ran, so nothing was applied");
    assert!(!err.is_retryable(), "the window is closed for this run");
    assert_eq!(err.replay_class(), ReplayClass::Refused);
    let reason = late_record.reason().unwrap();
    assert!(reason.contains("deadline group verify") && reason.contains("not run"), "{reason}");
    assert_eq!(status(&record, "after-late"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert_eq!(record.root_causes().map(|s| s.step_id().as_str()).collect::<Vec<_>>(), ["late"]);
    assert!(record.needs_verification().next().is_none(), "nothing ran, so nothing needs verifying");

    // Resume opens a new window, so the step runs.
    let resumed = resume(&record, &p, &ResumeOptions::new(opts.clone())).await.unwrap();
    assert_eq!(status(&resumed, "late"), StepStatus::Completed);
    assert_eq!(status(&resumed, "after-late"), StepStatus::Completed);
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn the_earlier_of_the_step_deadline_and_the_group_deadline_wins() {
    let log = Log::new();
    let p = plan(vec![
        member(&log, "own-first", &[], "g", Act::Hang).deadline(ms(50)),
        member(&log, "group-first", &[], "g", Act::Hang).deadline(ms(1_000)),
    ]);
    let opts = ExecOptions::new().mode(parallel(4)).group_deadline("g", ms(100)).no_retry_budget();
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(elapsed(&record, "own-first"), Some(50));
    assert_eq!(elapsed(&record, "group-first"), Some(100));
}

#[tokio::test(start_paused = true)]
async fn the_context_reports_the_effective_deadline() {
    let seen = Arc::new(Mutex::new(None));
    let p = plan(vec![{
        let seen = Arc::clone(&seen);
        ExecStep::new(
            sid("s"),
            "S",
            step_fn(move |ctx| {
                let seen = Arc::clone(&seen);
                async move {
                    *seen.lock().unwrap() = ctx.remaining();
                    Ok(())
                }
            }),
        )
        .deadline(ms(500))
        .deadline_group("g")
    }]);
    let opts = ExecOptions::new().group_deadline("g", ms(200));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(*seen.lock().unwrap(), Some(ms(200)));
}

#[tokio::test(start_paused = true)]
async fn groups_without_a_window_have_no_deadline_and_unused_windows_are_ignored() {
    let log = Log::new();
    let p = plan(vec![member(&log, "a", &[], "unbounded", after(500, Act::Ok))]);
    let opts = ExecOptions::new().group_deadline("other", ms(1)).attempt_timeout(ms(1_000));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::Completed);
}

#[tokio::test(start_paused = true)]
async fn a_window_too_long_for_the_clock_is_no_deadline() {
    let log = Log::new();
    let p = plan(vec![member(&log, "a", &[], "g", after(500, Act::Ok)), member(&log, "b", &["a"], "g", Act::Ok)]);
    let opts = ExecOptions::new().group_deadline("g", Duration::MAX).attempt_timeout(ms(1_000));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn retries_stop_when_the_backoff_would_outlast_the_group_window() {
    let log = Log::new();
    let p = plan(vec![exec("s", &[], Scripted::new(&log, "s", vec![Act::Err(ExecutionError::rejected("busy"))]))
        .deadline_group("g")
        .retry(retries(5, 100))]);
    let opts = ExecOptions::new().group_deadline("g", ms(150)).no_retry_budget();
    let record = execute(&p, rid("r"), &opts).await;
    let s = record.step(&sid("s")).unwrap();
    assert_eq!(s.attempts().len(), 2, "attempts at 0 and 100 ms; the next would start past 150 ms");
    assert!(s.reason().unwrap().ends_with("deadline group g reached"), "{:?}", s.reason());
}

#[tokio::test(start_paused = true)]
async fn a_step_waiting_for_its_concurrency_key_past_the_window_is_not_started() {
    let log = Log::new();
    let p = plan(vec![
        member(&log, "a", &[], "g", Act::Hang).concurrency_key("k"),
        member(&log, "b", &[], "g", Act::Ok).concurrency_key("k"),
    ]);
    let opts = ExecOptions::new()
        .mode(parallel(4))
        .concurrency_limit("k", NonZeroUsize::new(1).unwrap())
        .group_deadline("g", ms(50))
        .no_retry_budget();
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(elapsed(&record, "a"), Some(50));
    assert_eq!(status(&record, "b"), StepStatus::Failed);
    assert_eq!(log.starts(), ["a"], "b got the slot only after the window closed, so it did not start");
}

#[tokio::test(start_paused = true)]
async fn under_fail_fast_a_closed_window_stops_the_run() {
    let log = Log::new();
    let p = plan(vec![
        member(&log, "early", &[], "g", after(10, Act::Ok)),
        act(&log, "slow", &[], after(100, Act::Ok)),
        member(&log, "late", &["slow"], "g", Act::Ok),
        act(&log, "other", &["slow"], after(10, Act::Ok)),
    ]);
    let opts = ExecOptions::new().mode(parallel(1)).group_deadline("g", ms(50)).failure_policy(FailurePolicy::FailFast);
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "late"), StepStatus::Failed);
    assert_eq!(status(&record, "other"), StepStatus::Skipped(SkipReason::Cancelled));
}

#[test]
fn options_debug_lists_the_windows() {
    let opts = ExecOptions::new().group_deadline("verify", ms(300));
    assert!(format!("{opts:?}").contains("group_deadlines: {\"verify\": 300ms}"), "{opts:?}");
}

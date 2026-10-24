//! Regression tests for the 0.4 hardening pass: records that cannot read as a false success, rebuilt records that
//! keep the latest write per step, a root cause for every failed run, overflow-safe deadlines, server retry hints
//! honoured past `max_delay`, per-step jitter and client errors that do not trip a breaker.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, resume, BreakerConfig, BreakerRegistry, ExecOptions, ExecutionError, NotRunReason, ResumeError,
    ResumeOptions, RunId, RunOutcome, RunRecord, StepId, StepObserver, StepRecord, StepStatus,
};

// ---- 1: a record missing plan steps is never a success ----

#[tokio::test(start_paused = true)]
async fn a_rebuilt_record_with_missing_steps_lists_them_as_pending_and_is_not_a_success() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);

    let empty = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, vec![]).unwrap();
    assert_eq!(empty.steps().len(), 2, "one record per plan step");
    assert_eq!(statuses(&empty), [("a".into(), StepStatus::Pending), ("b".into(), StepStatus::Pending)]);
    assert_ne!(empty.outcome(), RunOutcome::Succeeded);
    assert!(empty.error().is_some(), "a run that is not a success always has an error to report");

    let full = execute(&p, rid("r"), &ExecOptions::new()).await;
    let only_a = vec![full.step(&sid("a")).unwrap().clone()];
    let partial = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, only_a).unwrap();
    assert_eq!(status(&partial, "b"), StepStatus::Pending);
    assert_eq!(partial.outcome(), RunOutcome::Failed);
    let resumed = resume(&partial, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(statuses(&resumed), [("a".into(), StepStatus::CompletedEarlier), ("b".into(), StepStatus::Completed)]);
}

#[test]
fn from_parts_rejects_a_changed_plan_and_a_foreign_step() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok)]);
    let other = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &[], Act::Ok)]);
    assert_eq!(
        RunRecord::from_parts(rid("r"), other.fingerprint(), &p, vec![]).unwrap_err(),
        ResumeError::FingerprintMismatch { record: other.fingerprint(), plan: p.fingerprint() }
    );
    assert_eq!(
        RunRecord::from_parts(rid("r"), p.fingerprint(), &p, vec![StepRecord::in_flight(sid("zzz"), "Z")]).unwrap_err(),
        ResumeError::UnknownStep { step_id: sid("zzz") }
    );
}

#[cfg(feature = "serde")]
#[tokio::test(start_paused = true)]
async fn a_serialized_record_must_list_every_plan_step_once() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(serde_json::from_value::<RunRecord>(json.clone()).unwrap(), record, "a complete record round-trips");

    let mut no_steps = json.clone();
    no_steps["steps"] = serde_json::json!([]);
    let err = serde_json::from_value::<RunRecord>(no_steps).unwrap_err().to_string();
    assert!(err.contains("lists 0 steps but its plan has 2"), "{err}");

    let mut twice = json.clone();
    let first = twice["steps"][0].clone();
    twice["steps"][1] = first;
    let err = serde_json::from_value::<RunRecord>(twice).unwrap_err().to_string();
    assert!(err.contains("step a is listed twice"), "{err}");

    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, vec![]).unwrap();
    let back: RunRecord = serde_json::from_value(serde_json::to_value(&rebuilt).unwrap()).unwrap();
    assert_eq!(back, rebuilt, "a rebuilt record with pending steps round-trips");
    assert_eq!(back.outcome(), RunOutcome::Failed);
}

// ---- 2: the documented observer pattern rebuilds the latest status per step ----

/// Persists exactly what the observer docs suggest, appending only: an in-flight record on start, the finished
/// record on finish.
#[derive(Default)]
struct AppendOnly(Mutex<Vec<StepRecord>>);

impl StepObserver for AppendOnly {
    fn on_step_started(&self, _: &RunId, step_id: &StepId) {
        self.0.lock().unwrap().push(StepRecord::in_flight(step_id.clone(), "in flight"));
    }
    fn on_step_finished(&self, _: &RunId, record: &StepRecord) {
        self.0.lock().unwrap().push(record.clone());
    }
}

#[tokio::test(start_paused = true)]
async fn a_record_rebuilt_from_append_only_observer_saves_keeps_the_last_write_per_step() {
    let log = Log::new();
    let a = Scripted::new(&log, "a", vec![Act::Ok]);
    let calls = Arc::clone(&a.calls);
    let p = plan(vec![exec("a", &[], a)]);
    let store = Arc::new(AppendOnly::default());
    let run = execute(&p, rid("r"), &ExecOptions::new().observer(store.clone())).await;
    assert_eq!(run.outcome(), RunOutcome::Succeeded);

    let saved = store.0.lock().unwrap().clone();
    assert_eq!(saved.iter().map(StepRecord::status).collect::<Vec<_>>(), [StepStatus::Running, StepStatus::Completed]);
    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, saved).unwrap();
    assert_eq!(rebuilt.steps().len(), 1);
    assert_eq!(status(&rebuilt, "a"), StepStatus::Completed, "the finished record replaces the in-flight one");
    assert_eq!(rebuilt.outcome(), RunOutcome::Succeeded);

    let resumed = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(status(&resumed, "a"), StepStatus::CompletedEarlier);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "never re-run");
}

// ---- 3: needs_verification lists every step resume holds for verification ----

#[tokio::test(start_paused = true)]
async fn needs_verification_lists_panicked_and_running_steps_and_running_is_a_root_cause() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Panic), act(&log, "b", &[], Act::Ok)]);
    let run = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(run.needs_verification().map(|s| s.step_id().to_string()).collect::<Vec<_>>(), ["a"]);

    let crashed =
        RunRecord::from_parts(rid("r"), p.fingerprint(), &p, vec![StepRecord::in_flight(sid("a"), "A")]).unwrap();
    assert_eq!(crashed.needs_verification().map(|s| s.step_id().to_string()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(crashed.root_causes().map(|s| s.step_id().to_string()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(crashed.outcome(), RunOutcome::OutcomeUnknown, "a step left running may have taken effect");
    let err = crashed.error().expect("a failed run names its cause");
    assert!(matches!(&err, ExecutionError::StepFailed { step_id, .. } if step_id == "a"), "{err:?}");
}

// ---- 4: a step that cancels itself while the run is not cancelled is a root cause ----

#[tokio::test(start_paused = true)]
async fn a_step_returning_cancelled_outside_a_cancelled_run_fails_as_a_root_cause() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Err(ExecutionError::Cancelled)), act(&log, "b", &["a"], Act::Ok)]);
    let run = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert!(!run.was_cancelled());
    assert_eq!(status(&run, "a"), StepStatus::Failed);
    assert_eq!(run.step(&sid("b")).unwrap().caused_by(), Some(&sid("a")));
    assert_eq!(run.root_causes().map(|s| s.step_id().to_string()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(run.outcome(), RunOutcome::Failed);
    assert!(run.error().is_some());
    let resumed = resume(&run, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(log.starts().iter().filter(|s| *s == "a").count(), 2, "it never took effect, so resume runs it again");
    assert_eq!(status(&resumed, "a"), StepStatus::Failed);
}

// ---- 5: a huge deadline means no deadline, not a panic ----

#[tokio::test(start_paused = true)]
async fn a_huge_step_deadline_does_not_overflow() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok)]);
    let run = execute(&p, rid("r"), &ExecOptions::new().step_deadline(Duration::MAX)).await;
    assert_eq!(status(&run, "a"), StepStatus::Completed, "{:?}", run.steps()[0].reason());

    let p = plan(vec![act(&log, "b", &[], Act::Ok).deadline(Duration::from_secs(u64::MAX / 2))]);
    let run = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&run, "b"), StepStatus::Completed, "{:?}", run.steps()[0].reason());

    // Huge retry hints with a huge deadline: the retry waits the hint instead of panicking. (The hint cap is lifted
    // so the huge hint reaches the deadline arithmetic; by default it ends the step without waiting.)
    let p = plan(vec![exec(
        "c",
        &[],
        Scripted::new(&log, "c", vec![Act::Err(ExecutionError::CircuitOpen { retry_after_ms: u64::MAX }), Act::Ok]),
    )
    .deadline(Duration::MAX)]);
    let opts = ExecOptions::new().retry_after_cap(Duration::MAX);
    let run = tokio::time::timeout(Duration::from_secs(3600), execute(&p, rid("r"), &opts)).await;
    assert!(run.is_err(), "the step is still waiting out the server's hint, not panicked");
}

// ---- 12: a server's retry hint is honoured past RetryConfig::max_delay ----

#[tokio::test(start_paused = true)]
async fn a_retry_after_longer_than_max_delay_is_honoured() {
    let log = Log::new();
    let step =
        Scripted::new(&log, "s", vec![Act::Err(ExecutionError::CircuitOpen { retry_after_ms: 120_000 }), Act::Ok]);
    let p = plan(vec![exec("s", &[], step)]);
    let run = execute(&p, rid("r"), &ExecOptions::new().retry(retries(1, 10))).await;
    assert_eq!(status(&run, "s"), StepStatus::Completed);
    let timed = log.timed();
    let starts: Vec<u64> = timed.iter().filter(|(_, e)| e == "start:s").map(|(t, _)| *t).collect();
    assert_eq!(starts, [0, 120_000], "waited the full hint, not max_delay (10 ms)");

    let http =
        ExecutionError::Http { status: 429, message: "slow down".into(), retry_after: Some(Duration::from_secs(45)) };
    let log = Log::new();
    let p = plan(vec![exec("t", &[], Scripted::new(&log, "t", vec![Act::Err(http), Act::Ok]))]);
    let run = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&run, "t"), StepStatus::Completed);
    assert_eq!(run.elapsed(), Duration::from_secs(45), "default max_delay is 30 s; the server asked for 45 s");
}

#[tokio::test(start_paused = true)]
async fn a_retry_after_past_the_step_deadline_stops_retrying_and_says_so() {
    let log = Log::new();
    let step =
        Scripted::new(&log, "s", vec![Act::Err(ExecutionError::CircuitOpen { retry_after_ms: 120_000 }), Act::Ok]);
    let p = plan(vec![exec("s", &[], step).deadline(Duration::from_secs(60))]);
    let run = execute(&p, rid("r"), &ExecOptions::new().retry(retries(3, 10))).await;
    let s = run.step(&sid("s")).unwrap();
    assert_eq!(s.status(), StepStatus::NotRun(NotRunReason::CircuitOpen));
    assert_eq!(s.attempts().len(), 1, "no point sleeping past the deadline");
    assert!(s.reason().unwrap().contains("step deadline reached"), "{:?}", s.reason());
    assert!(run.elapsed() < Duration::from_secs(1), "it did not wait");
}

// ---- 20: jitter differs per step and attempt unless the caller fixes the seed ----

async fn retry_offsets(run: &str) -> Vec<u64> {
    let log = Log::new();
    let retry =
        michi_core::resilience::RetryConfig::try_new(1, Duration::from_millis(1000), Duration::from_secs(10), 1.0)
            .unwrap();
    let p = plan(
        ["a", "b", "c", "d"]
            .iter()
            .map(|id| exec(id, &[], Scripted::new(&log, id, vec![Act::Err(http(503)), Act::Ok])))
            .collect(),
    );
    let opts = ExecOptions::new()
        .retry(retry)
        .mode(michi_pipeline::Mode::Parallel { max_concurrency: std::num::NonZeroUsize::new(4).unwrap() });
    let record = execute(&p, rid(run), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    let mut second_starts: Vec<u64> = Vec::new();
    for id in ["a", "b", "c", "d"] {
        let label = format!("start:{id}");
        let t = log.timed().into_iter().filter(|(_, e)| *e == label).map(|(t, _)| t).nth(1).unwrap();
        second_starts.push(t);
    }
    second_starts
}

#[tokio::test(start_paused = true)]
async fn default_jitter_differs_per_step_but_is_reproducible_per_run() {
    let first = retry_offsets("r").await;
    let mut distinct = first.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(distinct.len() > 1, "steps must not retry in lockstep: {first:?}");
    assert!(first.iter().all(|t| (1000..=2000).contains(t)), "within base delay plus 100% jitter: {first:?}");
    assert_eq!(retry_offsets("r").await, first, "the same run id and steps give the same delays");
    assert_ne!(retry_offsets("other-run").await, first, "another run decorrelates");
}

// ---- 19: client errors do not trip the breaker ----

#[tokio::test(start_paused = true)]
async fn client_errors_do_not_count_as_breaker_failures() {
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(30))));
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Err(http(404)))]);
    for i in 0..4 {
        let run = execute(&p, rid(&format!("r{i}")), &ExecOptions::new().breakers(Arc::clone(&registry))).await;
        assert_eq!(status(&run, "a"), StepStatus::Failed, "run {i}: a 404 says the downstream is up");
    }
    assert_eq!(registry.breaker("a").phase(), michi_pipeline::BreakerPhase::Closed);

    for status_code in [408, 429, 500] {
        let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(30))));
        let p =
            plan(
                vec![exec("b", &[], Scripted::new(&log, "b", vec![Act::Err(http(status_code))])).retry(retries(0, 1))],
            );
        let opts = ExecOptions::new().breakers(Arc::clone(&registry));
        let _ = execute(&p, rid("x0"), &opts).await;
        let _ = execute(&p, rid("x1"), &opts).await;
        let third = execute(&p, rid("x2"), &opts).await;
        assert_eq!(
            status(&third, "b"),
            StepStatus::NotRun(NotRunReason::CircuitOpen),
            "{status_code} is a downstream failure"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_client_error_resets_the_consecutive_failure_count() {
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(30))));
    let log = Log::new();
    let acts = vec![Act::Err(http(500)), Act::Err(http(400)), Act::Err(http(500)), Act::Ok];
    // Idempotent so a 500 ends `Failed` rather than `OutcomeUnknown`: this test is about the breaker, not replay.
    let p = plan(vec![exec("a", &[], Scripted::new(&log, "a", acts).idempotent()).retry(retries(0, 1))]);
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    for i in 0..3 {
        let run = execute(&p, rid(&format!("r{i}")), &opts).await;
        assert_eq!(status(&run, "a"), StepStatus::Failed, "run {i}");
    }
    assert_eq!(registry.breaker("a").phase(), michi_pipeline::BreakerPhase::Closed, "500, 400, 500 is not 2 in a row");
}

// ---- 0.4 API hygiene: every public type is Debug, without bounds on the caller's closure ----

#[test]
fn fn_step_is_debug_without_requiring_a_debug_closure() {
    let step = michi_pipeline::step_fn(|_ctx| async { Ok(()) }).idempotent();
    let shown = format!("{step:?}");
    assert!(shown.starts_with("FnStep"), "got: {shown}");
    assert!(shown.contains("Idempotent"), "the replay class is shown: {shown}");
}

#[tokio::test(start_paused = true)]
async fn a_refused_step_counts_as_healthy_for_the_breaker() {
    // A step that rejects its own input before any call says nothing about the downstream: three validation failures
    // on `registry-api` used to open the breaker and stop a healthy step on the same key.
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(3, Duration::from_secs(30))));
    let log = Log::new();
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    let bad = plan(vec![exec(
        "bad",
        &[],
        Scripted::new(&log, "bad", vec![Act::Err(ExecutionError::refused("invalid version"))]),
    )
    .breaker_key("registry-api")]);
    for i in 0..3 {
        let _ = execute(&bad, rid(&format!("bad{i}")), &opts).await;
    }
    let good = plan(vec![act(&log, "good", &[], Act::Ok).breaker_key("registry-api")]);
    let run = execute(&good, rid("good"), &opts).await;
    assert_eq!(status(&run, "good"), StepStatus::Completed);

    // `rejected` (a dependency turned the request away) and `failed` (unknown) still count as failures.
    for err in [ExecutionError::rejected("busy"), ExecutionError::failed("lost reply")] {
        let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(30))));
        let opts = ExecOptions::new().breakers(Arc::clone(&registry));
        let p = plan(vec![exec("b", &[], Scripted::new(&log, "b", vec![Act::Err(err.clone())])).retry(retries(0, 1))]);
        let _ = execute(&p, rid("x0"), &opts).await;
        let _ = execute(&p, rid("x1"), &opts).await;
        let third = execute(&p, rid("x2"), &opts).await;
        assert_eq!(status(&third, "b"), StepStatus::NotRun(NotRunReason::CircuitOpen), "{err:?}");
    }
}

//! A server's `Retry-After` is honoured past the backoff cap, but not without limit: a hint beyond
//! `ExecOptions::retry_after_cap` ends the step with its last error instead of parking the run for hours.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use michi_pipeline::{execute, Effect, ExecOptions, ExecutionError, ReplayClass, RunOutcome, StepStatus};

fn too_many(secs: u64) -> ExecutionError {
    ExecutionError::Http { status: 429, message: "slow down".into(), retry_after: Some(Duration::from_secs(secs)) }
}

fn opts() -> ExecOptions {
    ExecOptions::new().retry(retries(3, 10)).no_retry_budget()
}

#[tokio::test(start_paused = true)]
async fn a_retry_hint_beyond_the_cap_is_not_waited_for_and_the_step_ends_with_its_last_error() {
    let log = Log::new();
    let step = Scripted::new(&log, "a", vec![Act::Err(too_many(86_400)), Act::Ok]);
    let calls = Arc::clone(&step.calls);
    let p = plan(vec![exec("a", &[], step)]);
    let started = tokio::time::Instant::now();
    let record = execute(&p, rid("r"), &opts()).await;

    assert_eq!(calls.load(Ordering::SeqCst), 1, "no retry");
    assert!(started.elapsed() < Duration::from_secs(1), "did not wait: {:?}", started.elapsed());
    let step = record.step(&sid("a")).unwrap();
    assert_eq!(step.status(), StepStatus::Failed);
    let err = step.error().unwrap();
    assert!(matches!(err, ExecutionError::Http { status: 429, .. }), "{err:?}");
    assert_eq!(err.effect(), Effect::NotApplied, "class and effect are unchanged");
    assert_eq!(step.attempts()[0].class(), Some(ReplayClass::Rejected));
    let reason = step.reason().unwrap();
    assert!(reason.contains("retry hint") && reason.contains("exceeded"), "{reason}");
    assert!(reason.contains("86400000ms") && reason.contains("300000ms"), "{reason}");
    assert_eq!(record.outcome(), RunOutcome::Failed);
}

#[tokio::test(start_paused = true)]
async fn a_retry_hint_within_the_default_cap_is_still_honoured_past_max_delay() {
    let log = Log::new();
    let step = Scripted::new(&log, "a", vec![Act::Err(too_many(120)), Act::Ok]);
    let calls = Arc::clone(&step.calls);
    let p = plan(vec![exec("a", &[], step)]);
    let started = tokio::time::Instant::now();
    let record = execute(&p, rid("r"), &opts()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(started.elapsed() >= Duration::from_secs(120), "{:?}", started.elapsed());
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn the_cap_is_configurable_and_a_hint_equal_to_it_is_honoured() {
    let log = Log::new();
    let at_cap = Scripted::new(&log, "a", vec![Act::Err(too_many(10)), Act::Ok]);
    let over_cap = Scripted::new(&log, "b", vec![Act::Err(too_many(11)), Act::Ok]);
    let (at_calls, over_calls) = (Arc::clone(&at_cap.calls), Arc::clone(&over_cap.calls));
    let p = plan(vec![exec("a", &[], at_cap), exec("b", &[], over_cap)]);
    let record = execute(&p, rid("r"), &opts().retry_after_cap(Duration::from_secs(10))).await;
    assert_eq!(at_calls.load(Ordering::SeqCst), 2, "a hint equal to the cap is honoured");
    assert_eq!(over_calls.load(Ordering::SeqCst), 1, "a hint above it is not");
    assert_eq!(status(&record, "a"), StepStatus::Completed);
    assert_eq!(status(&record, "b"), StepStatus::Failed);
}

#[tokio::test(start_paused = true)]
async fn raising_the_cap_restores_waiting_for_long_hints() {
    let log = Log::new();
    let step = Scripted::new(&log, "a", vec![Act::Err(too_many(86_400)), Act::Err(too_many(86_400)), Act::Ok]);
    let calls = Arc::clone(&step.calls);
    let p = plan(vec![exec("a", &[], step)]);
    let record = execute(&p, rid("r"), &opts().retry_after_cap(Duration::from_secs(86_400))).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3, "with the cap raised to the hint it is honoured as before");
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

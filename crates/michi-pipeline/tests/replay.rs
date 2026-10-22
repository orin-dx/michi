//! Change 4: the replay rule, ported from the `CircuitBreaker::call` tests onto `execute`. One classification decides
//! whether the executor retries, the terminal status and whether the error says replay is safe.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, ExecOptions, ExecutionError, NotRunReason, ReplayClass, RunRecord, SkipReason, StepStatus, UnknownCause,
};

async fn run_one(step: Scripted, opts: ExecOptions) -> (RunRecord, u32) {
    let calls = Arc::clone(&step.calls);
    let record = execute(&plan(vec![exec("s", &[], step)]), rid("r"), &opts).await;
    (record, calls.load(Ordering::SeqCst))
}

#[tokio::test(start_paused = true)]
async fn gateway_502_and_504_do_not_reinvoke_a_non_idempotent_step() {
    for status in [502, 504] {
        let log = Log::new();
        let (record, calls) =
            run_one(Scripted::new(&log, "s", vec![Act::Err(http(status))]), ExecOptions::new().retry(retries(3, 1)))
                .await;
        assert_eq!(calls, 1, "{status}: the attempt may have committed");
        let s = &record.steps()[0];
        assert_eq!(s.status(), StepStatus::OutcomeUnknown(UnknownCause::Gateway));
        let err = s.error().unwrap();
        assert!(matches!(err, ExecutionError::AmbiguousOutcome { .. }));
        assert!(!err.is_retryable());
        let domain: michi_core::DomainError = record.error().unwrap().into();
        assert!(!domain.retryable);
        assert!(domain.hints.iter().any(|h| h.0.contains("verify whether the operation completed")));
    }
}

#[tokio::test(start_paused = true)]
async fn gateway_502_and_504_still_retry_an_idempotent_step_and_stay_retryable() {
    for status in [502, 504] {
        let log = Log::new();
        let step = Scripted::new(&log, "s", vec![Act::Err(http(status))]).idempotent();
        let (record, calls) = run_one(step, ExecOptions::new().retry(retries(3, 1))).await;
        assert_eq!(calls, 4);
        let s = &record.steps()[0];
        assert_eq!(s.status(), StepStatus::Failed);
        assert_eq!(s.error(), Some(&http(status)), "returned bare");
        assert!(s.error().unwrap().is_retryable());
    }
}

#[tokio::test(start_paused = true)]
async fn rejected_429_and_503_retry_every_step() {
    for status in [429, 503] {
        let log = Log::new();
        let (record, calls) =
            run_one(Scripted::new(&log, "s", vec![Act::Err(http(status))]), ExecOptions::new().retry(retries(3, 1)))
                .await;
        assert_eq!(calls, 4, "{status} means not processed, so even a non-idempotent step retries");
        let s = &record.steps()[0];
        assert_eq!(s.status(), StepStatus::Failed);
        assert!(s.attempts().iter().all(|a| a.class() == Some(ReplayClass::Rejected)));
    }
}

#[tokio::test(start_paused = true)]
async fn a_step_returned_timeout_is_ambiguous_too() {
    let log = Log::new();
    let timeout = ExecutionError::Timeout { elapsed_ms: 9 };
    let (record, calls) =
        run_one(Scripted::new(&log, "s", vec![Act::Err(timeout.clone())]), ExecOptions::new().retry(retries(3, 1)))
            .await;
    assert_eq!(calls, 1);
    assert_eq!(record.steps()[0].status(), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    let (record, calls) = run_one(
        Scripted::new(&log, "s", vec![Act::Err(timeout.clone())]).idempotent(),
        ExecOptions::new().retry(retries(3, 1)),
    )
    .await;
    assert_eq!(calls, 4);
    assert_eq!(record.steps()[0].error(), Some(&timeout));
}

#[tokio::test(start_paused = true)]
async fn a_refused_failure_is_one_attempt_in_zero_time() {
    let log = Log::new();
    let (record, calls) =
        run_one(Scripted::new(&log, "s", vec![Act::Err(http(400))]), ExecOptions::new().retry(retries(3, 1_000))).await;
    assert_eq!(calls, 1);
    assert_eq!(record.elapsed(), Duration::ZERO);
    assert_eq!(record.steps()[0].attempts()[0].class(), Some(ReplayClass::Refused));
    assert_eq!(record.steps()[0].status(), StepStatus::Failed);
}

#[tokio::test(start_paused = true)]
async fn an_http_500_is_one_attempt_and_ends_outcome_unknown_for_a_non_idempotent_step() {
    let log = Log::new();
    let (record, calls) =
        run_one(Scripted::new(&log, "s", vec![Act::Err(http(500))]), ExecOptions::new().retry(retries(3, 1_000))).await;
    assert_eq!(calls, 1, "a 500 is not retryable");
    assert_eq!(record.steps()[0].attempts()[0].class(), Some(ReplayClass::Ambiguous));
    assert_eq!(record.steps()[0].status(), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));
    assert_eq!(record.render(), "step[1]{id,name,status,reason}:\n  s,Step s,outcome_unknown,step_failed");
}

#[tokio::test(start_paused = true)]
async fn retries_until_success_with_exponential_backoff_and_records_each_attempt() {
    let log = Log::new();
    let retry =
        michi_core::resilience::RetryConfig::try_new(3, Duration::from_millis(100), Duration::from_secs(10), 0.0)
            .unwrap();
    let step = Scripted::new(&log, "s", vec![Act::Err(http(503)), Act::Err(http(503)), Act::Ok]);
    let (record, calls) = run_one(step, ExecOptions::new().retry(retry)).await;
    assert_eq!(calls, 3);
    assert_eq!(record.elapsed(), Duration::from_millis(300), "100 ms then 200 ms of backoff");
    let classes: Vec<_> = record.steps()[0].attempts().iter().map(|a| a.class()).collect();
    assert_eq!(classes, [Some(ReplayClass::Rejected), Some(ReplayClass::Rejected), None]);
    assert_eq!(record.steps()[0].status(), StepStatus::Completed);
}

#[tokio::test(start_paused = true)]
async fn retry_after_hints_are_honoured_including_inside_step_failed() {
    let hinted = |e: ExecutionError| async move {
        let log = Log::new();
        let retry =
            michi_core::resilience::RetryConfig::try_new(1, Duration::from_millis(1), Duration::from_secs(60), 0.0)
                .unwrap();
        let (record, _) =
            run_one(Scripted::new(&log, "s", vec![Act::Err(e), Act::Ok]), ExecOptions::new().retry(retry)).await;
        record.elapsed()
    };
    let http_hint =
        ExecutionError::Http { status: 503, message: "x".into(), retry_after: Some(Duration::from_secs(7)) };
    assert_eq!(hinted(http_hint.clone()).await, Duration::from_secs(7));
    assert_eq!(
        hinted(ExecutionError::CircuitOpen { retry_after_ms: 4_000 }).await,
        Duration::from_secs(4),
        "a downstream breaker's wait is honoured"
    );
    let wrapped =
        ExecutionError::StepFailed { step_id: "inner".into(), step_name: "Inner".into(), source: Box::new(http_hint) };
    assert_eq!(hinted(wrapped).await, Duration::from_secs(7));
}

#[tokio::test(start_paused = true)]
async fn a_step_that_reports_cancelled_is_skipped_only_when_the_run_was_cancelled() {
    let log = Log::new();
    let (record, calls) =
        run_one(Scripted::new(&log, "s", vec![Act::Err(ExecutionError::Cancelled)]), ExecOptions::new()).await;
    assert_eq!(calls, 1, "not executed, but not retried either");
    let s = &record.steps()[0];
    assert_eq!(s.status(), StepStatus::Failed, "it cancelled itself: a root cause, not a consequence");
    assert_eq!(s.attempts()[0].class(), Some(ReplayClass::NotExecuted));
    assert!(s.reason().unwrap().contains("the run was not cancelled"), "{:?}", s.reason());

    let token = michi_pipeline::CancellationToken::new();
    let opts = ExecOptions::new().cancellation(token.clone());
    let step = Scripted::new(&log, "s", vec![Act::Cooperative]);
    let run = run_one(step, opts);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        token.cancel();
    };
    let ((record, _), ()) = tokio::join!(run, cancel);
    assert_eq!(record.steps()[0].status(), StepStatus::Skipped(SkipReason::Cancelled));
}

#[tokio::test(start_paused = true)]
async fn the_jitter_seed_reaches_the_backoff_math() {
    let log = Log::new();
    let retry =
        michi_core::resilience::RetryConfig::try_new(1, Duration::from_millis(100), Duration::from_secs(10), 1.0)
            .unwrap();
    let step = Scripted::new(&log, "s", vec![Act::Err(http(503)), Act::Ok]);
    let (record, _) = run_one(step, ExecOptions::new().retry(retry).jitter_seed(0.5)).await;
    assert_eq!(record.elapsed(), Duration::from_millis(150), "100 ms plus 50% jitter");
}

// ---- ported from the old `CircuitBreaker::call` and `execute_pipeline` tests ----

#[tokio::test(start_paused = true)]
async fn a_step_failed_wrapping_circuit_open_waits_its_retry_after_and_ends_not_run() {
    let retry = michi_core::resilience::RetryConfig::try_new(1, Duration::from_millis(1), Duration::from_secs(60), 0.0)
        .unwrap();
    let wrapped = ExecutionError::StepFailed {
        step_id: "inner".into(),
        step_name: "Inner".into(),
        source: Box::new(ExecutionError::CircuitOpen { retry_after_ms: 4_000 }),
    };
    let log = Log::new();
    let step = Scripted::new(&log, "s", vec![Act::Err(wrapped.clone()), Act::Ok]);
    let (record, calls) = run_one(step, ExecOptions::new().retry(retry.clone())).await;
    assert_eq!(calls, 2);
    assert_eq!(record.elapsed(), Duration::from_secs(4), "the wrapped breaker's wait is honoured");
    assert_eq!(record.steps()[0].status(), StepStatus::Completed);

    let (record, _) =
        run_one(Scripted::new(&log, "s", vec![Act::Err(wrapped.clone())]), ExecOptions::new().retry(retry)).await;
    let s = &record.steps()[0];
    assert_eq!(s.status(), StepStatus::NotRun(NotRunReason::CircuitOpen), "classified through the wrapper");
    assert_eq!(s.error(), Some(&wrapped));
}

#[tokio::test(start_paused = true)]
async fn the_jitter_seed_is_forwarded_unmodified_even_when_out_of_range() {
    let retry = michi_core::resilience::RetryConfig::try_new(2, Duration::from_millis(10), Duration::from_secs(1), 0.5)
        .unwrap();
    for seed in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 7.5, -3.0] {
        let log = Log::new();
        let step = Scripted::new(&log, "s", vec![Act::Err(http(503)), Act::Ok]);
        let (record, _) = run_one(step, ExecOptions::new().retry(retry.clone()).jitter_seed(seed)).await;
        assert_eq!(record.steps()[0].status(), StepStatus::Completed, "seed {seed}");
        let expected = michi_core::resilience::next_retry_delay(&retry, 0, seed, None).unwrap();
        assert_eq!(record.elapsed(), expected, "seed {seed}");
    }
}

#[tokio::test(start_paused = true)]
async fn every_step_uses_the_same_jitter_seed() {
    let log = Log::new();
    let retry =
        michi_core::resilience::RetryConfig::try_new(1, Duration::from_millis(10), Duration::from_millis(100), 0.5)
            .unwrap();
    let p = plan(
        ["a", "b", "c"]
            .iter()
            .map(|id| exec(id, &[], Scripted::new(&log, id, vec![Act::Err(http(503)), Act::Ok])))
            .collect(),
    );
    let seed = 0.42;
    let record = execute(&p, rid("r"), &ExecOptions::new().retry(retry.clone()).jitter_seed(seed)).await;
    assert!(record.steps().iter().all(|s| s.status() == StepStatus::Completed));
    let per_step = michi_core::resilience::next_retry_delay(&retry, 0, seed, None).unwrap();
    assert_eq!(record.elapsed(), per_step * 3, "three identical delays");
}

#[tokio::test(start_paused = true)]
async fn a_step_failed_wrapping_a_timeout_is_ambiguous_and_retried_only_if_idempotent() {
    let wrapped = ExecutionError::StepFailed {
        step_id: "inner".into(),
        step_name: "Inner".into(),
        source: Box::new(ExecutionError::Timeout { elapsed_ms: 7 }),
    };
    let log = Log::new();
    let step = Scripted::new(&log, "s", vec![Act::Err(wrapped.clone())]);
    let (record, calls) = run_one(step, ExecOptions::new().retry(retries(3, 1))).await;
    assert_eq!(calls, 1, "the wrapped timeout may have committed");
    let s = &record.steps()[0];
    assert_eq!(s.status(), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    assert_eq!(s.error(), Some(&ExecutionError::AmbiguousOutcome { source: Box::new(wrapped.clone()) }));
    assert!(!s.error().unwrap().is_retryable());

    let step = Scripted::new(&log, "s", vec![Act::Err(wrapped.clone())]).idempotent();
    let (record, calls) = run_one(step, ExecOptions::new().retry(retries(3, 1))).await;
    assert_eq!(calls, 4);
    assert_eq!(record.steps()[0].status(), StepStatus::Failed);
    assert_eq!(record.steps()[0].error(), Some(&wrapped), "an idempotent step's outcome stays bare");
}

#[tokio::test(start_paused = true)]
async fn a_retryable_failed_error_retries_a_non_idempotent_step() {
    let log = Log::new();
    let flaky = ExecutionError::rejected("flaky");
    let step = Scripted::new(&log, "s", vec![Act::Err(flaky.clone()), Act::Err(flaky), Act::Ok]);
    let (record, calls) = run_one(step, ExecOptions::new().retry(retries(5, 10))).await;
    assert_eq!(calls, 3, "the step vouched that retrying is safe");
    assert_eq!(record.steps()[0].status(), StepStatus::Completed);
    assert_eq!(record.steps()[0].attempts()[0].class(), Some(ReplayClass::Rejected));
}

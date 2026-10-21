//! `execute`: status model and run record (changes 2, 3), scheduling and failure policy (6), cancellation and time
//! (7), breakers and retry budget (8), the observer (9). All on Tokio's paused clock; assertions check dispatch order
//! and virtual time, not completion races.

mod common;

use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_core::resilience::RetryBudget;
use michi_pipeline::{
    execute, BreakerConfig, BreakerPhase, BreakerRegistry, CancellationToken, ExecOptions, ExecStep, ExecutionError,
    FailurePolicy, Mode, NotRunReason, ReplayClass, RunId, RunOutcome, SkipReason, StepObserver, StepRecord,
    StepStatus, UnknownCause,
};

fn parallel(cap: usize) -> Mode {
    Mode::Parallel { max_concurrency: NonZeroUsize::new(cap).unwrap() }
}

// ---- change 2/3: record and status model ----

#[tokio::test(start_paused = true)]
async fn all_success_run_is_succeeded_with_one_attempt_and_timing_per_step() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], after(5, Act::Ok)), act(&log, "b", &["a"], after(7, Act::Ok))]);
    let record = execute(&p, rid("run-1"), &ExecOptions::new()).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(record.run_id().as_str(), "run-1");
    assert_eq!(record.plan_fingerprint(), p.fingerprint());
    assert_eq!(statuses(&record), [("a".into(), StepStatus::Completed), ("b".into(), StepStatus::Completed)]);
    let b = record.step(&sid("b")).unwrap();
    assert_eq!(b.attempts().len(), 1);
    assert_eq!(b.attempts()[0].class(), None);
    assert_eq!(b.started_at(), Some(Duration::from_millis(5)));
    assert_eq!(b.elapsed(), Some(Duration::from_millis(7)));
    assert_eq!(record.completion_order(), &[sid("a"), sid("b")]);
    assert_eq!(record.elapsed(), Duration::from_millis(12));
    assert_eq!(record.root_causes().count(), 0);
    assert!(record.error().is_none());
}

#[tokio::test(start_paused = true)]
async fn sequential_skip_dependents_runs_independent_steps_and_separates_root_cause_from_consequences() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "a", &[], Act::Err(fatal("bad input"))),
        act(&log, "b", &["a"], Act::Ok),
        act(&log, "c", &[], Act::Ok),
        act(&log, "d", &["b"], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(
        statuses(&record),
        [
            ("a".into(), StepStatus::Failed),
            ("b".into(), StepStatus::Skipped(SkipReason::PrerequisiteFailed)),
            ("c".into(), StepStatus::Completed),
            ("d".into(), StepStatus::Skipped(SkipReason::PrerequisiteFailed)),
        ]
    );
    assert_eq!(log.starts(), ["a", "c"], "the independent step still runs after a failure");
    assert_eq!(record.outcome(), RunOutcome::Failed);
    assert_eq!(record.root_causes().map(|s| s.step_id().as_str()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(record.consequential().map(|s| s.step_id().as_str()).collect::<Vec<_>>(), ["b", "d"]);
    assert_eq!(record.step(&sid("d")).unwrap().caused_by(), Some(&sid("a")), "the transitive skip names the root");
    match record.error() {
        Some(ExecutionError::StepFailed { step_id, step_name, source }) => {
            assert_eq!((step_id.as_str(), step_name.as_str()), ("a", "Step a"));
            assert_eq!(*source, fatal("bad input"));
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
    assert_eq!(
        record.render(),
        "step[4]{id,name,status,reason}:\n  a,Step a,failed,null\n  b,Step b,skipped,prerequisite_failed\n  \
         c,Step c,completed,null\n  d,Step d,skipped,prerequisite_failed"
    );
}

#[tokio::test(start_paused = true)]
async fn a_panicking_step_is_panicked_not_failed_and_does_not_unwind_the_run() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "boom", &[], Act::Panic),
        act(&log, "after", &["boom"], Act::Ok),
        act(&log, "ok", &[], Act::Ok),
    ]);
    for mode in [Mode::Sequential, parallel(4)] {
        let record = execute(&p, rid("r"), &ExecOptions::new().mode(mode)).await;
        assert_eq!(status(&record, "boom"), StepStatus::Panicked);
        assert_eq!(status(&record, "after"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
        assert_eq!(status(&record, "ok"), StepStatus::Completed);
        let boom = record.step(&sid("boom")).unwrap();
        assert!(matches!(boom.error(), Some(ExecutionError::Panicked { message }) if message == "scripted panic"));
        assert_eq!(boom.attempts().len(), 1);
        assert_eq!(record.outcome(), RunOutcome::Failed);
    }
}

// ---- change 6: scheduling and failure policy ----

#[tokio::test(start_paused = true)]
async fn sequential_mode_dispatches_one_step_at_a_time_in_execution_order() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "c", &["a"], after(10, Act::Ok)),
        act(&log, "a", &[], after(10, Act::Ok)),
        act(&log, "b", &[], after(10, Act::Ok)),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(log.starts(), ["a", "c", "b"]);
    assert_eq!(log.starts(), p.execution_order().iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(log.max_in_flight(), 1);
    assert_eq!(record.elapsed(), Duration::from_millis(30));
}

#[tokio::test(start_paused = true)]
async fn parallel_mode_caps_in_flight_steps_and_dispatches_in_declaration_order() {
    let log = Log::new();
    let p = plan(["a", "b", "c", "d", "e"].iter().map(|id| act(&log, id, &[], after(10, Act::Ok))).collect());
    let record = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(2))).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(log.max_in_flight(), 2, "the cap holds");
    assert_eq!(log.starts(), ["a", "b", "c", "d", "e"]);
    assert_eq!(record.elapsed(), Duration::from_millis(30), "ceil(5 / 2) waves of 10 ms");
    let started: Vec<_> = record.steps().iter().map(|s| s.started_at().unwrap().as_millis()).collect();
    assert_eq!(started, [0, 0, 10, 10, 20]);
}

#[tokio::test(start_paused = true)]
async fn a_dependent_waits_for_every_prerequisite_and_newly_ready_steps_go_in_declaration_order() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "slow", &[], after(30, Act::Ok)),
        act(&log, "fast", &[], after(10, Act::Ok)),
        act(&log, "join", &["slow", "fast"], Act::Ok),
        act(&log, "x", &["fast"], Act::Ok),
        act(&log, "w", &["fast"], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(8))).await;
    assert_eq!(log.starts(), ["slow", "fast", "x", "w", "join"]);
    assert_eq!(record.step(&sid("join")).unwrap().started_at(), Some(Duration::from_millis(30)));
}

#[tokio::test(start_paused = true)]
async fn skip_dependents_in_parallel_lets_an_unrelated_in_flight_branch_finish() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "fails", &[], after(5, Act::Err(fatal("x")))),
        act(&log, "dep", &["fails"], Act::Ok),
        act(&log, "long", &[], after(50, Act::Ok)),
        act(&log, "later", &["long"], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(4))).await;
    assert_eq!(status(&record, "dep"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert_eq!(status(&record, "long"), StepStatus::Completed);
    assert_eq!(status(&record, "later"), StepStatus::Completed, "a branch that becomes ready after the failure runs");
}

#[tokio::test(start_paused = true)]
async fn fail_fast_cancels_in_flight_steps_after_the_grace_period_and_skips_unstarted_ones() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "fails", &[], after(5, Act::Err(fatal("x")))),
        act(&log, "hangs", &[], Act::Hang),
        exec("idem", &[], Scripted::new(&log, "idem", vec![Act::Hang]).idempotent()),
        act(&log, "polite", &[], Act::Cooperative),
        act(&log, "queued", &[], Act::Ok),
        act(&log, "dep", &["fails"], Act::Ok),
    ]);
    let opts = ExecOptions::new()
        .mode(parallel(4))
        .failure_policy(FailurePolicy::FailFast)
        .cancel_grace(Duration::from_millis(20));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "fails"), StepStatus::Failed);
    assert_eq!(status(&record, "hangs"), StepStatus::OutcomeUnknown(UnknownCause::Cancelled));
    assert!(matches!(
        record.step(&sid("hangs")).unwrap().error(),
        Some(ExecutionError::AmbiguousOutcome { source }) if matches!(**source, ExecutionError::Interrupted { .. })
    ));
    assert_eq!(status(&record, "idem"), StepStatus::Failed, "an idempotent step dropped mid-flight is safe to rerun");
    assert_eq!(status(&record, "polite"), StepStatus::Skipped(SkipReason::Cancelled), "stopped cooperatively");
    assert_eq!(status(&record, "queued"), StepStatus::Skipped(SkipReason::Cancelled), "never started");
    assert_eq!(status(&record, "dep"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert!(!log.starts().contains(&"queued".to_string()));
    assert_eq!(record.elapsed(), Duration::from_millis(25), "failure at 5 ms plus the 20 ms grace");
    assert_eq!(record.outcome(), RunOutcome::OutcomeUnknown, "the hanging step must be verified");
    assert!(!record.was_cancelled(), "fail-fast is not the caller's cancellation");
}

// ---- change 7: cancellation and time ----

#[tokio::test(start_paused = true)]
async fn a_precancelled_token_skips_every_step_without_invoking_any() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);
    let token = CancellationToken::new();
    token.cancel();
    let record = execute(&p, rid("r"), &ExecOptions::new().cancellation(token)).await;
    assert!(log.starts().is_empty());
    assert!(record.steps().iter().all(|s| s.status() == StepStatus::Skipped(SkipReason::Cancelled)));
    assert_eq!(record.outcome(), RunOutcome::Cancelled);
    assert!(record.was_cancelled());
}

#[tokio::test(start_paused = true)]
async fn cancelling_mid_run_lets_the_in_flight_step_finish_within_grace_and_skips_the_rest() {
    let log = Log::new();
    let p =
        plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &[], after(30, Act::Ok)), act(&log, "c", &[], Act::Ok)]);
    let token = CancellationToken::new();
    let canceller = {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            token.cancel();
        })
    };
    let record = execute(&p, rid("r"), &ExecOptions::new().cancellation(token.clone())).await;
    canceller.await.unwrap();
    assert_eq!(
        statuses(&record),
        [
            ("a".into(), StepStatus::Completed),
            ("b".into(), StepStatus::Completed),
            ("c".into(), StepStatus::Skipped(SkipReason::Cancelled)),
        ]
    );
    assert_eq!(record.outcome(), RunOutcome::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn an_attempt_still_running_after_the_grace_period_is_dropped_and_its_outcome_is_unknown() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Hang)]);
    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            token.cancel();
        });
    }
    let opts = ExecOptions::new().cancellation(token).cancel_grace(Duration::from_millis(5));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::OutcomeUnknown(UnknownCause::Cancelled));
    assert_eq!(record.elapsed(), Duration::from_millis(15));
    assert_eq!(record.outcome(), RunOutcome::OutcomeUnknown, "unknown outranks cancelled: verify first");
    assert_eq!(record.needs_verification().count(), 1);
}

#[tokio::test(start_paused = true)]
async fn backoff_sleep_races_cancellation() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Err(http(503)))]);
    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            token.cancel();
        });
    }
    let opts = ExecOptions::new().cancellation(token).retry(retries(3, 10_000));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.elapsed(), Duration::from_secs(1), "the 10 s backoff was cut short");
    let a = record.step(&sid("a")).unwrap();
    assert_eq!(a.attempts().len(), 1);
    assert_eq!(a.status(), StepStatus::Failed, "503 means not processed, so it is safe to rerun");
    assert!(a.reason().unwrap().contains("cancel"), "{:?}", a.reason());
}

#[tokio::test(start_paused = true)]
async fn per_attempt_timeout_is_ambiguous_retried_only_for_idempotent_steps() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "once", &[], after(1_000, Act::Ok)),
        exec("idem", &[], Scripted::new(&log, "idem", vec![after(1_000, Act::Ok)]).idempotent()),
    ]);
    let opts = ExecOptions::new().attempt_timeout(Duration::from_millis(10)).retry(retries(2, 1)).mode(parallel(2));
    let record = execute(&p, rid("r"), &opts).await;
    let once = record.step(&sid("once")).unwrap();
    assert_eq!(once.status(), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    assert_eq!(once.attempts().len(), 1);
    assert_eq!(once.attempts()[0].class(), Some(ReplayClass::Ambiguous));
    assert!(matches!(once.error(), Some(ExecutionError::AmbiguousOutcome { .. })));
    let idem = record.step(&sid("idem")).unwrap();
    assert_eq!(idem.status(), StepStatus::Failed);
    assert_eq!(idem.attempts().len(), 3);
    assert_eq!(idem.error(), Some(&ExecutionError::Timeout { elapsed_ms: 10 }));
}

#[tokio::test(start_paused = true)]
async fn the_step_deadline_bounds_retries_and_cuts_the_last_attempt() {
    let log = Log::new();
    let p = plan(vec![
        exec("retrying", &[], Scripted::new(&log, "retrying", vec![Act::Err(http(503))])),
        act(&log, "slow", &[], after(1_000, Act::Ok)),
    ]);
    let opts = ExecOptions::new().retry(retries(10, 100)).step_deadline(Duration::from_millis(250)).mode(parallel(2));
    let record = execute(&p, rid("r"), &opts).await;
    let retrying = record.step(&sid("retrying")).unwrap();
    assert_eq!(retrying.attempts().len(), 3, "attempts at 0, 100 and 200 ms; a fourth at 300 ms would overrun");
    assert!(retrying.reason().unwrap().contains("deadline"), "{:?}", retrying.reason());
    let slow = record.step(&sid("slow")).unwrap();
    assert_eq!(slow.status(), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    assert_eq!(slow.elapsed(), Some(Duration::from_millis(250)), "the attempt is cut to the time left");
}

#[tokio::test(start_paused = true)]
async fn the_step_context_carries_a_child_token_and_the_deadline() {
    let seen = Arc::new(Mutex::new(None));
    let p = plan(vec![
        exec("a", &[], {
            let seen = Arc::clone(&seen);
            michi_pipeline::step_fn(move |ctx| {
                let seen = Arc::clone(&seen);
                async move {
                    ctx.cancellation().cancel(); // a step cancelling its own token
                    *seen.lock().unwrap() = Some((ctx.remaining(), ctx.attempt(), ctx.run_id().to_string()));
                    Ok(())
                }
            })
        }),
        exec("b", &[], michi_pipeline::step_fn(|_| async { Ok(()) })),
    ]);
    let token = CancellationToken::new();
    let opts = ExecOptions::new().step_deadline(Duration::from_secs(2)).cancellation(token.clone());
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(*seen.lock().unwrap(), Some((Some(Duration::from_secs(2)), 0, "r".to_string())));
    assert!(!token.is_cancelled(), "a step's token is a child: cancelling it reaches neither the run nor the caller");
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

// ---- change 8: keyed breakers and the retry budget ----

#[tokio::test(start_paused = true)]
async fn breakers_are_keyed_so_one_failing_service_does_not_trip_another() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(60))));
    let gh_second = Scripted::new(&log, "gh2", vec![Act::Ok]);
    let gh_second_calls = Arc::clone(&gh_second.calls);
    let p = plan(vec![
        act(&log, "gh1", &[], Act::Err(fatal("down"))).breaker_key("github"),
        act(&log, "slack", &[], Act::Ok).breaker_key("slack"),
        exec("gh2", &[], gh_second).breaker_key("github"),
        act(&log, "after", &["gh2"], Act::Ok),
        act(&log, "own", &[], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().breakers(Arc::clone(&registry))).await;
    assert_eq!(status(&record, "slack"), StepStatus::Completed);
    assert_eq!(status(&record, "gh2"), StepStatus::NotRun(NotRunReason::CircuitOpen));
    assert_eq!(gh_second_calls.load(Ordering::SeqCst), 0, "an open breaker never invokes the step");
    assert!(record.step(&sid("gh2")).unwrap().attempts().is_empty());
    assert!(matches!(record.step(&sid("gh2")).unwrap().error(), Some(ExecutionError::CircuitOpen { .. })));
    assert_eq!(status(&record, "after"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert_eq!(registry.phase("github"), Some(BreakerPhase::Open));
    assert_eq!(registry.phase("slack"), Some(BreakerPhase::Closed));
    assert_eq!(
        registry.keys(),
        ["github", "own", "slack"],
        "the default key is the step id; skipped steps never touch one"
    );
}

#[tokio::test(start_paused = true)]
async fn a_shared_registry_accumulates_failures_across_runs_one_per_step_not_per_attempt() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(60))));
    let p = plan(vec![exec("a", &[], Scripted::new(&log, "a", vec![Act::Err(http(503))]))]);
    let opts = ExecOptions::new().breakers(Arc::clone(&registry)).retry(retries(3, 1));
    let first = execute(&p, rid("r1"), &opts).await;
    assert_eq!(first.step(&sid("a")).unwrap().attempts().len(), 4);
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Closed), "four failed attempts are one call-level failure");
    let _ = execute(&p, rid("r2"), &opts).await;
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Open));
    let third = execute(&p, rid("r3"), &opts).await;
    assert_eq!(status(&third, "a"), StepStatus::NotRun(NotRunReason::CircuitOpen));
}

#[tokio::test(start_paused = true)]
async fn a_half_open_probe_gets_exactly_one_attempt() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10))));
    let step = Scripted::new(&log, "a", vec![Act::Err(http(503))]);
    let calls = Arc::clone(&step.calls);
    let p = plan(vec![exec("a", &[], step)]);
    let opts = ExecOptions::new().breakers(Arc::clone(&registry)).retry(retries(3, 1));
    let _ = execute(&p, rid("r1"), &opts).await;
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(registry.phase("a"), Some(BreakerPhase::HalfOpen));
    let probe = execute(&p, rid("r2"), &opts).await;
    assert_eq!(calls.load(Ordering::SeqCst), 5, "the probe is not retried");
    assert_eq!(probe.step(&sid("a")).unwrap().attempts().len(), 1);
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Open), "a failed probe reopens");
}

#[tokio::test(start_paused = true)]
async fn a_panicking_probe_releases_the_probe_slot() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10))));
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    let _ = execute(&plan(vec![act(&log, "a", &[], Act::Err(fatal("x")))]), rid("r1"), &opts).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    let panicked = execute(&plan(vec![act(&log, "a", &[], Act::Panic)]), rid("r2"), &opts).await;
    assert_eq!(status(&panicked, "a"), StepStatus::Panicked);
    assert_eq!(registry.phase("a"), Some(BreakerPhase::HalfOpen), "the panic reported neither success nor failure");
    let recovered = execute(&plan(vec![act(&log, "a", &[], Act::Ok)]), rid("r3"), &opts).await;
    assert_eq!(status(&recovered, "a"), StepStatus::Completed);
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Closed));
}

#[tokio::test(start_paused = true)]
async fn cancellation_is_not_reported_as_a_breaker_failure() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10))));
    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            token.cancel();
        });
    }
    let opts = ExecOptions::new().breakers(Arc::clone(&registry)).cancellation(token).cancel_grace(Duration::ZERO);
    let record = execute(&plan(vec![act(&log, "a", &[], Act::Hang)]), rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::OutcomeUnknown(UnknownCause::Cancelled));
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Closed));
}

#[tokio::test(start_paused = true)]
async fn the_retry_budget_caps_retries_across_the_run() {
    let log = Log::new();
    let a = Scripted::new(&log, "a", vec![Act::Err(http(503))]);
    let b = Scripted::new(&log, "b", vec![Act::Err(http(503))]);
    let (a_calls, b_calls) = (Arc::clone(&a.calls), Arc::clone(&b.calls));
    let p = plan(vec![exec("a", &[], a), exec("b", &[], b)]);
    let opts = ExecOptions::new().retry(retries(3, 1)).retry_budget(RetryBudget::new(0.0, 1, 1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(a_calls.load(Ordering::SeqCst), 2, "the one budgeted retry");
    assert_eq!(b_calls.load(Ordering::SeqCst), 1, "first attempts are never budgeted, retries are");
    assert!(record.step(&sid("b")).unwrap().reason().unwrap().contains("retry budget"));

    let unbudgeted = execute(&p, rid("r2"), &ExecOptions::new().retry(retries(3, 1)).no_retry_budget()).await;
    assert!(unbudgeted.steps().iter().all(|s| s.attempts().len() == 4));
}

#[tokio::test(start_paused = true)]
async fn successes_earn_retry_budget() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "ok", &[], Act::Ok),
        exec("flaky", &["ok"], Scripted::new(&log, "flaky", vec![Act::Err(http(503)), Act::Ok])),
    ]);
    let opts = ExecOptions::new().retry(retries(3, 1)).retry_budget(RetryBudget::new(1.0, 0, 10));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "flaky"), StepStatus::Completed);
    assert_eq!(record.step(&sid("flaky")).unwrap().attempts().len(), 2);
}

// ---- change 9: the observer ----

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
    finished: Mutex<Vec<StepRecord>>,
}

impl StepObserver for Recorder {
    fn on_step_started(&self, run_id: &RunId, step_id: &michi_pipeline::StepId) {
        self.events.lock().unwrap().push(format!("{run_id}:started:{step_id}"));
    }
    fn on_step_finished(&self, run_id: &RunId, record: &StepRecord) {
        self.events.lock().unwrap().push(format!("{run_id}:finished:{}:{}", record.step_id(), record.status().label()));
        self.finished.lock().unwrap().push(record.clone());
    }
}

#[tokio::test(start_paused = true)]
async fn the_observer_sees_every_start_and_every_terminal_record_as_it_happens() {
    let log = Log::new();
    let recorder = Arc::new(Recorder::default());
    let p = plan(vec![
        act(&log, "a", &[], Act::Ok),
        act(&log, "b", &[], Act::Err(fatal("x"))),
        act(&log, "c", &["b"], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().observer(recorder.clone())).await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        ["r:started:a", "r:finished:a:completed", "r:started:b", "r:finished:b:failed", "r:finished:c:skipped"]
    );
    let mut finished = recorder.finished.lock().unwrap().clone();
    finished.sort_by(|x, y| x.step_id().cmp(y.step_id()));
    assert_eq!(finished, record.steps(), "the observed records are the final records");
}

#[tokio::test(start_paused = true)]
async fn a_closure_is_an_observer() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = {
        let seen = Arc::clone(&seen);
        move |_: &RunId, r: &StepRecord| seen.lock().unwrap().push(r.step_id().to_string())
    };
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok)]);
    let _ = execute(&p, rid("r"), &ExecOptions::new().observer(Arc::new(observer))).await;
    assert_eq!(*seen.lock().unwrap(), ["a"]);
}

#[tokio::test(start_paused = true)]
async fn a_panicking_observer_neither_aborts_the_run_nor_loses_the_record() {
    struct Panicky(Mutex<Vec<String>>);
    impl StepObserver for Panicky {
        fn on_step_started(&self, _: &RunId, step_id: &michi_pipeline::StepId) {
            self.0.lock().unwrap().push(format!("started:{step_id}"));
            if step_id.as_str() == "a" {
                panic!("observer bug on start");
            }
        }
        fn on_step_finished(&self, _: &RunId, record: &StepRecord) {
            self.0.lock().unwrap().push(format!("finished:{}", record.step_id()));
            if record.step_id().as_str() == "b" {
                panic!("observer bug on finish");
            }
        }
    }
    let log = Log::new();
    let observer = Arc::new(Panicky(Mutex::default()));
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok), act(&log, "c", &["b"], Act::Ok)]);
    for mode in [Mode::Sequential, parallel(2)] {
        observer.0.lock().unwrap().clear();
        let record = execute(&p, rid("r"), &ExecOptions::new().mode(mode).observer(observer.clone())).await;
        assert_eq!(record.outcome(), RunOutcome::Succeeded, "{mode:?}: the steps ran despite the observer");
        assert!(record.steps().iter().all(|s| s.status() == StepStatus::Completed));
        assert_eq!(record.observer_panics(), 2, "{mode:?}: both caught panics are counted");
        assert_eq!(
            *observer.0.lock().unwrap(),
            ["started:a", "finished:a", "started:b", "finished:b", "started:c", "finished:c"],
            "{mode:?}: the observer keeps being called after it panicked"
        );
    }
    let quiet = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(quiet.observer_panics(), 0);
}

// ---- coverage ported from the old executors ----

#[tokio::test(start_paused = true)]
async fn an_empty_plan_succeeds_immediately() {
    let record = execute(&plan(vec![]), rid("r"), &ExecOptions::new()).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(record.render(), "step: []");
}

#[tokio::test(start_paused = true)]
async fn a_zero_attempt_timeout_is_floored_to_one_millisecond() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], after(5, Act::Ok))]);
    let record = execute(&p, rid("r"), &ExecOptions::new().attempt_timeout(Duration::ZERO)).await;
    let expected = ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::Timeout { elapsed_ms: 1 }) };
    assert_eq!(record.step(&sid("a")).unwrap().error(), Some(&expected));
}

#[tokio::test(start_paused = true)]
async fn sequential_fail_fast_stops_at_the_first_failure() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "a", &[], Act::Ok),
        act(&log, "b", &[], Act::Err(fatal("x"))),
        act(&log, "c", &[], Act::Ok),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().failure_policy(FailurePolicy::FailFast)).await;
    assert_eq!(
        statuses(&record),
        [
            ("a".into(), StepStatus::Completed),
            ("b".into(), StepStatus::Failed),
            ("c".into(), StepStatus::Skipped(SkipReason::Cancelled)),
        ]
    );
    assert_eq!(log.starts(), ["a", "b"]);
    assert_eq!(record.outcome(), RunOutcome::Failed);
}

#[tokio::test(start_paused = true)]
async fn a_dependent_is_skipped_as_soon_as_one_prerequisite_fails() {
    let log = Log::new();
    let recorder = Arc::new(Recorder::default());
    let p = plan(vec![
        act(&log, "slow", &[], after(50, Act::Ok)),
        act(&log, "fails", &[], after(5, Act::Err(fatal("x")))),
        act(&log, "join", &["slow", "fails"], Act::Ok),
    ]);
    let _ = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(4)).observer(recorder.clone())).await;
    let events = recorder.events.lock().unwrap().clone();
    let skip = events.iter().position(|e| e == "r:finished:join:skipped").unwrap();
    let slow = events.iter().position(|e| e == "r:finished:slow:completed").unwrap();
    assert!(skip < slow, "join is skipped without waiting for slow: {events:?}");
}

#[tokio::test(start_paused = true)]
async fn a_second_step_on_a_half_open_key_is_not_run_while_the_probe_is_in_flight() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10))));
    let opts = ExecOptions::new().breakers(Arc::clone(&registry)).mode(parallel(2));
    let _ =
        execute(&plan(vec![act(&log, "trip", &[], Act::Err(fatal("x"))).breaker_key("svc")]), rid("r1"), &opts).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    let loser = Scripted::new(&log, "loser", vec![Act::Ok]);
    let loser_calls = Arc::clone(&loser.calls);
    let p = plan(vec![
        act(&log, "probe", &[], after(5, Act::Ok)).breaker_key("svc"),
        exec("loser", &[], loser).breaker_key("svc"),
    ]);
    let record = execute(&p, rid("r2"), &opts).await;
    assert_eq!(status(&record, "probe"), StepStatus::Completed);
    assert_eq!(status(&record, "loser"), StepStatus::NotRun(NotRunReason::CircuitOpen));
    assert_eq!(
        record.step(&sid("loser")).unwrap().error(),
        Some(&ExecutionError::CircuitOpen { retry_after_ms: 10_000 })
    );
    assert_eq!(loser_calls.load(Ordering::SeqCst), 0, "only the probe may invoke its step");
    assert_eq!(registry.phase("svc"), Some(BreakerPhase::Closed));
}

#[tokio::test(start_paused = true)]
async fn a_panicking_step_reports_nothing_to_a_closed_breaker() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(2, Duration::from_secs(60))));
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    let p = plan(vec![
        act(&log, "boom", &[], Act::Panic).breaker_key("svc"),
        act(&log, "fails", &[], Act::Err(fatal("x"))).breaker_key("svc"),
    ]);
    let record = execute(&p, rid("r1"), &opts).await;
    assert_eq!(status(&record, "boom"), StepStatus::Panicked);
    assert_eq!(registry.phase("svc"), Some(BreakerPhase::Closed), "one real failure; the panic counted for nothing");
    let _ =
        execute(&plan(vec![act(&log, "fails", &[], Act::Err(fatal("x"))).breaker_key("svc")]), rid("r2"), &opts).await;
    assert_eq!(registry.phase("svc"), Some(BreakerPhase::Open), "the breaker still counts real failures");
}

#[tokio::test(start_paused = true)]
async fn an_ambiguous_half_open_probe_is_not_retryable_and_reopens_the_breaker() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10))));
    let opts = ExecOptions::new().breakers(Arc::clone(&registry)).retry(retries(3, 1));
    let _ = execute(&plan(vec![act(&log, "a", &[], Act::Err(fatal("x")))]), rid("r1"), &opts).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    let probe = Scripted::new(&log, "a", vec![Act::Err(http(504))]);
    let calls = Arc::clone(&probe.calls);
    let record = execute(&plan(vec![exec("a", &[], probe)]), rid("r2"), &opts).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let a = record.step(&sid("a")).unwrap();
    assert_eq!(a.status(), StepStatus::OutcomeUnknown(UnknownCause::Gateway));
    assert!(matches!(a.error(), Some(ExecutionError::AmbiguousOutcome { .. })));
    assert!(!a.error().unwrap().is_retryable());
    assert_eq!(registry.phase("a"), Some(BreakerPhase::Open), "an ambiguous probe is a failed probe");
}

#[tokio::test(start_paused = true)]
async fn a_cancellation_after_every_step_started_does_not_turn_success_into_cancelled() {
    // Parallel: both steps are in flight when the caller cancels and both finish within the grace period.
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], after(10, Act::Ok)), act(&log, "b", &[], after(20, Act::Ok))]);
    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            token.cancel();
        });
    }
    let record = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(2)).cancellation(token)).await;
    assert!(record.steps().iter().all(|s| s.status() == StepStatus::Completed));
    assert_eq!(record.outcome(), RunOutcome::Succeeded, "nothing was left to cancel");
    assert!(record.was_cancelled());

    // Sequential: the last step cancels the caller's token itself.
    let token = CancellationToken::new();
    let last = {
        let token = token.clone();
        michi_pipeline::step_fn(move |_| {
            let token = token.clone();
            async move {
                token.cancel();
                Ok(())
            }
        })
    };
    let p = plan(vec![act(&log, "a", &[], Act::Ok), exec("b", &["a"], last)]);
    let record = execute(&p, rid("r"), &ExecOptions::new().cancellation(token.clone())).await;
    assert!(token.is_cancelled());
    assert!(record.steps().iter().all(|s| s.status() == StepStatus::Completed));
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_failure_and_a_cancellation_in_one_run_keep_the_failure_as_the_root_cause() {
    let log = Log::new();
    let token = CancellationToken::new();
    let fails_and_cancels = {
        let token = token.clone();
        michi_pipeline::step_fn(move |_| {
            let token = token.clone();
            async move {
                token.cancel();
                Err(ExecutionError::Failed { message: "boom".into(), retryable: false })
            }
        })
    };
    let later = Scripted::new(&log, "c", vec![Act::Ok]);
    let later_calls = Arc::clone(&later.calls);
    let p = plan(vec![exec("a", &[], fails_and_cancels), act(&log, "b", &[], Act::Ok), exec("c", &["b"], later)]);
    let record = execute(&p, rid("r"), &ExecOptions::new().cancellation(token)).await;
    assert_eq!(status(&record, "a"), StepStatus::Failed);
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);
    assert_eq!(record.root_causes().map(|s| s.step_id().as_str()).collect::<Vec<_>>(), ["a"]);
    assert!(matches!(record.error(), Some(ExecutionError::StepFailed { step_id, .. }) if step_id == "a"));
    assert_eq!(record.outcome(), RunOutcome::Cancelled, "cancelled outranks failed: resume comes first");
}

#[tokio::test(start_paused = true)]
async fn a_cancellation_at_dispatch_skips_the_step_without_invoking_it() {
    struct CancelOnStart(CancellationToken);
    impl StepObserver for CancelOnStart {
        fn on_step_started(&self, _: &RunId, _: &michi_pipeline::StepId) {
            self.0.cancel();
        }
        fn on_step_finished(&self, _: &RunId, _: &StepRecord) {}
    }
    let log = Log::new();
    let step = Scripted::new(&log, "a", vec![Act::Ok]);
    let calls = Arc::clone(&step.calls);
    let token = CancellationToken::new();
    let opts = ExecOptions::new().cancellation(token.clone()).observer(Arc::new(CancelOnStart(token)));
    let record = execute(&plan(vec![exec("a", &[], step)]), rid("r"), &opts).await;
    let a = record.step(&sid("a")).unwrap();
    assert_eq!(a.status(), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(a.reason(), Some("run cancelled before the step started"));
    assert!(a.attempts().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0, "cancelled between dispatch and the first attempt");
}

#[tokio::test(start_paused = true)]
async fn an_attempt_that_fails_after_cancellation_is_not_retried() {
    let log = Log::new();
    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            token.cancel();
        });
    }
    let opts = ExecOptions::new().cancellation(token).retry(retries(3, 1_000)).cancel_grace(Duration::from_secs(1));
    let record = execute(&plan(vec![act(&log, "a", &[], after(5, Act::Err(http(503))))]), rid("r"), &opts).await;
    let a = record.step(&sid("a")).unwrap();
    assert_eq!(a.attempts().len(), 1);
    assert_eq!(a.status(), StepStatus::Failed);
    assert!(a.reason().unwrap().ends_with("; run cancelled"), "stopped before backoff: {:?}", a.reason());
    assert_eq!(record.elapsed(), Duration::from_millis(5));
}

#[tokio::test(start_paused = true)]
async fn a_zero_per_step_attempt_timeout_and_a_zero_step_deadline_are_floored_to_one_millisecond() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "timeout", &[], after(5, Act::Ok)).attempt_timeout(Duration::ZERO),
        act(&log, "deadline", &[], after(5, Act::Ok)).deadline(Duration::ZERO),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    let expected = ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::Timeout { elapsed_ms: 1 }) };
    assert_eq!(record.step(&sid("timeout")).unwrap().error(), Some(&expected));
    assert_eq!(record.step(&sid("deadline")).unwrap().error(), Some(&expected));
}

#[tokio::test(start_paused = true)]
async fn two_independent_failures_are_both_root_causes_and_error_names_the_first_declared() {
    let log = Log::new();
    let p = plan(vec![
        ExecStep::new(sid("a"), "Alpha", Scripted::new(&log, "a", vec![after(10, Act::Err(fatal("a")))])),
        ExecStep::new(sid("b"), "Beta", Scripted::new(&log, "b", vec![after(5, Act::Err(fatal("b")))])),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new().mode(parallel(2))).await;
    assert_eq!(statuses(&record), [("a".into(), StepStatus::Failed), ("b".into(), StepStatus::Failed)]);
    assert_eq!(record.root_causes().map(|s| s.step_id().as_str()).collect::<Vec<_>>(), ["a", "b"]);
    match record.error() {
        Some(ExecutionError::StepFailed { step_id, step_name, source }) => {
            assert_eq!((step_id.as_str(), step_name.as_str()), ("a", "Alpha"), "declaration order, not finish order");
            assert_eq!(*source, fatal("a"));
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

//! The effect model (`docs/spec/10-pipeline-effects.md`): two facts per failure, precheck and durable starts. Each
//! test here reproduces a path where retries or resume could repeat an effect that may already have landed.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, resume, BreakerConfig, BreakerPhase, BreakerRegistry, DurableObserver, Effect, ExecOptions,
    ExecutionError, NotRunReason, Precheck, ReplayClass, ResumeError, ResumeOptions, RunId, RunOutcome, RunRecord,
    SkipReason, StepId, StepObserver, StepRecord, StepStatus, UnknownCause, Verification,
};

// ---- Two facts per failure ----

/// The external review's probe: a non-idempotent step commits its effect, then reports a failure it can't vouch for.
/// Before the fix it ended `Failed` and resume committed a second time.
#[tokio::test(start_paused = true)]
async fn a_failure_after_a_commit_is_held_by_resume_until_verified() {
    for (label, err) in
        [("failed(msg)", ExecutionError::failed("committed, then lost the reply")), ("http 500", http(500))]
    {
        let log = Log::new();
        let commits = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let charge = {
            let commits = Arc::clone(&commits);
            let err = err.clone();
            michi_pipeline::step_fn(move |_| {
                let commits = Arc::clone(&commits);
                let err = err.clone();
                async move {
                    if commits.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(err)
                    } else {
                        Ok(())
                    }
                }
            })
        };
        let p = plan(vec![exec("charge", &[], charge), act(&log, "email", &["charge"], Act::Ok)]);
        let first = execute(&p, rid("r"), &ExecOptions::new()).await;
        assert_eq!(status(&first, "charge"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed), "{label}");
        assert!(matches!(first.step(&sid("charge")).unwrap().error(), Some(ExecutionError::AmbiguousOutcome { .. })));
        assert_eq!(first.outcome(), RunOutcome::OutcomeUnknown, "{label}");
        assert_eq!(first.needs_verification().count(), 1, "{label}");

        let held = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
        assert_eq!(commits.load(Ordering::SeqCst), 1, "{label}: resume must not commit a second time");
        assert_eq!(status(&held, "charge"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed), "{label}");
        assert_eq!(status(&held, "email"), StepStatus::Skipped(SkipReason::PrerequisiteFailed), "{label}");

        let verified = ResumeOptions::default().verified(sid("charge"), Verification::NotApplied);
        let done = resume(&held, &p, &verified).await.unwrap();
        assert_eq!(commits.load(Ordering::SeqCst), 2, "{label}: verified as not applied, so it runs once more");
        assert_eq!(done.outcome(), RunOutcome::Succeeded, "{label}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_refused_failure_ends_failed_and_resume_runs_it_again() {
    let log = Log::new();
    let s = Scripted::new(&log, "s", vec![Act::Err(ExecutionError::refused("bad input")), Act::Ok]);
    let calls = Arc::clone(&s.calls);
    let p = plan(vec![exec("s", &[], s)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&first, "s"), StepStatus::Failed);
    assert_eq!(first.step(&sid("s")).unwrap().attempts()[0].class(), Some(ReplayClass::Refused));
    assert_eq!(first.needs_verification().count(), 0, "nothing took effect, so there is nothing to verify");
    let second = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(second.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn retrying_follows_the_effect_not_just_retryable() {
    // `transient` is retryable but its effect is unknown: a plain step never retries it, an idempotent one does.
    let log = Log::new();
    let once = Scripted::new(&log, "once", vec![Act::Err(ExecutionError::transient("reset")), Act::Ok]);
    let idem = Scripted::new(&log, "idem", vec![Act::Err(ExecutionError::transient("reset")), Act::Ok]).idempotent();
    let rejected = Scripted::new(&log, "rej", vec![Act::Err(ExecutionError::rejected("busy")), Act::Ok]);
    let (once_calls, idem_calls, rej_calls) =
        (Arc::clone(&once.calls), Arc::clone(&idem.calls), Arc::clone(&rejected.calls));
    let p = plan(vec![exec("once", &[], once), exec("idem", &[], idem), exec("rej", &[], rejected)]);
    let record = execute(&p, rid("r"), &ExecOptions::new().retry(retries(3, 10))).await;
    assert_eq!(once_calls.load(Ordering::SeqCst), 1);
    assert_eq!(status(&record, "once"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));
    assert_eq!(idem_calls.load(Ordering::SeqCst), 2);
    assert_eq!(rej_calls.load(Ordering::SeqCst), 2, "not applied and retryable: retried for every step");
    assert_eq!(status(&record, "rej"), StepStatus::Completed);
}

#[tokio::test(start_paused = true)]
async fn an_idempotent_step_whose_effect_is_unknown_ends_failed_and_resume_runs_it() {
    let log = Log::new();
    let s = Scripted::new(&log, "s", vec![Act::Err(http(500)), Act::Ok]).idempotent();
    let calls = Arc::clone(&s.calls);
    let p = plan(vec![exec("s", &[], s)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&first, "s"), StepStatus::Failed);
    assert_eq!(first.step(&sid("s")).unwrap().error(), Some(&http(500)), "an idempotent step's error stays bare");
    let second = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(second.outcome(), RunOutcome::Succeeded);
}

// ---- Precheck ----

#[tokio::test(start_paused = true)]
async fn a_precheck_that_finds_the_effect_applied_skips_the_step_in_a_fresh_run() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Ok]).prechecked(vec![Ok(Precheck::Applied)]);
    let (calls, checks) = (Arc::clone(&s.calls), Arc::clone(&s.check_calls));
    let p = plan(vec![exec("publish", &[], s), act(&log, "after", &["publish"], Act::Ok)]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!((calls.load(Ordering::SeqCst), checks.load(Ordering::SeqCst)), (0, 1));
    let step = record.step(&sid("publish")).unwrap();
    assert_eq!(step.status(), StepStatus::CompletedEarlier);
    assert_eq!(step.reason(), Some("already applied (precheck)"));
    assert!(step.attempts().is_empty(), "a precheck is not an attempt");
    assert_eq!(status(&record, "after"), StepStatus::Completed);
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_precheck_that_proves_the_effect_absent_runs_the_step() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Ok]).prechecked(vec![Ok(Precheck::NotApplied)]);
    let calls = Arc::clone(&s.calls);
    let record = execute(&plan(vec![exec("publish", &[], s)]), rid("r"), &ExecOptions::new()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status(&record, "publish"), StepStatus::Completed);
    assert_eq!(log.events(), ["check:publish", "start:publish", "end:publish"], "the precheck runs first");
}

#[tokio::test(start_paused = true)]
async fn an_unknown_precheck_without_a_prior_attempt_is_not_run_unverified_and_resume_retries() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Ok]).prechecked(vec![
        Ok(Precheck::Unknown),
        Err(ExecutionError::failed("registry read failed")),
        Ok(Precheck::NotApplied),
    ]);
    let (calls, checks) = (Arc::clone(&s.calls), Arc::clone(&s.check_calls));
    let p = plan(vec![exec("publish", &[], s), act(&log, "after", &["publish"], Act::Ok)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing was invoked");
    assert_eq!(status(&first, "publish"), StepStatus::NotRun(NotRunReason::Unverified));
    assert_eq!(status(&first, "after"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert_eq!(first.outcome(), RunOutcome::Failed, "never Succeeded with a not-run step");
    assert_eq!(first.needs_verification().count(), 0, "nothing ran, so nothing to verify");
    assert!(first.render().contains("publish,Step publish,not_run,unverified"), "{}", first.render());

    let second = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(status(&second, "publish"), StepStatus::NotRun(NotRunReason::Unverified), "an Err counts as unknown");
    let third = resume(&second, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(checks.load(Ordering::SeqCst), 3, "every resume prechecks first");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(third.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_timed_out_attempt_then_an_applied_precheck_ends_completed_earlier_with_one_attempt() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Hang])
        .prechecked(vec![Ok(Precheck::NotApplied), Ok(Precheck::Applied)]);
    let calls = Arc::clone(&s.calls);
    let opts = ExecOptions::new().attempt_timeout(Duration::from_millis(50)).retry(retries(3, 10));
    let record = execute(&plan(vec![exec("publish", &[], s)]), rid("r"), &opts).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the ambiguous attempt was retried through the precheck, not re-run");
    let step = record.step(&sid("publish")).unwrap();
    assert_eq!(step.status(), StepStatus::CompletedEarlier);
    assert_eq!(step.attempts().len(), 1);
    assert_eq!(step.attempts()[0].class(), Some(ReplayClass::Ambiguous));
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn an_unknown_precheck_after_an_ambiguous_attempt_ends_outcome_unknown_with_that_cause() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Hang, Act::Ok]).prechecked(vec![
        Ok(Precheck::NotApplied),
        Ok(Precheck::Unknown),
        Ok(Precheck::Applied),
    ]);
    let calls = Arc::clone(&s.calls);
    let p = plan(vec![exec("publish", &[], s)]);
    let opts = ExecOptions::new().attempt_timeout(Duration::from_millis(50)).retry(retries(3, 10));
    let first = execute(&p, rid("r"), &opts).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status(&first, "publish"), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    assert_eq!(first.outcome(), RunOutcome::OutcomeUnknown);

    // Resume needs no verdict for a prechecked step: its precheck decides.
    let second = resume(&first, &p, &ResumeOptions::new(opts)).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status(&second, "publish"), StepStatus::CompletedEarlier, "applied on resume, not run again");
}

#[tokio::test(start_paused = true)]
async fn a_caller_verdict_takes_precedence_over_the_precheck_on_resume() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Err(http(500)), Act::Ok])
        .prechecked(vec![Ok(Precheck::NotApplied), Ok(Precheck::Unknown)]);
    let checks = Arc::clone(&s.check_calls);
    let p = plan(vec![exec("publish", &[], s)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&first, "publish"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));
    let applied = ResumeOptions::default().verified(sid("publish"), Verification::Applied);
    let second = resume(&first, &p, &applied).await.unwrap();
    assert_eq!(checks.load(Ordering::SeqCst), 1, "a verdict of applied does not consult the precheck");
    assert_eq!(status(&second, "publish"), StepStatus::CompletedEarlier);
}

#[tokio::test(start_paused = true)]
async fn a_refused_precheck_fails_the_step_without_running_it() {
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Ok])
        .prechecked(vec![Err(ExecutionError::refused("the tag already points elsewhere"))]);
    let calls = Arc::clone(&s.calls);
    let record = execute(&plan(vec![exec("publish", &[], s)]), rid("r"), &ExecOptions::new()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let step = record.step(&sid("publish")).unwrap();
    assert_eq!(step.status(), StepStatus::Failed);
    assert_eq!(step.error().map(ExecutionError::effect), Some(Effect::NotApplied));
    assert_eq!(step.reason(), Some("precheck: the tag already points elsewhere"));
}

#[tokio::test(start_paused = true)]
async fn a_precheck_is_not_reported_to_the_breaker() {
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(30))));
    let log = Log::new();
    let s = Scripted::new(&log, "publish", vec![Act::Ok]).prechecked(vec![
        Ok(Precheck::Unknown),
        Err(ExecutionError::failed("x")),
        Ok(Precheck::Applied),
    ]);
    let p = plan(vec![exec("publish", &[], s)]);
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    for i in 0..3 {
        let _ = execute(&p, rid(&format!("r{i}")), &opts).await;
        assert_eq!(registry.breaker("publish").phase(), BreakerPhase::Closed, "run {i}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_precheck_that_outlives_the_attempt_timeout_counts_as_unknown() {
    struct SlowCheck;
    impl michi_pipeline::Step for SlowCheck {
        fn run<'a>(&'a self, _: &'a michi_pipeline::StepContext) -> michi_pipeline::StepFuture<'a> {
            Box::pin(async { panic!("must not run") })
        }
        fn replay(&self) -> michi_pipeline::Replay {
            michi_pipeline::Replay::Prechecked
        }
        fn precheck<'a>(&'a self, _: &'a michi_pipeline::StepContext) -> michi_pipeline::PrecheckFuture<'a> {
            Box::pin(std::future::pending())
        }
    }
    let p = plan(vec![exec("s", &[], SlowCheck)]);
    let record = execute(&p, rid("r"), &ExecOptions::new().attempt_timeout(Duration::from_millis(20))).await;
    assert_eq!(status(&record, "s"), StepStatus::NotRun(NotRunReason::Unverified));
    assert_eq!(record.elapsed(), Duration::from_millis(20));
}

// ---- Durable starts ----

/// An observer whose start hook is scripted, logging every save in order (an append-only log).
#[derive(Default)]
struct Journal {
    fail_start: Option<&'static str>,
    /// Fail only this step's start (every step's when `None`).
    fail_only: Option<&'static str>,
    panic_start: bool,
    saved: Mutex<Vec<StepRecord>>,
}

impl DurableObserver for Journal {
    fn record_start(&self, _: &RunId, step_id: &StepId) -> Result<(), String> {
        if self.panic_start {
            panic!("disk on fire");
        }
        if let Some(reason) = self.fail_start {
            if self.fail_only.is_none_or(|id| id == step_id.as_str()) {
                return Err(reason.to_string());
            }
        }
        self.saved.lock().unwrap().push(StepRecord::in_flight(step_id.clone(), "in flight"));
        Ok(())
    }
}

impl StepObserver for Journal {
    fn on_step_finished(&self, _: &RunId, record: &StepRecord) {
        self.saved.lock().unwrap().push(record.clone());
    }
}

#[tokio::test(start_paused = true)]
async fn a_start_that_cannot_be_recorded_never_invokes_the_step() {
    for journal in [
        Journal { fail_start: Some("write failed"), ..Journal::default() },
        Journal { panic_start: true, ..Journal::default() },
    ] {
        let panics = journal.panic_start;
        let log = Log::new();
        let s = Scripted::new(&log, "charge", vec![Act::Ok]);
        let calls = Arc::clone(&s.calls);
        let journal = Arc::new(journal);
        let p = plan(vec![exec("charge", &[], s), act(&log, "email", &["charge"], Act::Ok)]);
        let opts = ExecOptions::new().durable_starts(journal.clone());
        let record = execute(&p, rid("r"), &opts).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0, "panic={panics}: the step was never invoked");
        let step = record.step(&sid("charge")).unwrap();
        assert_eq!(step.status(), StepStatus::NotRun(NotRunReason::StartNotRecorded));
        assert!(step.reason().unwrap().contains(if panics { "disk on fire" } else { "write failed" }));
        assert_eq!(status(&record, "email"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
        assert_eq!(record.observer_panics(), u32::from(panics));
        assert_eq!(record.outcome(), RunOutcome::Failed);
        assert!(record.render().contains("charge,Step charge,not_run,start_not_recorded"), "{}", record.render());
    }
}

#[tokio::test(start_paused = true)]
async fn an_unrecorded_start_leaves_independent_branches_running_and_resume_runs_the_step_once() {
    // The review's crash gap: without durable starts, a start hook that failed to save the marker still dispatched
    // the step, a crash rebuilt it as `Pending`, and resume ran it a second time. With durable starts it never ran.
    let log = Log::new();
    let s = Scripted::new(&log, "charge", vec![Act::Ok]);
    let calls = Arc::clone(&s.calls);
    let p = plan(vec![exec("charge", &[], s), act(&log, "other", &[], Act::Ok)]);
    let failing =
        Arc::new(Journal { fail_start: Some("write failed"), fail_only: Some("charge"), ..Journal::default() });
    let first = execute(&p, rid("r"), &ExecOptions::new().durable_starts(failing)).await;
    assert_eq!(status(&first, "charge"), StepStatus::NotRun(NotRunReason::StartNotRecorded));
    assert_eq!(status(&first, "other"), StepStatus::Completed, "an independent branch carries on");
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let healthy = Arc::new(Journal::default());
    let opts = ResumeOptions::new(ExecOptions::new().durable_starts(healthy));
    let second = resume(&first, &p, &opts).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "it never ran before, so it runs exactly once now");
    assert_eq!(second.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_crash_after_a_recorded_start_rebuilds_the_step_as_running_never_pending() {
    let log = Log::new();
    let s = Scripted::new(&log, "charge", vec![Act::Hang, Act::Ok]);
    let calls = Arc::clone(&s.calls);
    let p = plan(vec![act(&log, "first", &[], Act::Ok), exec("charge", &["first"], s)]);
    let journal = Arc::new(Journal::default());
    let opts = ExecOptions::new().durable_starts(journal.clone());
    let crashed = tokio::time::timeout(Duration::from_millis(10), execute(&p, rid("r"), &opts)).await;
    assert!(crashed.is_err(), "the run was dropped mid-step");
    let saved = journal.saved.lock().unwrap().clone();
    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, saved).unwrap();
    assert_eq!(status(&rebuilt, "first"), StepStatus::Completed);
    assert_eq!(status(&rebuilt, "charge"), StepStatus::Running);

    let held = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a step that may have run is never re-run blindly");
    assert_eq!(status(&held, "charge"), StepStatus::OutcomeUnknown(UnknownCause::Interrupted));
}

#[tokio::test(start_paused = true)]
async fn setting_another_observer_turns_durable_starts_off() {
    // Durable starts and their observer are set together, so they cannot be misconfigured (a closure observer, which
    // cannot record starts, does not compile there). A later `observer(..)` replaces both.
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok)]);
    let refusing = Arc::new(Journal { fail_start: Some("no"), ..Journal::default() });
    let closure = |_: &RunId, _: &StepRecord| {};
    let opts = ExecOptions::new().durable_starts(refusing).observer(Arc::new(closure));
    assert_eq!(status(&execute(&p, rid("r"), &opts).await, "a"), StepStatus::Completed);
}

#[tokio::test(start_paused = true)]
async fn without_durable_starts_a_panicking_start_hook_still_dispatches() {
    // Unchanged behaviour, kept for callers whose start hook is best effort.
    struct Panicky;
    impl StepObserver for Panicky {
        fn on_step_started(&self, _: &RunId, _: &StepId) {
            panic!("best effort");
        }
        fn on_step_finished(&self, _: &RunId, _: &StepRecord) {}
    }
    let log = Log::new();
    let record =
        execute(&plan(vec![act(&log, "a", &[], Act::Ok)]), rid("r"), &ExecOptions::new().observer(Arc::new(Panicky)))
            .await;
    assert_eq!(status(&record, "a"), StepStatus::Completed);
    assert_eq!(record.observer_panics(), 1);
}

// ---- Resume and records ----

#[tokio::test(start_paused = true)]
async fn a_verdict_for_a_step_that_is_not_in_doubt_is_refused() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Err(ExecutionError::refused("bad")))]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    let err =
        resume(&first, &p, &ResumeOptions::default().verified(sid("a"), Verification::Applied)).await.unwrap_err();
    assert_eq!(err, ResumeError::NotAwaitingVerification { step_id: sid("a"), status: "failed" });
}

#[tokio::test(start_paused = true)]
async fn every_new_status_renders_as_toon_and_no_run_with_them_succeeds() {
    let log = Log::new();
    let journal = Arc::new(Journal { fail_start: Some("no"), ..Journal::default() });
    let p = plan(vec![
        act(&log, "unknown", &[], Act::Err(ExecutionError::failed("x"))),
        exec(
            "unverified",
            &[],
            Scripted::new(&log, "unverified", vec![Act::Ok]).prechecked(vec![Ok(Precheck::Unknown)]),
        ),
        exec("applied", &[], Scripted::new(&log, "applied", vec![Act::Ok]).prechecked(vec![Ok(Precheck::Applied)])),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(
        record.render(),
        "step[3]{id,name,status,reason}:\n  unknown,Step unknown,outcome_unknown,step_failed\n  \
         unverified,Step unverified,not_run,unverified\n  applied,Step applied,completed_earlier,null"
    );
    assert_eq!(record.outcome(), RunOutcome::OutcomeUnknown);
    let refused = execute(&p, rid("r"), &ExecOptions::new().durable_starts(journal)).await;
    assert!(refused.steps().iter().all(|s| s.status() == StepStatus::NotRun(NotRunReason::StartNotRecorded)));
    assert_eq!(refused.outcome(), RunOutcome::Failed);
    assert!(refused.error().is_some());
}

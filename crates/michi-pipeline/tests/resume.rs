//! Change 5: resume continues a run from its record under the same run id, never re-runs finished steps, and
//! respects ambiguity.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, resume, BreakerConfig, BreakerRegistry, CancellationToken, ExecOptions, ResumeError, ResumeOptions, RunId,
    RunOutcome, RunRecord, SkipReason, StepObserver, StepRecord, StepStatus, UnknownCause, Verification,
};

#[tokio::test(start_paused = true)]
async fn resume_reruns_only_what_did_not_succeed_under_the_same_run_id_and_idempotency_keys() {
    let log = Log::new();
    let a = Scripted::new(&log, "a", vec![Act::Ok]);
    let b = Scripted::new(&log, "b", vec![Act::Err(fatal("first time")), Act::Ok]);
    let (a_calls, b_calls, b_keys) = (Arc::clone(&a.calls), Arc::clone(&b.calls), Arc::clone(&b.keys));
    let p = plan(vec![exec("a", &[], a), exec("b", &["a"], b), act(&log, "c", &["b"], Act::Ok)]);
    let first = execute(&p, rid("run-9"), &ExecOptions::new()).await;
    assert_eq!(first.outcome(), RunOutcome::Failed);

    let second = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(second.run_id().as_str(), "run-9");
    assert_eq!(a_calls.load(Ordering::SeqCst), 1, "a completed before and is never re-run");
    assert_eq!(b_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        statuses(&second),
        [
            ("a".into(), StepStatus::CompletedEarlier),
            ("b".into(), StepStatus::Completed),
            ("c".into(), StepStatus::Completed),
        ]
    );
    assert_eq!(second.outcome(), RunOutcome::Succeeded);
    assert_eq!(*b_keys.lock().unwrap(), ["run-9:b", "run-9:b"], "the key is stable across resume");
    assert_eq!(second.completion_order(), &[sid("b"), sid("c")], "carried-over steps are not in this run's order");

    let third = resume(&second, &p, &ResumeOptions::default()).await.unwrap();
    assert!(third.steps().iter().all(|s| s.status() == StepStatus::CompletedEarlier));
    assert_eq!(third.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn resume_refuses_a_changed_plan_before_running_anything() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Err(fatal("x")))]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    let changed = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);
    let err = resume(&first, &changed, &ResumeOptions::default()).await.unwrap_err();
    assert_eq!(err, ResumeError::FingerprintMismatch { record: p.fingerprint(), plan: changed.fingerprint() });
    assert_eq!(log.starts(), ["a"], "nothing ran on the refused resume");
    let domain: michi_core::DomainError = err.into();
    assert_eq!(domain.code, michi_core::ErrorCode::Conflict);

    let forged = RunRecord::from_parts(
        rid("r"),
        p.fingerprint(),
        vec![StepRecord::in_flight(sid("a"), "A"), StepRecord::in_flight(sid("zzz"), "Z")],
    );
    assert_eq!(
        resume(&forged, &p, &ResumeOptions::default()).await.unwrap_err(),
        ResumeError::UnknownStep { step_id: sid("zzz") }
    );
}

#[tokio::test(start_paused = true)]
async fn an_unknown_outcome_is_kept_until_the_caller_verifies_it() {
    let log = Log::new();
    let charge = Scripted::new(&log, "charge", vec![Act::Err(http(504)), Act::Ok]);
    let calls = Arc::clone(&charge.calls);
    let p = plan(vec![exec("charge", &[], charge), act(&log, "email", &["charge"], Act::Ok)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&first, "charge"), StepStatus::OutcomeUnknown(UnknownCause::Gateway));

    let kept = resume(&first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a non-idempotent ambiguous step is never replayed blindly");
    assert_eq!(status(&kept, "charge"), StepStatus::OutcomeUnknown(UnknownCause::Gateway));
    assert_eq!(status(&kept, "email"), StepStatus::Skipped(SkipReason::PrerequisiteFailed));
    assert_eq!(kept.outcome(), RunOutcome::OutcomeUnknown);

    let applied =
        resume(&kept, &p, &ResumeOptions::default().verified(sid("charge"), Verification::Applied)).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(status(&applied, "charge"), StepStatus::CompletedEarlier);
    assert_eq!(status(&applied, "email"), StepStatus::Completed);

    let not_applied =
        resume(&kept, &p, &ResumeOptions::default().verified(sid("charge"), Verification::NotApplied)).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2, "verified as not applied, so it runs again");
    assert_eq!(not_applied.outcome(), RunOutcome::Succeeded);

    let err = resume(&not_applied, &p, &ResumeOptions::default().verified(sid("email"), Verification::Applied))
        .await
        .unwrap_err();
    assert_eq!(err, ResumeError::NotAwaitingVerification { step_id: sid("email"), status: "completed" });
}

#[derive(Default)]
struct Store(Mutex<Vec<StepRecord>>);

impl StepObserver for Store {
    fn on_step_started(&self, _: &RunId, step_id: &michi_pipeline::StepId) {
        self.0.lock().unwrap().push(StepRecord::in_flight(step_id.clone(), "in flight"));
    }
    fn on_step_finished(&self, _: &RunId, record: &StepRecord) {
        let mut saved = self.0.lock().unwrap();
        saved.retain(|r| r.step_id() != record.step_id());
        saved.push(record.clone());
    }
}

#[tokio::test(start_paused = true)]
async fn after_a_crash_a_step_left_running_is_ambiguous_unless_idempotent() {
    let log = Log::new();
    let store = Arc::new(Store::default());
    let p = plan(vec![
        act(&log, "done", &[], Act::Ok),
        exec("idem", &["done"], Scripted::new(&log, "idem", vec![Act::Hang, Act::Ok]).idempotent()),
        act(&log, "once", &["done"], Act::Hang),
    ]);
    let opts = ExecOptions::new()
        .observer(store.clone())
        .mode(michi_pipeline::Mode::Parallel { max_concurrency: std::num::NonZeroUsize::new(4).unwrap() });
    // Simulate a crash: drop the run while idem and once are in flight.
    let crashed = tokio::time::timeout(Duration::from_millis(10), execute(&p, rid("r"), &opts)).await;
    assert!(crashed.is_err());
    let saved = store.0.lock().unwrap().clone();
    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), saved);
    assert_eq!(status(&rebuilt, "idem"), StepStatus::Running);

    let resumed = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(status(&resumed, "done"), StepStatus::CompletedEarlier);
    assert_eq!(status(&resumed, "idem"), StepStatus::Completed, "idempotent, so safe to run again");
    assert_eq!(status(&resumed, "once"), StepStatus::OutcomeUnknown(UnknownCause::Interrupted));
    assert_eq!(log.starts().iter().filter(|s| *s == "once").count(), 1, "the in-flight step was not re-run");
}

#[tokio::test(start_paused = true)]
async fn not_run_skipped_and_panicked_steps_on_resume() {
    let log = Log::new();
    let registry = Arc::new(BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(5))));
    let p = plan(vec![
        exec("trip", &[], Scripted::new(&log, "trip", vec![Act::Err(fatal("x")), Act::Ok])).breaker_key("svc"),
        exec("open", &[], Scripted::new(&log, "open", vec![Act::Ok])).breaker_key("svc"),
        exec("boom", &[], Scripted::new(&log, "boom", vec![Act::Panic, Act::Ok])),
    ]);
    let opts = ExecOptions::new().breakers(Arc::clone(&registry));
    let first = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&first, "open"), StepStatus::NotRun(michi_pipeline::NotRunReason::CircuitOpen));
    assert_eq!(status(&first, "boom"), StepStatus::Panicked);

    tokio::time::advance(Duration::from_secs(5)).await;
    let second = resume(&first, &p, &ResumeOptions::new(opts.clone())).await.unwrap();
    assert_eq!(status(&second, "trip"), StepStatus::Completed, "failed steps run again (the probe succeeded)");
    assert_eq!(status(&second, "open"), StepStatus::Completed, "a not-run step never ran, so it runs");
    assert_eq!(status(&second, "boom"), StepStatus::Panicked, "a panic may have taken effect: kept until verified");

    let third =
        resume(&second, &p, &ResumeOptions::new(opts).verified(sid("boom"), Verification::NotApplied)).await.unwrap();
    assert_eq!(third.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_run_resumes_to_completion_and_the_observer_sees_carried_over_steps() {
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);
    let token = CancellationToken::new();
    let cancel_after_a = {
        let token = token.clone();
        move |_: &RunId, r: &StepRecord| {
            if r.step_id().as_str() == "a" {
                token.cancel();
            }
        }
    };
    let first = execute(&p, rid("r"), &ExecOptions::new().cancellation(token).observer(Arc::new(cancel_after_a))).await;
    assert_eq!(status(&first, "b"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(first.outcome(), RunOutcome::Cancelled);

    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = {
        let seen = Arc::clone(&seen);
        move |_: &RunId, r: &StepRecord| seen.lock().unwrap().push(format!("{}:{}", r.step_id(), r.status().label()))
    };
    let opts = ResumeOptions::new(ExecOptions::new().observer(Arc::new(observer)));
    let second = resume(&first, &p, &opts).await.unwrap();
    assert_eq!(second.outcome(), RunOutcome::Succeeded);
    assert_eq!(*seen.lock().unwrap(), ["a:completed_earlier", "b:completed"]);
}

#[tokio::test(start_paused = true)]
async fn a_record_listing_a_step_twice_resumes_from_its_first_entry() {
    // `resume` looks prior steps up through an index built once; with duplicate entries (possible via `from_parts`)
    // it must keep `RunRecord::step`'s first-match answer.
    let log = Log::new();
    let p = plan(vec![act(&log, "a", &[], Act::Ok), act(&log, "b", &["a"], Act::Ok)]);
    let first = execute(&p, rid("r"), &ExecOptions::new()).await;
    let completed_a = first.step(&sid("a")).unwrap().clone();

    let done_first = RunRecord::from_parts(
        rid("r"),
        p.fingerprint(),
        vec![completed_a.clone(), StepRecord::in_flight(sid("a"), "A")],
    );
    assert_eq!(done_first.step(&sid("a")).unwrap().status(), StepStatus::Completed);
    let resumed = resume(&done_first, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(status(&resumed, "a"), StepStatus::CompletedEarlier);
    assert_eq!(status(&resumed, "b"), StepStatus::Completed);

    let running_first =
        RunRecord::from_parts(rid("r"), p.fingerprint(), vec![StepRecord::in_flight(sid("a"), "A"), completed_a]);
    let verified = ResumeOptions::default().verified(sid("a"), Verification::Applied);
    let resumed = resume(&running_first, &p, &verified).await.unwrap();
    assert_eq!(status(&resumed, "a"), StepStatus::CompletedEarlier, "the first entry is the one awaiting verification");
    assert_eq!(log.starts().iter().filter(|s| *s == "a").count(), 1, "a never ran again");
}

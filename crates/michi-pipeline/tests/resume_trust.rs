//! Resume trusts the current plan as well as the record: a step whose replay class was tightened since the run is
//! held, not re-invoked blindly.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::*;
use michi_pipeline::{execute, resume, ExecOptions, ResumeOptions, RunOutcome, StepStatus, UnknownCause, Verification};

#[tokio::test(start_paused = true)]
async fn a_step_made_not_idempotent_since_the_run_is_held_not_re_invoked() {
    let log = Log::new();
    // Run 1: idempotent, ends Failed(500).
    let first_step = Scripted::new(&log, "pay", vec![Act::Err(http(500))]).idempotent();
    let p1 = plan(vec![exec("pay", &[], first_step)]);
    let first = execute(&p1, rid("r"), &ExecOptions::new().retry(retries(0, 1))).await;
    assert_eq!(status(&first, "pay"), StepStatus::Failed);

    // The plan is edited: same structure, now not idempotent.
    let second_step = Scripted::new(&log, "pay", vec![Act::Ok]);
    let calls = Arc::clone(&second_step.calls);
    let p2 = plan(vec![exec("pay", &[], second_step)]);
    assert_eq!(p1.fingerprint(), p2.fingerprint());

    let held = resume(&first, &p2, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the effect of the failed attempt is unknown: not re-invoked");
    assert_eq!(status(&held, "pay"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));
    assert_eq!(held.outcome(), RunOutcome::OutcomeUnknown);

    // A verdict is accepted for it, even though the old record called the step idempotent.
    let verified =
        resume(&first, &p2, &ResumeOptions::default().verified(sid("pay"), Verification::NotApplied)).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(verified.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_step_made_idempotent_since_the_run_is_still_held_because_the_record_is_stricter() {
    let log = Log::new();
    let p1 = plan(vec![act(&log, "pay", &[], Act::Err(down("lost")))]);
    let first = execute(&p1, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&first, "pay"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));

    let step = Scripted::new(&log, "pay", vec![Act::Ok]).idempotent();
    let calls = Arc::clone(&step.calls);
    let p2 = plan(vec![exec("pay", &[], step)]);
    let kept = resume(&first, &p2, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the run did not know the step was idempotent");
    assert_eq!(kept.outcome(), RunOutcome::OutcomeUnknown);
}

//! With the `serde` feature a run record round-trips, so callers can persist it and resume later.

#![cfg(feature = "serde")]

mod common;

use common::*;
use michi_pipeline::{execute, resume, ExecOptions, ResumeOptions, RunOutcome, RunRecord, StepId};

#[tokio::test(start_paused = true)]
async fn a_record_round_trips_through_json_and_resumes() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "a", &[], Act::Ok),
        exec("b", &["a"], Scripted::new(&log, "b", vec![Act::Err(http(504)), Act::Ok])),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    let json = serde_json::to_string(&record).unwrap();
    assert!(json.contains(&format!("\"plan_fingerprint\":\"{}\"", p.fingerprint())), "{json}");
    assert!(json.contains("\"outcome_unknown\":\"gateway\""), "{json}");
    let back: RunRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, record);
    let without_counter = json.replace(",\"observer_panics\":0", "");
    assert_ne!(without_counter, json);
    let older: RunRecord = serde_json::from_str(&without_counter).unwrap();
    assert_eq!(older, record, "a record saved without observer_panics reads as 0");
    let resumed =
        resume(&back, &p, &ResumeOptions::default().verified(sid("b"), michi_pipeline::Verification::NotApplied))
            .await
            .unwrap();
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn a_0_4_failed_error_without_effect_reads_as_unknown_and_resume_holds_the_step() {
    let log = Log::new();
    let charge = Scripted::new(&log, "charge", vec![Act::Err(michi_pipeline::ExecutionError::refused("x")), Act::Ok]);
    let calls = std::sync::Arc::clone(&charge.calls);
    let p = plan(vec![exec("charge", &[], charge)]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    let json = serde_json::to_string(&record).unwrap();
    assert!(json.contains("\"effect\":\"not_applied\""), "{json}");
    assert!(json.contains("\"class\":\"refused\""), "{json}");

    // What 0.4 wrote for `Failed { retryable: false }`: no `effect`, and the attempt class `definitive`.
    let old =
        json.replace(",\"effect\":\"not_applied\"", "").replace("\"class\":\"refused\"", "\"class\":\"definitive\"");
    let back: RunRecord = serde_json::from_str(&old).unwrap();
    let step = back.step(&sid("charge")).unwrap();
    assert_eq!(step.error().map(michi_pipeline::ExecutionError::effect), Some(michi_pipeline::Effect::Unknown));
    assert_eq!(step.attempts()[0].class(), Some(michi_pipeline::ReplayClass::Ambiguous));
    assert!(step.awaits_verification());
    assert_eq!(back.needs_verification().count(), 1);

    let held = resume(&back, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "a 0.4 failure is not re-run blindly");
    assert_eq!(
        held.step(&sid("charge")).unwrap().status(),
        michi_pipeline::StepStatus::OutcomeUnknown(michi_pipeline::UnknownCause::StepFailed),
        "the held step now names the right recovery: verify, then resume"
    );
    let verified = ResumeOptions::default().verified(sid("charge"), michi_pipeline::Verification::NotApplied);
    let done = resume(&held, &p, &verified).await.unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(done.outcome(), RunOutcome::Succeeded);
}

#[test]
fn ids_are_validated_when_deserialized() {
    assert!(serde_json::from_str::<StepId>("\"ok-id\"").is_ok());
    assert!(serde_json::from_str::<StepId>("\"not:ok\"").is_err());
}

#[tokio::test(start_paused = true)]
async fn a_step_whose_deadline_group_closed_round_trips_and_resume_runs_it() {
    let log = Log::new();
    let p = plan(vec![
        act(&log, "early", &[], Act::Ok).deadline_group("g"),
        act(&log, "slow", &[], after(100, Act::Ok)),
        act(&log, "late", &["slow"], Act::Ok).deadline_group("g"),
    ]);
    let opts = ExecOptions::new().group_deadline("g", std::time::Duration::from_millis(10));
    let record = execute(&p, rid("r"), &opts).await;
    let json = serde_json::to_string(&record).unwrap();
    assert!(json.contains("\"deadline_passed\""), "{json}");
    let back: RunRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, record);
    let resumed = resume(&back, &p, &ResumeOptions::new(opts)).await.unwrap();
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
}

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

#[test]
fn ids_are_validated_when_deserialized() {
    assert!(serde_json::from_str::<StepId>("\"ok-id\"").is_ok());
    assert!(serde_json::from_str::<StepId>("\"not:ok\"").is_err());
}

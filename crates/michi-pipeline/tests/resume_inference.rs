//! A completed dependent proves its prerequisites completed, so a store that lost their records does not make resume
//! run them again; a contradictory record is refused.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use common::*;
use michi_pipeline::{
    execute, resume, ExecOptions, Plan, ResumeError, ResumeOptions, RunOutcome, RunRecord, StepRecord, StepStatus,
};

fn chain(log: &Log) -> (Plan, Vec<Arc<AtomicU32>>) {
    let a = Scripted::new(log, "a", vec![Act::Ok]);
    let b = Scripted::new(log, "b", vec![Act::Ok]);
    let c = Scripted::new(log, "c", vec![Act::Ok]);
    let calls = vec![Arc::clone(&a.calls), Arc::clone(&b.calls), Arc::clone(&c.calls)];
    (plan(vec![exec("a", &[], a), exec("b", &["a"], b), exec("c", &["b"], c)]), calls)
}

fn total(calls: &[Arc<AtomicU32>]) -> u32 {
    calls.iter().map(|c| c.load(Ordering::SeqCst)).sum()
}

/// A record of the chain in which `a` failed, for building contradictions.
async fn failed_a(log: &Log) -> RunRecord {
    let failing = plan(vec![
        act(log, "a", &[], Act::Err(fatal("nope"))),
        act(log, "b", &["a"], Act::Ok),
        act(log, "c", &["b"], Act::Ok),
    ]);
    let run = execute(&failing, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&run, "a"), StepStatus::Failed);
    run
}

#[tokio::test(start_paused = true)]
async fn lost_prerequisite_records_are_inferred_completed_transitively_and_not_re_run() {
    let log = Log::new();
    let (p, calls) = chain(&log);
    let full = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(total(&calls), 3);
    // The store lost a's and b's records but kept c's.
    let saved = vec![full.step(&sid("c")).unwrap().clone()];
    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, saved).unwrap();
    for id in ["a", "b"] {
        assert_eq!(status(&rebuilt, id), StepStatus::CompletedEarlier, "{id} is inferred from a completed dependent");
        let reason = rebuilt.step(&sid(id)).unwrap().reason().unwrap();
        assert!(reason.contains("inferred from a completed dependent"), "{reason}");
    }
    assert_eq!(rebuilt.outcome(), RunOutcome::Succeeded);

    let resumed = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(total(&calls), 3, "nothing ran again");
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
}

#[tokio::test(start_paused = true)]
async fn an_in_flight_prerequisite_of_a_completed_dependent_is_inferred_completed() {
    let log = Log::new();
    let (p, calls) = chain(&log);
    let full = execute(&p, rid("r"), &ExecOptions::new()).await;
    let saved = vec![
        StepRecord::in_flight(sid("a"), "A"),
        StepRecord::in_flight(sid("b"), "B"),
        full.step(&sid("c")).unwrap().clone(),
    ];
    let rebuilt = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, saved).unwrap();
    assert_eq!(status(&rebuilt, "a"), StepStatus::CompletedEarlier);
    assert_eq!(status(&rebuilt, "b"), StepStatus::CompletedEarlier);
    let resumed = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
    assert_eq!(total(&calls), 3);
}

#[tokio::test(start_paused = true)]
async fn a_prerequisite_recorded_failed_under_a_completed_dependent_is_refused() {
    let log = Log::new();
    let (p, _) = chain(&log);
    let full = execute(&p, rid("r"), &ExecOptions::new()).await;
    let failed = failed_a(&log).await;
    let saved = vec![
        failed.step(&sid("a")).unwrap().clone(),
        full.step(&sid("b")).unwrap().clone(),
        full.step(&sid("c")).unwrap().clone(),
    ];
    let err = RunRecord::from_parts(rid("r"), p.fingerprint(), &p, saved).unwrap_err();
    assert!(
        matches!(&err, ResumeError::ContradictoryRecord { step_id, prerequisite, .. }
            if step_id.as_str() == "b" && prerequisite.as_str() == "a"),
        "{err:?}"
    );
    let domain: michi_core::DomainError = err.into();
    assert_eq!(domain.code, michi_core::ErrorCode::Conflict);
}

#[cfg(feature = "serde")]
#[tokio::test(start_paused = true)]
async fn resume_infers_and_refuses_on_a_deserialized_record_too() {
    let log = Log::new();
    let (p, calls) = chain(&log);
    let full = execute(&p, rid("r"), &ExecOptions::new()).await;
    let mut json = serde_json::to_value(&full).unwrap();
    // `a` shows as in flight in the saved record although `b` and `c` completed.
    json["steps"][0] = serde_json::to_value(StepRecord::in_flight(sid("a"), "A")).unwrap();
    let rebuilt: RunRecord = serde_json::from_value(json.clone()).unwrap();
    let resumed = resume(&rebuilt, &p, &ResumeOptions::default()).await.unwrap();
    assert_eq!(total(&calls), 3, "a is not run again");
    assert_eq!(resumed.outcome(), RunOutcome::Succeeded);

    // `a` failed although `b` completed: refused before anything runs.
    let failed = failed_a(&log).await;
    json["steps"][0] = serde_json::to_value(failed.step(&sid("a")).unwrap()).unwrap();
    let contradictory: RunRecord = serde_json::from_value(json).unwrap();
    let err = resume(&contradictory, &p, &ResumeOptions::default()).await.unwrap_err();
    assert!(matches!(err, ResumeError::ContradictoryRecord { .. }), "{err:?}");
    assert_eq!(total(&calls), 3);
}

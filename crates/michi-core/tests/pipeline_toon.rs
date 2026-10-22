//! `Pipeline::render()` with every `StepStatus` variant decodes under the strict TOON 4.4 test decoder.

mod support;

use michi_core::pipeline::{NotRunReason, Pipeline, PipelineStep, SkipReason, StepStatus, UnknownCause};
use support::toon_parser::{decode, Node, Prim};

fn every_status() -> Vec<StepStatus> {
    vec![
        StepStatus::Pending,
        StepStatus::Running,
        StepStatus::Completed,
        StepStatus::CompletedEarlier,
        StepStatus::Failed,
        StepStatus::OutcomeUnknown(UnknownCause::Timeout),
        StepStatus::OutcomeUnknown(UnknownCause::Gateway),
        StepStatus::OutcomeUnknown(UnknownCause::Cancelled),
        StepStatus::OutcomeUnknown(UnknownCause::Interrupted),
        StepStatus::Skipped(SkipReason::PrerequisiteFailed),
        StepStatus::Skipped(SkipReason::Cancelled),
        StepStatus::NotRun(NotRunReason::CircuitOpen),
        StepStatus::Panicked,
    ]
}

#[test]
fn every_status_renders_a_conformant_status_and_reason_cell() {
    let statuses = every_status();
    let pipeline = Pipeline::new(
        "run-1",
        statuses.iter().enumerate().map(|(i, s)| PipelineStep::new(format!("s{i}"), format!("Step {i}"), *s)).collect(),
    );
    let out = pipeline.render();
    let doc = decode(&out).unwrap_or_else(|e| panic!("non-conformant TOON: {e}\n{out}"));
    let Some(Node::Table { fields, rows }) = doc.get("step") else { panic!("expected a step table, got {out}") };
    assert_eq!(fields, &["id", "name", "status", "reason"]);
    assert_eq!(rows.len(), statuses.len());
    for (row, status) in rows.iter().zip(&statuses) {
        assert_eq!(row[2], Prim::str(status.label()));
        match status.reason() {
            Some(reason) => assert_eq!(row[3], Prim::str(reason)),
            None => assert_eq!(row[3], Prim::Null),
        }
    }
}

#[test]
fn status_labels_and_reasons_are_distinct_snake_case_words() {
    let pairs: Vec<(&str, Option<&str>)> = every_status().iter().map(|s| (s.label(), s.reason())).collect();
    for (i, a) in pairs.iter().enumerate() {
        for b in &pairs[i + 1..] {
            assert_ne!(a, b, "two statuses render identically");
        }
        assert!(a.0.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{a:?}");
    }
    assert_eq!(StepStatus::Skipped(SkipReason::PrerequisiteFailed).reason(), Some("prerequisite_failed"));
    assert_eq!(StepStatus::NotRun(NotRunReason::CircuitOpen).label(), "not_run");
    assert_eq!(StepStatus::OutcomeUnknown(UnknownCause::Gateway).label(), "outcome_unknown");
    assert_eq!(StepStatus::CompletedEarlier.label(), "completed_earlier");
    assert_eq!(StepStatus::Completed.reason(), None);
}

#[test]
fn only_completed_and_completed_earlier_are_success() {
    for status in every_status() {
        let expected = matches!(status, StepStatus::Completed | StepStatus::CompletedEarlier);
        assert_eq!(status.is_success(), expected, "{status:?}");
    }
}

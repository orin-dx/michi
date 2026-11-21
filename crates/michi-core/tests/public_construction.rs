//! 0.4 API hygiene: `StatusItem`, `KvItem`, `FailedOp`, `CompletedOp`, `SkippedOp`, `PartialSuccess`, `RecoveryHint`,
//! `PipelineStep` and `Pipeline` are `#[non_exhaustive]` (struct literals are rejected, see `tests/ui_fail.rs`), so
//! each has a public constructor that builds it from outside the crate. The plain records keep public fields, readable
//! and writable after construction; `PartialSuccess` keeps its fields private and is read through getters, so one
//! index cannot get two outcomes.
//! pipeline AC-008/AC-009: `PipelineStep`/`Pipeline` fields are directly readable and writable from outside the
//! module.

use michi_core::pipeline::{Pipeline, PipelineStep, StepStatus};
use michi_core::{CompletedOp, FailedOp, Health, KvItem, KvValue, PartialSuccess, RecoveryHint, SkippedOp, StatusItem};

#[test]
fn status_item_constructor_builds_it_from_outside_the_crate() {
    let item = StatusItem::new("index", KvValue::Int(1)).health(Health::Ok);
    assert_eq!(item.key, "index");
    assert_eq!(item.value, KvValue::Int(1));
    assert_eq!(item.health, Some(Health::Ok));
    assert_eq!(StatusItem::new("cache", KvValue::Missing).health, None);
}

#[test]
fn kv_item_constructor_builds_it_from_outside_the_crate() {
    let mut item = KvItem::new("name", KvValue::Text("michi".into()));
    assert_eq!(item.key, "name");
    assert_eq!(item.value, KvValue::Text("michi".into()));
    item.value = KvValue::Bool(true);
    assert_eq!(item.value, KvValue::Bool(true));
}

#[test]
fn failed_op_and_partial_success_constructors_build_them_from_outside_the_crate() {
    let failed = FailedOp::new(2, "assign_user", "User 'ghost' not found").recovery(RecoveryHint::new("assign_user"));
    assert_eq!(failed.index, 2);
    assert_eq!(failed.operation, "assign_user");
    assert_eq!(failed.reason, "User 'ghost' not found");
    assert_eq!(failed.recovery, Some(RecoveryHint::new("assign_user")));
    assert_eq!(FailedOp::new(0, "x", "y").recovery, None);

    let skipped = SkippedOp::new(3, "notify").reason("budget_exhausted");
    assert_eq!(
        (skipped.index, skipped.operation.as_str(), skipped.reason.as_deref()),
        (3, "notify", Some("budget_exhausted"))
    );
    assert_eq!(SkippedOp::new(0, "x").reason, None);
    let completed = CompletedOp::new(0, "create_issue");
    assert_eq!((completed.index, completed.operation.as_str()), (0, "create_issue"));

    let ps = PartialSuccess::try_new(vec![completed.clone()], vec![failed], vec![skipped.clone()]).expect("distinct");
    assert_eq!(ps.completed(), [completed]);
    assert_eq!(ps.failed().len(), 1);
    assert_eq!(ps.skipped(), [skipped]);
    assert_eq!(Ok(PartialSuccess::default()), PartialSuccess::try_new(vec![], vec![], vec![]));
}

#[test]
fn ac008_ac009_pipeline_and_pipeline_step_fields_are_readable_and_writable_from_outside() {
    let mut step = PipelineStep::new("s", "n", StepStatus::Pending);
    step.status = StepStatus::Completed;
    assert_eq!(step.id, "s");
    assert_eq!(step.name, "n");
    assert_eq!(step.status, StepStatus::Completed);

    let mut pipeline = Pipeline::new(vec![step]);
    pipeline.steps.push(PipelineStep::new("t", "m", StepStatus::Pending));
    assert_eq!(pipeline.steps.len(), 2);
}

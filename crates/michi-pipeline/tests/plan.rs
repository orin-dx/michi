//! Change 2 (definition vs run): one validation pass returning every violation, the dry-run order and the plan
//! fingerprint.

mod common;

use common::*;
use michi_pipeline::{step_fn, ExecStep, Plan, PlanFingerprint, PlanViolation};

fn noop(id: &str, deps: &[&str]) -> ExecStep {
    exec(id, deps, step_fn(|_| async { Ok(()) }))
}

#[test]
fn every_violation_is_reported_at_once() {
    let err =
        Plan::new(vec![noop("a", &["ghost"]), noop("b", &["b"]), noop("a", &[]), noop("c", &["d"]), noop("d", &["c"])])
            .unwrap_err();
    assert_eq!(
        err.violations(),
        &[
            PlanViolation::DuplicateStepId { step_id: sid("a") },
            PlanViolation::UnknownDependency { step_id: sid("a"), dependency: sid("ghost") },
            PlanViolation::SelfDependency { step_id: sid("b") },
            PlanViolation::Cycle { step_ids: vec![sid("c"), sid("d")] },
        ]
    );
    let text = err.to_string();
    assert!(text.starts_with("invalid plan: duplicate step id a; "), "{text}");
    let domain: michi_core::DomainError = err.into();
    assert_eq!(domain.code, michi_core::ErrorCode::InvalidInput);
    assert!(!domain.retryable);
}

#[test]
fn a_duplicate_id_that_a_dependency_names_reports_only_the_duplicate() {
    let err = Plan::new(vec![noop("a", &[]), noop("a", &[]), noop("b", &["a"])]).unwrap_err();
    assert_eq!(err.violations(), &[PlanViolation::DuplicateStepId { step_id: sid("a") }]);
}

#[test]
fn a_cycle_reports_only_its_members_not_steps_hanging_off_it() {
    let err =
        Plan::new(vec![noop("a", &["b"]), noop("b", &["a"]), noop("tail", &["a"]), noop("head", &[])]).unwrap_err();
    assert_eq!(err.violations(), &[PlanViolation::Cycle { step_ids: vec![sid("a"), sid("b")] }]);
}

#[test]
fn execution_order_is_topological_with_ties_broken_by_declaration_order() {
    let plan = plan(vec![noop("c", &["a"]), noop("a", &[]), noop("b", &[]), noop("d", &["c", "b"])]);
    let order: Vec<&str> = plan.execution_order().into_iter().map(|id| id.as_str()).collect();
    assert_eq!(order, ["a", "c", "b", "d"], "c (declared first) runs as soon as a is done, before b");
}

#[test]
fn repeated_dependencies_collapse_and_an_empty_plan_is_valid() {
    let p = plan(vec![noop("a", &[]), noop("b", &["a", "a"])]);
    assert_eq!(p.execution_order().len(), 2);
    assert!(plan(vec![]).is_empty());
}

#[test]
fn a_100k_step_chain_and_cycle_validate_without_overflowing_the_stack() {
    let chain = |close: bool| {
        (0..100_000)
            .map(|i| {
                let dep = if i == 0 {
                    if close {
                        Some(99_999)
                    } else {
                        None
                    }
                } else {
                    Some(i - 1)
                };
                let id = format!("s{i}");
                match dep {
                    Some(d) => noop(&id, &[&format!("s{d}")]),
                    None => noop(&id, &[]),
                }
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(Plan::new(chain(false)).unwrap().execution_order().len(), 100_000);
    match Plan::new(chain(true)).unwrap_err().violations() {
        [PlanViolation::Cycle { step_ids }] => assert_eq!(step_ids.len(), 100_000),
        other => panic!("expected one cycle, got {} violations", other.len()),
    }
}

#[test]
fn fingerprint_covers_ids_and_dependencies_only() {
    let base = plan(vec![noop("a", &[]), noop("b", &["a"])]).fingerprint();
    let reordered = plan(vec![noop("b", &["a"]), noop("a", &[])]).fingerprint();
    let renamed = plan(vec![ExecStep::new(sid("a"), "Another name", step_fn(|_| async { Ok(()) })), noop("b", &["a"])])
        .fingerprint();
    let rewired = plan(vec![noop("a", &[]), noop("b", &[])]).fingerprint();
    let extra = plan(vec![noop("a", &[]), noop("b", &["a"]), noop("c", &[])]).fingerprint();
    assert_eq!(base, reordered, "declaration order is not structure");
    assert_eq!(base, renamed, "display names are not structure");
    assert_ne!(base, rewired);
    assert_ne!(base, extra);
    let text = base.to_string();
    assert_eq!(text.len(), 16);
    assert_eq!(text.parse::<PlanFingerprint>().unwrap(), base);
    assert!("xyz".parse::<PlanFingerprint>().is_err());
}

#[test]
fn the_fingerprint_keeps_its_published_value_so_saved_records_still_resume() {
    // FNV-1a over "a␟␝ b␟a␞␝ c␟a␞b␞␝" (ids and sorted prerequisites), computed independently of michi-graph.
    let fp = plan(vec![noop("c", &["b", "a"]), noop("a", &[]), noop("b", &["a"])]).fingerprint();
    assert_eq!(fp.to_string(), "b7b6687929998429");
}

#[test]
fn a_step_stuck_between_two_cycles_is_not_reported_as_on_a_cycle() {
    // x depends on cycle {a, b}, and cycle {c, d} depends on x: x can never run, but it lies on no cycle.
    let err = Plan::new(vec![
        noop("a", &["b"]),
        noop("b", &["a"]),
        noop("x", &["a"]),
        noop("c", &["d", "x"]),
        noop("d", &["c"]),
    ])
    .unwrap_err();
    assert_eq!(err.violations(), &[PlanViolation::Cycle { step_ids: vec![sid("a"), sid("b"), sid("c"), sid("d")] }]);
}

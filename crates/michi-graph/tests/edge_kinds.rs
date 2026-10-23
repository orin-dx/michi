//! Edge kinds survive validation: `Dag::prerequisite_edges` and per-edge readiness (`Dag::ready_by_edge`).

use std::collections::BTreeMap;

use michi_graph::{Dag, GraphBuilder, Order, UnknownTargets};

/// How a release operation depends on another: a publish may follow a sibling's publish once it is issued, while a
/// tag waits for the publish to be confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Needs {
    Issued,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Issued,
    Confirmed,
}

fn release() -> Dag<&'static str, Needs> {
    let mut b = GraphBuilder::<&str, Needs>::new();
    b.node("publish:core");
    let cli = b.node("publish:cli");
    let tag = b.node("tag");
    b.depends_on(cli, "publish:core", Needs::Issued);
    b.depends_on(tag, "publish:core", Needs::Confirmed).depends_on(tag, "publish:cli", Needs::Confirmed);
    b.build(UnknownTargets::Error).validate(Order::Key).expect("valid")
}

fn ready(dag: &Dag<&'static str, Needs>, state: &BTreeMap<&str, State>) -> Vec<&'static str> {
    dag.ready_by_edge(
        |k| state.get(k) == Some(&State::Pending),
        |p, needs| {
            matches!(
                (needs, state.get(p)),
                (Needs::Issued, Some(State::Issued | State::Confirmed)) | (Needs::Confirmed, Some(State::Confirmed))
            )
        },
    )
    .map(|ix| *dag.key(ix).expect("index from this dag"))
    .collect()
}

#[test]
fn a_publish_wave_follows_issued_siblings_while_the_tag_waits_for_confirmation() {
    let dag = release();
    let mut state: BTreeMap<&str, State> =
        [("publish:core", State::Pending), ("publish:cli", State::Pending), ("tag", State::Pending)].into();
    assert_eq!(ready(&dag, &state), ["publish:core"]);

    state.insert("publish:core", State::Issued);
    assert_eq!(ready(&dag, &state), ["publish:cli"], "a sibling publish needs only the issued publish");

    state.insert("publish:cli", State::Issued);
    assert!(ready(&dag, &state).is_empty(), "the tag waits for both publishes to be confirmed");

    state.insert("publish:core", State::Confirmed);
    state.insert("publish:cli", State::Confirmed);
    assert_eq!(ready(&dag, &state), ["tag"]);
}

#[test]
fn every_kind_on_a_pair_is_kept_and_must_be_satisfied() {
    let mut b = GraphBuilder::<&str, Needs>::new();
    b.node("a");
    let c = b.node("c");
    b.depends_on(c, "a", Needs::Confirmed).depends_on(c, "a", Needs::Issued).depends_on(c, "a", Needs::Issued);
    let dag = b.build(UnknownTargets::Error).validate(Order::Declaration).expect("valid");
    let c = dag.index_of("c").expect("c");
    let a = dag.index_of("a").expect("a");
    assert_eq!(dag.prerequisites(c), [a], "prerequisites lists the node once");
    assert_eq!(dag.prerequisite_edges(c), [(a, Needs::Issued), (a, Needs::Confirmed)], "one entry per kind, sorted");

    let state: BTreeMap<&str, State> = [("a", State::Issued), ("c", State::Pending)].into();
    let only_issued: Vec<_> = dag
        .ready_by_edge(
            |k| state.get(k) == Some(&State::Pending),
            |p, needs| *needs == Needs::Issued && state.get(p) == Some(&State::Issued),
        )
        .collect();
    assert!(only_issued.is_empty(), "the Confirmed edge on the same pair is not met");
}

#[test]
fn filtered_out_kinds_are_not_on_the_dag() {
    let mut b = GraphBuilder::<&str, Needs>::new();
    b.node("a");
    let c = b.node("c");
    b.depends_on(c, "a", Needs::Issued).depends_on(c, "a", Needs::Confirmed);
    let dag =
        b.build(UnknownTargets::Error).validate_filtered(|e| *e.kind == Needs::Confirmed, Order::Key).expect("valid");
    let (c, a) = (dag.index_of("c").expect("c"), dag.index_of("a").expect("a"));
    assert_eq!(dag.prerequisite_edges(c), [(a, Needs::Confirmed)]);
}

#[test]
fn untyped_dags_have_unit_kinds_and_foreign_indices_are_empty() {
    let dag = Dag::from_nodes([("a", vec![]), ("b", vec!["a"])], Order::Declaration).expect("valid");
    let (a, b) = (dag.index_of("a").expect("a"), dag.index_of("b").expect("b"));
    assert_eq!(dag.prerequisite_edges(b), [(a, ())]);
    let ready: Vec<_> = dag.ready_by_edge(|_| true, |_, ()| false).collect();
    assert_eq!(ready, [a], "a has no prerequisites; b's edge is unmet");

    let bigger = Dag::from_nodes([("x", vec![]), ("y", vec![]), ("z", vec!["x"])], Order::Key).expect("valid");
    let foreign = bigger.index_of("z").expect("z");
    assert!(dag.prerequisite_edges(foreign).is_empty());
}

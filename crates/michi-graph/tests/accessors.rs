//! The small public accessors and the message text of violations, which the behavior tests in `graph.rs` do not
//! reach. Found by a `cargo mutants` run: replacing each of these with a constant still passed every test.

use michi_graph::{Dag, GraphBuilder, Order, UnknownTargets, Violation};

fn dag(nodes: &[(&'static str, &[&'static str])]) -> Dag<&'static str> {
    Dag::from_nodes(nodes.iter().map(|&(k, deps)| (k, deps.iter().copied())), Order::Declaration).expect("valid dag")
}

fn found(nodes: &[(&'static str, &[&'static str])]) -> michi_graph::Violations<&'static str> {
    Dag::from_nodes(nodes.iter().map(|&(k, deps)| (k, deps.iter().copied())), Order::Declaration).expect_err("invalid")
}

#[test]
fn dag_lookups_by_key_position_and_index() {
    let d = dag(&[("a", &[]), ("b", &["a"]), ("c", &["a", "b"])]);
    assert!(!d.is_empty());
    assert_eq!(d.len(), 3);

    let b = d.index_of("b").expect("b is a node");
    assert_eq!(d.key(b), Some(&"b"));
    assert_eq!(d.index_of("nope"), None);

    // Declaration position 1 is `b`; a position past the end is none, including exactly one past.
    assert_eq!(d.node_at(1), Some(b));
    assert_eq!(d.node_at(0).and_then(|ix| d.key(ix)), Some(&"a"));
    assert_eq!(d.node_at(2).and_then(|ix| d.key(ix)), Some(&"c"));
    assert_eq!(d.node_at(3), None);
    assert_eq!(d.node_at(usize::MAX), None);
}

#[test]
fn dependents_are_the_nodes_that_name_a_prerequisite() {
    let d = dag(&[("a", &[]), ("b", &["a"]), ("c", &["a", "b"])]);
    let names = |key: &str| {
        let ix = d.index_of(key).expect("node");
        let mut keys: Vec<_> = d.dependents(ix).iter().map(|&i| *d.key(i).expect("node")).collect();
        keys.sort_unstable();
        keys
    };
    assert_eq!(names("a"), ["b", "c"]);
    assert_eq!(names("b"), ["c"]);
    assert!(names("c").is_empty());
}

#[test]
fn an_empty_dag_has_no_nodes_and_no_layers() {
    let d = dag(&[]);
    assert!(d.is_empty());
    assert_eq!(d.len(), 0);
    assert!(d.layers().is_empty());
    assert_eq!(d.node_at(0), None);

    let d = dag(&[("a", &[])]);
    assert!(!d.layers().is_empty());
    assert_eq!(d.layers().len(), 1);
}

#[test]
fn graph_lookups_and_duplicates() {
    let mut b = GraphBuilder::<&str>::new();
    assert!(b.clone().build(UnknownTargets::Error).is_empty());
    let a = b.node("a");
    b.node("b");
    b.node("a");
    b.depends_on(a, "ghost", ());
    let g = b.build(UnknownTargets::Ignore);

    assert!(!g.is_empty());
    assert_eq!(g.len(), 2);
    let a_ix = g.index_of("a").expect("a is a node");
    assert_eq!(g.key(a_ix), Some(&"a"));
    assert_eq!(g.index_of("ghost"), None);
    assert_eq!(g.duplicates(), ["a"], "one entry per later declaration");
    assert_eq!(g.dangling().len(), 1);
}

#[test]
fn violations_read_as_one_sentence_per_problem() {
    let v = found(&[("a", &["ghost"]), ("b", &["b"]), ("a", &[]), ("c", &["d"]), ("d", &["c"])]);
    assert_eq!(
        v.to_string(),
        "duplicate node a; a depends on unknown node ghost; b depends on itself; dependency cycle: c depends on d, which depends on c"
    );
    assert_eq!(v.as_slice().len(), 4);
    assert_eq!(v.iter().count(), 4);
    assert_eq!((&v).into_iter().count(), 4);
    assert_eq!(v.into_iter().count(), 4);
}

#[test]
fn each_violation_displays_on_its_own() {
    let v = found(&[("a", &["ghost"]), ("b", &["b"]), ("a", &[])]).into_vec();
    let text: Vec<String> = v.iter().map(ToString::to_string).collect();
    assert_eq!(text, ["duplicate node a", "a depends on unknown node ghost", "b depends on itself"]);
    assert!(matches!(v[0], Violation::DuplicateNode { key: "a" }));
}

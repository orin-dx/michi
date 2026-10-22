//! Regressions for defects found in review: filtered dangling edges, foreign declarations, self-loop paths and
//! many small cycles.

use michi_graph::{GraphBuilder, Order, UnknownTargets, Violation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Runtime,
    Dev,
}

#[test]
fn the_edge_filter_decides_dangling_edges_too() {
    let mut b = GraphBuilder::<&str, Kind>::new();
    let app = b.node("app");
    b.depends_on(app, "test-helper", Kind::Dev).depends_on(app, "missing", Kind::Runtime);
    let g = b.build(UnknownTargets::Error);

    let found =
        g.validate_filtered(|e| *e.kind == Kind::Runtime, Order::Key).expect_err("runtime edge unknown").into_vec();
    assert_eq!(found, [Violation::UnknownDependency { dependent: "app", prerequisite: "missing" }]);

    let mut seen_dangling = Vec::new();
    let dev_only = g.validate_filtered(
        |e| {
            if e.prerequisite_ix.is_none() {
                seen_dangling.push(*e.prerequisite);
            }
            *e.kind == Kind::Dev
        },
        Order::Key,
    );
    assert!(dev_only.is_err(), "the dev edge is dangling too, and kept");
    assert_eq!(seen_dangling, ["test-helper", "missing"], "the filter sees dangling edges in declaration order");

    assert!(g.validate_filtered(|e| e.prerequisite_ix.is_some(), Order::Key).is_ok());
}

#[test]
fn a_declaration_from_another_builder_adds_nothing() {
    let mut first = GraphBuilder::<&str>::new();
    let foreign = first.node("x");
    let mut second = GraphBuilder::<&str>::new();
    second.node("a");
    second.node("b");
    second.depends_on(foreign, "b", ());
    let dag = second.build(UnknownTargets::Error).validate(Order::Declaration).expect("no edges");
    let a = dag.index_of("a").expect("a");
    assert!(dag.prerequisites(a).is_empty(), "the foreign token must not name a");
}

#[test]
fn a_self_loop_is_the_shortest_loop_through_its_node() {
    let mut b = GraphBuilder::<&str>::new();
    let a = b.node("a");
    let c = b.node("c");
    b.depends_on(a, "a", ()).depends_on(a, "c", ()).depends_on(c, "a", ());
    let cycles = b.build(UnknownTargets::Error).cycles(|_| true, Order::Key);
    let [cycle] = cycles.as_slice() else { panic!("{cycles:?}") };
    assert_eq!(cycle.members(), ["a", "c"]);
    assert_eq!(cycle.path(), ["a"]);
}

#[test]
fn many_small_cycles_are_reported_in_linear_time() {
    // 25,000 two-node cycles. Per-cycle scratch sized to the whole graph made this quadratic: over 4 s in release.
    let mut b = GraphBuilder::<u32>::with_capacity(50_000);
    for i in 0..25_000u32 {
        let x = b.node(2 * i);
        let y = b.node(2 * i + 1);
        b.depends_on(x, 2 * i + 1, ()).depends_on(y, 2 * i, ());
    }
    let graph = b.build(UnknownTargets::Error);
    let started = std::time::Instant::now();
    let found = graph.validate(Order::Declaration).expect_err("cycles").into_vec();
    let elapsed = started.elapsed();
    assert_eq!(found.len(), 25_000);
    assert!(found.iter().all(|v| matches!(v, Violation::Cycle(c) if c.path().len() == 2)));
    // Generous for unoptimized builds; the linear version takes tens of milliseconds.
    assert!(elapsed < std::time::Duration::from_secs(3), "took {elapsed:?}");
}

//! Behavior of building, validation and queries, plus property tests against a brute-force oracle.

// The oracle indexes matrices on purpose: it should read like the textbook definition, not like the implementation.
#![allow(clippy::needless_range_loop)]

use std::collections::BTreeSet;

use michi_graph::{Dag, GraphBuilder, NodeIx, Order, UnknownTargets, Violation};
use proptest::prelude::*;

fn keys<'a>(dag: &'a Dag<&str>, ixs: &[NodeIx]) -> Vec<&'a str> {
    ixs.iter().map(|&ix| *dag.key(ix).expect("index from this dag")).collect()
}

fn dag(nodes: &[(&'static str, &[&'static str])], order: Order) -> Dag<&'static str> {
    Dag::from_nodes(nodes.iter().map(|&(k, deps)| (k, deps.iter().copied())), order).expect("valid dag")
}

fn violations(nodes: &[(&'static str, &[&'static str])], order: Order) -> Vec<Violation<&'static str>> {
    Dag::from_nodes(nodes.iter().map(|&(k, deps)| (k, deps.iter().copied())), order).expect_err("invalid").into_vec()
}

#[test]
fn every_problem_is_reported_at_once_in_a_fixed_order() {
    let found =
        violations(&[("a", &["ghost"]), ("b", &["b"]), ("a", &[]), ("c", &["d"]), ("d", &["c"])], Order::Declaration);
    let [Violation::DuplicateNode { key: "a" }, Violation::UnknownDependency { dependent: "a", prerequisite: "ghost" }, Violation::SelfDependency { key: "b" }, Violation::Cycle(cycle)] =
        found.as_slice()
    else {
        panic!("unexpected violations: {found:?}");
    };
    assert_eq!(cycle.members(), ["c", "d"]);
    assert_eq!(cycle.path(), ["c", "d"]);
}

#[test]
fn a_duplicate_declarations_edges_are_dropped_with_it() {
    let found = violations(&[("a", &[]), ("a", &["ghost", "a"]), ("b", &["a"])], Order::Declaration);
    assert_eq!(found, [Violation::DuplicateNode { key: "a" }]);
}

#[test]
fn edge_problems_follow_the_dependent_then_the_edge_or_the_key() {
    let nodes: &[(&str, &[&str])] = &[("x", &["x", "gone", "x"]), ("y", &["missing"])];
    // Declaration order: the edges as declared.
    assert_eq!(
        violations(nodes, Order::Declaration),
        [
            Violation::SelfDependency { key: "x" },
            Violation::UnknownDependency { dependent: "x", prerequisite: "gone" },
            Violation::UnknownDependency { dependent: "y", prerequisite: "missing" },
        ]
    );
    // Key order: by the missing key, a self-dependency sorting as the node's own key.
    assert_eq!(
        violations(nodes, Order::Key),
        [
            Violation::UnknownDependency { dependent: "x", prerequisite: "gone" },
            Violation::SelfDependency { key: "x" },
            Violation::UnknownDependency { dependent: "y", prerequisite: "missing" },
        ]
    );
}

#[test]
fn a_cycle_reports_its_members_not_nodes_hanging_off_it() {
    let found = violations(&[("a", &["b"]), ("b", &["a"]), ("tail", &["a"]), ("head", &[])], Order::Declaration);
    let [Violation::Cycle(cycle)] = found.as_slice() else { panic!("{found:?}") };
    assert_eq!(cycle.members(), ["a", "b"]);
}

#[test]
fn a_node_between_two_cycles_belongs_to_neither() {
    // x depends on cycle {a, b} and cycle {c, d} depends on x: x is stuck, but not on a cycle.
    let found = violations(
        &[("a", &["b"]), ("b", &["a"]), ("x", &["a"]), ("c", &["d", "x"]), ("d", &["c"])],
        Order::Declaration,
    );
    let members: Vec<_> = found
        .iter()
        .map(|v| match v {
            Violation::Cycle(c) => c.members().to_vec(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(members, [vec!["a", "b"], vec!["c", "d"]]);
}

#[test]
fn the_cycle_path_is_a_shortest_loop_through_the_first_member() {
    // a -> b -> c -> a and a -> c -> a: the shortest loop through a has two steps.
    let found = violations(&[("a", &["b", "c"]), ("b", &["c"]), ("c", &["a"])], Order::Key);
    let [Violation::Cycle(cycle)] = found.as_slice() else { panic!("{found:?}") };
    assert_eq!(cycle.members(), ["a", "b", "c"]);
    assert_eq!(cycle.path(), ["a", "c"]);
    assert_eq!(cycle.to_string(), "dependency cycle: a depends on c, which depends on a");
}

#[test]
fn long_cycle_messages_are_capped() {
    let mut b = GraphBuilder::<u32>::new();
    for i in 0..20u32 {
        let node = b.node(i);
        b.depends_on(node, (i + 1) % 20, ());
    }
    let text = b.build(UnknownTargets::Error).validate(Order::Key).expect_err("cycle").to_string();
    assert_eq!(
        text,
        "dependency cycle: 0 depends on 1, which depends on 2, which depends on 3, which depends on 4, which depends on 5, which depends on 6, which depends on 7, which depends on 8 … and 11 more"
    );
}

#[test]
fn display_with_renders_keys_without_display() {
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Op(u8);
    let err = Dag::from_nodes([(Op(1), vec![Op(1)])], Order::Key).expect_err("self dependency");
    assert_eq!(err.display_with(|op, f| write!(f, "op{}", op.0)).to_string(), "op1 depends on itself");
}

#[test]
fn order_is_topological_with_ties_broken_by_the_chosen_rule() {
    let nodes: &[(&str, &[&str])] = &[("c", &["a"]), ("a", &[]), ("b", &[]), ("d", &["c", "b"])];
    let declaration = dag(nodes, Order::Declaration);
    assert_eq!(declaration.keys_in_order().copied().collect::<Vec<_>>(), ["a", "c", "b", "d"]);
    let key = dag(nodes, Order::Key);
    assert_eq!(key.keys_in_order().copied().collect::<Vec<_>>(), ["a", "b", "c", "d"]);
}

#[test]
fn layers_group_nodes_that_can_run_together() {
    let d = dag(
        &[("build", &[]), ("test", &["build"]), ("lint", &[]), ("docs", &["build"]), ("ship", &["test", "lint"])],
        Order::Declaration,
    );
    let layers: Vec<Vec<&str>> = d.layers().iter().map(|l| keys(&d, l)).collect();
    assert_eq!(layers, [vec!["build", "lint"], vec!["test", "docs"], vec!["ship"]]);
    assert!(dag(&[], Order::Key).layers().is_empty());
}

#[test]
fn ready_needs_every_prerequisite_satisfied_and_the_node_eligible() {
    let d = dag(&[("a", &[]), ("b", &[]), ("c", &["a", "b"]), ("e", &["c"])], Order::Key);
    let done: BTreeSet<&str> = ["a"].into();
    let ready: Vec<_> = d.ready(|k| done.contains(k), |k| !done.contains(k)).collect();
    assert_eq!(keys(&d, &ready), ["b"]);
    let done: BTreeSet<&str> = ["a", "b"].into();
    let ready: Vec<_> = d.ready(|k| done.contains(k), |k| !done.contains(k)).collect();
    assert_eq!(keys(&d, &ready), ["c"]);
}

#[test]
fn ancestors_and_descendants_exclude_the_roots() {
    let d = dag(&[("a", &[]), ("b", &["a"]), ("c", &["b"]), ("x", &[]), ("y", &["c", "x"])], Order::Declaration);
    let c = d.index_of("c").expect("c");
    assert_eq!(keys(&d, &d.ancestors(&[c])), ["a", "b"]);
    assert_eq!(keys(&d, &d.descendants(&[c])), ["y"]);
    let a = d.index_of("a").expect("a");
    assert_eq!(keys(&d, &d.descendants(&[a, c])), ["b", "y"]);
}

#[test]
fn an_index_from_another_graph_never_panics() {
    let small = dag(&[("a", &[])], Order::Key);
    let big = dag(&[("a", &[]), ("b", &[]), ("c", &["a"])], Order::Key);
    let foreign = big.index_of("c").expect("c");
    assert!(small.key(foreign).is_none());
    assert!(small.prerequisites(foreign).is_empty());
    assert!(small.dependents(foreign).is_empty());
    assert!(small.descendants(&[foreign]).is_empty());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Runtime,
    Dev,
}

#[test]
fn edge_filters_choose_which_kinds_count() {
    let mut b = GraphBuilder::<&str, Kind>::new();
    let app = b.node("app");
    let lib = b.node("lib");
    b.depends_on(app, "lib", Kind::Runtime)
        .depends_on(app, "lib", Kind::Dev)
        .depends_on(lib, "app", Kind::Dev)
        .depends_on(lib, "lib", Kind::Dev);
    let g = b.build(UnknownTargets::Error);

    let cascade = g.validate_filtered(|e| *e.kind == Kind::Runtime, Order::Key).expect("dev cycle filtered out");
    assert_eq!(cascade.keys_in_order().copied().collect::<Vec<_>>(), ["lib", "app"]);
    let app_ix = cascade.index_of("app").expect("app");
    assert_eq!(cascade.prerequisites(app_ix).len(), 1, "two kinds between one pair collapse to one edge");

    let all = g.validate(Order::Key).expect_err("dev edges form a cycle").into_vec();
    assert!(matches!(all.as_slice(), [Violation::SelfDependency { key: "lib" }, Violation::Cycle(_)]), "{all:?}");

    let cycles = g.cycles(|_| true, Order::Key);
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles.first().expect("one").members(), ["app", "lib"]);
    let self_loops = g.cycles(|e| e.dependent == e.prerequisite, Order::Key);
    assert_eq!(self_loops.first().expect("self loop").path(), ["lib"]);
}

#[test]
fn ignored_unknown_targets_stay_visible_as_dangling_edges() {
    let mut b = GraphBuilder::<&str>::new();
    let app = b.node("app");
    b.depends_on(app, "serde", ()).depends_on(app, "serde", ());
    let g = b.build(UnknownTargets::Ignore);
    assert!(g.validate(Order::Key).is_ok());
    assert_eq!(g.dangling().len(), 1);
    assert_eq!(g.dangling().first().map(|d| d.prerequisite), Some("serde"));
}

#[test]
fn canonical_form_ignores_declaration_order_and_keeps_isolated_nodes() {
    let one = dag(&[("b", &["a"]), ("a", &[]), ("z", &[])], Order::Declaration);
    let two = dag(&[("z", &[]), ("a", &[]), ("b", &["a"])], Order::Key);
    let form = |d: &Dag<&'static str>| {
        d.canonical().map(|(k, ps)| (*k, ps.into_iter().copied().collect::<Vec<_>>())).collect::<Vec<_>>()
    };
    assert_eq!(form(&one), form(&two));
    assert_eq!(form(&one), [("a", vec![]), ("b", vec!["a"]), ("z", vec![])]);
}

#[test]
fn a_100k_chain_and_cycle_never_overflow_the_stack() {
    let chain = |close: bool| {
        let mut b = GraphBuilder::<u32>::with_capacity(100_000);
        for i in 0..100_000u32 {
            let node = b.node(i);
            if i > 0 {
                b.depends_on(node, i - 1, ());
            } else if close {
                b.depends_on(node, 99_999, ());
            }
        }
        b.build(UnknownTargets::Error)
    };
    let d = chain(false).validate(Order::Declaration).expect("chain");
    assert_eq!(d.order().len(), 100_000);
    assert_eq!(d.layers().len(), 100_000);
    assert_eq!(d.descendants(&[d.order()[0]]).len(), 99_999);
    let found = chain(true).validate(Order::Declaration).expect_err("cycle").into_vec();
    let [Violation::Cycle(cycle)] = found.as_slice() else { panic!("expected one cycle") };
    assert_eq!(cycle.members().len(), 100_000);
    assert_eq!(cycle.path().len(), 100_000);
}

// ---- Property tests against a brute-force oracle ----

/// A random graph over `n` nodes: each node lists prerequisite indices (possibly itself or repeats).
fn graphs() -> impl Strategy<Value = Vec<Vec<usize>>> {
    (1usize..14).prop_flat_map(|n| proptest::collection::vec(proptest::collection::vec(0..n, 0..4), n))
}

/// `reach[i][j]`: j is reachable from i by following prerequisites (one or more steps).
fn reachability(adj: &[Vec<usize>]) -> Vec<Vec<bool>> {
    let n = adj.len();
    let mut reach = vec![vec![false; n]; n];
    for (i, deps) in adj.iter().enumerate() {
        for &j in deps {
            reach[i][j] = true;
        }
    }
    for k in 0..n {
        for i in 0..n {
            if reach[i][k] {
                for j in 0..n {
                    if reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
    }
    reach
}

fn build(adj: &[Vec<usize>], names: &[u32]) -> michi_graph::Graph<u32> {
    let mut b = GraphBuilder::<u32>::new();
    let decls: Vec<_> = names.iter().map(|&k| b.node(k)).collect();
    for (d, deps) in decls.iter().zip(adj) {
        for &p in deps {
            b.depends_on(*d, names[p], ());
        }
    }
    b.build(UnknownTargets::Error)
}

proptest! {
    #[test]
    fn validation_matches_the_oracle(adj in graphs(), order_is_key in any::<bool>(), seed in any::<u64>()) {
        let n = adj.len();
        // Keys are a shuffled permutation, so key order differs from declaration order.
        let mut names: Vec<u32> = (0..n as u32).collect();
        let mut s = seed | 1;
        for i in (1..n).rev() {
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            names.swap(i, (s % (i as u64 + 1)) as usize);
        }
        let order = if order_is_key { Order::Key } else { Order::Declaration };
        let rank = |i: usize| if order_is_key { names[i] as usize } else { i };
        let reach = reachability(&adj);
        let self_dep: BTreeSet<usize> = (0..n).filter(|&i| adj[i].contains(&i)).collect();
        // Without self-edges: strongly connected components of size > 1.
        let no_self: Vec<Vec<usize>> = adj.iter().enumerate().map(|(i, d)| d.iter().copied().filter(|&p| p != i).collect()).collect();
        let r2 = reachability(&no_self);
        let mut expected_cycles: BTreeSet<Vec<u32>> = BTreeSet::new();
        for i in 0..n {
            let mut comp: Vec<usize> = (0..n).filter(|&j| j == i || (r2[i][j] && r2[j][i])).collect();
            if comp.len() > 1 {
                comp.sort_by_key(|&j| rank(j));
                expected_cycles.insert(comp.iter().map(|&j| names[j]).collect());
            }
        }

        match build(&adj, &names).validate(order) {
            Ok(dag) => {
                prop_assert!(self_dep.is_empty() && expected_cycles.is_empty());
                let _ = reach;
                // Topological validity and lexicographic minimality under the rank.
                let pos: Vec<usize> = { let mut p = vec![0; n]; for (k, ix) in dag.order().iter().enumerate() { p[ix.index()] = k; } p };
                for (i, deps) in adj.iter().enumerate() { for &p in deps { prop_assert!(pos[p] < pos[i]); } }
                let mut placed = vec![false; n];
                for ix in dag.order() {
                    let best = (0..n).filter(|&j| !placed[j] && adj[j].iter().all(|&p| placed[p])).min_by_key(|&j| rank(j));
                    prop_assert_eq!(Some(ix.index()), best);
                    placed[ix.index()] = true;
                }
                // Layers: layer(n) = 1 + max(layer(p)).
                let mut layer = vec![0usize; n];
                for (li, l) in dag.layers().iter().enumerate() { for ix in l { layer[ix.index()] = li; } }
                for i in 0..n {
                    let want = adj[i].iter().map(|&p| layer[p] + 1).max().unwrap_or(0);
                    prop_assert_eq!(layer[i], want);
                }
                // Descendants match reachability.
                for i in 0..n {
                    let got: BTreeSet<usize> = dag.descendants(&[ix_of(&dag, names[i])]).iter().map(|ix| ix.index()).collect();
                    let want: BTreeSet<usize> = (0..n).filter(|&j| reach[j][i]).collect();
                    prop_assert_eq!(got, want);
                }
            }
            Err(v) => {
                let got_self: BTreeSet<u32> = v.iter().filter_map(|x| match x { Violation::SelfDependency { key } => Some(*key), _ => None }).collect();
                prop_assert_eq!(got_self, self_dep.iter().map(|&i| names[i]).collect::<BTreeSet<_>>());
                let got_cycles: BTreeSet<Vec<u32>> = v.iter().filter_map(|x| match x { Violation::Cycle(c) => Some(c.members().to_vec()), _ => None }).collect();
                prop_assert_eq!(&got_cycles, &expected_cycles);
                for x in &v {
                    if let Violation::Cycle(c) = x {
                        // The path is a real loop inside the component, starting at its first member.
                        let path = c.path();
                        prop_assert_eq!(path.first(), c.members().first());
                        let idx = |k: u32| names.iter().position(|&m| m == k).unwrap();
                        for w in 0..path.len() {
                            let (a, b) = (idx(path[w]), idx(path[(w + 1) % path.len()]));
                            prop_assert!(adj[a].contains(&b) && a != b);
                        }
                    }
                }
            }
        }
    }
}

fn ix_of(dag: &Dag<u32>, key: u32) -> NodeIx {
    dag.index_of(&key).expect("key in dag")
}

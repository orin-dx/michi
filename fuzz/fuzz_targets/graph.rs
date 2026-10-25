//! `GraphBuilder` never panics on any node keys, edges or kinds. Validation accepts exactly the graphs an
//! independent oracle accepts, and a valid `Dag` has a topological order, partitioning layers, consistent closures and
//! the documented tie-breaking.

#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_graph::{Dag, GraphBuilder, NodeIx, Order, UnknownTargets, Violation};

#[derive(Arbitrary, Debug)]
struct Input {
    /// `(key, [(prerequisite key, edge kind)])`. Keys are folded into a small range so duplicates, self-dependencies,
    /// unknown targets and cycles are common.
    nodes: Vec<(u8, Vec<(u8, u8)>)>,
    ignore_unknown: bool,
    by_key: bool,
    roots: Vec<u8>,
    satisfied_mask: u32,
    keep_kind_mod: u8,
}

const KEYS: u8 = 24;

fuzz_target!(|input: Input| {
    if input.nodes.len() > 200 {
        return;
    }
    let order = if input.by_key { Order::Key } else { Order::Declaration };
    let unknown = if input.ignore_unknown { UnknownTargets::Ignore } else { UnknownTargets::Error };
    let nodes: Vec<(u8, Vec<(u8, u8)>)> = input
        .nodes
        .iter()
        .map(|(k, deps)| (k % KEYS, deps.iter().take(8).map(|(p, kind)| (p % KEYS, kind % 3)).collect()))
        .collect();

    let mut b = GraphBuilder::<u8, u8>::new();
    for (key, deps) in &nodes {
        let node = b.node(*key);
        for (p, kind) in deps {
            b.depends_on(node, *p, *kind);
        }
    }
    let graph = b.build(unknown);

    // Oracle: the first declaration of a key is the node.
    let mut keys: Vec<u8> = Vec::new();
    let mut has_dup = false;
    for (k, _) in &nodes {
        if keys.contains(k) {
            has_dup = true;
        } else {
            keys.push(*k);
        }
    }
    assert_eq!(graph.len(), keys.len());
    assert_eq!(graph.duplicates().len(), nodes.len() - keys.len());

    // Edges by node index, over first declarations only (a duplicate's edges are not the node's).
    let mut prereqs: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); keys.len()];
    let mut seen_decl: BTreeSet<u8> = BTreeSet::new();
    let (mut unknown_edge, mut self_edge) = (false, false);
    for (k, deps) in &nodes {
        if !seen_decl.insert(*k) {
            continue;
        }
        let me = keys.iter().position(|x| x == k).expect("declared");
        for (p, _) in deps {
            match keys.iter().position(|x| x == p) {
                Some(t) if t == me => self_edge = true,
                Some(t) => {
                    prereqs[me].insert(t);
                }
                None => unknown_edge = true,
            }
        }
    }
    let oracle_topo = topo_oracle(&prereqs, &keys, input.by_key);
    let acyclic = oracle_topo.len() == keys.len();
    let should_be_valid = !has_dup && !self_edge && acyclic && !(unknown_edge && unknown == UnknownTargets::Error);

    let result = graph.validate(order);
    assert_eq!(result.is_ok(), should_be_valid, "oracle disagrees on {nodes:?} ({unknown:?})");
    // The one-call constructor agrees where it applies.
    if unknown == UnknownTargets::Error {
        let plain = Dag::from_nodes(nodes.iter().map(|(k, d)| (*k, d.iter().map(|(p, _)| *p))), order);
        assert_eq!(plain.is_ok(), should_be_valid);
    }
    // Reports and cycle search must never panic, valid or not.
    let cycles = graph.cycles(|_| true, order);
    if !has_dup {
        assert_eq!(cycles.is_empty(), acyclic && !self_edge);
    }
    let _ = graph.validate_filtered(|e| u32::from(*e.kind) % u32::from(input.keep_kind_mod.max(1)) == 0, order);

    let dag = match result {
        Ok(dag) => dag,
        Err(violations) => {
            assert!(!violations.as_slice().is_empty());
            let _ = violations.to_string();
            for v in violations.iter() {
                let _ = v.to_string();
                if let Violation::Cycle(c) = v {
                    assert!(!c.members().is_empty());
                    assert!(c.members().iter().all(|m| keys.contains(m)));
                }
            }
            return;
        }
    };

    // Topological order: a permutation of the nodes, every prerequisite first, ties as documented.
    let n = dag.len();
    assert_eq!(n, keys.len());
    let position: BTreeMap<NodeIx, usize> = dag.order().iter().enumerate().map(|(i, ix)| (*ix, i)).collect();
    assert_eq!(position.len(), n, "order repeats or drops a node");
    for ix in dag.order() {
        assert!(dag.key(*ix).is_some());
        for p in dag.prerequisites(*ix) {
            assert!(position[p] < position[ix], "prerequisite after its dependent");
            assert!(dag.dependents(*p).contains(ix), "dependents mirror prerequisites");
        }
    }
    let got_order: Vec<u8> = dag.keys_in_order().copied().collect();
    let want_order: Vec<u8> = oracle_topo.iter().map(|&i| keys[i]).collect();
    assert_eq!(got_order, want_order, "tie-breaking differs from the documented order");
    for (i, key) in keys.iter().enumerate() {
        let ix = dag.index_of(key).expect("every key is a node");
        let got: BTreeSet<u8> = dag.prerequisites(ix).iter().filter_map(|p| dag.key(*p).copied()).collect();
        let want: BTreeSet<u8> = prereqs[i].iter().map(|&t| keys[t]).collect();
        assert_eq!(got, want, "prerequisites of {key}");
    }

    // Layers partition the nodes, and a node sits one layer below its deepest prerequisite.
    let layers = dag.layers();
    let mut layer_of: BTreeMap<NodeIx, usize> = BTreeMap::new();
    for (depth, layer) in layers.iter().enumerate() {
        assert!(!layer.is_empty(), "empty layer");
        for ix in layer {
            assert!(layer_of.insert(*ix, depth).is_none(), "node in two layers");
        }
    }
    assert_eq!(layer_of.len(), n, "layers drop a node");
    for ix in dag.order() {
        let want = dag.prerequisites(*ix).iter().map(|p| layer_of[p] + 1).max().unwrap_or(0);
        assert_eq!(layer_of[ix], want);
    }

    // Closures: transitive, exclude the roots, and each is the other's mirror.
    let roots: Vec<NodeIx> = input.roots.iter().filter_map(|r| dag.index_of(&(r % KEYS))).collect();
    let up = dag.ancestors(&roots);
    let down = dag.descendants(&roots);
    for ix in dag.order() {
        let is_up = reaches(&dag, *ix, &roots, true);
        let is_down = reaches(&dag, *ix, &roots, false);
        if !roots.contains(ix) {
            assert_eq!(up.contains(ix), is_up);
            assert_eq!(down.contains(ix), is_down);
        }
    }
    for r in &roots {
        for a in dag.ancestors(&[*r]) {
            assert!(dag.descendants(&[a]).contains(r), "ancestor/descendant mirror");
        }
    }
    // Closures list nodes "in the dag's Order": by declaration (or by key), not topologically.
    for list in [&up, &down] {
        let rank =
            |ix: &NodeIx| if input.by_key { (*dag.key(*ix).expect("node"), ix.index()) } else { (0, ix.index()) };
        assert!(list.windows(2).all(|w| rank(&w[0]) < rank(&w[1])), "closure not in the dag's Order");
    }

    // Ready sets: nothing satisfied -> roots of the graph; everything satisfied -> everything eligible.
    let none: Vec<NodeIx> = dag.ready(|_| false, |_| true).collect();
    assert!(none.iter().all(|ix| dag.prerequisites(*ix).is_empty()));
    assert_eq!(none.len(), dag.order().iter().filter(|ix| dag.prerequisites(**ix).is_empty()).count());
    assert_eq!(dag.ready(|_| true, |_| true).count(), n);
    let mask = input.satisfied_mask;
    let ok = move |k: &u8| mask >> (u32::from(*k) % 32) & 1 == 1;
    for ix in dag.ready(ok, |_| true) {
        assert!(dag.prerequisites(ix).iter().all(|p| dag.key(*p).is_some_and(ok)));
    }
    let _ = dag.ready_by_edge(|_, kind| *kind != 2, |_| true).count();
    let _ = dag.canonical().count();
});

/// The topological order with ties broken by declaration index, or by key under `by_key`: Kahn's algorithm with a
/// smallest-first ready set. Shorter than `n` when a cycle remains.
fn topo_oracle(prereqs: &[BTreeSet<usize>], keys: &[u8], by_key: bool) -> Vec<usize> {
    let n = prereqs.len();
    let mut done = vec![false; n];
    let mut out = Vec::with_capacity(n);
    loop {
        let next = (0..n).filter(|&i| !done[i] && prereqs[i].iter().all(|&p| done[p])).min_by_key(|&i| {
            if by_key {
                (keys[i], i)
            } else {
                (0, i)
            }
        });
        match next {
            Some(i) => {
                done[i] = true;
                out.push(i);
            }
            None => return out,
        }
    }
}

/// Whether `ix` lies upstream (`up`) or downstream of some root, strictly.
fn reaches(dag: &Dag<u8, u8>, ix: NodeIx, roots: &[NodeIx], up: bool) -> bool {
    let mut stack: Vec<NodeIx> = roots.to_vec();
    let mut seen: BTreeSet<NodeIx> = BTreeSet::new();
    while let Some(cur) = stack.pop() {
        let next = if up { dag.prerequisites(cur) } else { dag.dependents(cur) };
        for m in next {
            if *m == ix {
                return true;
            }
            if seen.insert(*m) {
                stack.push(*m);
            }
        }
    }
    false
}

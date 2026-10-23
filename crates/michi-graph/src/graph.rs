//! The built graph, which may contain cycles, and validation into a [`Dag`].

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::borrow::Borrow;

use crate::algo::{cyclic_components, kahn, shortest_cycle, Csr, CycleScratch};
use crate::builder::lookup;
use crate::dag::Dag;
use crate::violation::{Cycle, Violation, Violations};
use crate::{NodeIx, Order, UnknownTargets};

#[derive(Debug, Clone)]
pub(crate) struct Edge<E> {
    pub(crate) dependent: u32,
    pub(crate) prerequisite: u32,
    pub(crate) kind: E,
    pub(crate) seq: usize,
}

/// An edge whose prerequisite no node has. Kept whatever the [`UnknownTargets`] policy.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dangling<K, E = ()> {
    /// The node declaring the edge.
    pub dependent: NodeIx,
    /// The key no node has.
    pub prerequisite: K,
    /// The edge's kind.
    pub kind: E,
    pub(crate) seq: usize,
}

/// One declared edge as an edge filter sees it, including edges whose prerequisite no node has.
#[non_exhaustive]
#[derive(Debug)]
pub struct EdgeRef<'a, K, E> {
    /// The node that depends on `prerequisite`.
    pub dependent: &'a K,
    /// The key of the node that comes first.
    pub prerequisite: &'a K,
    /// The edge's kind.
    pub kind: &'a E,
    /// `dependent`'s index.
    pub dependent_ix: NodeIx,
    /// `prerequisite`'s index, or `None` for a dangling edge (no node has that key).
    pub prerequisite_ix: Option<NodeIx>,
}

impl<K, E> Clone for EdgeRef<'_, K, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, E> Copy for EdgeRef<'_, K, E> {}

/// A built graph. It may contain cycles; [`Graph::validate`] proves it does not and returns a [`Dag`].
#[derive(Debug, Clone)]
pub struct Graph<K, E = ()> {
    pub(crate) keys: Arc<[K]>,
    pub(crate) by_key: Arc<[u32]>,
    pub(crate) edges: Vec<Edge<E>>,
    pub(crate) dangling: Vec<Dangling<K, E>>,
    pub(crate) duplicates: Vec<K>,
    pub(crate) unknown: UnknownTargets,
    pub(crate) overflow: bool,
}

/// An edge problem waiting to be ordered.
struct Problem<'a, K> {
    rank: u32,
    seq: usize,
    target: &'a K,
    violation: Violation<K>,
}

/// The adjacency of the edges a filter kept, without self-loops.
pub(crate) struct Kept {
    pub(crate) prerequisites: Csr,
    pub(crate) dependents: Csr,
    self_loop: Vec<bool>,
    /// The declaration sequence of each node's first kept self-edge.
    self_loop_seq: Vec<Option<usize>>,
    /// Whether the filter kept each dangling edge, by position in `Graph::dangling`.
    dangling_kept: Vec<bool>,
    pub(crate) rank: Vec<u32>,
    pub(crate) by_rank: Vec<u32>,
    /// Indices into `Graph::edges` of the kept edges that are not self-loops.
    pub(crate) kept_edges: Vec<usize>,
}

impl<K: Ord + Clone, E: Ord> Graph<K, E> {
    /// Number of nodes, not counting duplicate declarations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the graph has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The key of node `ix`.
    #[must_use]
    pub fn key(&self, ix: NodeIx) -> Option<&K> {
        self.keys.get(ix.u())
    }

    /// The node with `key`.
    pub fn index_of<Q>(&self, key: &Q) -> Option<NodeIx>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        lookup(&self.keys, &self.by_key, key).map(NodeIx)
    }

    /// Edges whose prerequisite no node has, in declaration order.
    #[must_use]
    pub fn dangling(&self) -> &[Dangling<K, E>] {
        &self.dangling
    }

    /// Keys declared more than once, one entry per later declaration, in declaration order.
    #[must_use]
    pub fn duplicates(&self) -> &[K] {
        &self.duplicates
    }

    /// Validates the graph with every edge. See [`Graph::validate_filtered`].
    ///
    /// # Errors
    ///
    /// [`Violations`] listing every problem when the graph is not a valid dag.
    pub fn validate(&self, order: Order) -> Result<Dag<K, E>, Violations<K>>
    where
        E: Clone,
    {
        self.validate_filtered(|_| true, order)
    }

    /// Validates the graph with the edges `keep` accepts and returns a [`Dag`], or every problem at once, in this
    /// order: duplicate nodes, then unknown dependencies (unless ignored) and self-dependencies by the dependent's
    /// declaration position and the edge's, then cycles. `keep` sees every declared edge, dangling ones included
    /// (with no `prerequisite_ix`), and only the edges it keeps can be problems: an unknown dev dependency is not one
    /// when dev edges are filtered out, and cycles made only of rejected edges are not problems.
    ///
    /// # Errors
    ///
    /// [`Violations`] listing every problem when the kept edges do not form a valid dag.
    pub fn validate_filtered(
        &self,
        keep: impl FnMut(EdgeRef<'_, K, E>) -> bool,
        order: Order,
    ) -> Result<Dag<K, E>, Violations<K>>
    where
        E: Clone,
    {
        let kept = self.kept(keep, order);
        let n = self.len();
        // Duplicates in declaration order, or by key under `Order::Key` so shuffling declarations changes nothing.
        let mut duplicates: Vec<&K> = self.duplicates.iter().collect();
        if order == Order::Key {
            duplicates.sort();
        }
        let mut violations: Vec<Violation<K>> =
            duplicates.into_iter().map(|key| Violation::DuplicateNode { key: key.clone() }).collect();
        if self.overflow {
            violations.push(Violation::TooManyNodes);
        }

        // Unknown dependencies and self-dependencies, by the dependent's rank, then (declaration order) the edge's
        // position or (key order) the missing key, a self-dependency sorting as its own key.
        let rank_of = |ix: u32| kept.rank.get(ix as usize).copied().unwrap_or(u32::MAX);
        let mut problems: Vec<Problem<'_, K>> = Vec::new();
        if self.unknown == UnknownTargets::Error {
            for (d, _) in self.dangling.iter().zip(&kept.dangling_kept).filter(|(_, kept)| **kept) {
                if let Some(dependent) = self.key(d.dependent) {
                    problems.push(Problem {
                        rank: rank_of(d.dependent.0),
                        seq: d.seq,
                        target: &d.prerequisite,
                        violation: Violation::UnknownDependency {
                            dependent: dependent.clone(),
                            prerequisite: d.prerequisite.clone(),
                        },
                    });
                }
            }
        }
        for (i, seq) in kept.self_loop_seq.iter().enumerate() {
            if let (Some(seq), Some(key), Ok(i)) = (seq, self.keys.get(i), u32::try_from(i)) {
                problems.push(Problem {
                    rank: rank_of(i),
                    seq: *seq,
                    target: key,
                    violation: Violation::SelfDependency { key: key.clone() },
                });
            }
        }
        match order {
            Order::Declaration => problems.sort_by_key(|p| (p.rank, p.seq)),
            Order::Key => problems.sort_by(|a, b| (a.rank, a.target, a.seq).cmp(&(b.rank, b.target, b.seq))),
        }
        // The same missing key under several kept edge kinds is one problem, reported where it first appears.
        let mut seen = alloc::collections::BTreeSet::new();
        problems.retain(|p| seen.insert((p.rank, p.target, matches!(p.violation, Violation::SelfDependency { .. }))));
        violations.extend(problems.into_iter().map(|p| p.violation));

        let topo = kahn(n, &kept.prerequisites, &kept.dependents, &kept.rank, &kept.by_rank);
        if topo.len() < n {
            let mut left = vec![true; n];
            for ix in &topo {
                if let Some(l) = left.get_mut(ix.u()) {
                    *l = false;
                }
            }
            let no_self = vec![false; n];
            for cycle in self.cycles_in(&left, &kept, &no_self) {
                violations.push(Violation::Cycle(cycle));
            }
        }

        if !violations.is_empty() {
            return Err(Violations::new(violations));
        }
        let edges = self.edge_kinds(&kept);
        Ok(Dag::new(Arc::clone(&self.keys), Arc::clone(&self.by_key), kept, topo, edges))
    }

    /// The cycles among the edges `keep` accepts, for graphs that may legitimately contain them. Each strongly
    /// connected component that forms a cycle is reported once, a node with an edge to itself included.
    #[must_use]
    pub fn cycles(&self, keep: impl FnMut(EdgeRef<'_, K, E>) -> bool, order: Order) -> Vec<Cycle<K>> {
        let kept = self.kept(keep, order);
        let all = vec![true; self.len()];
        self.cycles_in(&all, &kept, &kept.self_loop)
    }

    fn cycles_in(&self, members: &[bool], kept: &Kept, self_loop: &[bool]) -> Vec<Cycle<K>> {
        let rank = |ix: &NodeIx| kept.rank.get(ix.u()).copied().unwrap_or(u32::MAX);
        let mut components = cyclic_components(members, &kept.prerequisites, &kept.dependents, self_loop);
        for c in &mut components {
            c.sort_unstable_by_key(rank);
        }
        components.sort_unstable_by_key(|c| c.first().map_or(u32::MAX, rank));
        let mut inside = vec![false; self.len()];
        let mut scratch = CycleScratch::new(self.len());
        components
            .into_iter()
            .map(|component| {
                for ix in &component {
                    if let Some(f) = inside.get_mut(ix.u()) {
                        *f = true;
                    }
                }
                // A node's own self-edge is the shortest loop through it.
                let path = match component.first() {
                    Some(&start) if self_loop.get(start.u()).copied().unwrap_or(false) => vec![start],
                    Some(&start) if component.len() > 1 => {
                        shortest_cycle(start, &inside, &kept.prerequisites, &mut scratch)
                    }
                    Some(&start) => vec![start],
                    None => Vec::new(),
                };
                for ix in &component {
                    if let Some(f) = inside.get_mut(ix.u()) {
                        *f = false;
                    }
                }
                let keys = |ixs: &[NodeIx]| ixs.iter().filter_map(|ix| self.key(*ix).cloned()).collect();
                Cycle::new(keys(&component), keys(&path))
            })
            .collect()
    }

    fn edge_ref<'a>(&'a self, e: &'a Edge<E>) -> Option<EdgeRef<'a, K, E>> {
        Some(EdgeRef {
            dependent: self.keys.get(e.dependent as usize)?,
            prerequisite: self.keys.get(e.prerequisite as usize)?,
            kind: &e.kind,
            dependent_ix: NodeIx(e.dependent),
            prerequisite_ix: Some(NodeIx(e.prerequisite)),
        })
    }

    fn kept(&self, mut keep: impl FnMut(EdgeRef<'_, K, E>) -> bool, order: Order) -> Kept {
        let n = self.len();
        let rank: Vec<u32> = match order {
            Order::Declaration => (0..n).filter_map(|i| u32::try_from(i).ok()).collect(),
            Order::Key => {
                let mut rank = vec![0u32; n];
                for (r, &i) in self.by_key.iter().enumerate() {
                    if let (Some(slot), Ok(r)) = (rank.get_mut(i as usize), u32::try_from(r)) {
                        *slot = r;
                    }
                }
                rank
            }
        };
        let mut by_rank = vec![0u32; n];
        for (i, &r) in rank.iter().enumerate() {
            if let (Some(slot), Ok(i)) = (by_rank.get_mut(r as usize), u32::try_from(i)) {
                *slot = i;
            }
        }
        let mut self_loop = vec![false; n];
        let mut self_loop_seq: Vec<Option<usize>> = vec![None; n];
        let mut forward = Vec::with_capacity(self.edges.len());
        let mut kept_edges = Vec::with_capacity(self.edges.len());
        for (idx, e) in self.edges.iter().enumerate() {
            let Some(edge) = self.edge_ref(e) else { continue };
            if !keep(edge) {
                continue;
            }
            if e.dependent == e.prerequisite {
                if let Some(s) = self_loop.get_mut(e.dependent as usize) {
                    *s = true;
                }
                if let Some(s @ None) = self_loop_seq.get_mut(e.dependent as usize) {
                    *s = Some(e.seq);
                }
            } else {
                forward.push((e.dependent, e.prerequisite));
                kept_edges.push(idx);
            }
        }
        let dangling_kept = self
            .dangling
            .iter()
            .map(|d| {
                self.key(d.dependent).is_some_and(|dependent| {
                    keep(EdgeRef {
                        dependent,
                        prerequisite: &d.prerequisite,
                        kind: &d.kind,
                        dependent_ix: d.dependent,
                        prerequisite_ix: None,
                    })
                })
            })
            .collect();
        let backward = forward.iter().map(|&(d, p)| (p, d)).collect();
        Kept {
            prerequisites: Csr::from_pairs(n, forward, &rank),
            dependents: Csr::from_pairs(n, backward, &rank),
            self_loop,
            self_loop_seq,
            dangling_kept,
            rank,
            by_rank,
            kept_edges,
        }
    }

    /// The kept edges with their kinds, grouped by dependent and ordered by prerequisite rank, then kind.
    fn edge_kinds(&self, kept: &Kept) -> EdgeKinds<E>
    where
        E: Clone,
    {
        let n = self.len();
        let rank = |i: u32| kept.rank.get(i as usize).copied().unwrap_or(u32::MAX);
        let mut items: Vec<(u32, u32, &E)> = kept
            .kept_edges
            .iter()
            .filter_map(|&i| self.edges.get(i))
            .map(|e| (e.dependent, e.prerequisite, &e.kind))
            .collect();
        items.sort_by(|a, b| (a.0, rank(a.1), a.2).cmp(&(b.0, rank(b.1), b.2)));
        let mut offsets = vec![0u32; n + 1];
        for &(d, _, _) in &items {
            if let Some(o) = offsets.get_mut(d as usize + 1) {
                *o += 1;
            }
        }
        for i in 1..offsets.len() {
            let prev = offsets.get(i - 1).copied().unwrap_or(0);
            if let Some(o) = offsets.get_mut(i) {
                *o += prev;
            }
        }
        EdgeKinds { offsets, items: items.into_iter().map(|(_, p, k)| (NodeIx(p), k.clone())).collect() }
    }
}

/// Each node's kept prerequisite edges with their kinds, compressed like [`Csr`].
#[derive(Debug, Clone)]
pub(crate) struct EdgeKinds<E> {
    offsets: Vec<u32>,
    items: Vec<(NodeIx, E)>,
}

impl<E> EdgeKinds<E> {
    pub(crate) fn of(&self, i: usize) -> &[(NodeIx, E)] {
        let (Some(&start), Some(&end)) = (self.offsets.get(i), self.offsets.get(i + 1)) else { return &[] };
        self.items.get(start as usize..end as usize).unwrap_or(&[])
    }
}

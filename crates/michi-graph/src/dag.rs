//! A validated, acyclic graph and its queries.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::borrow::Borrow;

use crate::algo::Csr;
use crate::builder::lookup;
use crate::graph::Kept;
use crate::violation::Violations;
use crate::{GraphBuilder, NodeIx, Order, UnknownTargets};

/// A graph proven to have no cycles, with the edges validation kept. Immutable; cheap to clone.
#[derive(Debug, Clone)]
pub struct Dag<K> {
    keys: Arc<[K]>,
    by_key: Arc<[u32]>,
    prerequisites: Arc<Csr>,
    dependents: Arc<Csr>,
    order: Arc<[NodeIx]>,
    rank: Arc<[u32]>,
    by_rank: Arc<[u32]>,
}

/// The layers of a [`Dag`]: layer 0 holds nodes without prerequisites, and a node's layer is one more than its deepest
/// prerequisite's. Nodes in one layer do not depend on each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layers {
    nodes: Vec<NodeIx>,
    bounds: Vec<usize>,
}

impl Layers {
    /// Number of layers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bounds.len().saturating_sub(1)
    }

    /// Whether there are no layers (the dag is empty).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Layer `i`, in the dag's [`Order`].
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&[NodeIx]> {
        let (&start, &end) = (self.bounds.get(i)?, self.bounds.get(i + 1)?);
        self.nodes.get(start..end)
    }

    /// Iterates the layers, first to last.
    pub fn iter(&self) -> impl Iterator<Item = &[NodeIx]> + '_ {
        (0..self.len()).filter_map(|i| self.get(i))
    }
}

impl<K: Ord + Clone> Dag<K> {
    pub(crate) fn new(keys: Arc<[K]>, by_key: Arc<[u32]>, kept: Kept, order: Vec<NodeIx>) -> Self {
        Self {
            keys,
            by_key,
            prerequisites: Arc::new(kept.prerequisites),
            dependents: Arc::new(kept.dependents),
            order: Arc::from(order),
            rank: Arc::from(kept.rank),
            by_rank: Arc::from(kept.by_rank),
        }
    }

    /// Builds and validates in one call, for graphs without edge kinds where an unknown dependency is an error.
    ///
    /// # Errors
    ///
    /// [`Violations`] listing every problem when the nodes do not form a valid dag.
    pub fn from_nodes<I, D>(nodes: I, order: Order) -> Result<Self, Violations<K>>
    where
        I: IntoIterator<Item = (K, D)>,
        D: IntoIterator<Item = K>,
    {
        let nodes = nodes.into_iter();
        let mut b = GraphBuilder::<K>::with_capacity(nodes.size_hint().0);
        for (key, prerequisites) in nodes {
            let node = b.node(key);
            for p in prerequisites {
                b.depends_on(node, p, ());
            }
        }
        b.build(UnknownTargets::Error).validate(order)
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the dag has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The key of node `ix`.
    #[must_use]
    pub fn key(&self, ix: NodeIx) -> Option<&K> {
        self.keys.get(ix.u())
    }

    /// The node at declaration position `position`, for callers that keep per-node data in declaration order. Valid
    /// only for a dag whose graph had no duplicate declarations, which validation guarantees.
    #[must_use]
    pub fn node_at(&self, position: usize) -> Option<NodeIx> {
        if position < self.len() {
            u32::try_from(position).ok().map(NodeIx)
        } else {
            None
        }
    }

    /// The node with `key`.
    pub fn index_of<Q>(&self, key: &Q) -> Option<NodeIx>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        lookup(&self.keys, &self.by_key, key).map(NodeIx)
    }

    /// The nodes in topological order: the lexicographically smallest valid order under the dag's [`Order`].
    #[must_use]
    pub fn order(&self) -> &[NodeIx] {
        &self.order
    }

    /// The keys in topological order.
    pub fn keys_in_order(&self) -> impl Iterator<Item = &K> + '_ {
        self.order.iter().filter_map(|&ix| self.key(ix))
    }

    /// The nodes `ix` depends on, in the dag's [`Order`]. Empty for an index the dag does not have.
    #[must_use]
    pub fn prerequisites(&self, ix: NodeIx) -> &[NodeIx] {
        self.prerequisites.of(ix.u())
    }

    /// The nodes that depend on `ix`, in the dag's [`Order`]. Empty for an index the dag does not have.
    #[must_use]
    pub fn dependents(&self, ix: NodeIx) -> &[NodeIx] {
        self.dependents.of(ix.u())
    }

    /// The dag's layers. See [`Layers`].
    #[must_use]
    pub fn layers(&self) -> Layers {
        let n = self.len();
        let mut depth = vec![0usize; n];
        for &ix in self.order.iter() {
            let d = self.prerequisites(ix).iter().filter_map(|p| depth.get(p.u())).max().map_or(0, |d| d + 1);
            if let Some(slot) = depth.get_mut(ix.u()) {
                *slot = d;
            }
        }
        let layers = depth.iter().max().map_or(0, |d| d + 1);
        let mut bounds = vec![0usize; layers + 1];
        for &d in &depth {
            if let Some(b) = bounds.get_mut(d + 1) {
                *b += 1;
            }
        }
        for i in 1..bounds.len() {
            let prev = bounds.get(i - 1).copied().unwrap_or(0);
            if let Some(b) = bounds.get_mut(i) {
                *b += prev;
            }
        }
        let mut cursor = bounds.clone();
        let mut nodes = vec![NodeIx(0); n];
        for &i in self.by_rank.iter() {
            let Some(&d) = depth.get(i as usize) else { continue };
            if let Some(c) = cursor.get_mut(d) {
                if let Some(slot) = nodes.get_mut(*c) {
                    *slot = NodeIx(i);
                }
                *c += 1;
            }
        }
        Layers { nodes, bounds }
    }

    /// The nodes that can start now: those for which `eligible(key)` holds and `satisfied(key)` holds for every
    /// prerequisite, in the dag's [`Order`]. One pass over the graph.
    ///
    /// When `satisfied` holds only for nodes that succeeded, a node behind a failure never becomes ready, so direct
    /// prerequisites are enough.
    pub fn ready<'a, S, R>(&'a self, satisfied: S, eligible: R) -> impl Iterator<Item = NodeIx> + 'a
    where
        S: Fn(&K) -> bool + 'a,
        R: Fn(&K) -> bool + 'a,
    {
        self.by_rank.iter().map(|&i| NodeIx(i)).filter(move |&ix| {
            self.key(ix).is_some_and(&eligible)
                && self.prerequisites(ix).iter().all(|&p| self.key(p).is_some_and(&satisfied))
        })
    }

    /// Every node upstream of `roots`, not including the roots, in the dag's [`Order`].
    #[must_use]
    pub fn ancestors(&self, roots: &[NodeIx]) -> Vec<NodeIx> {
        self.closure(roots, &self.prerequisites)
    }

    /// Every node downstream of `roots`, not including the roots, in the dag's [`Order`].
    #[must_use]
    pub fn descendants(&self, roots: &[NodeIx]) -> Vec<NodeIx> {
        self.closure(roots, &self.dependents)
    }

    fn closure(&self, roots: &[NodeIx], next: &Csr) -> Vec<NodeIx> {
        let n = self.len();
        let mut seen = vec![false; n];
        let mut stack: Vec<NodeIx> = Vec::new();
        for r in roots {
            if let Some(s) = seen.get_mut(r.u()) {
                *s = true;
                stack.push(*r);
            }
        }
        let mut found = Vec::new();
        while let Some(ix) = stack.pop() {
            for &m in next.of(ix.u()) {
                if let Some(s) = seen.get_mut(m.u()) {
                    if !*s {
                        *s = true;
                        found.push(m);
                        stack.push(m);
                    }
                }
            }
        }
        found.sort_unstable_by_key(|ix| self.rank.get(ix.u()).copied().unwrap_or(u32::MAX));
        found
    }

    /// Each node's key with its prerequisites' keys, nodes and prerequisites sorted by key, isolated nodes included:
    /// a canonical form of the structure for callers that hash it with their own hasher.
    pub fn canonical(&self) -> impl Iterator<Item = (&K, Vec<&K>)> + '_ {
        self.by_key.iter().filter_map(move |&i| {
            let key = self.keys.get(i as usize)?;
            let mut prerequisites: Vec<&K> =
                self.prerequisites.of(i as usize).iter().filter_map(|&p| self.key(p)).collect();
            prerequisites.sort_unstable();
            Some((key, prerequisites))
        })
    }
}

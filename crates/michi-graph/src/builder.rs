//! Declaring nodes and edges.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::graph::{Dangling, Edge, Graph};
use crate::{NodeIx, UnknownTargets};

/// A node declaration, returned by [`GraphBuilder::node`]. Edges hang off the declaration rather than the key, so the
/// edges of a duplicate declaration are dropped with it instead of landing on the first node with that key.
///
/// A declaration belongs to the builder that made it. A clone of that builder also accepts the declarations made
/// before it was cloned, since it holds the same nodes, but not later ones. Passing a declaration to any other
/// builder adds nothing, rather than silently naming that builder's node at the same position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decl {
    builder: usize,
    index: u32,
}

/// Distinguishes builders, so a [`Decl`] from one is ignored by another. Pointer-sized, so it does not wrap in
/// practice on 64-bit targets.
static NEXT_BUILDER: AtomicUsize = AtomicUsize::new(0);

fn next_builder_id() -> usize {
    NEXT_BUILDER.fetch_add(1, Ordering::Relaxed)
}

/// Collects nodes and edges. [`GraphBuilder::build`] never fails; problems are reported when the graph is validated.
#[derive(Debug)]
pub struct GraphBuilder<K, E = ()> {
    id: usize,
    /// Builders this one was cloned from, each with how many declarations it had then: their earlier declarations
    /// name the same nodes here.
    ancestors: Vec<(usize, u32)>,
    decls: Vec<K>,
    edges: Vec<(u32, K, E)>,
    overflow: bool,
}

impl<K: Clone, E: Clone> Clone for GraphBuilder<K, E> {
    /// A copy with its own identity: declarations made on either builder after the clone don't work on the other.
    fn clone(&self) -> Self {
        let mut ancestors = self.ancestors.clone();
        ancestors.push((self.id, u32::try_from(self.decls.len()).unwrap_or(u32::MAX)));
        Self {
            id: next_builder_id(),
            ancestors,
            decls: self.decls.clone(),
            edges: self.edges.clone(),
            overflow: self.overflow,
        }
    }
}

impl<K: Ord + Clone, E: Ord> Default for GraphBuilder<K, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, E: Ord> GraphBuilder<K, E> {
    /// An empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// An empty builder with room for `nodes` declarations.
    #[must_use]
    pub fn with_capacity(nodes: usize) -> Self {
        Self {
            id: next_builder_id(),
            ancestors: Vec::new(),
            decls: Vec::with_capacity(nodes),
            edges: Vec::with_capacity(nodes),
            overflow: false,
        }
    }

    /// Declares a node. A later declaration reusing a key is reported as [`crate::Violation::DuplicateNode`], and the
    /// edges added to it are dropped.
    pub fn node(&mut self, key: K) -> Decl {
        match u32::try_from(self.decls.len()) {
            Ok(n) if n < u32::MAX => {
                self.decls.push(key);
                Decl { builder: self.id, index: n }
            }
            _ => {
                self.overflow = true;
                Decl { builder: self.id, index: u32::MAX }
            }
        }
    }

    /// Adds an edge: `dependent` depends on the node keyed `prerequisite`, which may be declared before or after it.
    /// Repeating an edge with the same kind has no further effect.
    pub fn depends_on(&mut self, dependent: Decl, prerequisite: K, kind: E) -> &mut Self {
        let mine = dependent.builder == self.id
            || self.ancestors.iter().any(|&(id, upto)| id == dependent.builder && dependent.index < upto);
        if mine && (dependent.index as usize) < self.decls.len() {
            self.edges.push((dependent.index, prerequisite, kind));
        }
        self
    }

    /// Builds the graph, keeping every node and edge it can use and recording the rest: duplicate declarations,
    /// edges to unknown keys (always kept in [`Graph::dangling`]; `unknown` decides whether validation reports them)
    /// and more than `u32::MAX - 1` nodes.
    #[must_use]
    pub fn build(self, unknown: UnknownTargets) -> Graph<K, E> {
        let Self { id: _, ancestors: _, decls, edges, overflow } = self;

        // Declarations sorted by key, ties by position: the first of each run of equal keys is the node.
        let mut sorted: Vec<u32> = (0..decls.len()).filter_map(|i| u32::try_from(i).ok()).collect();
        sorted.sort_by(|&a, &b| decl(&decls, a).cmp(&decl(&decls, b)).then(a.cmp(&b)));
        let mut is_duplicate = alloc::vec![false; decls.len()];
        for pair in sorted.windows(2) {
            if let [a, b] = *pair {
                if decl(&decls, a) == decl(&decls, b) {
                    if let Some(d) = is_duplicate.get_mut(b as usize) {
                        *d = true;
                    }
                }
            }
        }

        let mut node_of_decl: Vec<Option<u32>> = Vec::with_capacity(decls.len());
        let mut keys: Vec<K> = Vec::with_capacity(decls.len());
        let mut duplicates: Vec<K> = Vec::new();
        for (key, dup) in decls.into_iter().zip(is_duplicate) {
            if dup {
                node_of_decl.push(None);
                duplicates.push(key);
            } else {
                node_of_decl.push(u32::try_from(keys.len()).ok());
                keys.push(key);
            }
        }
        let mut by_key: Vec<u32> = (0..keys.len()).filter_map(|i| u32::try_from(i).ok()).collect();
        by_key.sort_by(|&a, &b| decl(&keys, a).cmp(&decl(&keys, b)));

        let mut resolved: Vec<Edge<E>> = Vec::with_capacity(edges.len());
        let mut dangling: Vec<Dangling<K, E>> = Vec::new();
        for (seq, (d, prerequisite, kind)) in edges.into_iter().enumerate() {
            let Some(dependent) = node_of_decl.get(d as usize).copied().flatten() else { continue };
            match lookup(&keys, &by_key, &prerequisite) {
                Some(p) => resolved.push(Edge { dependent, prerequisite: p, kind, seq }),
                None => dangling.push(Dangling { dependent: NodeIx(dependent), prerequisite, kind, seq }),
            }
        }
        // One edge per (dependent, prerequisite, kind), the first declared; then back to declaration order.
        resolved.sort_by(|a, b| {
            (a.dependent, a.prerequisite, &a.kind, a.seq).cmp(&(b.dependent, b.prerequisite, &b.kind, b.seq))
        });
        resolved.dedup_by(|b, a| a.dependent == b.dependent && a.prerequisite == b.prerequisite && a.kind == b.kind);
        resolved.sort_by_key(|e| e.seq);
        dangling.sort_by(|a, b| {
            (a.dependent, &a.prerequisite, &a.kind, a.seq).cmp(&(b.dependent, &b.prerequisite, &b.kind, b.seq))
        });
        dangling.dedup_by(|b, a| a.dependent == b.dependent && a.prerequisite == b.prerequisite && a.kind == b.kind);
        dangling.sort_by_key(|e| e.seq);

        Graph {
            keys: Arc::from(keys),
            by_key: Arc::from(by_key),
            edges: resolved,
            dangling,
            duplicates,
            unknown,
            overflow,
        }
    }
}

fn decl<K>(keys: &[K], i: u32) -> Option<&K> {
    keys.get(i as usize)
}

/// Binary search of `by_key` (node indices sorted by key) for `key`.
pub(crate) fn lookup<K, Q>(keys: &[K], by_key: &[u32], key: &Q) -> Option<u32>
where
    K: core::borrow::Borrow<Q>,
    Q: Ord + ?Sized,
{
    let pos = by_key
        .binary_search_by(|&i| match keys.get(i as usize) {
            Some(k) => k.borrow().cmp(key),
            None => core::cmp::Ordering::Less,
        })
        .ok()?;
    by_key.get(pos).copied()
}

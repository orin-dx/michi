//! # michi-graph
//!
//! Deterministic dependency graphs for dev tools: steps in a pipeline, operations in a release, packages in a
//! workspace, tasks in a plan.
//!
//! 1. Declare nodes and edges with a [`GraphBuilder`]. An edge says a dependent depends on a prerequisite, and has a
//!    kind (`()` unless you need kinds). Building never fails: problems are recorded for validation.
//! 2. [`Graph::validate`] (every edge) or [`Graph::validate_filtered`] (edges you choose) returns a [`Dag`], or
//!    [`Violations`] listing every problem at once: duplicate nodes, unknown dependencies, self-dependencies and
//!    cycles, each cycle with one concrete loop.
//! 3. Query the [`Dag`]: topological order, layers, ready sets, ancestors and descendants.
//!
//! The caller picks how ties break ([`Order::Declaration`] or [`Order::Key`]) and every output follows it, so the same
//! input always gives the same result. Nothing recurses, so long chains cannot overflow the stack.
//!
//! ```
//! use michi_graph::{Dag, Order};
//!
//! let dag = Dag::from_nodes(
//!     [("build", vec![]), ("test", vec!["build"]), ("lint", vec![]), ("ship", vec!["test", "lint"])],
//!     Order::Declaration,
//! )
//! .unwrap();
//! assert_eq!(dag.keys_in_order().copied().collect::<Vec<_>>(), ["build", "test", "lint", "ship"]);
//!
//! let layers: Vec<Vec<&str>> = dag.layers().iter().map(|l| l.iter().map(|&ix| *dag.key(ix).unwrap()).collect()).collect();
//! assert_eq!(layers, [vec!["build", "lint"], vec!["test"], vec!["ship"]]);
//!
//! let err = Dag::from_nodes([("a", vec!["b"]), ("b", vec!["a"])], Order::Key).unwrap_err();
//! assert_eq!(err.to_string(), "dependency cycle: a depends on b, which depends on a");
//! ```
//!
//! See `docs/spec/09-graph.md` in the michi repository.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![warn(clippy::must_use_candidate, clippy::missing_errors_doc, clippy::missing_panics_doc)]
#![warn(missing_debug_implementations)]

extern crate alloc;

// Runs the README examples as doctests without making them the rendered crate docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod algo;
mod builder;
mod dag;
mod graph;
mod violation;

pub use builder::{Decl, GraphBuilder};
pub use dag::{Dag, Layers};
pub use graph::{Dangling, EdgeRef, Graph};
pub use violation::{Cycle, Violation, Violations};

/// A node's position among its graph's nodes, in declaration order: the first node declared is 0. Duplicate
/// declarations are not nodes and don't count, so in a graph that validates (no duplicates) it is exactly the
/// declaration position.
///
/// There is no public constructor. Methods that take a `NodeIx` return an empty slice or `None` for an index their
/// graph does not have, so an index from another graph never panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeIx(u32);

impl NodeIx {
    /// The declaration position as a `usize`, for indexing your own per-node storage.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    fn u(self) -> usize {
        self.0 as usize
    }
}

/// How ties break wherever order is free: in the topological order, layers, ready sets, closures and cycle reports.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// The earliest-declared node first: things run in the order they were written.
    #[default]
    Declaration,
    /// The smallest key first: shuffling the declarations changes nothing.
    Key,
}

/// What validation does with an edge whose prerequisite no node has. The edge is kept in [`Graph::dangling`] either
/// way.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnknownTargets {
    /// Report it as [`Violation::UnknownDependency`].
    #[default]
    Error,
    /// Leave it out of the dag without a violation, for prerequisites that live outside the graph on purpose.
    Ignore,
}

//! What validation reports.

use alloc::vec::Vec;
use core::fmt;

/// Steps of a cycle path printed before the rest is summarized.
const PATH_LIMIT: usize = 8;

/// One strongly connected component that forms a cycle.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle<K> {
    members: Vec<K>,
    path: Vec<K>,
}

impl<K> Cycle<K> {
    pub(crate) fn new(members: Vec<K>, path: Vec<K>) -> Self {
        Self { members, path }
    }

    /// Every node of the component, in the validation's [`crate::Order`].
    #[must_use]
    pub fn members(&self) -> &[K] {
        &self.members
    }

    /// One shortest loop through the first member: each node depends on the next, and the last depends on the first.
    #[must_use]
    pub fn path(&self) -> &[K] {
        &self.path
    }

    fn write(
        &self,
        f: &mut fmt::Formatter<'_>,
        key: &dyn Fn(&K, &mut fmt::Formatter<'_>) -> fmt::Result,
    ) -> fmt::Result {
        let Some(first) = self.path.first() else { return f.write_str("dependency cycle") };
        f.write_str("dependency cycle: ")?;
        key(first, f)?;
        for (i, k) in self.path.iter().skip(1).enumerate() {
            if i == PATH_LIMIT {
                return write!(f, " … and {} more", self.path.len() - 1 - PATH_LIMIT);
            }
            f.write_str(if i == 0 { " depends on " } else { ", which depends on " })?;
            key(k, f)?;
        }
        f.write_str(if self.path.len() == 1 { " depends on " } else { ", which depends on " })?;
        key(first, f)
    }
}

/// One problem found by validation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation<K> {
    /// A later declaration reuses an earlier node's key. Its edges were dropped.
    DuplicateNode {
        /// The repeated key.
        key: K,
    },
    /// A node depends on a key no node has.
    UnknownDependency {
        /// The node declaring the dependency.
        dependent: K,
        /// The missing key.
        prerequisite: K,
    },
    /// A node depends on itself. Reported once per node, never as a cycle.
    SelfDependency {
        /// The node.
        key: K,
    },
    /// Nodes that depend on each other in a loop.
    Cycle(Cycle<K>),
    /// More than `u32::MAX - 1` nodes were declared; the rest were dropped.
    TooManyNodes,
}

impl<K> Violation<K> {
    fn write(
        &self,
        f: &mut fmt::Formatter<'_>,
        key: &dyn Fn(&K, &mut fmt::Formatter<'_>) -> fmt::Result,
    ) -> fmt::Result {
        match self {
            Self::DuplicateNode { key: k } => {
                f.write_str("duplicate node ")?;
                key(k, f)
            }
            Self::UnknownDependency { dependent, prerequisite } => {
                key(dependent, f)?;
                f.write_str(" depends on unknown node ")?;
                key(prerequisite, f)
            }
            Self::SelfDependency { key: k } => {
                key(k, f)?;
                f.write_str(" depends on itself")
            }
            Self::Cycle(cycle) => cycle.write(f, key),
            Self::TooManyNodes => write!(f, "more than {} nodes", u32::MAX - 1),
        }
    }
}

impl<K: fmt::Display> fmt::Display for Violation<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, &|k, f| k.fmt(f))
    }
}

impl<K: fmt::Display> fmt::Display for Cycle<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, &|k, f| k.fmt(f))
    }
}

/// Every problem validation found, in a fixed order: duplicates, then [`Violation::TooManyNodes`], then edge
/// problems, then cycles, each group in the validation's [`crate::Order`]. Under [`crate::Order::Declaration`] edge
/// problems go by the dependent's declaration position, then the edge's; under [`crate::Order::Key`] by the
/// dependent's key, then the missing key. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violations<K> {
    list: Vec<Violation<K>>,
}

impl<K> Violations<K> {
    pub(crate) fn new(list: Vec<Violation<K>>) -> Self {
        Self { list }
    }

    /// The violations.
    #[must_use]
    pub fn as_slice(&self) -> &[Violation<K>] {
        &self.list
    }

    /// The violations, owned.
    #[must_use]
    pub fn into_vec(self) -> Vec<Violation<K>> {
        self.list
    }

    /// Iterates the violations.
    pub fn iter(&self) -> core::slice::Iter<'_, Violation<K>> {
        self.list.iter()
    }

    /// Renders with `key` writing each key, for key types without `Display`.
    pub fn display_with<F>(&self, key: F) -> impl fmt::Display + '_
    where
        F: Fn(&K, &mut fmt::Formatter<'_>) -> fmt::Result + 'static,
    {
        DisplayWith { violations: self, key }
    }

    fn write(
        &self,
        f: &mut fmt::Formatter<'_>,
        key: &dyn Fn(&K, &mut fmt::Formatter<'_>) -> fmt::Result,
    ) -> fmt::Result {
        for (i, v) in self.list.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            v.write(f, key)?;
        }
        Ok(())
    }
}

impl<'a, K> IntoIterator for &'a Violations<K> {
    type Item = &'a Violation<K>;
    type IntoIter = core::slice::Iter<'a, Violation<K>>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

impl<K> IntoIterator for Violations<K> {
    type Item = Violation<K>;
    type IntoIter = alloc::vec::IntoIter<Violation<K>>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

impl<K: fmt::Display> fmt::Display for Violations<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, &|k, f| k.fmt(f))
    }
}

impl<K: fmt::Debug + fmt::Display> core::error::Error for Violations<K> {}

struct DisplayWith<'a, K, F> {
    violations: &'a Violations<K>,
    key: F,
}

impl<K, F> fmt::Display for DisplayWith<'_, K, F>
where
    F: Fn(&K, &mut fmt::Formatter<'_>) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.violations.write(f, &self.key)
    }
}

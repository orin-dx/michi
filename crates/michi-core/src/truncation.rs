//! # List truncation
//!
//! [`truncate`](mod@crate::truncate) cuts one string; this module reports what a cap on a **list** cut.
//!
//! Deciding which items survive a cap is the calling tool's job: a first-N cutoff, a token budget, a ranking tuned to
//! its data. Reporting what was cut is the same everywhere, so michi owns that contract and not the heuristic. Wrap
//! whatever was kept in a [`TruncationOutcome`] (with [`TruncationOutcome::new`], or one of the two strategies michi
//! ships, [`truncate_by_count`] and [`truncate_by_size`]) and every tool gets the same signal: the total before the
//! cut, and a `help[]` hint worded the same way. [`AgentResponse::truncation`](crate::AgentResponse::truncation)
//! puts both into a response.
//!
//! ```
//! use michi_core::{truncate_by_count, Hint};
//!
//! let outcome = truncate_by_count(vec!["a", "b", "c", "d"], 2);
//! assert_eq!(outcome.kept, ["a", "b"]);
//! assert_eq!(outcome.omitted(), 2);
//! assert_eq!(outcome.hint("pass --limit 4"), Some(Hint::new("showing 2 of 4; pass --limit 4")));
//! ```

use crate::hints::Hint;

/// What survived a cap on a list, however the survivors were chosen.
///
/// `truncated()` and `omitted()` are derived from `kept.len()` and `total_before`, never stored, so they cannot
/// disagree with the data. Build one with [`TruncationOutcome::new`], [`truncate_by_count`] or [`truncate_by_size`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TruncationOutcome<T> {
    /// The items that survived, in the order the strategy kept them.
    pub kept: Vec<T>,
    /// How many items there were before the cut. Never less than `kept.len()` when built by michi.
    pub total_before: usize,
}

impl<T> TruncationOutcome<T> {
    /// Wrap the result of the caller's own selection: `kept` and the item count before the cut.
    ///
    /// A `total_before` below `kept.len()` cannot be right, so it is raised to `kept.len()` (infallible constructors
    /// normalize; see `PRINCIPLES.md` §6).
    #[must_use]
    pub fn new(kept: Vec<T>, total_before: usize) -> Self {
        Self { total_before: total_before.max(kept.len()), kept }
    }

    /// Whether any item was cut: `kept.len() < total_before`.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.kept.len() < self.total_before
    }

    /// How many items were cut: `total_before - kept.len()`, or 0 if the public fields were later assigned so that
    /// `kept` outnumbers `total_before`.
    #[must_use]
    pub fn omitted(&self) -> usize {
        self.total_before.saturating_sub(self.kept.len())
    }

    /// The `help[]` hint reporting the cut: `showing {kept} of {total_before}; {guidance}`, or
    /// `showing {kept} of {total_before}` when `guidance` is empty. `None` when nothing was cut.
    ///
    /// `guidance` is the caller's instruction for getting the rest (`pass --limit 120`, `use offset=50`): michi never
    /// invents a flag the calling tool may not have, as with [`Truncated::hint`](crate::truncate::Truncated::hint).
    #[must_use]
    pub fn hint(&self, guidance: impl Into<String>) -> Option<Hint> {
        if !self.truncated() {
            return None;
        }
        let guidance = guidance.into();
        let kept = self.kept.len().to_string();
        let total = self.total_before.to_string();
        let mut text = String::with_capacity(14 + kept.len() + total.len() + guidance.len());
        text.push_str("showing ");
        text.push_str(&kept);
        text.push_str(" of ");
        text.push_str(&total);
        if !guidance.is_empty() {
            text.push_str("; ");
            text.push_str(&guidance);
        }
        Some(Hint::new(text))
    }
}

/// Keep the first `max` items, in order, and record how many there were.
///
/// ```
/// use michi_core::truncate_by_count;
///
/// let outcome = truncate_by_count(vec![5, 4, 3], 2);
/// assert_eq!(outcome.kept, [5, 4]);
/// assert_eq!(outcome.total_before, 3);
/// ```
#[must_use]
pub fn truncate_by_count<T>(mut items: Vec<T>, max: usize) -> TruncationOutcome<T> {
    let total_before = items.len();
    items.truncate(max);
    TruncationOutcome { kept: items, total_before }
}

/// Keep items in order while their summed `size_of` stays within `budget`, stopping at the first item that does not
/// fit.
///
/// `size_of` is the caller's measure (bytes, LLM tokens, rendered lines), so michi never guesses what a budget
/// counts. The walk stops at the first item that would overflow the budget, even if a later, smaller item would fit:
/// `kept` is always a prefix of `items`, so the order says exactly what was dropped. A tool that prefers to skip and
/// keep filling builds its own selection and wraps it with [`TruncationOutcome::new`].
///
/// A first item larger than the whole budget leaves `kept` empty, and `truncated()` is true. Items of size 0 always
/// fit. The running total is checked, not wrapping: a sum past `usize::MAX` counts as over budget.
///
/// ```
/// use michi_core::truncate_by_size;
///
/// let outcome = truncate_by_size(vec!["alpha", "beta", "gamma"], 10, |s| s.len());
/// assert_eq!(outcome.kept, ["alpha", "beta"]);
/// assert_eq!(outcome.omitted(), 1);
/// ```
#[must_use]
pub fn truncate_by_size<T>(
    mut items: Vec<T>,
    budget: usize,
    mut size_of: impl FnMut(&T) -> usize,
) -> TruncationOutcome<T> {
    let total_before = items.len();
    let mut used = 0usize;
    let mut fit = 0;
    for item in &items {
        match used.checked_add(size_of(item)) {
            Some(next) if next <= budget => used = next,
            _ => break,
        }
        fit += 1;
    }
    items.truncate(fit);
    TruncationOutcome { kept: items, total_before }
}

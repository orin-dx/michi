//! Repeatable response sections: several named blocks in one [`crate::AgentResponse`].

use crate::kv::KvItem;
use crate::toon::Value;

/// One named block within a response. A response may hold any number; they render in insertion order.
///
/// A section is a root key of the rendered TOON document: a list or text section under its `name`, a KV section as its
/// item keys. Root keys must be unique, so two sections cannot share a name, and no name or KV key may be one of the
/// keys michi writes itself (`budget`, `sections`, `provenance`, `truncated`, `totalCount`, `help`, `recovery`,
/// `error`, `message`, `causes`, `exit_code`). A response that breaks this renders the `toon_validation_failed` (or
/// `kv_validation_failed`) error naming the key, never invalid TOON.
///
/// The fields are private, so what a section reports about itself has one source each: [`count`](Self::count) is the
/// body's rows or items, [`total`](Self::total) is the one number the caller sets, and
/// [`truncated`](Self::truncated) is derived from those two. No section can print `totalCount: 5` while its JSON says
/// it was not cut.
///
/// # Stability
///
/// The multi-section envelope (this type, [`crate::BudgetSummary`], [`crate::ProvenanceBlock`]) is unstable. It
/// shipped ahead of a second independent consumer (ADR 10), so its shape may change before 1.0, and 1.0 promises only
/// what consumers have used by then.
#[derive(Debug, Clone)]
pub struct Section {
    name: String,
    body: SectionBody,
    total_count: Option<usize>,
}

/// What a [`Section`] holds.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SectionBody {
    /// A uniform list, rendered as a TOON list with the response's per-cell truncation applied.
    Toon {
        /// Ordered, distinct field names.
        fields: Vec<String>,
        /// Rows, parallel to `fields`.
        rows: Vec<Vec<Value>>,
    },
    /// A single receipt or a small set of labelled scalars.
    Kv(Vec<KvItem>),
    /// One string value, rendered as `name: <value>` and escaped like any other TOON string (quoted when it has a
    /// newline, a colon, or looks like a number). Prose cannot start a line of its own, so it cannot forge a `help[]`
    /// line. It is not a way to insert a fragment of TOON.
    Text {
        /// The value.
        text: String,
        /// How many items the prose stands for, since michi cannot count items in prose.
        count: usize,
    },
}

impl Section {
    /// A TOON list section. Its count is `rows.len()`.
    #[must_use]
    pub fn toon(name: impl Into<String>, fields: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self::with(name, SectionBody::Toon { fields, rows })
    }

    /// A key-value section. Its count is `items.len()`.
    #[must_use]
    pub fn kv(name: impl Into<String>, items: Vec<KvItem>) -> Self {
        Self::with(name, SectionBody::Kv(items))
    }

    /// A text section: a single string value, rendered as `name: <escaped body>`. Its count is 1 for non-empty text
    /// and 0 for empty; use [`text_with_count`](Self::text_with_count) when the prose stands for more items.
    #[must_use]
    pub fn text(name: impl Into<String>, body: impl Into<String>) -> Self {
        let text = body.into();
        let count = usize::from(!text.is_empty());
        Self::with(name, SectionBody::Text { text, count })
    }

    /// A text section that stands for `count` items, such as a summary of `count` matches.
    #[must_use]
    pub fn text_with_count(name: impl Into<String>, body: impl Into<String>, count: usize) -> Self {
        Self::with(name, SectionBody::Text { text: body.into(), count })
    }

    fn with(name: impl Into<String>, body: SectionBody) -> Self {
        Self { name: name.into(), body, total_count: None }
    }

    /// Record how many items existed before a cut. `Some(n)` with `n` greater than [`count`](Self::count) renders
    /// `totalCount: n` and makes [`truncated`](Self::truncated) true.
    #[must_use]
    pub fn total_count(mut self, total: Option<usize>) -> Self {
        self.total_count = total;
        self
    }

    /// The section's type name: the identifier at the head of its TOON line, and the key of a text body. A KV body
    /// prints no heading, so there the name only labels it for the caller (and for `sections` in JSON).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The section's contents.
    #[must_use]
    pub fn body(&self) -> &SectionBody {
        &self.body
    }

    /// How many items this section carries: `rows.len()` or `items.len()` for `Toon` and `Kv` bodies, the count given
    /// to the constructor for `Text`.
    #[must_use]
    pub fn count(&self) -> usize {
        match &self.body {
            SectionBody::Toon { rows, .. } => rows.len(),
            SectionBody::Kv(items) => items.len(),
            SectionBody::Text { count, .. } => *count,
        }
    }

    /// The pre-cut total set with [`total_count`](Self::total_count), as given.
    #[must_use]
    pub fn total(&self) -> Option<usize> {
        self.total_count
    }

    /// Whether this section was cut: its total exceeds its count. Derived, never set.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.total_count.is_some_and(|n| n > self.count())
    }

    /// The `totalCount` this section reports, the one rule every surface reads (the text's `totalCount`, the JSON
    /// `sections` entry, the budget roll-up's `total` column): the total when it exceeds [`count`](Self::count), `0` for
    /// an empty `Toon` list with no larger total (the definitive empty state), otherwise none. A total at or below the
    /// count says nothing the count does not.
    pub(crate) fn reported_total(&self) -> Option<usize> {
        let cut = self.total_count.filter(|n| *n > self.count());
        cut.or_else(|| matches!(&self.body, SectionBody::Toon { rows, .. } if rows.is_empty()).then_some(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_is_exactly_total_above_count() {
        let one = || Section::toon("t", vec!["a".into()], vec![vec![Value::Int(1)]]);
        assert!(one().total_count(Some(2)).truncated());
        assert!(!one().total_count(Some(1)).truncated());
        assert!(!one().total_count(Some(0)).truncated());
        assert!(!one().truncated());
        assert!(Section::text_with_count("n", "x", 3).total_count(Some(4)).truncated());
        assert!(!Section::text_with_count("n", "x", 3).total_count(Some(3)).truncated());
    }
}

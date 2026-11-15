//! Repeatable response sections: several named blocks in one [`crate::AgentResponse`].

use crate::kv::KvItem;
use crate::toon::Value;

/// One named block within a response. A response may hold any number; they render in insertion order.
///
/// A section is a root key of the rendered TOON document: a list or text section under its `name`, a KV section as its
/// item keys. Root keys must be unique, so two sections cannot share a name, and no name or KV key may be one of the
/// keys michi writes itself (`budget`, `sections`, `provenance`, `truncated`, `totalCount`, `help`, `recovery`). A
/// response that breaks this renders the `toon_validation_failed` error naming the key, never invalid TOON.
///
/// # Stability
///
/// The multi-section envelope (this type, [`crate::BudgetSummary`], [`crate::ProvenanceBlock`]) is unstable. It
/// shipped ahead of a second independent consumer (ADR 10), so its shape may change before 1.0, and 1.0 promises only
/// what consumers have used by then.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Section {
    /// The section's type name: the identifier at the head of its TOON line, and the key of a text body. A KV body
    /// prints no heading, so there the name only labels it for the caller (and for `sections` in JSON).
    pub name: String,
    /// The section's contents.
    pub body: SectionBody,
    /// How many items this section carries, as set by the constructor. Rendering never trusts this field for `Toon`
    /// and `Kv` bodies: it counts `rows` / `items` itself (see [`Section::count`]). `Text` bodies keep the declared
    /// value, since michi cannot count items in prose.
    pub returned_count: usize,
    /// How many existed before truncation. `Some(n)` with `n > returned_count` renders `totalCount: n`.
    pub total_count: Option<usize>,
    /// Whether producing this section dropped anything. Carried for the caller; it does not change the text render.
    pub truncated: bool,
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
    Text(String),
}

impl Section {
    /// A TOON list section. `returned_count` is `rows.len()`.
    #[must_use]
    pub fn toon(name: impl Into<String>, fields: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self::with(name, rows.len(), SectionBody::Toon { fields, rows })
    }

    /// A key-value section. `returned_count` is `items.len()`.
    #[must_use]
    pub fn kv(name: impl Into<String>, items: Vec<KvItem>) -> Self {
        Self::with(name, items.len(), SectionBody::Kv(items))
    }

    /// A text section: a single string value, rendered as `name: <escaped body>`. `returned_count` starts at 1 for
    /// non-empty text and 0 for empty; set it with [`returned_count`](Self::returned_count). A `totalCount` renders
    /// as for the other bodies, when [`total_count`](Self::total_count) exceeds that count.
    #[must_use]
    pub fn text(name: impl Into<String>, body: impl Into<String>) -> Self {
        let body = body.into();
        Self::with(name, usize::from(!body.is_empty()), SectionBody::Text(body))
    }

    fn with(name: impl Into<String>, returned_count: usize, body: SectionBody) -> Self {
        Self { name: name.into(), body, returned_count, total_count: None, truncated: false }
    }

    /// Record the pre-truncation count. `Some(n)` with `n` greater than the returned count renders `totalCount: n`.
    #[must_use]
    pub fn total_count(mut self, total: Option<usize>) -> Self {
        self.total_count = total;
        self
    }

    /// Record whether producing this section dropped anything.
    #[must_use]
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    /// Declare how many items a `Text` body carries. Ignored for `Toon` and `Kv` bodies, whose count is derived.
    #[must_use]
    pub fn returned_count(mut self, n: usize) -> Self {
        if matches!(self.body, SectionBody::Text(_)) {
            self.returned_count = n;
        }
        self
    }

    /// The count rendering uses: `rows.len()` or `items.len()` for `Toon` and `Kv` bodies, the declared
    /// `returned_count` for `Text`. A hand-edited `returned_count` field cannot disagree with the body.
    #[must_use]
    pub fn count(&self) -> usize {
        match &self.body {
            SectionBody::Toon { rows, .. } => rows.len(),
            SectionBody::Kv(items) => items.len(),
            SectionBody::Text(_) => self.returned_count,
        }
    }

    /// The `totalCount` to render: `total_count` only when it exceeds [`count`](Self::count).
    pub(crate) fn shown_total(&self) -> Option<usize> {
        self.total_count.filter(|n| *n > self.count())
    }
}

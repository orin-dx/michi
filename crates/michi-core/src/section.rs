//! Repeatable response sections: several named blocks in one [`AgentResponse`](crate::AgentResponse).

use crate::kv::KvItem;
use crate::toon::Value;

/// One named block within a response. A response may hold any number; they render in insertion order.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Section {
    /// The section's type name: the identifier at the head of its TOON line. KV and text bodies carry no heading, so
    /// the name only labels them for the caller.
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
    /// Prose or an already-rendered fragment, passed through unchanged. It is not escaped or validated: the caller
    /// owns its shape.
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

    /// A text section, passed through unchanged. `returned_count` starts at 1 for non-empty text and 0 for empty;
    /// set it with [`returned_count`](Self::returned_count).
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

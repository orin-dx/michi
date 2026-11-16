//! Response-level budget report: what ceilings a caller worked under and what the response cost. michi renders a
//! budget; it never computes one. See `docs/spec/07-wisp-and-monokl-envelope.md` §2.

use std::fmt::Write as _;

use crate::section::Section;
use crate::toon::table::{self, Cell};

/// What a budget block renders: the ceilings a caller was working under and what the response cost against them.
///
/// Every number is optional, so a caller who measured only tokens sets two fields and leaves the rest empty. Absent
/// fields are omitted from the rendered block, never written as `null`. `u64` rather than `usize` because these
/// values reach JSON, where a platform-dependent width means nothing.
///
/// The per-section roll-up is not part of this type: michi derives it from the response's own sections when it
/// renders, so the roll-up can only name sections the response holds, with the counts they render.
///
/// `#[non_exhaustive]`: start from [`BudgetSummary::new`] or `Default` and use the builder methods.
///
/// # Stability
///
/// Part of the multi-section envelope, which is unstable until a second independent consumer has used it (ADR 10).
/// Its shape may change before 1.0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename_all = "camelCase"))]
#[non_exhaustive]
pub struct BudgetSummary {
    /// Token ceiling the caller set.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub max_tokens: Option<u64>,
    /// Byte ceiling the caller set.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub max_bytes: Option<u64>,
    /// Item ceiling the caller set.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub max_items: Option<u64>,
    /// Tokens the response cost, as the caller measured it.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tokens_used: Option<u64>,
    /// Bytes the response cost, as the caller measured it.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub bytes_used: Option<u64>,
    /// Whether the budget dropped something the sections cannot show, such as a whole section left out. The rendered
    /// `truncated` is also true whenever a section of the response was cut.
    pub truncated: bool,
}

impl BudgetSummary {
    /// An empty summary: no ceilings, no measurements, not truncated.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the token ceiling.
    #[must_use]
    pub fn max_tokens(mut self, n: u64) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Set the byte ceiling.
    #[must_use]
    pub fn max_bytes(mut self, n: u64) -> Self {
        self.max_bytes = Some(n);
        self
    }

    /// Set the item ceiling.
    #[must_use]
    pub fn max_items(mut self, n: u64) -> Self {
        self.max_items = Some(n);
        self
    }

    /// Set the tokens the response cost.
    #[must_use]
    pub fn tokens_used(mut self, n: u64) -> Self {
        self.tokens_used = Some(n);
        self
    }

    /// Set the bytes the response cost.
    #[must_use]
    pub fn bytes_used(mut self, n: u64) -> Self {
        self.bytes_used = Some(n);
        self
    }

    /// Set whether the budget dropped something the sections cannot show (a section left out entirely). A cut a
    /// section reports through its own total needs no call: it makes the rendered `truncated` true by itself.
    #[must_use]
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }
}

/// The `truncated` value a budget renders: the caller's flag, or any section that was cut.
fn rendered_truncated(b: &BudgetSummary, sections: &[Section]) -> bool {
    b.truncated || sections.iter().any(Section::truncated)
}

/// Render the `budget:` block, with no trailing newline. With two or more `sections`, the
/// `sections[n]{name,returned,total,truncated}:` roll-up of those sections follows. An unknown `total` is the TOON
/// `null` cell, because a table row needs a value in every column.
pub(crate) fn render_budget_block(b: &BudgetSummary, sections: &[Section]) -> String {
    let mut out = String::with_capacity(160 + sections.len() * 48);
    out.push_str("budget:");
    for (key, value) in numbers(b) {
        if let Some(n) = value {
            let _ = write!(out, "\n  {key}: {n}");
        }
    }
    let _ = write!(out, "\n  truncated: {}", rendered_truncated(b, sections));
    if sections.len() >= 2 {
        table::push_header(&mut out, "sections", sections.len(), ["name", "returned", "total", "truncated"]);
        for s in sections {
            let cells =
                [Cell::Str(s.name()), Cell::count(s.count()), Cell::opt_count(s.total()), Cell::Bool(s.truncated())];
            table::push_row(&mut out, &cells);
        }
    }
    out
}

/// The optional counters in render order.
fn numbers(b: &BudgetSummary) -> [(&'static str, Option<u64>); 5] {
    [
        ("tokensUsed", b.tokens_used),
        ("bytesUsed", b.bytes_used),
        ("maxTokens", b.max_tokens),
        ("maxBytes", b.max_bytes),
        ("maxItems", b.max_items),
    ]
}

/// Append the budget as a JSON object with camelCase keys; absent numbers are omitted. There is no per-section list:
/// the response's own `sections` array carries each section's counts.
pub(crate) fn push_budget_json(out: &mut String, b: &BudgetSummary, sections: &[Section]) {
    out.push('{');
    for (key, value) in numbers(b) {
        if let Some(n) = value {
            let _ = write!(out, "\"{key}\":{n},");
        }
    }
    out.push_str("\"truncated\":");
    crate::json::push_bool(out, rendered_truncated(b, sections));
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toon::Value;

    fn spec_example() -> BudgetSummary {
        BudgetSummary::new().tokens_used(7412).bytes_used(31208).max_tokens(8000).max_bytes(2_097_152)
    }

    /// The two sections of spec 07's example: 12 of 40 symbols, all 5 refs.
    fn spec_sections() -> Vec<Section> {
        let rows = |n| vec![vec![Value::Int(1)]; n];
        vec![
            Section::toon("brief.symbols", vec!["n".into()], rows(12)).total_count(Some(40)),
            Section::toon("brief.refs", vec!["n".into()], rows(5)).total_count(Some(5)),
        ]
    }

    fn json(b: &BudgetSummary, sections: &[Section]) -> String {
        let mut out = String::new();
        push_budget_json(&mut out, b, sections);
        out
    }

    const SPEC_BLOCK: &str =
        "budget:\n  tokensUsed: 7412\n  bytesUsed: 31208\n  maxTokens: 8000\n  maxBytes: 2097152\n  truncated: true";
    const SPEC_ROLLUP: &str =
        "\nsections[2]{name,returned,total,truncated}:\n  brief.symbols,12,40,true\n  brief.refs,5,5,false";

    #[test]
    fn spec_example_renders_exactly_with_rollup() {
        assert_eq!(render_budget_block(&spec_example(), &spec_sections()), format!("{SPEC_BLOCK}{SPEC_ROLLUP}"));
    }

    #[test]
    fn rollup_only_with_two_or_more_sections() {
        assert_eq!(render_budget_block(&spec_example(), &spec_sections()[..1]), SPEC_BLOCK);
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let b = BudgetSummary::new().tokens_used(10).max_items(3);
        let out = render_budget_block(&b, &[]);
        assert_eq!(out, "budget:\n  tokensUsed: 10\n  maxItems: 3\n  truncated: false");
        assert!(!out.contains("null"));
        assert_eq!(render_budget_block(&BudgetSummary::new(), &[]), "budget:\n  truncated: false");
    }

    #[test]
    fn unknown_total_is_a_null_cell_and_hostile_names_are_quoted() {
        let sections = [Section::kv("a,b\nbudget: x", vec![]), Section::text("n", "x")];
        let out = render_budget_block(&BudgetSummary::new(), &sections);
        assert_eq!(
            out,
            "budget:\n  truncated: false\nsections[2]{name,returned,total,truncated}:\n  \"a,b\\nbudget: x\",0,null,false\n  \
             n,1,null,false"
        );
        assert_eq!(out.lines().count(), 5, "no fake lines");
    }

    #[test]
    fn json_matches_spec_shape() {
        assert_eq!(
            json(&spec_example(), &spec_sections()),
            "{\"tokensUsed\":7412,\"bytesUsed\":31208,\"maxTokens\":8000,\"maxBytes\":2097152,\"truncated\":true}"
        );
        assert_eq!(json(&BudgetSummary::new(), &[]), "{\"truncated\":false}");
        assert_eq!(json(&BudgetSummary::new().truncated(true), &[]), "{\"truncated\":true}");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn hand_built_json_parses_and_matches_serde() {
        // Without sections the rendered `truncated` is the caller's flag, which is what serde writes.
        for b in [spec_example(), BudgetSummary::new().max_tokens(1).truncated(true), BudgetSummary::new()] {
            let hand: serde_json::Value = serde_json::from_str(&json(&b, &[])).expect("valid json");
            let derived = serde_json::to_value(&b).expect("serializes");
            assert_eq!(hand, derived);
        }
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn schema_uses_camel_case() {
        let schema = serde_json::to_string(&schemars::schema_for!(BudgetSummary)).expect("schema");
        for key in ["maxTokens", "tokensUsed", "bytesUsed", "maxItems"] {
            assert!(schema.contains(key), "{key} missing from {schema}");
        }
        assert!(!schema.contains("tokens_used"));
    }
}

//! Response-level budget report: what ceilings a caller worked under, what the response cost, and what each section
//! cost. michi renders a budget; it never computes one. See `docs/spec/07-wisp-and-monokl-envelope.md` §2.

use std::fmt::Write as _;

/// What a budget block renders: the ceilings a caller was working under, what the response cost against them, and
/// what that cost each section.
///
/// Every number is optional, so a caller who measured only tokens sets two fields and leaves the rest empty. Absent
/// fields are omitted from the rendered block, never written as `null`. `u64` rather than `usize` because these
/// values reach JSON, where a platform-dependent width means nothing.
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
    /// Whether the budget forced anything out of the response.
    pub truncated: bool,
    /// Per-section accounting, rendered in the order given.
    pub sections: Vec<SectionCost>,
}

impl BudgetSummary {
    /// An empty summary: no ceilings, no measurements, not truncated, no sections.
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

    /// Set whether the budget forced anything out of the response.
    #[must_use]
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    /// Append one section's accounting line. Lines render in the order added.
    #[must_use]
    pub fn section(mut self, cost: SectionCost) -> Self {
        self.sections.push(cost);
        self
    }
}

/// One section's line in the budget roll-up.
///
/// `#[non_exhaustive]`: build it with [`SectionCost::new`] and the builder methods.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename_all = "camelCase"))]
#[non_exhaustive]
pub struct SectionCost {
    /// The section's name.
    pub name: String,
    /// How many items the section returned.
    pub returned: u64,
    /// How many existed before truncation, when the caller knows.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub total: Option<u64>,
    /// Whether this section was cut.
    pub truncated: bool,
}

impl SectionCost {
    /// A section line with no known total, not truncated.
    #[must_use]
    pub fn new(name: impl Into<String>, returned: u64) -> Self {
        Self { name: name.into(), returned, total: None, truncated: false }
    }

    /// Set how many items existed before truncation.
    #[must_use]
    pub fn total(mut self, total: u64) -> Self {
        self.total = Some(total);
        self
    }

    /// Set whether this section was cut.
    #[must_use]
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }
}

/// Render the `budget:` block, with no trailing newline. When `show_rollup` is set and there are sections, the
/// `sections[n]{name,returned,total,truncated}:` table follows. An unknown `total` is the TOON `null` cell, because a
/// table row needs a value in every column.
pub(crate) fn render_budget_block(b: &BudgetSummary, show_rollup: bool) -> String {
    let mut out = String::with_capacity(160 + b.sections.len() * 48);
    out.push_str("budget:");
    for (key, value) in numbers(b) {
        if let Some(n) = value {
            let _ = write!(out, "\n  {key}: {n}");
        }
    }
    let _ = write!(out, "\n  truncated: {}", b.truncated);
    if show_rollup && !b.sections.is_empty() {
        let _ = write!(out, "\nsections[{}]{{name,returned,total,truncated}}:", b.sections.len());
        for s in &b.sections {
            let _ = write!(out, "\n  {},{},", crate::toon::escape_value(&s.name), s.returned);
            match s.total {
                Some(t) => {
                    let _ = write!(out, "{t}");
                }
                None => out.push_str("null"),
            }
            let _ = write!(out, ",{}", s.truncated);
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

/// The budget as a JSON object with camelCase keys. `sections` is always present; absent numbers are omitted.
pub(crate) fn budget_json(b: &BudgetSummary) -> String {
    let mut out = String::with_capacity(128 + b.sections.len() * 64);
    out.push('{');
    for (key, value) in numbers(b) {
        if let Some(n) = value {
            let _ = write!(out, "\"{key}\":{n},");
        }
    }
    let _ = write!(out, "\"truncated\":{},\"sections\":[", b.truncated);
    for (i, s) in b.sections.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        crate::kv::json_escape_str(&mut out, &s.name);
        let _ = write!(out, ",\"returned\":{}", s.returned);
        if let Some(t) = s.total {
            let _ = write!(out, ",\"total\":{t}");
        }
        let _ = write!(out, ",\"truncated\":{}}}", s.truncated);
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_example() -> BudgetSummary {
        BudgetSummary::new()
            .tokens_used(7412)
            .bytes_used(31208)
            .max_tokens(8000)
            .max_bytes(2_097_152)
            .truncated(true)
            .section(SectionCost::new("brief.symbols", 12).total(40).truncated(true))
            .section(SectionCost::new("brief.refs", 5).total(5))
    }

    const SPEC_BLOCK: &str =
        "budget:\n  tokensUsed: 7412\n  bytesUsed: 31208\n  maxTokens: 8000\n  maxBytes: 2097152\n  truncated: true";
    const SPEC_ROLLUP: &str =
        "\nsections[2]{name,returned,total,truncated}:\n  brief.symbols,12,40,true\n  brief.refs,5,5,false";

    #[test]
    fn spec_example_renders_exactly_with_rollup() {
        assert_eq!(render_budget_block(&spec_example(), true), format!("{SPEC_BLOCK}{SPEC_ROLLUP}"));
    }

    #[test]
    fn rollup_only_when_requested() {
        assert_eq!(render_budget_block(&spec_example(), false), SPEC_BLOCK);
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let b = BudgetSummary::new().tokens_used(10).max_items(3);
        let out = render_budget_block(&b, true);
        assert_eq!(out, "budget:\n  tokensUsed: 10\n  maxItems: 3\n  truncated: false");
        assert!(!out.contains("null"));
        assert_eq!(render_budget_block(&BudgetSummary::new(), true), "budget:\n  truncated: false");
    }

    #[test]
    fn unknown_total_is_a_null_cell_and_hostile_names_are_quoted() {
        let b = BudgetSummary::new().section(SectionCost::new("a,b\nbudget: x", 1));
        let out = render_budget_block(&b, true);
        assert_eq!(
            out,
            "budget:\n  truncated: false\nsections[1]{name,returned,total,truncated}:\n  \"a,b\\nbudget: x\",1,null,false"
        );
        assert_eq!(out.lines().count(), 4, "no fake lines");
    }

    #[test]
    fn json_matches_spec_shape() {
        assert_eq!(
            budget_json(&spec_example()),
            "{\"tokensUsed\":7412,\"bytesUsed\":31208,\"maxTokens\":8000,\"maxBytes\":2097152,\"truncated\":true,\
             \"sections\":[{\"name\":\"brief.symbols\",\"returned\":12,\"total\":40,\"truncated\":true},\
             {\"name\":\"brief.refs\",\"returned\":5,\"total\":5,\"truncated\":false}]}"
        );
        assert_eq!(budget_json(&BudgetSummary::new()), "{\"truncated\":false,\"sections\":[]}");
        let b = BudgetSummary::new().section(SectionCost::new("x", 1));
        assert_eq!(
            budget_json(&b),
            "{\"truncated\":false,\"sections\":[{\"name\":\"x\",\"returned\":1,\"truncated\":false}]}"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn hand_built_json_parses_and_matches_serde() {
        let hostile = BudgetSummary::new()
            .max_tokens(1)
            .section(SectionCost::new("q\"uote\\\n\u{1}", 2).total(3))
            .section(SectionCost::new("plain", 0));
        for b in [spec_example(), hostile, BudgetSummary::new()] {
            let hand: serde_json::Value = serde_json::from_str(&budget_json(&b)).expect("valid json");
            let derived = serde_json::to_value(&b).expect("serializes");
            assert_eq!(hand, derived);
        }
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn schema_uses_camel_case() {
        let schema = serde_json::to_string(&schemars::schema_for!(BudgetSummary)).expect("schema");
        for key in ["maxTokens", "tokensUsed", "bytesUsed", "maxItems", "returned"] {
            assert!(schema.contains(key), "{key} missing from {schema}");
        }
        assert!(!schema.contains("tokens_used"));
    }
}

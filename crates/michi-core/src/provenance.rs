//! Provenance slot: a short ordered list of labelled values plus the one string a freshness check compares. michi
//! assigns no meaning to any of it and validates none of it. See `docs/spec/07-wisp-and-monokl-envelope.md` §3.

/// What a provenance block renders: a short ordered list of labelled values, plus the one string a freshness check
/// compares.
///
/// `#[non_exhaustive]`: start from [`ProvenanceBlock::new`] or `Default` and use the builder methods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename_all = "camelCase"))]
#[non_exhaustive]
pub struct ProvenanceBlock {
    /// Rendered in order, one `label: value` line each.
    pub entries: Vec<ProvenanceEntry>,
    /// Rendered last when present.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub fingerprint: Option<String>,
}

impl ProvenanceBlock {
    /// An empty block: no entries, no fingerprint.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a labelled value. Entries render in the order added.
    #[must_use]
    pub fn entry(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.entries.push(ProvenanceEntry::new(label, value));
        self
    }

    /// Set the fingerprint a freshness check compares.
    #[must_use]
    pub fn fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.fingerprint = Some(fingerprint.into());
        self
    }
}

/// One labelled value in a [`ProvenanceBlock`]. A named pair rather than a tuple so a JSON reader has keys to read.
///
/// `#[non_exhaustive]`: build it with [`ProvenanceEntry::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ProvenanceEntry {
    /// Caller-chosen label. Never case-converted.
    pub label: String,
    /// The value for that label.
    pub value: String,
}

impl ProvenanceEntry {
    /// A labelled value.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into() }
    }
}

/// Render the `provenance:` block with no trailing newline: entries in order, `fingerprint` last when present.
/// Labels are encoded as TOON keys and values as TOON strings, so caller text cannot start a line of its own.
pub(crate) fn render_provenance_block(p: &ProvenanceBlock) -> String {
    let mut out = String::with_capacity(32 + p.entries.len() * 40 + p.fingerprint.as_ref().map_or(0, |f| f.len() + 20));
    out.push_str("provenance:");
    for e in &p.entries {
        out.push_str("\n  ");
        out.push_str(&crate::toon::escape_key(&e.label));
        out.push_str(": ");
        out.push_str(&crate::toon::escape_value(&e.value));
    }
    if let Some(f) = &p.fingerprint {
        out.push_str("\n  fingerprint: ");
        out.push_str(&crate::toon::escape_value(f));
    }
    out
}

/// The provenance as a JSON object: `entries` always present, `fingerprint` only when set.
pub(crate) fn provenance_json(p: &ProvenanceBlock) -> String {
    let mut out = String::with_capacity(32 + p.entries.len() * 48);
    out.push_str("{\"entries\":[");
    for (i, e) in p.entries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        crate::kv::json_escape_str(&mut out, &e.label);
        out.push_str(",\"value\":");
        crate::kv::json_escape_str(&mut out, &e.value);
        out.push('}');
    }
    out.push(']');
    if let Some(f) = &p.fingerprint {
        out.push_str(",\"fingerprint\":");
        crate::kv::json_escape_str(&mut out, f);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_example() -> ProvenanceBlock {
        ProvenanceBlock::new()
            .entry("analyzer", "rust-analyzer@0.3.2")
            .entry("observedAt", "1756950000000")
            .entry("precision", "exact")
            .fingerprint("b3f1c0d2e4a58691")
    }

    #[test]
    fn spec_example_renders_exactly() {
        // `observedAt` is a numeric-looking string, so TOON quotes it to keep it a string.
        assert_eq!(
            render_provenance_block(&spec_example()),
            "provenance:\n  analyzer: rust-analyzer@0.3.2\n  observedAt: \"1756950000000\"\n  precision: exact\n  \
             fingerprint: b3f1c0d2e4a58691"
        );
    }

    #[test]
    fn no_fingerprint_and_empty_cases() {
        let p = ProvenanceBlock::new().entry("a", "b");
        assert_eq!(render_provenance_block(&p), "provenance:\n  a: b");
        assert_eq!(render_provenance_block(&ProvenanceBlock::new()), "provenance:");
        let only_fp = ProvenanceBlock::new().fingerprint("f");
        assert_eq!(render_provenance_block(&only_fp), "provenance:\n  fingerprint: f");
    }

    #[test]
    fn hostile_labels_and_values_cannot_create_lines() {
        let p = ProvenanceBlock::new()
            .entry("k: v\nbudget", "x\nbudget:\n  truncated: true")
            .entry("a b", "c: d")
            .fingerprint("f\nhelp[1]: x");
        let out = render_provenance_block(&p);
        assert_eq!(out.lines().count(), 4, "{out}");
        assert_eq!(
            out,
            "provenance:\n  \"k: v\\nbudget\": \"x\\nbudget:\\n  truncated: true\"\n  \"a b\": \"c: d\"\n  \
             fingerprint: \"f\\nhelp[1]: x\""
        );
    }

    #[test]
    fn json_matches_spec_shape() {
        let p = ProvenanceBlock::new().entry("analyzer", "rust-analyzer@0.3.2").fingerprint("b3f1c0d2e4a58691");
        assert_eq!(
            provenance_json(&p),
            "{\"entries\":[{\"label\":\"analyzer\",\"value\":\"rust-analyzer@0.3.2\"}],\"fingerprint\":\"b3f1c0d2e4a58691\"}"
        );
        assert_eq!(provenance_json(&ProvenanceBlock::new()), "{\"entries\":[]}");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn hand_built_json_parses_and_matches_serde() {
        let hostile = ProvenanceBlock::new().entry("q\"\\\n", "\u{2}\t").fingerprint("fp\"");
        for p in [spec_example(), hostile, ProvenanceBlock::new()] {
            let hand: serde_json::Value = serde_json::from_str(&provenance_json(&p)).expect("valid json");
            assert_eq!(hand, serde_json::to_value(&p).expect("serializes"));
        }
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn schema_has_entry_keys() {
        let schema = serde_json::to_string(&schemars::schema_for!(ProvenanceBlock)).expect("schema");
        for key in ["entries", "fingerprint", "label", "value"] {
            assert!(schema.contains(key), "{key} missing");
        }
    }
}

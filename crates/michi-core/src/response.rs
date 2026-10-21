use crate::hints::Hint;
use crate::kv::KvItem;
use crate::recovery::RecoveryHint;
use crate::toon::Value;

/// The serialisation format for `AgentResponse::render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum OutputFormat {
    /// Plain-text TOON / kv format. Default.
    #[default]
    Text,
    /// Compact JSON object.
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RenderTarget {
    Unset,
    Toon,
    Kv,
}

/// Default per-cell truncation limit used by [`AgentResponse::new`].
pub const DEFAULT_TRUNCATE_CELLS: usize = 200;

/// Builder for an agent-facing response.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentResponse {
    type_name: String,
    target: RenderTarget,
    items: Vec<Vec<Value>>,
    fields: Vec<String>,
    single_item: Vec<KvItem>,
    total_count: Option<usize>,
    hints: Vec<Hint>,
    recovery: Vec<RecoveryHint>,
    truncate_cells_at: usize,
    is_error: bool,
    human_content: Option<String>,
}

impl AgentResponse {
    /// Create a new, empty response for the given type name.
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            target: RenderTarget::Unset,
            items: Vec::new(),
            fields: Vec::new(),
            single_item: Vec::new(),
            total_count: None,
            hints: Vec::new(),
            recovery: Vec::new(),
            truncate_cells_at: DEFAULT_TRUNCATE_CELLS,
            is_error: false,
            human_content: None,
        }
    }

    /// Populate the TOON list path.
    #[must_use]
    pub fn items(mut self, rows: Vec<Vec<Value>>, fields: &[&str]) -> Self {
        self.items = rows;
        self.fields = fields.iter().map(|s| (*s).to_string()).collect();
        self.target = RenderTarget::Toon;
        self
    }

    /// Set the total available count.
    #[must_use]
    pub fn total_count(mut self, n: usize) -> Self {
        self.total_count = Some(n);
        self
    }

    /// Populate the KV single-item path.
    #[must_use]
    pub fn kv_items(mut self, items: Vec<KvItem>) -> Self {
        self.single_item = items;
        self.target = RenderTarget::Kv;
        self
    }

    /// Append a contextual hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint::new(hint));
        self
    }

    /// Replace all contextual hints.
    #[must_use]
    pub fn hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Append a recovery hint.
    #[must_use]
    pub fn recovery_hint(mut self, r: RecoveryHint) -> Self {
        self.recovery.push(r);
        self
    }

    /// Set the max cell length before inline truncation on the TOON path.
    #[must_use]
    pub fn truncate_cells_at(mut self, limit: usize) -> Self {
        self.truncate_cells_at = limit;
        self
    }

    /// Mark this response as an error state.
    #[must_use]
    pub fn as_error(mut self) -> Self {
        self.is_error = true;
        self
    }

    /// Attach a human-facing companion block (`audience: user`).
    #[must_use]
    pub fn human_content(mut self, text: impl Into<String>) -> Self {
        self.human_content = Some(text.into());
        self
    }

    /// The TOON body, and whether it is a validation-failure error block instead of the requested rows.
    fn toon_body(&self) -> (String, bool) {
        let opts = crate::toon::ToonOptions::new(self.type_name.clone(), self.fields.clone(), self.items.clone())
            .total_count(self.total_count)
            .max_cell_len(self.truncate_cells_at);
        match opts.validate() {
            Ok(doc) => (doc.render(), false),
            Err(e) => (format!("error: toon_validation_failed\nmessage: {e}\n"), true),
        }
    }

    fn kv_body(&self) -> String {
        crate::kv::render_kv(&self.single_item, self.total_count, &[])
    }

    /// The selected body, and whether this response is an error: set by `as_error()` or by a body that failed
    /// validation. Every output surface reads its error flag from here so text, JSON and MCP never disagree.
    fn body(&self) -> (String, bool) {
        match self.target {
            RenderTarget::Toon | RenderTarget::Unset => {
                let (body, failed) = self.toon_body();
                (body, failed || self.is_error)
            }
            RenderTarget::Kv => (self.kv_body(), self.is_error),
        }
    }

    fn finish_text(&self, body: &str) -> String {
        let mut out = String::with_capacity(body.len() + self.hints.len() * 60 + self.recovery.len() * 80);
        out.push_str(body);
        crate::hints::append_hints(&mut out, &self.hints);
        crate::recovery::append_recovery(&mut out, &self.recovery);
        out
    }

    /// Render the response in the requested format.
    #[must_use]
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Text => self.render_text(),
            OutputFormat::Json => self.render_json(),
        }
    }

    /// Render via the TOON path.
    #[must_use]
    pub fn render_toon(&self) -> String {
        self.finish_text(&self.toon_body().0)
    }

    /// Render via the KV path.
    #[must_use]
    pub fn render_kv(&self) -> String {
        self.finish_text(&self.kv_body())
    }

    fn render_text(&self) -> String {
        self.finish_text(&self.body().0)
    }

    /// Whether this response is an error: `as_error()` was called, or the TOON rows failed validation and the body
    /// is a `toon_validation_failed` error block.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.body().1
    }

    /// Render just the `help[N]:` block for `self.hints`.
    #[must_use]
    pub fn render_hints_only(&self) -> String {
        crate::hints::render_hints(&self.hints)
    }

    /// Render for the given audience.
    #[must_use]
    pub fn render_for(&self, audience: crate::audience::Audience) -> String {
        match audience {
            crate::audience::Audience::Assistant => self.render_text(),
            crate::audience::Audience::User => self.human_content.clone().unwrap_or_else(|| self.render_text()),
        }
    }

    /// Whether `.human_content()` was set on this builder.
    #[must_use]
    pub fn has_human_content(&self) -> bool {
        self.human_content.is_some()
    }

    /// Build the MCP `CallToolResult` for this response.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        let (body, is_error) = self.body();
        let mut content = vec![crate::mcp::ContentBlock {
            text: self.finish_text(&body),
            audience: vec![crate::audience::Audience::Assistant],
        }];
        if let Some(human) = &self.human_content {
            content.push(crate::mcp::ContentBlock {
                text: human.clone(),
                audience: vec![crate::audience::Audience::User],
            });
        }
        crate::mcp::CallToolResult { content, is_error, structured_content: self.json_from_body(&body, is_error) }
    }

    fn render_json(&self) -> String {
        let (body, is_error) = self.body();
        self.json_from_body(&body, is_error)
    }

    fn json_from_body(&self, body: &str, is_error: bool) -> String {
        let capacity = body.len()
            + self.hints.iter().map(|h| h.as_str().len() + 16).sum::<usize>()
            + self.recovery.len() * 64
            + 64;
        let mut out = String::with_capacity(capacity);
        out.push_str("{\"body\":");
        json_string(&mut out, body);
        out.push_str(",\"hints\":[");
        for (i, h) in self.hints.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_string(&mut out, h.as_str());
        }
        out.push_str("],\"recovery\":[");
        for (i, r) in self.recovery.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"tool\":");
            json_string(&mut out, &r.tool);
            out.push_str(",\"params\":{");
            for (j, (k, v)) in r.params.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                json_string(&mut out, k);
                out.push(':');
                crate::kv::kv_value_to_json(&mut out, v);
            }
            out.push('}');
            if let Some(reason) = &r.reason {
                out.push_str(",\"reason\":");
                json_string(&mut out, reason);
            }
            out.push('}');
        }
        out.push_str("],\"isError\":");
        out.push_str(if is_error { "true" } else { "false" });
        out.push('}');
        out
    }
}

fn json_string(out: &mut String, s: &str) {
    crate::kv::json_escape_str(out, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_path_renders_toon() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"]);
        let out = r.render(OutputFormat::Text);
        assert!(out.starts_with("issues[1]{id,state}:\n  1,open\n"), "got: {out}");
    }

    // AC-001: Unset target (no .items()/.kv_items() call) routes to TOON
    // rendering with zero rows/fields, not KV or an empty string.
    #[test]
    fn ac001_unset_target_renders_empty_toon_exactly() {
        let r = AgentResponse::new("widget");
        assert_eq!(r.render(OutputFormat::Text), "widget[0]{}:\n");
    }

    // AC-002: the test above (items_path_renders_toon) only checks
    // starts_with, which would still pass if extra bytes followed the row.
    // Pin the exact full output for a builder with no hints/recovery set.
    #[test]
    fn ac002_items_path_renders_exact_toon_literal() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"]);
        assert_eq!(r.render(OutputFormat::Text), "issues[1]{id,state}:\n  1,open\n");
    }

    // AC-003: render(Text) via kv_items() matches the direct render_kv() call
    // for the same items, not TOON list structure.
    #[test]
    fn ac003_kv_items_path_matches_direct_render_kv_call() {
        use crate::kv::{KvItem, KvValue};
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let r = AgentResponse::new("t").kv_items(items.clone());
        assert_eq!(r.render(OutputFormat::Text), crate::kv::render_kv(&items, None, &[]));
    }

    #[test]
    fn render_json_basic_structure() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("hello world")]], &["name"]);
        let json = r.render_json();
        assert!(json.contains("\"isError\":false"), "got: {json}");
        assert!(json.contains("\"body\""), "got: {json}");
        assert!(json.contains("\"hints\":[]"), "got: {json}");
    }

    #[test]
    fn render_json_is_error_flag() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("fail")]], &["name"]).as_error();
        let json = r.render_json();
        assert!(json.contains("\"isError\":true"), "got: {json}");
    }

    // AC-004: render(Json) parses as an object with exactly the 4 named
    // top-level keys, for all three construction paths (items/kv_items/neither).
    #[test]
    #[cfg(feature = "serde")]
    fn ac004_render_json_top_level_keys_are_exactly_the_specified_set_for_every_target() {
        let unset = AgentResponse::new("t");
        let toon = AgentResponse::new("t").items(vec![vec![Value::from("x")]], &["a"]);
        let kv = AgentResponse::new("t")
            .kv_items(vec![crate::kv::KvItem { key: "k".into(), value: crate::kv::KvValue::Int(1) }]);
        for r in [unset, toon, kv] {
            let parsed: serde_json::Value = serde_json::from_str(&r.render(OutputFormat::Json)).expect("valid JSON");
            let obj = parsed.as_object().expect("top-level object");
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, vec!["body", "hints", "isError", "recovery"]);
        }
    }

    // AC-006: recovery array entries carry tool/params always, and reason
    // only when Some — key absent (not null) when None.
    #[test]
    #[cfg(feature = "serde")]
    fn ac006_recovery_json_entry_shape_and_reason_presence() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let with_reason = RecoveryHint::new("retry").param("n", KvValue::Int(1)).reason("why");
        let without_reason = RecoveryHint::new("retry2");
        let r = AgentResponse::new("t").recovery_hint(with_reason).recovery_hint(without_reason);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        let recovery = parsed["recovery"].as_array().expect("recovery array");
        assert_eq!(recovery.len(), 2);
        assert_eq!(recovery[0]["tool"], serde_json::json!("retry"));
        assert_eq!(recovery[0]["params"], serde_json::json!({"n": 1}));
        assert_eq!(recovery[0]["reason"], serde_json::json!("why"));
        assert_eq!(recovery[1]["tool"], serde_json::json!("retry2"));
        assert!(recovery[1].as_object().unwrap().get("reason").is_none(), "got: {recovery:?}");
    }

    #[test]
    fn render_json_recovery_int_param_is_numeric_not_string() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let recovery = RecoveryHint::new("retry").param("limit", KvValue::Int(10));
        let r =
            AgentResponse::new("items").items(vec![vec![Value::from("hit limit")]], &["name"]).recovery_hint(recovery);
        let json = r.render_json();
        assert!(json.contains("\"limit\":10"), "Int param must be unquoted, got: {json}");
        assert!(!json.contains("\"limit\":\"10\""), "Int param must not be quoted string, got: {json}");
    }

    #[test]
    fn render_json_hint_with_quotes_is_escaped() {
        let r = AgentResponse::new("items").hint(r#"Use "get_item" instead"#);
        let json = r.render_json();
        assert!(json.contains("\\\"get_item\\\""), "quotes in hints must be escaped, got: {json}");
    }

    // AC-038: the test above only exercises the quote-escaping half; the
    // embedded-newline half and the body-value escaping half were untested.
    #[test]
    #[cfg(feature = "serde")]
    fn ac038_hint_and_body_with_quote_backslash_newline_produce_valid_json() {
        let r = AgentResponse::new("items").hint("has \"quotes\" and \\backslash\nand newline").kv_items(vec![
            crate::kv::KvItem { key: "k".into(), value: crate::kv::KvValue::Text("body \"quoted\" \\slash".into()) },
        ]);
        let raw = r.render_json();
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"][0].as_str().unwrap(), "has \"quotes\" and \\backslash\nand newline");
        assert_eq!(parsed["body"].as_str().unwrap(), "k: body \"quoted\" \\slash\n");
    }

    // AC-009: render_toon()/render_kv() always run their own named path
    // regardless of which builder method last set the target.
    #[test]
    fn ac009_render_toon_and_render_kv_diverge_when_only_items_was_set() {
        let r = AgentResponse::new("t").items(vec![vec![Value::from("x")]], &["a"]);
        assert_eq!(r.render_toon(), "t[1]{a}:\n  x\n");
        assert_eq!(r.render_kv(), "");
    }

    // AC-010: render(Text) target-based dispatch agrees with render_kv()
    // when kv_items() was the last builder call used.
    #[test]
    fn ac010_render_text_agrees_with_render_kv_when_kv_items_set() {
        use crate::kv::{KvItem, KvValue};
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let r = AgentResponse::new("t").kv_items(items);
        assert_eq!(r.render(OutputFormat::Text), r.render_kv());
    }

    // AC-012: Missing rendered via the AgentResponse Json body is the
    // em-dash embedded in the body string, not a nested JSON null.
    #[test]
    #[cfg(feature = "serde")]
    fn ac012_missing_in_kv_items_json_body_is_embedded_em_dash() {
        use crate::kv::{KvItem, KvValue};
        let r = AgentResponse::new("t").kv_items(vec![KvItem { key: "k".into(), value: KvValue::Missing }]);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        assert_eq!(parsed["body"].as_str().unwrap(), "k: —\n");
    }

    // AC-014: NaN/Inf/-Inf via the AgentResponse Json body are the literal
    // tokens embedded in body text, not standalone JSON string values.
    #[test]
    #[cfg(feature = "serde")]
    fn ac014_float_special_values_in_kv_items_json_body_are_embedded_tokens() {
        use crate::kv::{KvItem, KvValue};
        for (value, token) in [
            (KvValue::Float(f64::NAN, 2), "NaN"),
            (KvValue::Float(f64::INFINITY, 0), "inf"),
            (KvValue::Float(f64::NEG_INFINITY, 0), "-inf"),
        ] {
            let r = AgentResponse::new("t").kv_items(vec![KvItem { key: "k".into(), value }]);
            let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
            assert_eq!(parsed["body"].as_str().unwrap(), format!("k: {token}\n"), "token: {token}");
        }
    }

    // AC-016: Duration via the AgentResponse Json body is the literal '1.5s'
    // token embedded in body text.
    #[test]
    #[cfg(feature = "serde")]
    fn ac016_duration_in_kv_items_json_body_is_embedded_token() {
        use crate::kv::{KvItem, KvValue};
        let r = AgentResponse::new("t").kv_items(vec![KvItem {
            key: "k".into(),
            value: KvValue::Duration(std::time::Duration::from_millis(1500)),
        }]);
        let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
        assert_eq!(parsed["body"].as_str().unwrap(), "k: 1.5s\n");
    }

    // AC-041 through AC-045: RecoveryHint params through AgentResponse's Json
    // path use kv_value_to_json (bare null/quoted-token/bare bool), in
    // contrast to kv_items' plain-text/embedded-token body rendering above.
    #[test]
    #[cfg(feature = "serde")]
    fn ac041_to_ac045_recovery_params_use_kv_value_to_json_typing() {
        use crate::kv::KvValue;
        use crate::recovery::RecoveryHint;
        let cases: Vec<(KvValue, serde_json::Value)> = vec![
            (KvValue::Missing, serde_json::Value::Null),
            (KvValue::Float(f64::NAN, 2), serde_json::json!("NaN")),
            (KvValue::Float(f64::INFINITY, 0), serde_json::json!("inf")),
            (KvValue::Float(f64::NEG_INFINITY, 0), serde_json::json!("-inf")),
            (KvValue::Duration(std::time::Duration::from_millis(1500)), serde_json::json!("1.5s")),
            (KvValue::Bool(true), serde_json::json!(true)),
        ];
        for (value, expected) in cases {
            let recovery = RecoveryHint::new("x").param("p", value.clone());
            let r = AgentResponse::new("t").recovery_hint(recovery);
            let parsed: serde_json::Value = serde_json::from_str(&r.render_json()).expect("valid JSON");
            assert_eq!(parsed["recovery"][0]["params"]["p"], expected, "value: {value:?}");
        }
    }

    #[test]
    fn to_call_tool_result_composes_correctly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]).as_error();
        let result = r.to_call_tool_result();
        assert!(result.is_error);
        assert!(!result.content.is_empty(), "content must not be empty");
        assert!(result.structured_content.starts_with('{'), "structured_content must be JSON object");
    }

    // AC-023: the test above only checks !content.is_empty(), which would
    // still pass with 2+ blocks or a wrong audience/text. Pin the exact shape.
    #[test]
    fn ac023_call_tool_result_has_exactly_one_assistant_block_matching_render_text() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].audience, vec![crate::audience::Audience::Assistant]);
        assert_eq!(result.content[0].text, r.render(OutputFormat::Text));
    }

    // AC-024: human_content set adds a second, User-tagged block whose text
    // equals the human_content value exactly.
    #[test]
    fn ac024_call_tool_result_adds_user_block_when_human_content_set() {
        let r = AgentResponse::new("items").human_content("For humans");
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].audience, vec![crate::audience::Audience::User]);
        assert_eq!(result.content[1].text, "For humans");
    }

    // AC-025: without human_content, to_call_tool_result() does NOT fall
    // back to a text-output User block (unlike render_for, which does).
    #[test]
    fn ac025_call_tool_result_has_no_user_block_without_human_content() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert!(!result.content.iter().any(|c| c.audience == vec![crate::audience::Audience::User]));
    }

    // AC-026: structured_content equals render(Json) exactly, and is_error
    // mirrors self.is_error in both directions.
    #[test]
    fn ac026_call_tool_result_structured_content_and_is_error_match_exactly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let result = r.to_call_tool_result();
        assert_eq!(result.structured_content, r.render(OutputFormat::Json));
        assert!(!result.is_error);

        let err = r.as_error();
        let err_result = err.to_call_tool_result();
        assert_eq!(err_result.structured_content, err.render(OutputFormat::Json));
        assert!(err_result.is_error);
    }

    #[test]
    fn toon_body_returns_error_on_invalid_type_name() {
        let r = AgentResponse::new("items[bad]").items(vec![vec![Value::from("x")]], &["name"]);
        let out = r.render(OutputFormat::Text);
        assert!(out.contains("error:"), "expected error output for invalid type_name, got: {out}");
    }

    // AC-031: the test above only checks the generic substring "error:",
    // which many unrelated messages could also contain. Pin the exact
    // required prefix.
    #[test]
    fn ac031_invalid_type_name_produces_the_exact_error_prefix() {
        let r = AgentResponse::new("items[bad]");
        let out = r.render(OutputFormat::Text);
        assert!(out.starts_with("error: toon_validation_failed\nmessage: "), "got: {out}");
    }

    // AC-039: a row whose length differs from fields.len() surfaces
    // RowLengthMismatch with row index, expected, and actual counts.
    #[test]
    fn ac039_row_length_mismatch_names_indices_and_counts() {
        let r = AgentResponse::new("t").items(vec![vec![Value::Int(1)]], &["a", "b"]);
        let out = r.render(OutputFormat::Text);
        assert_eq!(out, "error: toon_validation_failed\nmessage: row 0 has 1 values but 2 fields declared\n");
    }

    // AC-040: a field name with a structural character (comma) surfaces
    // InvalidFieldName naming the offending field.
    #[test]
    fn ac040_invalid_field_name_names_the_offending_field() {
        let r = AgentResponse::new("t").items(vec![], &["a,b"]);
        let out = r.render(OutputFormat::Text);
        assert_eq!(out, "error: toon_validation_failed\nmessage: field \"a,b\" contains a structural character\n");
    }

    #[test]
    fn render_hints_only_contains_help_block() {
        let r = AgentResponse::new("items").hint("Try again with fewer params");
        let out = r.render_hints_only();
        assert!(out.contains("help[1]:"), "expected help block, got: {out}");
        assert!(out.contains("Try again with fewer params"), "got: {out}");
    }

    #[test]
    fn render_hints_only_empty_when_no_hints() {
        let r = AgentResponse::new("items");
        assert!(r.render_hints_only().is_empty(), "no hints should produce empty string");
    }

    #[test]
    fn render_for_assistant_returns_text_output() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("ok")]], &["name"]);
        let out = r.render_for(crate::audience::Audience::Assistant);
        assert!(!out.is_empty(), "assistant render must not be empty");
        assert!(out.contains("items"), "got: {out}");
    }

    #[test]
    fn render_for_user_returns_human_content_when_set() {
        let r = AgentResponse::new("items").human_content("Human-readable summary.");
        let out = r.render_for(crate::audience::Audience::User);
        assert_eq!(out, "Human-readable summary.");
    }

    #[test]
    fn render_for_user_falls_back_to_text_when_no_human_content() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let assistant_out = r.render_for(crate::audience::Audience::Assistant);
        let user_out = r.render_for(crate::audience::Audience::User);
        assert_eq!(assistant_out, user_out, "user render must match assistant render when no human_content set");
    }

    // AC-021: the test above only proves assistant_out == user_out; the
    // criterion also requires both equal render(Text) — untested until now.
    #[test]
    fn ac021_render_for_fallback_matches_render_text_directly() {
        let r = AgentResponse::new("items").items(vec![vec![Value::from("x")]], &["name"]);
        let text_out = r.render(OutputFormat::Text);
        assert_eq!(r.render_for(crate::audience::Audience::Assistant), text_out);
        assert_eq!(r.render_for(crate::audience::Audience::User), text_out);
    }

    // AC-029: a truncated cell's total char count (including any marker)
    // never exceeds the configured limit.
    #[test]
    fn ac029_truncate_cells_at_bounds_total_cell_length() {
        let r = AgentResponse::new("t").truncate_cells_at(5).items(vec![vec![Value::from("abcdefghij")]], &["name"]);
        let out = r.render_toon();
        let row_line = out.lines().nth(1).expect("row line present");
        // Strip exactly the 2-space TOON row indent (not trim_start, which
        // would also eat the truncation marker's own leading space and
        // silently undercount when keep_chars == 0).
        let cell = row_line.strip_prefix("  ").expect("row indent present");
        assert!(cell.chars().count() <= 5, "cell {} chars, got: {row_line:?}", cell.chars().count());
    }

    // AC-030: truncate_cells_at has no observable effect on the KV path.
    #[test]
    fn ac030_truncate_cells_at_does_not_affect_kv_path() {
        use crate::kv::{KvItem, KvValue};
        let long_value = "x".repeat(50);
        let r = AgentResponse::new("t")
            .truncate_cells_at(5)
            .kv_items(vec![KvItem { key: "k".into(), value: KvValue::Text(long_value.clone()) }]);
        let out = r.render_kv();
        assert!(out.contains(&long_value), "value must be untruncated, got: {out}");
    }

    // AC-051: .total_count() is threaded into the KV render path, not
    // silently dropped.
    #[test]
    fn ac051_total_count_is_wired_into_kv_render_path() {
        use crate::kv::{KvItem, KvValue};
        let r =
            AgentResponse::new("t").total_count(3).kv_items(vec![KvItem { key: "k".into(), value: KvValue::Int(1) }]);
        let out = r.render_kv();
        assert!(out.contains("totalCount: 3\n"), "got: {out}");
    }

    // AC-052: .total_count() is threaded into the TOON render path via
    // ToonOptions::total_count(), not silently dropped.
    #[test]
    fn ac052_total_count_is_wired_into_toon_render_path() {
        let r = AgentResponse::new("t").total_count(3).items(vec![vec![Value::from("x")]], &["c"]);
        let out = r.render_toon();
        assert!(out.contains('3'), "expected total_count 3 to surface, got: {out}");

        let without_total_count = AgentResponse::new("t").items(vec![vec![Value::from("x")]], &["c"]);
        assert_ne!(out, without_total_count.render_toon(), "total_count must change the output");
    }

    // AC-053: .hints() replaces the full hint list rather than appending to
    // hints previously added via .hint() — contrast with .recovery_hint()'s
    // append semantics (AC-006).
    #[test]
    fn ac053_hints_replaces_rather_than_appends() {
        let r = AgentResponse::new("t").hint("a").hints(vec![Hint::from("b")]);
        let out = r.render_hints_only();
        assert!(out.contains('b'), "got: {out}");
        assert!(!out.contains('a'), "got: {out}");
    }

    // AC-038 (extended): json_escape_str's generic below-0x20 control-
    // character branch (\u00XX), distinct from the dedicated \n/\r/\t arms.
    #[test]
    #[cfg(feature = "serde")]
    fn ac038_control_character_below_0x20_is_escaped_and_json_stays_valid() {
        let r = AgentResponse::new("t").hint("before\u{1}after");
        let raw = r.render_json();
        assert!(raw.contains(r"\u0001"), "got: {raw}");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("must be valid JSON");
        assert_eq!(parsed["hints"][0].as_str().unwrap(), "before\u{1}after");
    }

    // AC-049: DEFAULT_TRUNCATE_CELLS is a public constant equal to 200.
    #[test]
    fn ac049_default_truncate_cells_is_200() {
        assert_eq!(DEFAULT_TRUNCATE_CELLS, 200);
    }

    // AC-050: without an explicit .truncate_cells_at() call, the default
    // (200) boundary holds: 199 chars untruncated, 250 chars truncated.
    #[test]
    fn ac050_default_truncate_boundary_holds_without_explicit_call() {
        let untruncated = AgentResponse::new("t").items(vec![vec![Value::from("x".repeat(199))]], &["c"]);
        let out = untruncated.render_toon();
        let cell_line = out.lines().nth(1).expect("row line present");
        assert!(cell_line.contains(&"x".repeat(199)), "199 chars must be untruncated, got: {cell_line:?}");

        let truncated = AgentResponse::new("t").items(vec![vec![Value::from("x".repeat(250))]], &["c"]);
        let out = truncated.render_toon();
        let cell_line = out.lines().nth(1).expect("row line present");
        let value_part = cell_line.trim_start();
        assert!(
            value_part.chars().count() <= DEFAULT_TRUNCATE_CELLS,
            "got line with {} chars: {cell_line:?}",
            value_part.chars().count()
        );
    }

    #[test]
    fn has_human_content_false_when_empty() {
        let r = AgentResponse::new("items");
        assert!(!r.has_human_content());
    }

    #[test]
    fn has_human_content_true_when_set() {
        let r = AgentResponse::new("items").human_content("Some text for humans.");
        assert!(r.has_human_content());
    }
}

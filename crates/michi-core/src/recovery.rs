use crate::kv::KvValue;

/// A structured recovery hint for an agent encountering an error.
///
/// With the `serde` feature it serializes to the same JSON as the structured companions of `AgentResponse` and
/// `DomainError`: `{"tool": ..., "params": {key: value, ...}, "reason": ...}`, with `reason` omitted when absent and
/// each parameter as a plain JSON value. Deserializing maps strings to `Text`, integers that fit `i64` to `Int`, every
/// other number to `Float` with enough decimal places to reproduce its f64 reading exactly (the value `JSON.parse`
/// and serde_json give it; [`KvValue::FULL_PRECISION`] when that needs more than 254 places), booleans to `Bool` and
/// `null` to `Missing`. Arrays and objects are rejected. The JSON Schema admits exactly these values.
///
/// `#[non_exhaustive]`: build it with [`RecoveryHint::new`] and its builder methods; the fields stay public.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "RecoveryHintWire", try_from = "RecoveryHintWire"))]
#[non_exhaustive]
pub struct RecoveryHint {
    /// The tool/operation name the agent should call to recover.
    pub tool: String,
    /// Ordered key-value parameters to call `tool` with.
    ///
    /// Keys should be unique; [`RecoveryHint::param`] keeps them so. If this field is filled directly with a repeated
    /// key, every renderer emits that key once, at its first position, with its last value.
    pub params: Vec<(String, KvValue)>,
    /// Optional human-readable reason this recovery path applies.
    pub reason: Option<String>,
}

impl RecoveryHint {
    /// Create a recovery hint naming just the tool to call.
    ///
    /// Recovery hints are instructions to the agent: the tool, parameters and reason come from the tool author, never
    /// from data. See the trust model in `docs/spec/02-toon-format.md`.
    #[must_use]
    pub fn new(tool: impl Into<String>) -> Self {
        Self { tool: tool.into(), params: Vec::new(), reason: None }
    }

    /// Add a suggested parameter, or replace the value of an existing parameter with the same key in place (it keeps
    /// the position of its first insertion). Parameters render as a JSON object, which cannot repeat a key. Keys and
    /// values are tool-author text, never data.
    #[must_use]
    pub fn param(mut self, key: impl Into<String>, value: KvValue) -> Self {
        let key = key.into();
        match self.params.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.params.push((key, value)),
        }
        self
    }

    /// Attach a human-readable reason. Tool-author text, never data.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "RecoveryHint"))]
struct RecoveryHintWire {
    tool: String,
    #[cfg_attr(feature = "schemars", schemars(schema_with = "param_values_schema"))]
    params: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[cfg(feature = "serde")]
impl From<RecoveryHint> for RecoveryHintWire {
    fn from(h: RecoveryHint) -> Self {
        let params = h
            .params
            .iter()
            .map(|(k, v)| {
                // Same writer as the hand-built companions, so the two shapes cannot drift.
                let mut json = String::new();
                crate::kv::kv_value_to_json(&mut json, v);
                (k.clone(), serde_json::from_str(&json).unwrap_or(serde_json::Value::String(json)))
            })
            .collect();
        Self { tool: h.tool, params, reason: h.reason }
    }
}

/// Schema for parameter values: the scalar JSON types `json_to_kv_value` accepts. Every JSON number is accepted, so
/// the schema needs no numeric bounds.
#[cfg(feature = "schemars")]
fn param_values_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {
            "type": ["string", "number", "boolean", "null"],
            "description": "An integer past i64 must have an exact float form; one that doesn't is rejected rather than rounded. Send such values as strings."
        }
    })
}

/// `u` as an f64 when that loses nothing: the f64 is exactly `u`, or `u`'s digits are exactly the canonical form
/// michi writes for that f64 (`9223372036854776000` for 2^63), so they came from a float, not a longer integer.
#[cfg(feature = "serde")]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn exact_u64_as_f64(u: u64) -> Option<f64> {
    let f = u as f64;
    // 2^64 itself is the nearest f64 to values near u64::MAX, and does not convert back.
    if f < 18_446_744_073_709_551_616.0 && f as u64 == u {
        return Some(f);
    }
    let mut canonical = String::with_capacity(24);
    crate::toon::push_f64(&mut canonical, f);
    (canonical == u.to_string()).then_some(f)
}

#[cfg(feature = "serde")]
fn json_to_kv_value(key: &str, v: serde_json::Value) -> Result<KvValue, String> {
    match v {
        serde_json::Value::Null => Ok(KvValue::Missing),
        serde_json::Value::Bool(b) => Ok(KvValue::Bool(b)),
        serde_json::Value::String(s) => Ok(KvValue::Text(s)),
        serde_json::Value::Number(n) => {
            // Integers keep their exact digits: one past i64::MAX becomes a `Float` only when an f64 holds it exactly
            // (a `Float` michi wrote, such as `1e19`, prints as `10000000000000000000` and reads back), and is
            // rejected rather than rounded otherwise. An integer past u64::MAX or below i64::MIN reaches michi already
            // read as an f64 by serde_json, so its original digits cannot be checked.
            if let Some(i) = n.as_i64() {
                Ok(KvValue::Int(i))
            } else if let Some(u) = n.as_u64() {
                match exact_u64_as_f64(u) {
                    Some(f) => Ok(KvValue::Float(f, crate::kv::exact_decimals(f))),
                    None => Err(format!(
                        "parameter `{key}`: integer {u} is past i64 and has no exact float form; send it as a string"
                    )),
                }
            } else if let Some(f) = n.as_f64() {
                Ok(KvValue::Float(f, crate::kv::exact_decimals(f)))
            } else {
                Err(format!("parameter `{key}`: number {n} is not representable as f64"))
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(format!(
            "parameter `{key}`: arrays and objects are not supported; use a string, number, boolean or null"
        )),
    }
}

#[cfg(feature = "serde")]
impl TryFrom<RecoveryHintWire> for RecoveryHint {
    type Error = String;

    fn try_from(w: RecoveryHintWire) -> Result<Self, Self::Error> {
        let params = w
            .params
            .into_iter()
            .map(|(k, v)| json_to_kv_value(&k, v).map(|value| (k, value)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { tool: w.tool, params, reason: w.reason })
    }
}

// The schema describes the wire form above, not the Rust field layout.
#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl schemars::JsonSchema for RecoveryHint {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        RecoveryHintWire::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        RecoveryHintWire::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        RecoveryHintWire::json_schema(generator)
    }
}

/// Render recovery hints as the TOON 4.4 tabular array (§9.3) `recovery[N]{tool,params,reason}:`, one row per hint,
/// with no trailing newline.
///
/// Every hint has the same three primitive fields, so the tabular form is the one TOON prescribes (§1.4): `tool` is
/// the tool name; `params` is the string holding the parameters' compact JSON object text, exactly the `params` object
/// of the JSON companions (`{}` when there are none); `reason` is the reason or `null` when absent. Each cell is
/// quoted per §7.2, so the JSON text is always quoted with its `"` escaped as `\"`, and a line break in any field is an
/// escape, never a new line. Returns an empty string (the empty document) for an empty slice.
#[must_use]
pub fn render_recovery(hints: &[RecoveryHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let capacity = 40 + hints.len() * 60;
    let mut out = String::with_capacity(capacity);
    append_recovery(&mut out, hints);
    out
}

/// Append the `recovery[N]{tool,params,reason}:` block of [`render_recovery`] to a TOON document in place.
///
/// Like [`crate::hints::append_hints`], one LF is inserted first when `out` is non-empty and does not already end in
/// one, and nothing follows the block. Does nothing for an empty slice.
pub fn append_recovery(out: &mut String, hints: &[RecoveryHint]) {
    if hints.is_empty() {
        return;
    }
    crate::hints::start_block(out);
    out.push_str("recovery[");
    out.push_str(&hints.len().to_string());
    out.push_str("]{tool,params,reason}:");
    for hint in hints {
        out.push_str("\n  ");
        out.push_str(&crate::toon::escape_value(&hint.tool));
        out.push(',');
        out.push_str(&crate::toon::escape_value(&params_json(&hint.params)));
        out.push(',');
        match &hint.reason {
            Some(reason) => out.push_str(&crate::toon::escape_value(reason)),
            None => out.push_str("null"),
        }
    }
}

/// The parameters as a compact JSON object, written by the same writer as the JSON companions.
pub(crate) fn params_json(params: &[(String, KvValue)]) -> String {
    let mut json = String::with_capacity(2 + params.iter().map(|(k, _)| k.len() + 16).sum::<usize>());
    push_params_json(&mut json, params);
    json
}

/// Append the parameters as a compact JSON object. A repeated key is written once, at its first position, with its
/// last value -- what [`RecoveryHint::param`] would have produced -- so the object never holds a duplicate key.
pub(crate) fn push_params_json(out: &mut String, params: &[(String, KvValue)]) {
    out.push('{');
    // Each key's last value, then one pass in order writing a key the first time it is seen: O(P), not O(P²).
    // Internal lookup state only; the output order comes from `params`.
    #[allow(clippy::disallowed_types)]
    let mut last: std::collections::HashMap<&str, &KvValue> = std::collections::HashMap::with_capacity(params.len());
    for (k, v) in params {
        last.insert(k.as_str(), v);
    }
    let mut first = true;
    for (k, _) in params {
        let Some(v) = last.remove(k.as_str()) else { continue };
        if !first {
            out.push(',');
        }
        first = false;
        crate::kv::json_escape_str(out, k);
        out.push(':');
        crate::kv::kv_value_to_json(out, v);
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "recovery[1]{tool,params,reason}:\n  ";

    #[test]
    fn hint_with_params_renders_them_as_quoted_compact_json() {
        let hints = [RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".to_string()))];
        assert_eq!(render_recovery(&hints), format!("{HEADER}assign_user,\"{{\\\"user\\\":\\\"alice\\\"}}\",null"));
    }

    #[test]
    fn empty_hints_returns_empty_string() {
        assert_eq!(render_recovery(&[]), "");
    }

    #[test]
    fn hint_with_no_params_has_an_empty_params_object_and_null_reason() {
        let hints = [RecoveryHint::new("retry_upload")];
        assert_eq!(render_recovery(&hints), format!("{HEADER}retry_upload,\"{{}}\",null"));
    }

    #[test]
    fn hint_with_reason_but_no_params() {
        let hints = [RecoveryHint::new("reload_cache").reason("cache entry expired")];
        assert_eq!(render_recovery(&hints), format!("{HEADER}reload_cache,\"{{}}\",cache entry expired"));
    }

    #[test]
    fn multiple_hints_all_appear() {
        let hints = [RecoveryHint::new("step_a"), RecoveryHint::new("step_b")];
        assert_eq!(
            render_recovery(&hints),
            "recovery[2]{tool,params,reason}:\n  step_a,\"{}\",null\n  step_b,\"{}\",null"
        );
    }

    #[test]
    fn append_recovery_on_empty_slice_does_nothing() {
        let mut buf = "prefix\n".to_string();
        append_recovery(&mut buf, &[]);
        assert_eq!(buf, "prefix\n");
    }

    #[test]
    fn ac018_append_recovery_on_empty_slice_leaves_initially_empty_string_empty() {
        let mut buf = String::new();
        append_recovery(&mut buf, &[]);
        assert_eq!(buf, "");
    }

    #[test]
    fn ac012_builder_methods_produce_expected_fields() {
        let hint = RecoveryHint::new("tool_name").param("a", KvValue::Int(1)).param("b", KvValue::Int(2)).reason("why");
        assert_eq!(hint.tool, "tool_name");
        assert_eq!(hint.params, vec![("a".to_string(), KvValue::Int(1)), ("b".to_string(), KvValue::Int(2))]);
        assert_eq!(hint.reason, Some("why".to_string()));

        let bare = RecoveryHint::new("tool_name");
        assert_eq!(bare.tool, "tool_name");
        assert!(bare.params.is_empty());
        assert_eq!(bare.reason, None);
    }

    #[test]
    fn ac015_multiple_params_keep_insertion_order() {
        let hints = [RecoveryHint::new("assign_user")
            .param("user", KvValue::Text("alice".to_string()))
            .param("team", KvValue::Text("backend".to_string()))];
        assert_eq!(
            render_recovery(&hints),
            format!("{HEADER}assign_user,\"{{\\\"user\\\":\\\"alice\\\",\\\"team\\\":\\\"backend\\\"}}\",null")
        );
    }

    #[test]
    fn ac017_params_and_reason_appear_together_params_first() {
        let hints = [RecoveryHint::new("retry").param("attempt", KvValue::Int(2)).reason("transient network error")];
        assert_eq!(render_recovery(&hints), format!("{HEADER}retry,\"{{\\\"attempt\\\":2}}\",transient network error"));
    }

    #[test]
    fn ac018a_append_recovery_starts_a_new_line_after_a_finished_document() {
        let hints = [RecoveryHint::new("step_a"), RecoveryHint::new("step_b")];
        let mut out = "t: []".to_string();
        append_recovery(&mut out, &hints);
        assert_eq!(out, format!("t: []\n{}", render_recovery(&hints)));
        let mut out = "prefix\n".to_string();
        append_recovery(&mut out, &hints);
        assert_eq!(out, format!("prefix\n{}", render_recovery(&hints)));
    }

    #[test]
    fn ac018b_newline_in_tool_or_reason_is_quoted_and_escaped() {
        let hints = [RecoveryHint::new("a\nb").reason("c\nd")];
        let out = render_recovery(&hints);
        assert_eq!(out, format!("{HEADER}\"a\\nb\",\"{{}}\",\"c\\nd\""));
        assert_eq!(out.lines().count(), 2, "escaped newlines add no lines beyond the fixed count");
    }

    #[test]
    fn carriage_return_in_tool_or_reason_is_quoted_and_escaped() {
        let hints = [RecoveryHint::new("a\rb").reason("c\rd")];
        let out = render_recovery(&hints);
        assert_eq!(out, format!("{HEADER}\"a\\rb\",\"{{}}\",\"c\\rd\""));
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    #[test]
    fn ac004b_render_recovery_capacity_is_at_least_40_plus_len_times_60() {
        let hints = [RecoveryHint::new("a"), RecoveryHint::new("b")];
        let out = render_recovery(&hints);
        assert!(out.capacity() >= 40 + hints.len() * 60, "capacity {} too small", out.capacity());
    }

    // A line break in a param key is escaped twice: once by JSON (`\n`), then that backslash by TOON (`\\n`).
    #[test]
    fn ac018c_newline_in_param_key_is_escaped() {
        let hints = [RecoveryHint::new("t").param("k\nz", KvValue::Text("v".to_string()))];
        let out = render_recovery(&hints);
        assert_eq!(out, format!("{HEADER}t,\"{{\\\"k\\\\nz\\\":\\\"v\\\"}}\",null"));
        assert_eq!(out.lines().count(), 2, "the escaped \\n in the param key adds no line");
    }

    #[test]
    fn ac018c_carriage_return_in_param_key_is_escaped() {
        let hints = [RecoveryHint::new("t").param("k\rz", KvValue::Text("v".to_string()))];
        let out = render_recovery(&hints);
        assert_eq!(out, format!("{HEADER}t,\"{{\\\"k\\\\rz\\\":\\\"v\\\"}}\",null"));
        assert!(!out.contains('\r'), "no raw CR may survive, got: {out:?}");
    }

    // A JSON object cannot repeat a key, so `param` with an existing key replaces that value in place.
    #[test]
    fn param_with_an_existing_key_replaces_the_value_in_place() {
        let hint =
            RecoveryHint::new("t").param("x", KvValue::Int(1)).param("y", KvValue::Int(2)).param("x", KvValue::Int(3));
        assert_eq!(hint.params, vec![("x".to_string(), KvValue::Int(3)), ("y".to_string(), KvValue::Int(2))]);
    }

    // `params` is a public field, so duplicates can still be pushed directly; every writer then keeps one entry per
    // key, at its first position with its last value -- the same result `param` would give.
    #[test]
    fn directly_pushed_duplicate_params_render_one_key_with_the_last_value() {
        let mut hint = RecoveryHint::new("t");
        hint.params.push(("x".to_string(), KvValue::Int(1)));
        hint.params.push(("y".to_string(), KvValue::Int(2)));
        hint.params.push(("x".to_string(), KvValue::Int(3)));
        assert_eq!(params_json(&hint.params), "{\"x\":3,\"y\":2}");
    }

    // Deduplication used to rescan earlier params for every param: 40k params took 1.77 s to render. 50k distinct
    // params is over 1e9 comparisons that way, and milliseconds when linear.
    #[test]
    fn rendering_many_params_is_linear() {
        let mut hint = RecoveryHint::new("t");
        hint.params = (0..50_000).map(|i| (format!("k{i}"), KvValue::Int(i))).collect();
        hint.params.push(("k7".to_string(), KvValue::Int(-1)));
        let start = std::time::Instant::now();
        let json = params_json(&hint.params);
        let elapsed = start.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(2), "took {elapsed:?}");
        assert!(json.starts_with("{\"k0\":0,"), "{}", &json[..20]);
        assert!(json.contains("\"k7\":-1,\"k8\":8"), "a repeated key keeps its first position and last value");
        assert_eq!(json.matches("\"k7\"").count(), 1);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn duplicate_params_serialize_losslessly() {
        let hint = RecoveryHint::new("t").param("x", KvValue::Int(1)).param("x", KvValue::Int(2));
        let back: RecoveryHint = serde_json::from_str(&serde_json::to_string(&hint).unwrap()).unwrap();
        assert_eq!(back, hint);
        let mut direct = RecoveryHint::new("t");
        direct.params.push(("x".to_string(), KvValue::Int(1)));
        direct.params.push(("x".to_string(), KvValue::Int(2)));
        assert_eq!(serde_json::to_string(&direct).unwrap(), "{\"tool\":\"t\",\"params\":{\"x\":2}}");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac037b_serializes_params_as_an_object_and_omits_an_unset_reason() {
        assert_eq!(serde_json::to_string(&RecoveryHint::new("t")).unwrap(), "{\"tool\":\"t\",\"params\":{}}");
    }
}

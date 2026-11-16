//! The hand-built JSON writers behind every structured companion (`AgentResponse`'s JSON and `structuredContent`,
//! `DomainError::render_json`, the `params` cell of a recovery table). One copy of each writer, so a shape fixed here
//! is fixed everywhere. Hand-built so the default build needs no `serde_json`; the `serde` feature's derives are
//! checked against these writers in tests.

use std::fmt::Write as _;

use crate::kv::KvValue;
use crate::recovery::RecoveryHint;
use crate::toon::Value;

/// Append `s` as a JSON string: quoted, with `"`, `\` and every control character below U+0020 escaped.
pub(crate) fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if u32::from(other) < 0x20 => {
                let code = u32::from(other);
                out.push_str("\\u00");
                out.push(char::from_digit(code >> 4, 16).unwrap_or('0'));
                out.push(char::from_digit(code & 0xf, 16).unwrap_or('0'));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Append `true` or `false`.
pub(crate) fn push_bool(out: &mut String, b: bool) {
    out.push_str(if b { "true" } else { "false" });
}

/// Append an unsigned integer.
pub(crate) fn push_uint(out: &mut String, n: impl Into<u64>) {
    let _ = write!(out, "{}", n.into());
}

/// Append a JSON array: `[`, each item written by `item` with a comma between, `]`.
pub(crate) fn push_array<T>(
    out: &mut String,
    items: impl IntoIterator<Item = T>,
    mut item: impl FnMut(&mut String, T),
) {
    out.push('[');
    for (i, x) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        item(out, x);
    }
    out.push(']');
}

/// Append a JSON array of strings.
pub(crate) fn push_str_array<'a>(out: &mut String, items: impl IntoIterator<Item = &'a str>) {
    push_array(out, items, push_str);
}

/// Append a KV value. Non-finite floats are `null`, the token the TOON body uses (§3), so the surfaces agree.
pub(crate) fn push_kv_value(out: &mut String, value: &KvValue) {
    match value {
        KvValue::Text(s) => push_str(out, s),
        KvValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        KvValue::Float(f, decimals) => {
            if !f.is_finite() {
                out.push_str("null");
            } else if *decimals == KvValue::FULL_PRECISION {
                // TOON's canonical number form is also valid JSON (`1e-300`, `1e+21`), and padding to 255 places
                // would round a smaller value to zero.
                crate::toon::push_f64(out, *f);
            } else {
                let _ = write!(out, "{f:.*}", usize::from(*decimals));
            }
        }
        KvValue::Bool(b) => push_bool(out, *b),
        KvValue::Duration(d) => {
            let mut secs = String::with_capacity(16);
            let _ = write!(secs, "{:.1}s", d.as_secs_f64());
            push_str(out, &secs);
        }
        KvValue::Missing => out.push_str("null"),
    }
}

/// Append a TOON cell value. Non-finite floats are `null`, as in the text render.
pub(crate) fn push_value(out: &mut String, v: &Value) {
    match v {
        Value::Str(s) => push_str(out, s),
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Value::UInt(n) => push_uint(out, *n),
        Value::Float(f) => crate::toon::push_f64(out, *f),
        Value::Bool(b) => push_bool(out, *b),
        Value::Null => out.push_str("null"),
    }
}

/// Append recovery parameters as a JSON object. A repeated key is written once, at its first position, with its last
/// value -- what [`RecoveryHint::param`] would have produced -- so the object never holds a duplicate key.
pub(crate) fn push_params(out: &mut String, params: &[(String, KvValue)]) {
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
        push_str(out, k);
        out.push(':');
        push_kv_value(out, v);
    }
    out.push('}');
}

/// Append one recovery hint as `{"tool":…,"params":{…}}`, plus `"reason":…` only when it has one.
pub(crate) fn push_recovery(out: &mut String, r: &RecoveryHint) {
    out.push_str("{\"tool\":");
    push_str(out, &r.tool);
    out.push_str(",\"params\":");
    push_params(out, &r.params);
    if let Some(reason) = &r.reason {
        out.push_str(",\"reason\":");
        push_str(out, reason);
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str_json(s: &str) -> String {
        let mut out = String::new();
        push_str(&mut out, s);
        out
    }

    #[test]
    fn strings_escape_quotes_backslashes_and_control_characters() {
        assert_eq!(str_json("a\"b\\c\nd\re\tf\u{1}g\u{1f}"), r#""a\"b\\c\nd\re\tf\u0001g\u001f""#);
        assert_eq!(str_json("é\u{2028}"), "\"é\u{2028}\"");
    }

    #[test]
    fn arrays_separate_items_with_one_comma() {
        let mut out = String::new();
        push_str_array(&mut out, ["a", "b", "c"]);
        assert_eq!(out, r#"["a","b","c"]"#);
        let mut empty = String::new();
        push_str_array(&mut empty, []);
        assert_eq!(empty, "[]");
    }

    #[test]
    fn recovery_omits_an_absent_reason() {
        let mut out = String::new();
        push_recovery(&mut out, &RecoveryHint::new("t").param("n", KvValue::Int(1)));
        assert_eq!(out, r#"{"tool":"t","params":{"n":1}}"#);
        out.clear();
        push_recovery(&mut out, &RecoveryHint::new("t").reason("why"));
        assert_eq!(out, r#"{"tool":"t","params":{},"reason":"why"}"#);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn recovery_matches_the_serde_wire_form() {
        let r = RecoveryHint::new("q\"t").param("k", KvValue::Float(f64::NAN, 2)).reason("r\n");
        let mut out = String::new();
        push_recovery(&mut out, &r);
        let hand: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(hand["tool"], "q\"t");
        assert_eq!(hand["reason"], "r\n");
        assert!(hand["params"]["k"].is_null());
    }
}

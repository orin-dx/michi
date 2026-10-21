use crate::hints::Hint;
use std::fmt::Write as _;
use std::time::Duration;

/// A single key-value pair for [`render_kv`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: KvValue,
}

/// A value in a key-value item.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum KvValue {
    /// UTF-8 text value.
    Text(String),
    /// Signed integer.
    Int(i64),
    /// Floating-point number, rendered with the given number of decimal places.
    Float(f64, u8),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Elapsed time, rendered as fractional seconds with one decimal.
    Duration(Duration),
    /// Absent value, renders as `—` (em dash).
    Missing,
}

pub(crate) fn push_kv_value(out: &mut String, value: &KvValue) {
    match value {
        // Escaped, not stripped: a raw line break would end the one-line-per-key format.
        KvValue::Text(s) => out.push_str(&crate::text::escape_line_breaks(s)),
        KvValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        KvValue::Float(f, decimals) => {
            let _ = write!(out, "{f:.*}", *decimals as usize);
        }
        KvValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        KvValue::Duration(d) => {
            let _ = write!(out, "{:.1}s", d.as_secs_f64());
        }
        KvValue::Missing => out.push('—'),
    }
}

pub(crate) fn json_escape_str(out: &mut String, s: &str) {
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

pub(crate) fn kv_value_to_json(out: &mut String, value: &KvValue) {
    match value {
        KvValue::Text(s) => json_escape_str(out, s),
        KvValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        KvValue::Float(f, decimals) => {
            if f.is_nan() || f.is_infinite() {
                // NaN and Infinity are not valid JSON numbers — render as quoted strings
                // to avoid producing invalid JSON output
                let s = format!("{f:.*}", *decimals as usize);
                json_escape_str(out, &s);
            } else {
                let _ = write!(out, "{f:.*}", *decimals as usize);
            }
        }
        KvValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        KvValue::Duration(d) => {
            let mut secs = String::new();
            let _ = write!(secs, "{:.1}s", d.as_secs_f64());
            json_escape_str(out, &secs);
        }
        KvValue::Missing => out.push_str("null"),
    }
}

/// Render a list of key-value pairs as a column-aligned multi-line block.
///
/// Key padding uses `chars().count()`, not display width. Keys containing
/// CJK or other wide Unicode characters will appear misaligned in monospace
/// terminals. This is acceptable: michi targets agent readability (where
/// display width is not relevant), not human terminal rendering.
#[must_use]
pub fn render_kv(items: &[KvItem], total_count: Option<usize>, hints: &[Hint]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let keys: Vec<std::borrow::Cow<'_, str>> = items.iter().map(|i| crate::text::escape_line_breaks(&i.key)).collect();
    let max_key_len = keys.iter().map(|k| k.chars().count()).max().unwrap_or(0);
    let capacity = items.len() * (max_key_len + 24) + hints.len() * 50 + 20;
    let mut out = String::with_capacity(capacity);
    for (item, key) in items.iter().zip(&keys) {
        out.push_str(key);
        out.push(':');
        let pad = max_key_len - key.chars().count() + 1;
        for _ in 0..pad {
            out.push(' ');
        }
        push_kv_value(&mut out, &item.value);
        out.push('\n');
    }
    if let Some(total) = total_count {
        out.push_str("totalCount: ");
        let _ = write!(out, "{total}");
        out.push('\n');
    }
    crate::hints::append_hints(&mut out, hints);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_key_needs_no_padding() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        assert_eq!(render_kv(&items, None, &[]), "id: 1\n");
    }

    #[test]
    fn text_value_with_newline_is_stripped_in_render() {
        let item = KvItem { key: "msg".into(), value: KvValue::Text("line1\nline2".into()) };
        let out = render_kv(&[item], None, &[]);
        // A newline in value must not create extra lines — KV is one-line-per-key
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "newline in value must not break KV format, got:\n{out}");
    }

    // AC-008: \n and \r are neither emitted raw nor dropped -- each renders as
    // its visible two-character escape, keeping the value on one line.
    #[test]
    fn ac008_newline_and_cr_in_text_value_are_escaped() {
        let item = KvItem { key: "k".into(), value: KvValue::Text("line1\nline2\rline3".into()) };
        let out = render_kv(&[item], None, &[]);
        assert_eq!(out, "k: line1\\nline2\\rline3\n");
    }

    // AC-011: KvValue::Missing renders as the em-dash character, not empty,
    // "null", or "N/A".
    #[test]
    fn ac011_missing_renders_as_em_dash() {
        let mut out = String::new();
        push_kv_value(&mut out, &KvValue::Missing);
        assert_eq!(out, "—");
    }

    // AC-013: the NaN token is independent of the decimals argument.
    #[test]
    fn ac013_nan_token_is_independent_of_decimals() {
        let mut two = String::new();
        push_kv_value(&mut two, &KvValue::Float(f64::NAN, 2));
        let mut six = String::new();
        push_kv_value(&mut six, &KvValue::Float(f64::NAN, 6));
        assert_eq!(two, "NaN");
        assert_eq!(six, "NaN");
    }

    // AC-046: negative Int renders with a leading minus sign.
    #[test]
    fn ac046_negative_int_renders_with_minus_sign() {
        let items = vec![KvItem { key: "n".into(), value: KvValue::Int(-7) }];
        assert_eq!(render_kv(&items, None, &[]), "n: -7\n");
    }

    // AC-047: Bool renders lowercase and unquoted, both directions.
    #[test]
    fn ac047_bool_renders_lowercase_unquoted() {
        let mut t = String::new();
        push_kv_value(&mut t, &KvValue::Bool(true));
        assert_eq!(t, "true");
        let mut f = String::new();
        push_kv_value(&mut f, &KvValue::Bool(false));
        assert_eq!(f, "false");
    }

    // AC-048: Float with real decimal precision (not NaN/Inf) rounds and
    // truncates trailing digits per the decimals argument. 3.14159 is the
    // spec's pinned fixture value -- coincidentally pi-adjacent, not an
    // actual reference to std::f64::consts::PI.
    #[test]
    #[allow(clippy::approx_constant)]
    fn ac048_float_renders_with_given_decimal_precision() {
        let mut two = String::new();
        push_kv_value(&mut two, &KvValue::Float(3.14159, 2));
        assert_eq!(two, "3.14");
        let mut zero = String::new();
        push_kv_value(&mut zero, &KvValue::Float(3.14159, 0));
        assert_eq!(zero, "3");
    }

    // AC-017: total_count and hints are NOT rendered when items is empty —
    // not just the trivial all-None/empty case, but with both set.
    #[test]
    fn ac017_empty_items_ignores_total_count_and_hints() {
        assert_eq!(render_kv(&[], Some(42), &[Hint::from("x")]), "");
    }

    // AC-018: shorter and longer keys' values align to the same char offset.
    #[test]
    fn ac018_value_column_aligns_across_differing_key_lengths() {
        let items = vec![
            KvItem { key: "a".into(), value: KvValue::Text("short".into()) },
            KvItem { key: "longer_key".into(), value: KvValue::Text("val".into()) },
        ];
        let out = render_kv(&items, None, &[]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "got: {out:?}");
        let offset_of_value = |line: &str, value: &str| line.find(value).expect("value present");
        let a_offset = offset_of_value(lines[0], "short");
        let b_offset = offset_of_value(lines[1], "val");
        assert_eq!(a_offset, b_offset, "value columns must align, got:\n{out}");
        // The longest key's own line needs zero alignment padding, so its
        // value-start offset alone proves the separating space exists.
        assert_eq!(b_offset, "longer_key:".chars().count() + 1, "must include at least one separating space");
    }

    #[test]
    fn push_kv_value_float_nan_renders_as_nan_token() {
        // Plain-text KV is agent-readable text; NaN is an acceptable token here
        // (unlike JSON where bare NaN is syntactically invalid).
        let mut out = String::new();
        push_kv_value(&mut out, &KvValue::Float(f64::NAN, 2));
        assert_eq!(out, "NaN");
    }

    #[test]
    fn push_kv_value_float_inf_renders_as_inf_token() {
        let mut out = String::new();
        push_kv_value(&mut out, &KvValue::Float(f64::INFINITY, 0));
        assert_eq!(out, "inf");
    }

    #[test]
    fn push_kv_value_duration_renders_with_one_decimal() {
        let mut out = String::new();
        push_kv_value(&mut out, &KvValue::Duration(std::time::Duration::from_millis(1500)));
        assert_eq!(out, "1.5s");
    }

    #[test]
    fn render_kv_empty_items_returns_empty_string() {
        assert_eq!(render_kv(&[], None, &[]), "");
    }

    #[test]
    fn render_kv_total_count_appended() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let out = render_kv(&items, Some(42), &[]);
        assert!(out.contains("totalCount: 42\n"), "got: {out}");
    }

    #[test]
    fn float_nan_in_json_is_quoted() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Float(f64::NAN, 2));
        // Must be valid JSON — not the bare NaN token
        assert!(out.starts_with('"'), "NaN must be JSON-quoted, got: {out}");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn float_nan_in_json_is_valid_json_with_serde() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Float(f64::NAN, 2));
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
        assert!(parsed.is_string(), "NaN must serialize as a JSON string");
    }

    #[test]
    fn float_inf_in_json_is_quoted() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Float(f64::INFINITY, 0));
        assert!(out.starts_with('"'), "Inf must be JSON-quoted, got: {out}");
    }

    #[test]
    fn float_neg_inf_in_json_is_quoted() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Float(f64::NEG_INFINITY, 0));
        assert!(out.starts_with('"'), "-Inf must be JSON-quoted, got: {out}");
    }

    // AC-042/AC-043: the three prior tests above only check the leading quote
    // byte, which any quoted string (even a wrong one) would satisfy. Pin the
    // exact quoted content.
    #[test]
    fn ac042_ac043_float_special_values_render_exact_quoted_tokens() {
        let mut nan = String::new();
        kv_value_to_json(&mut nan, &KvValue::Float(f64::NAN, 2));
        assert_eq!(nan, "\"NaN\"");
        let mut inf = String::new();
        kv_value_to_json(&mut inf, &KvValue::Float(f64::INFINITY, 0));
        assert_eq!(inf, "\"inf\"");
        let mut neg_inf = String::new();
        kv_value_to_json(&mut neg_inf, &KvValue::Float(f64::NEG_INFINITY, 0));
        assert_eq!(neg_inf, "\"-inf\"");
    }

    // AC-041: Missing renders as a bare JSON null, not a string or absent.
    #[test]
    fn ac041_missing_renders_as_bare_json_null() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Missing);
        assert_eq!(out, "null");
    }

    // AC-044: Duration renders as a quoted JSON string, matching the
    // text-mode token.
    #[test]
    fn ac044_duration_renders_as_quoted_json_string() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Duration(std::time::Duration::from_millis(1500)));
        assert_eq!(out, "\"1.5s\"");
    }

    // AC-045: Bool renders as a bare JSON boolean, not a quoted string.
    #[test]
    fn ac045_bool_renders_as_bare_json_boolean() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Bool(true));
        assert_eq!(out, "true");
    }
}

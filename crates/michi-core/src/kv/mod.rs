use crate::hints::Hint;
use std::fmt::Write as _;
use std::time::Duration;

/// A single key-value pair for [`render_kv`].
#[derive(Debug, Clone, PartialEq)]
pub struct KvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: KvValue,
}

/// A value in a key-value item, rendered as a TOON 4.4 primitive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum KvValue {
    /// UTF-8 text value, quoted only when TOON 4.4 §7.2 requires it.
    Text(String),
    /// Signed integer.
    Int(i64),
    /// Floating-point number, rounded to the given number of decimal places and then written in canonical TOON
    /// number form (§2): the decimals bound the precision but never pad (`Float(1.5, 3)` renders `1.5`).
    ///
    /// NaN and ±Infinity render as `null` in every rendered output, TOON (§3) and the JSON companions alike. With
    /// the `serde` feature, `KvValue` itself serializes a non-finite float as the string `"NaN"`, `"Infinity"` or
    /// `"-Infinity"` (finite values stay JSON numbers) and accepts those strings back, so it round-trips exactly.
    Float(
        #[cfg_attr(feature = "serde", serde(with = "crate::float_wire"))]
        #[cfg_attr(feature = "schemars", schemars(schema_with = "crate::float_wire::schema"))]
        f64,
        u8,
    ),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Elapsed time, rendered as the string of fractional seconds with one decimal, e.g. `1.5s`.
    Duration(Duration),
    /// Absent value, renders as `null`.
    Missing,
}

/// Append a KV value as a TOON 4.4 primitive token (§2, §3, §7.2).
///
/// `Text` is quoted only when §7.2 requires it (line breaks become `\n`/`\r` inside quotes); `Int` and `Bool` are
/// literal; `Float(f, decimals)` is rounded to `decimals` places and the rounded value is written in canonical number
/// form; `Duration` is the string `{secs:.1}s`; `Missing` is `null`.
pub(crate) fn push_kv_value(out: &mut String, value: &KvValue) {
    match value {
        KvValue::Text(s) => out.push_str(&crate::toon::escape_value(s)),
        KvValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        KvValue::Float(f, decimals) => crate::toon::push_f64(out, round_to_decimals(*f, *decimals)),
        KvValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        KvValue::Duration(d) => {
            let mut secs = String::with_capacity(16);
            let _ = write!(secs, "{:.1}s", d.as_secs_f64());
            out.push_str(&crate::toon::escape_value(&secs));
        }
        KvValue::Missing => out.push_str("null"),
    }
}

/// `f` rounded to `decimals` fractional digits by the correctly rounded decimal formatting of `{f:.decimals}`.
/// Non-finite values pass through unchanged.
/// The decimal places `KvValue::Float` needs to reproduce `f` exactly: the fractional digits of its shortest exact
/// representation (Rust's `Display` for f64, which never uses an exponent), so `1e-7` needs 7 and `2.0` needs 0.
/// Non-finite values need 0. Used where a float arrives without a decimals setting.
#[cfg(any(feature = "serde", feature = "napi"))]
pub(crate) fn shortest_decimals(f: f64) -> usize {
    if !f.is_finite() {
        return 0;
    }
    f.to_string().split_once('.').map_or(0, |(_, frac)| frac.len())
}

fn round_to_decimals(f: f64, decimals: u8) -> f64 {
    if !f.is_finite() {
        return f;
    }
    format!("{f:.*}", usize::from(decimals)).parse().unwrap_or(f)
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
            if !f.is_finite() {
                // Not a JSON number: `null`, the same token the TOON body uses (§3), so the surfaces agree.
                out.push_str("null");
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

/// Root keys michi composes next to a body (a TOON list's type entry or KV items), so a body may not use them: the
/// document would otherwise hold a duplicate key, which TOON 4.4 §14.3 forbids.
pub(crate) const RESERVED_ROOT_KEYS: [&str; 3] = ["totalCount", "help", "recovery"];

/// Error returned by [`render_kv`] when the items cannot form a valid TOON document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KvError {
    /// An item key is a root key michi composes next to the items (`totalCount`, `help` or `recovery`). It is
    /// rejected whether or not that key is present in this document, so validity never depends on the data.
    #[error("key {key:?} is a reserved document key (totalCount, help, recovery)")]
    ReservedKey {
        /// The offending key.
        key: String,
    },
    /// Two items share a key; a TOON object may not repeat a key (§14.3).
    #[error("key {key:?} appears more than once")]
    DuplicateKey {
        /// The offending key.
        key: String,
    },
}

/// Check that `items` can be the fields of a TOON root object next to michi's reserved keys.
pub(crate) fn validate_keys(items: &[KvItem]) -> Result<(), KvError> {
    let mut seen = std::collections::HashSet::with_capacity(items.len());
    for item in items {
        if RESERVED_ROOT_KEYS.contains(&item.key.as_str()) {
            return Err(KvError::ReservedKey { key: item.key.clone() });
        }
        if !seen.insert(item.key.as_str()) {
            return Err(KvError::DuplicateKey { key: item.key.clone() });
        }
    }
    Ok(())
}

/// Append one `key: value` line (§8), preceded by a LF when `out` is non-empty and does not already end in one.
pub(crate) fn push_kv_line(out: &mut String, key: &str, value: &KvValue) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&crate::toon::escape_key(key));
    out.push_str(": ");
    push_kv_value(out, value);
}

/// Render key-value pairs as a TOON 4.4 root object (§8): one `key: value` line per item, then `totalCount: N` and
/// the inline `help[N]: …` array when present, with no trailing newline.
///
/// Keys are quoted per §7.3 and values written as TOON primitives (see [`KvValue`]). Exactly one space follows each
/// colon, so values are not column-aligned. Returns the empty string (the empty TOON document) for empty `items`,
/// ignoring `total_count` and `hints`.
///
/// # Errors
///
/// [`KvError::ReservedKey`] for an item key `totalCount`, `help` or `recovery`, and [`KvError::DuplicateKey`] for a
/// repeated key: either would make the document hold a duplicate key (§14.3).
pub fn render_kv(items: &[KvItem], total_count: Option<usize>, hints: &[Hint]) -> Result<String, KvError> {
    if items.is_empty() {
        return Ok(String::new());
    }
    validate_keys(items)?;
    let capacity = items.iter().map(|i| i.key.len() + 24).sum::<usize>() + hints.len() * 50 + 20;
    let mut out = String::with_capacity(capacity);
    for item in items {
        push_kv_line(&mut out, &item.key, &item.value);
    }
    if let Some(total) = total_count {
        out.push_str("\ntotalCount: ");
        let _ = write!(out, "{total}");
    }
    crate::hints::append_hints(&mut out, hints);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(v: &KvValue) -> String {
        let mut out = String::new();
        push_kv_value(&mut out, v);
        out
    }

    fn render(items: &[KvItem]) -> String {
        render_kv(items, None, &[]).expect("valid items")
    }

    #[test]
    fn single_key_renders_one_line_without_trailing_newline() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        assert_eq!(render(&items), "id: 1");
    }

    #[test]
    fn text_value_with_newline_stays_on_one_line() {
        let item = KvItem { key: "msg".into(), value: KvValue::Text("line1\nline2".into()) };
        let out = render(&[item]);
        assert_eq!(out.lines().count(), 1, "newline in value must not break the one-line-per-key format, got:\n{out}");
    }

    // AC-008: \n and \r are neither emitted raw nor dropped -- the value is quoted (TOON 4.4 §7.2) and each renders
    // as its two-character escape (§7.1).
    #[test]
    fn ac008_newline_and_cr_in_text_value_are_quoted_and_escaped() {
        let item = KvItem { key: "k".into(), value: KvValue::Text("line1\nline2\rline3".into()) };
        assert_eq!(render(&[item]), "k: \"line1\\nline2\\rline3\"");
    }

    // AC-011: KvValue::Missing renders as the TOON null literal (§2).
    #[test]
    fn ac011_missing_renders_as_null() {
        assert_eq!(value(&KvValue::Missing), "null");
    }

    // AC-013: the NaN token is independent of the decimals argument.
    #[test]
    fn ac013_nan_token_is_independent_of_decimals() {
        assert_eq!(value(&KvValue::Float(f64::NAN, 2)), "null");
        assert_eq!(value(&KvValue::Float(f64::NAN, 6)), "null");
    }

    // AC-046: negative Int renders with a leading minus sign.
    #[test]
    fn ac046_negative_int_renders_with_minus_sign() {
        let items = vec![KvItem { key: "n".into(), value: KvValue::Int(-7) }];
        assert_eq!(render(&items), "n: -7");
    }

    // AC-047: Bool renders lowercase and unquoted, both directions.
    #[test]
    fn ac047_bool_renders_lowercase_unquoted() {
        assert_eq!(value(&KvValue::Bool(true)), "true");
        assert_eq!(value(&KvValue::Bool(false)), "false");
    }

    // AC-048: Float rounds to the decimals argument. 3.14159 is the spec's pinned fixture value -- coincidentally
    // pi-adjacent, not an actual reference to std::f64::consts::PI.
    #[test]
    #[allow(clippy::approx_constant)]
    fn ac048_float_renders_with_given_decimal_precision() {
        assert_eq!(value(&KvValue::Float(3.14159, 2)), "3.14");
        assert_eq!(value(&KvValue::Float(3.14159, 0)), "3");
    }

    #[test]
    fn float_decimals_never_pad_with_trailing_zeros() {
        assert_eq!(value(&KvValue::Float(1.5, 3)), "1.5");
        assert_eq!(value(&KvValue::Float(2.0, 2)), "2");
        assert_eq!(value(&KvValue::Float(-0.001, 2)), "0");
    }

    // AC-017: total_count and hints are NOT rendered when items is empty — not just the trivial all-None/empty case,
    // but with both set.
    #[test]
    fn ac017_empty_items_ignores_total_count_and_hints() {
        assert_eq!(render_kv(&[], Some(42), &[Hint::from("x")]), Ok(String::new()));
    }

    // AC-018 (superseded): values are no longer column-aligned; TOON 4.4 §8/§12 require exactly one space after the
    // colon whatever the key lengths.
    #[test]
    fn ac018_exactly_one_space_follows_every_colon() {
        let items = vec![
            KvItem { key: "a".into(), value: KvValue::Text("short".into()) },
            KvItem { key: "longer_key".into(), value: KvValue::Text("val".into()) },
        ];
        assert_eq!(render(&items), "a: short\nlonger_key: val");
    }

    #[test]
    fn non_finite_floats_render_as_null() {
        assert_eq!(value(&KvValue::Float(f64::INFINITY, 0)), "null");
        assert_eq!(value(&KvValue::Float(f64::NEG_INFINITY, 0)), "null");
    }

    #[test]
    fn duration_renders_with_one_decimal() {
        assert_eq!(value(&KvValue::Duration(std::time::Duration::from_millis(1500))), "1.5s");
    }

    #[test]
    fn render_kv_total_count_appended() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        assert_eq!(render_kv(&items, Some(42), &[]), Ok("id: 1\ntotalCount: 42".to_string()));
    }

    #[test]
    fn render_kv_capacity_covers_the_items() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let out = render_kv(&items, None, &[]).expect("valid");
        assert!(out.capacity() >= "id".len() + 24, "capacity {} too small", out.capacity());
    }

    // AC-042/AC-043 (superseded): the JSON companion writes a non-finite float exactly as the TOON body does, as
    // `null` (TOON 4.4 §3; also what JSON.stringify does), so the two surfaces never disagree. It used to write the
    // Rust tokens "NaN"/"inf"/"-inf" as strings. `KvValue`'s own serde form, which must round-trip, uses the strings
    // "NaN", "Infinity" and "-Infinity" instead (see tests/kv_value_wire.rs).
    #[test]
    fn non_finite_floats_in_json_are_null_like_the_toon_body() {
        for (f, decimals) in [(f64::NAN, 2), (f64::INFINITY, 0), (f64::NEG_INFINITY, 0)] {
            let v = KvValue::Float(f, decimals);
            let mut json = String::new();
            kv_value_to_json(&mut json, &v);
            assert_eq!(json, "null", "{v:?}");
            assert_eq!(json, value(&v), "JSON and TOON tokens must agree for {v:?}");
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn float_nan_in_json_is_valid_json_with_serde() {
        let mut out = String::new();
        kv_value_to_json(&mut out, &KvValue::Float(f64::NAN, 2));
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("must be valid JSON");
        assert!(parsed.is_null(), "NaN must be JSON null");
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

//! TOON 4.4 string, key and number encoding (SPEC.md §2, §3, §7.1, §7.2, §7.3), for the comma document delimiter.

use std::borrow::Cow;

/// Encode a string value as a TOON token (TOON 4.4 §7.2), quoting it only when the spec requires it.
///
/// A string is quoted when it is empty; has leading or trailing whitespace (U+0020 or U+0009); equals `true`, `false`
/// or `null`; looks numeric (`/^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i`); contains `:`, `"`, `\`, `[`, `]`,
/// `{`, `}`, a control character U+0000..U+001F, or the comma document delimiter; or starts with `-` or `#`. Every
/// other string is returned unchanged, borrowed. Quoted strings use the escapes of [`escape_value_quoted`].
#[must_use]
pub fn escape_value(v: &str) -> Cow<'_, str> {
    if needs_quotes(v) {
        Cow::Owned(escape_value_quoted(v))
    } else {
        Cow::Borrowed(v)
    }
}

/// Wrap a string in double quotes unconditionally, escaping it per TOON 4.4 §7.1: `\` → `\\`, `"` → `\"`, LF → `\n`,
/// CR → `\r`, HTAB → `\t`, any other U+0000..U+001F control → `\u00xx` (lowercase hex). Everything else, including
/// DEL, C1 controls and supplementary-plane characters, is written as literal UTF-8.
#[must_use]
pub fn escape_value_quoted(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 8);
    out.push('"');
    for ch in v.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let code = u32::from(c);
                out.push_str("\\u00");
                out.push(char::from(HEX[(code >> 4) as usize]));
                out.push(char::from(HEX[(code & 0xf) as usize]));
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Encode an object key or header field name as a TOON token (TOON 4.4 §7.3).
///
/// A key matching `^[A-Za-z_][A-Za-z0-9_.]*$` is returned unchanged, borrowed; any other key, including the empty
/// key and every non-ASCII key, is quoted and escaped like [`escape_value_quoted`]. michi uses this for the type
/// name and the field names of a TOON header.
#[must_use]
pub fn escape_key(k: &str) -> Cow<'_, str> {
    let mut chars = k.chars();
    let unquoted_form = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if unquoted_form {
        Cow::Borrowed(k)
    } else {
        Cow::Owned(escape_value_quoted(k))
    }
}

/// The TOON 4.4 §7.2 quoting test for a non-root string value with the comma document delimiter.
fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.starts_with([' ', '\t', '-', '#'])
        || s.ends_with([' ', '\t'])
        || matches!(s, "true" | "false" | "null")
        || s.chars().any(|c| matches!(c, ':' | '"' | '\\' | '[' | ']' | '{' | '}' | ',') || u32::from(c) < 0x20)
        || is_numeric_like(s)
}

/// `/^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i` over ASCII digits (TOON 4.4 §7.2).
fn is_numeric_like(s: &str) -> bool {
    fn digits(b: &[u8], i: &mut usize) -> bool {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > start
    }
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    if !digits(b, &mut i) {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(b, &mut i) {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(b, &mut i) {
            return false;
        }
    }
    i == b.len()
}

/// Append a float in TOON 4.4 canonical number form (§2, §3).
///
/// NaN and ±Infinity become `null` (§3). Zero, including `-0`, is `0`. Values with `1e-6 <= |n| < 1e21` are plain
/// decimals with the fewest round-trip digits and no trailing fractional zeros (an integral value prints as an
/// integer); values outside that range use exponent notation with a lowercase `e` and an explicit sign, e.g.
/// `1e+21`, `2.5e-7`. Rust's `Display`/`LowerExp` for `f64` already produce the shortest round-trip digits.
pub(crate) fn push_f64(out: &mut String, f: f64) {
    use std::fmt::Write as _;
    if !f.is_finite() {
        out.push_str("null");
    } else if f == 0.0 {
        out.push('0');
    } else if (1e-6..1e21).contains(&f.abs()) {
        let _ = write!(out, "{f}");
    } else {
        let start = out.len();
        let _ = write!(out, "{f:e}");
        if let Some(e) = out[start..].find('e') {
            let after = start + e + 1;
            if !out[after..].starts_with('-') {
                out.insert(after, '+');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(f: f64) -> String {
        let mut s = String::new();
        push_f64(&mut s, f);
        s
    }

    #[test]
    fn plain_value_is_not_escaped() {
        assert_eq!(escape_value("hello"), "hello");
    }

    #[test]
    fn value_with_comma_is_quoted() {
        assert_eq!(escape_value("a,b"), r#""a,b""#);
    }

    #[test]
    fn value_with_quote_escapes_it() {
        assert_eq!(escape_value(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn empty_value_is_quoted() {
        assert_eq!(escape_value(""), r#""""#);
    }

    #[test]
    fn ac031_escape_value_quoted_always_wraps_in_quotes() {
        for s in ["", "plain", "a,b", "line1\nline2"] {
            let out = escape_value_quoted(s);
            assert!(out.starts_with('"') && out.ends_with('"'), "input {s:?} -> {out:?}");
        }
    }

    #[test]
    fn ac033_escape_value_quoted_escapes_embedded_quotes() {
        assert_eq!(escape_value_quoted(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn ac034_escape_value_quoted_escapes_newlines_and_carriage_returns() {
        let out = escape_value_quoted("a\nb\rc");
        assert!(!out.contains('\n') && !out.contains('\r'), "got: {out:?}");
        assert_eq!(out, "\"a\\nb\\rc\"");
    }

    #[test]
    fn escape_value_quoted_escapes_tab_and_other_controls_in_lowercase_hex() {
        assert_eq!(escape_value_quoted("a\tb\u{1}\u{1f}\u{7f}"), "\"a\\tb\\u0001\\u001f\u{7f}\"");
    }

    #[test]
    fn ac042_escape_value_cow_variant_matches_whether_quoting_happened() {
        assert!(matches!(escape_value("plain"), Cow::Borrowed(_)));
        assert!(matches!(escape_value("a,b"), Cow::Owned(_)));
        assert!(matches!(escape_value("a\"b"), Cow::Owned(_)));
        assert_eq!(escape_value("a\nb"), "\"a\\nb\"");
        assert_eq!(escape_value("a\rb"), "\"a\\rb\"");
        // A backslash always forces quoting (§7.2), so it is escaped, never left bare.
        assert_eq!(escape_value("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn numeric_like_matches_the_spec_pattern_exactly() {
        for s in ["0", "42", "-3.14", "+1", "05", "1e6", "1E-6", "1.5e+3"] {
            assert!(is_numeric_like(s), "{s}");
        }
        for s in ["", "+", "1.", ".5", "1e", "1e+", "0x10", "1_0", "--1", "٤٢", "NaN"] {
            assert!(!is_numeric_like(s), "{s}");
        }
    }

    #[test]
    fn keys_are_quoted_only_outside_the_unquoted_pattern() {
        assert!(matches!(escape_key("file_path.v2"), Cow::Borrowed("file_path.v2")));
        assert_eq!(escape_key("2x"), "\"2x\"");
        assert_eq!(escape_key(""), "\"\"");
        assert_eq!(escape_key("a b"), "\"a b\"");
    }

    #[test]
    fn floats_render_canonically() {
        assert_eq!(number(f64::NAN), "null");
        assert_eq!(number(f64::NEG_INFINITY), "null");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(2.0), "2");
        assert_eq!(number(1e21), "1e+21");
        assert_eq!(number(-1e21), "-1e+21");
        assert_eq!(number(9.99e-7), "9.99e-7");
        assert_eq!(number(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(number(f64::MIN_POSITIVE), "2.2250738585072014e-308");
    }
}

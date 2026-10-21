/// Escape a scalar value for TOON row output.
///
/// Values containing `,`, `"`, LF or CR are quoted. Inside quotes `\` starts an escape: `"` and `\` are written as
/// `\"` and `\\`, and line breaks as `\n` and `\r`, so a cell never spans lines and nothing is dropped. Unquoted values
/// have no escape syntax and keep backslashes literally.
pub fn escape_value(v: &str) -> std::borrow::Cow<'_, str> {
    if v.chars().any(|ch| matches!(ch, ',' | '"' | '\n' | '\r')) {
        std::borrow::Cow::Owned(escape_value_quoted(v))
    } else {
        std::borrow::Cow::Borrowed(v)
    }
}

/// Escape a value for TOON row output and unconditionally wrap it in quotes, using the escapes described on
/// [`escape_value`].
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
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Characters that are structurally significant in TOON header positions and
/// must not appear verbatim in type names, field names, or hints.
///
/// Shared with [`ToonOptions::validate()`] to keep the two in sync.
pub(crate) const STRUCTURAL: &[char] = &['[', ']', '{', '}', ',', '\n', '\r'];

/// Sanitize a TOON header token (type_name or field name) for safe embedding.
///
/// Replaces `\n`, `\r`, and structural characters (`[`, `]`, `{`, `}`, `,`)
/// with `_`. Header positions have no escaping syntax in the TOON grammar —
/// replacement is the only safe option that keeps output parseable.
#[must_use]
pub(crate) fn sanitize_header_token(s: &str) -> std::borrow::Cow<'_, str> {
    if s.chars().any(|c| STRUCTURAL.contains(&c)) {
        std::borrow::Cow::Owned(s.chars().map(|c| if STRUCTURAL.contains(&c) { '_' } else { c }).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Sanitize a TOON hint line. A hint is free text on its own line, so only line breaks are structural: they are
/// escaped as `\n` / `\r`, and ordinary punctuation such as `,` or `{` is kept.
#[must_use]
pub(crate) fn sanitize_hint(s: &str) -> std::borrow::Cow<'_, str> {
    crate::text::escape_line_breaks(s)
}

#[cfg(test)]
mod sanitize_tests {
    use super::*;

    #[test]
    fn plain_token_is_borrowed() {
        let result = sanitize_header_token("file_path");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "file_path");
    }

    #[test]
    fn newline_in_type_name_is_replaced() {
        assert_eq!(sanitize_header_token("foo\nbar"), "foo_bar");
    }

    #[test]
    fn carriage_return_is_replaced() {
        assert_eq!(sanitize_header_token("foo\rbar"), "foo_bar");
    }

    #[test]
    fn structural_chars_are_replaced() {
        assert_eq!(sanitize_header_token("a[b]c"), "a_b_c");
        assert_eq!(sanitize_header_token("a{b}c"), "a_b_c");
        assert_eq!(sanitize_header_token("a,b"), "a_b");
    }

    #[test]
    fn multiple_structural_chars() {
        assert_eq!(sanitize_header_token("foo{bar},baz\n"), "foo_bar__baz_");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn empty_value_is_unchanged() {
        assert_eq!(escape_value(""), "");
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
    fn ac042_escape_value_cow_variant_matches_whether_quoting_happened() {
        assert!(matches!(escape_value("plain"), std::borrow::Cow::Borrowed(_)));
        assert!(matches!(escape_value("a,b"), std::borrow::Cow::Owned(_)));
        assert!(matches!(escape_value("a\"b"), std::borrow::Cow::Owned(_)));
        // \n/\r alone (no comma or quote) triggers quoting -> Owned, wrapped in
        // quotes with the line break written as a visible escape.
        let newline_only = escape_value("a\nb");
        assert!(matches!(newline_only, std::borrow::Cow::Owned(_)));
        assert_eq!(newline_only, "\"a\\nb\"");
        let cr_only = escape_value("a\rb");
        assert!(matches!(cr_only, std::borrow::Cow::Owned(_)));
        assert_eq!(cr_only, "\"a\\rb\"");
        // A backslash with nothing else to quote stays unquoted and literal.
        assert!(matches!(escape_value("a\\b"), std::borrow::Cow::Borrowed("a\\b")));
    }
}

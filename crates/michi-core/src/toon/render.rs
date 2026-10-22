use compact_str::CompactString;

use super::escape::{escape_key, escape_value, push_f64};

/// A single cell value in a TOON row.
///
/// Rendering follows TOON 4.4: strings are quoted only when §7.2 requires it, numbers use the canonical form of §2,
/// and `Null` is the `null` literal.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Value {
    /// UTF-8 string (stack-inlined for strings <= 24 bytes via `CompactString`). The only variant subject to
    /// `max_cell_len` truncation.
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    Str(CompactString),
    /// Signed integer.
    Int(i64),
    /// Unsigned integer, used by `From<u64>`/`From<usize>` (and `toon::list()`) for values above `i64::MAX`, so they
    /// stay exact unquoted numbers instead of being clamped, rounded or stringified.
    UInt(u64),
    /// Floating-point number. NaN and ±Infinity render as `null` (TOON 4.4 §3).
    Float(f64),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Null renders as the `null` literal.
    Null,
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::Str(CompactString::new(s))
    }
}

impl From<&String> for Value {
    fn from(s: &String) -> Self {
        Self::Str(CompactString::new(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Str(CompactString::new(s))
    }
}

impl From<CompactString> for Value {
    fn from(s: CompactString) -> Self {
        Self::Str(s)
    }
}

impl From<std::borrow::Cow<'_, str>> for Value {
    fn from(s: std::borrow::Cow<'_, str>) -> Self {
        Self::Str(CompactString::new(s))
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Self::Int(i64::from(n))
    }
}

impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Self::Int(i64::from(n))
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

impl From<u64> for Value {
    /// [`Value::Int`] when the value fits in `i64`, otherwise [`Value::UInt`]: IDs, hashes and counters keep their
    /// exact digits and render as unquoted numbers, never clamped, rounded or truncated.
    fn from(n: u64) -> Self {
        i64::try_from(n).map_or(Self::UInt(n), Self::Int)
    }
}

impl From<usize> for Value {
    /// Same contract as `From<u64>`.
    fn from(n: usize) -> Self {
        match (i64::try_from(n), u64::try_from(n)) {
            (Ok(i), _) => Self::Int(i),
            (Err(_), Ok(u)) => Self::UInt(u),
            // Not reachable on any platform Rust supports (usize is at most 64 bits); keeps the exact digits.
            (Err(_), Err(_)) => Self::Str(compact_str::format_compact!("{n}")),
        }
    }
}

impl From<f32> for Value {
    fn from(f: f32) -> Self {
        Self::Float(f64::from(f))
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<Option<String>> for Value {
    fn from(s: Option<String>) -> Self {
        match s {
            Some(s) => Self::Str(CompactString::new(s)),
            None => Self::Null,
        }
    }
}

impl std::fmt::Display for Value {
    /// The value's content without TOON quoting: strings verbatim, numbers in TOON canonical form (§2), `null` for
    /// `Null` and for non-finite floats (§3).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Str(s) => f.write_str(s),
            Self::Int(n) => write!(f, "{n}"),
            Self::UInt(n) => write!(f, "{n}"),
            Self::Float(fl) => {
                let mut s = String::with_capacity(24);
                push_f64(&mut s, *fl);
                f.write_str(&s)
            }
            Self::Bool(b) => f.write_str(if *b { "true" } else { "false" }),
            Self::Null => f.write_str("null"),
        }
    }
}

/// Root-object keys michi composes next to a TOON list (`totalCount`, `help`, `recovery`), which a type name therefore
/// must not take: TOON 4.4 §14.3 forbids duplicate sibling keys.
pub(crate) const RESERVED_TYPE_NAMES: [&str; 3] = crate::kv::RESERVED_ROOT_KEYS;

/// Append one row cell: §7.2 quoting for strings, §2/§3 canonical numbers, the `null` literal. Only strings are
/// truncated to `max_cell_len` chars, counted before escaping, with the caller's `hint` (empty: none) in the suffix.
fn push_cell(out: &mut String, cell: &Value, max_cell_len: usize, hint: &crate::truncate::InlineHint<'_>) {
    use std::fmt::Write as _;
    match cell {
        Value::Str(s) => {
            if s.len() <= max_cell_len {
                out.push_str(&escape_value(s));
            } else {
                let truncated = hint.truncate(s, max_cell_len);
                out.push_str(&escape_value(&truncated));
            }
        }
        Value::Int(n) => {
            let _ = write!(out, "{n}");
        }
        Value::UInt(n) => {
            let _ = write!(out, "{n}");
        }
        Value::Float(f) => push_f64(out, *f),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}

/// Append the list entry in the form TOON 4.4 prescribes for its shape, each line ending in a newline:
/// - no rows: `key: []` (§9.1; the field list is not emitted, an empty array has no shape),
/// - rows with zero fields: `key[N]:` and N bare `-` items, since empty objects may not use tabular form
///   (§9.3, §9.4, §10),
/// - otherwise the tabular form `key[N]{f1,f2}:` with one row per line at depth 1 (§9.3).
fn push_list(out: &mut String, opts: &crate::toon::ToonOptions) {
    use std::fmt::Write as _;
    out.push_str(&escape_key(&opts.type_name));
    if opts.rows.is_empty() {
        out.push_str(": []\n");
        return;
    }
    let _ = write!(out, "[{}]", opts.rows.len());
    if opts.fields.is_empty() {
        out.push_str(":\n");
        for _ in &opts.rows {
            out.push_str("  -\n");
        }
        return;
    }
    out.push('{');
    for (i, field) in opts.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&escape_key(field));
    }
    out.push_str("}:\n");
    // Measured once per document, not once per truncated cell.
    let hint = crate::truncate::InlineHint::new(opts.truncation_hint.as_deref().unwrap_or(""));
    for row in &opts.rows {
        out.push_str("  ");
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_cell(out, cell, opts.max_cell_len, &hint);
        }
        out.push('\n');
    }
}

/// Owns [`ToonDocument`] in a module of its own -- containing nothing else
/// -- so the invariant "only `validate` can produce one" is enforced by
/// Rust's own field-privacy rule, not by scanning source text for known
/// bypass shapes. `opts` is visible only inside this module and its
/// descendants; since this module declares no descendants and nothing
/// besides `validate`/`render`, no code anywhere else in this crate --
/// regardless of what shape it takes (a free function, a second impl
/// block, a `From`/`TryFrom` impl, a type alias, or a module gated behind
/// an unanticipated `#[cfg(...)]`) -- can write the struct-literal
/// `ToonDocument { opts }` and have it compile. This module is `mod`, not
/// `pub`, so external code cannot even name `render::proof::ToonDocument`
/// directly; the type is reachable only through the `pub use` re-export
/// below.
///
/// This guarantee holds only GIVEN two preconditions that Rust's type
/// system does not itself enforce, and that are each pinned by a
/// dedicated assertion in `sole_constructor_for_toon_document` rather than
/// left as a precondition nothing verifies: (1) `opts` stays genuinely
/// bare-private -- widening its visibility to reach the rest of this
/// module is a one-token, entirely ordinary-looking edit that would grant
/// field access to all of `render.rs` and let a constructor live outside
/// `proof` entirely, defeating the guarantee above while every OTHER
/// check here stays green; and (2) `proof`'s true extent is what the
/// scanning code believes it is -- found, historically, to be defeatable
/// by a multi-line string literal (e.g. inside a `#[doc = "..."]`
/// attribute) containing a bare `}`, which an earlier, purely line-based
/// brace-matcher mistook for the module's real close. `find_closing_line`
/// now tracks actual lexical structure (string and char literals,
/// comments) rather than taking for granted that a closing brace is
/// always alone on its rustfmt-normalized line, closing that specific
/// hole; a companion assertion additionally pins the literal line
/// immediately following `proof`'s close, as
/// defense in depth against a future defect in that scan.
///
/// **Known, accepted residual:** none of the above closes a bypass hidden
/// behind `macro_rules!` expansion -- a `some_macro!();` item-position
/// invocation inside this module's own `impl` block is, after expansion,
/// code that genuinely lives inside `proof` and therefore has real field
/// access, the same way `validate` does. No text scan can see through
/// macro expansion (`include_str!`, which every guard test here reads
/// from, captures source as written, before expansion), so this is a
/// limit of the mechanism, not a missed pattern. Per this invariant's own
/// design record, the property being enforced is that a maintainer
/// *following this codebase's patterns* cannot reintroduce the defect by
/// accident; a maintainer deliberately writing macro-hidden field access
/// to evade this exact module boundary is choosing to defeat it, not
/// accidentally tripping over it, and is outside what a source-level
/// guard can mechanically prevent. Closing that residual for real would
/// mean asserting over post-expansion, cfg-resolved output (e.g. via
/// `cargo-expand`) rather than unexpanded source text -- a deliberate
/// scope decision, not an oversight.
mod proof {
    /// Proof that a `ToonOptions` value has passed structural validation.
    ///
    /// The only function that produces a `ToonDocument` is
    /// [`ToonDocument::validate`] -- see this module's own doc comment for
    /// why no other function anywhere in the crate can construct one.
    /// Borrows `&'a ToonOptions` immutably, so the validated options cannot
    /// be mutated while the proof is live.
    #[derive(Debug)]
    pub struct ToonDocument<'a> {
        opts: &'a crate::toon::ToonOptions,
    }

    impl<'a> ToonDocument<'a> {
        /// Validate `opts` and return a proof it satisfies every structural
        /// invariant [`ToonDocument::render`] depends on.
        ///
        /// Every type name and field name is encodable (TOON 4.4 §7.3 quotes
        /// the ones that need it), so validation only rejects options that
        /// would make the document itself invalid TOON.
        ///
        /// # Errors
        ///
        /// Checks run in this order and the first failure is returned:
        /// [`crate::toon::ToonError::InvalidTypeName`] if `type_name` is a
        /// reserved root key (`totalCount`, `help` or `recovery`),
        /// [`crate::toon::ToonError::TooManyFields`] past
        /// [`crate::toon::MAX_FIELDS`] fields,
        /// [`crate::toon::ToonError::InvalidFieldName`] if a field name appears
        /// more than once, [`crate::toon::ToonError::TooManyCells`] if
        /// `rows × fields` is past [`crate::toon::MAX_CELLS`],
        /// [`crate::toon::ToonError::TruncationHintTooLong`] if the truncation
        /// hint is past [`crate::toon::MAX_TRUNCATION_HINT_CHARS`] chars, and
        /// [`crate::toon::ToonError::RowLengthMismatch`] if any row's length
        /// differs from `fields.len()`. Every check is linear in the input.
        pub fn validate(opts: &'a crate::toon::ToonOptions) -> Result<Self, crate::toon::ToonError> {
            use crate::toon::{ToonError, MAX_CELLS, MAX_FIELDS, MAX_TRUNCATION_HINT_CHARS};
            if super::RESERVED_TYPE_NAMES.contains(&opts.type_name.as_str()) {
                return Err(ToonError::InvalidTypeName { name: opts.type_name.clone() });
            }
            if opts.fields.len() > MAX_FIELDS {
                return Err(ToonError::TooManyFields { count: opts.fields.len(), max: MAX_FIELDS });
            }
            let mut seen = std::collections::HashSet::with_capacity(opts.fields.len());
            for field in &opts.fields {
                if !seen.insert(field.as_str()) {
                    return Err(ToonError::InvalidFieldName { name: field.clone() });
                }
            }
            if opts.rows.len().saturating_mul(opts.fields.len()) > MAX_CELLS {
                return Err(ToonError::TooManyCells {
                    rows: opts.rows.len(),
                    fields: opts.fields.len(),
                    max: MAX_CELLS,
                });
            }
            if let Some(hint) = &opts.truncation_hint {
                // A hint of at most MAX bytes has at most MAX chars, so only a longer one needs counting.
                if hint.len() > MAX_TRUNCATION_HINT_CHARS {
                    let len = hint.chars().count();
                    if len > MAX_TRUNCATION_HINT_CHARS {
                        return Err(ToonError::TruncationHintTooLong { len, max: MAX_TRUNCATION_HINT_CHARS });
                    }
                }
            }
            for (i, row) in opts.rows.iter().enumerate() {
                if row.len() != opts.fields.len() {
                    return Err(ToonError::RowLengthMismatch {
                        row_index: i,
                        expected: opts.fields.len(),
                        actual: row.len(),
                    });
                }
            }
            Ok(Self { opts })
        }

        /// Render `self` to a TOON 4.4 document: the list entry, then
        /// `totalCount: N` and the inline `help[N]: …` array when present,
        /// with no trailing newline (§12). Infallible and total by
        /// construction: row arity was already proven equal to
        /// `fields.len()` by `validate`, so rows and fields are iterated in
        /// lockstep instead of indexed by position, which could fail.
        #[must_use]
        pub fn render(&self) -> String {
            let opts = self.opts;
            let capacity = 60 + opts.rows.len() * (opts.fields.len() * 12 + 10) + opts.hints.len() * 60;
            let mut out = String::with_capacity(capacity);
            super::push_list(&mut out, opts);
            if let Some(total) = opts.total_count {
                out.push_str("totalCount: ");
                out.push_str(&total.to_string());
                out.push('\n');
            }
            crate::hints::append_help_block(&mut out, opts.hints.iter().map(String::as_str));
            if out.ends_with('\n') {
                out.pop();
            }
            out
        }
    }
}
pub use proof::ToonDocument;

#[allow(dead_code)]
fn _assert_render_signature(doc: &ToonDocument<'_>) -> String {
    doc.render()
}

#[cfg(test)]
mod tests {
    use super::Value;
    use compact_str::CompactString;

    fn render(
        type_name: &str,
        fields: &[String],
        rows: &[Vec<Value>],
        total_count: Option<usize>,
        hints: &[String],
        max_cell_len: usize,
    ) -> String {
        let opts = crate::toon::ToonOptions {
            type_name: type_name.to_string(),
            fields: fields.to_vec(),
            rows: rows.to_vec(),
            total_count,
            hints: hints.to_vec(),
            max_cell_len,
            truncation_hint: None,
        };
        crate::toon::ToonDocument::validate(&opts).expect("test render() shim: options must validate").render()
    }

    /// One string cell `content` under field `a`, rendered with `max_cell_len` and an optional truncation hint.
    fn render_cell(content: &str, max_cell_len: usize, hint: Option<&str>) -> String {
        let mut opts = crate::toon::ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from(content)]])
            .max_cell_len(max_cell_len);
        if let Some(hint) = hint {
            opts = opts.truncation_hint(hint);
        }
        crate::toon::render_toon(&opts).expect("one-cell document validates")
    }

    #[test]
    fn from_str_slice() {
        let v: Value = "hello".into();
        assert_eq!(v, Value::Str(CompactString::new("hello")));
    }

    #[test]
    fn from_i64() {
        let v: Value = 42i64.into();
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn non_finite_floats_render_as_null() {
        // TOON 4.4 §3: NaN and ±Infinity normalize to null.
        for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let out = render("t", &["v".to_string()], &[vec![Value::Float(f)]], None, &[], 200);
            assert_eq!(out, "t[1]{v}:\n  null", "{f}");
        }
    }

    #[test]
    fn display_matches_the_unquoted_token() {
        assert_eq!(Value::Null.to_string(), "null");
        assert_eq!(Value::Float(-0.0).to_string(), "0");
        assert_eq!(Value::Float(f64::NAN).to_string(), "null");
        assert_eq!(Value::Float(1e21).to_string(), "1e+21");
        assert_eq!(Value::UInt(u64::MAX).to_string(), "18446744073709551615");
        assert_eq!(Value::from("a,b").to_string(), "a,b");
    }

    #[test]
    fn hint_with_newline_is_escaped() {
        let out = render("t", &[], &[], None, &["line1\nline2".to_string()], 200);
        // The hint is quoted and its LF written as `\n`, so it cannot inject a line.
        assert_eq!(out, "t: []\nhelp[1]: \"line1\\nline2\"");
        assert_eq!(out.lines().count(), 2, "hint newline must not inject a line, got: {out:?}");
    }

    #[test]
    fn ac011_truncated_cell_content_is_exact() {
        // No hint: " (317 more chars)" is 17 chars, so 183 are kept, and 183 + 317 = 500.
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(500))]], None, &[], 200);
        let expected_prefix = "a".repeat(183);
        assert_eq!(out, format!("t[1]{{a}}:\n  {expected_prefix} (317 more chars)"));
    }

    #[test]
    fn truncated_cell_without_hint_names_no_parameter() {
        let out = render_cell(&"a".repeat(500), 200, None);
        assert!(!out.contains("full=true"), "michi must not invent a hint, got: {out:?}");
        assert!(!out.contains("use"), "no hint means no `use` clause, got: {out:?}");
        assert!(!out.contains('\u{2014}'), "no hint means no em dash, got: {out:?}");
    }

    #[test]
    fn truncated_cell_carries_the_callers_hint() {
        // " (333 more chars — use full=true)" is 33 chars: 167 kept, and 167 + 333 = 500.
        let out = render_cell(&"a".repeat(500), 200, Some("full=true"));
        assert_eq!(out, format!("t[1]{{a}}:\n  {} (333 more chars — use full=true)", "a".repeat(167)));
        let out = render_cell(&"a".repeat(500), 200, Some("--verbose"));
        assert_eq!(out, format!("t[1]{{a}}:\n  {} (333 more chars — use --verbose)", "a".repeat(167)));
    }

    #[test]
    fn spec_02_truncated_field_value_example_is_real_output() {
        let desc = |kept: &str, total: usize| format!("{kept}{}", "x".repeat(total - kept.chars().count()));
        let opts = crate::toon::ToonOptions::new(
            "components",
            vec!["name".to_string(), "description".to_string(), "tokens".to_string()],
            vec![
                vec![Value::from("Button"), Value::from(desc("Primary action element", 170)), Value::Int(12)],
                vec![Value::from("Icon"), Value::from(desc("Scalable vector glyphs", 225)), Value::Int(8)],
            ],
        )
        .total_count(Some(84))
        .hints(vec!["Call get_component with name=<name> and full=true for complete description".to_string()])
        .max_cell_len(55)
        .truncation_hint("full=true");
        assert_eq!(
            crate::toon::render_toon(&opts).unwrap(),
            "components[2]{name,description,tokens}:\n  \
             Button,Primary action element (148 more chars — use full=true),12\n  \
             Icon,Scalable vector glyphs (203 more chars — use full=true),8\n\
             totalCount: 84\n\
             help[1]: Call get_component with name=<name> and full=true for complete description"
        );
    }

    #[test]
    fn empty_truncation_hint_is_no_hint() {
        assert_eq!(render_cell(&"a".repeat(500), 200, Some("")), render_cell(&"a".repeat(500), 200, None));
    }

    #[test]
    fn ac011_truncated_cell_escapes_newlines_in_kept_prefix() {
        // Truncation runs on the cell value (keeping 183 chars = "a\n" x 91 + "a", 417 removed),
        // then the whole truncated cell is quoted and each LF written as `\n`.
        let content = "a\n".repeat(300);
        assert_eq!(content.chars().count(), 600);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content)]], None, &[], 200);
        let expected_prefix = format!("{}a", "a\\n".repeat(91));
        assert_eq!(out, format!("t[1]{{a}}:\n  \"{expected_prefix} (417 more chars)\""));
        assert_eq!(out.lines().count(), 2, "no line break survives in the cell, got: {out:?}");
        let cell = out.strip_prefix("t[1]{a}:\n  ").unwrap();
        // The cap counts chars before escaping: the escaped, quoted cell is longer than max_cell_len.
        assert_eq!(cell.chars().count(), 293, "91 * 3 + 1 escaped prefix + 17 suffix + 2 quotes");
    }

    #[test]
    fn ac011b_max_cell_len_bounds_chars_not_bytes() {
        let content = "日".repeat(100);
        assert_eq!(content.chars().count(), 100);
        assert_eq!(content.len(), 300);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content.clone())]], None, &[], 200);
        assert_eq!(
            out,
            format!("t[1]{{a}}:\n  {content}"),
            "a 300-byte/100-char cell must not truncate at max_cell_len=200"
        );
    }

    #[test]
    fn ac011c_max_cell_len_smaller_than_suffix_falls_back_to_ellipsis() {
        // " (49 more chars)" plus one kept char needs 17 > 10: 9 kept chars and an ellipsis, no suffix fragment.
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(50))]], None, &[], 10);
        assert_eq!(out, "t[1]{a}:\n  aaaaaaaaa…");
        let out = render_cell(&"a".repeat(50), 1, Some("full=true"));
        assert_eq!(out, "t[1]{a}:\n  …");
    }

    #[test]
    fn ac008_render_level_comma_cell_is_quoted_when_untruncated() {
        let out = render("t", &["a".to_string()], &[vec![Value::from("x,y")]], None, &[], 200);
        assert_eq!(out, "t[1]{a}:\n  \"x,y\"");
    }

    #[test]
    fn ac009_render_level_quote_cell_is_wrapped_and_escaped_when_untruncated() {
        let out = render("t", &["a".to_string()], &[vec![Value::from(r#"say "hi""#)]], None, &[], 200);
        assert_eq!(out, "t[1]{a}:\n  \"say \\\"hi\\\"\"");
    }

    #[test]
    fn ac008b_ac011d_truncated_cell_with_comma_in_kept_prefix_is_quoted() {
        let content = "a,".repeat(300);
        assert_eq!(content.chars().count(), 600);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content)]], None, &[], 200);
        let expected_kept: String = "a,".repeat(300).chars().take(183).collect();
        let expected = format!("t[1]{{a}}:\n  \"{expected_kept} (417 more chars)\"");
        assert_eq!(out, expected);
        let cell = out.strip_prefix("t[1]{a}:\n  ").unwrap();
        assert_eq!(cell.chars().count(), 202);
        assert!(cell.starts_with('"') && cell.ends_with('"'));
    }

    #[test]
    fn ac012a_full_render_with_default_max_cell_len() {
        // Constructed via ToonOptions::new + render_toon (not super::render with a
        // literal 200) so a regression to ToonOptions::new's actual default is caught.
        let opts = crate::toon::ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(500))]]);
        let out = crate::toon::render_toon(&opts).unwrap();
        let expected_prefix = "a".repeat(183);
        assert_eq!(out, format!("t[1]{{a}}:\n  {expected_prefix} (317 more chars)"));
    }

    #[test]
    fn ac012c_toon_options_new_defaults() {
        let opts = crate::toon::ToonOptions::new("t", vec![], vec![]);
        assert_eq!(opts.total_count, None);
        assert!(opts.hints.is_empty());
        assert_eq!(opts.max_cell_len, 200);
        assert_eq!(opts.truncation_hint, None);
    }

    #[test]
    fn ac012b_untruncated_cell_at_150_chars_near_boundary() {
        let content = "a".repeat(150);
        let opts = crate::toon::ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from(content.clone())]]);
        let out = crate::toon::render_toon(&opts).unwrap();
        assert_eq!(out, format!("t[1]{{a}}:\n  {content}"));
        assert!(!out.contains("more chars"));
    }

    #[test]
    fn ac021_leading_space_in_cell_content_is_quoted() {
        let out = render("t", &["a".to_string()], &[vec![Value::from(" x")]], None, &[], 200);
        assert_eq!(out, "t[1]{a}:\n  \" x\"");
    }

    #[test]
    fn ac023_leading_space_in_hint_is_quoted() {
        let out = render("t", &[], &[], None, &[" h".to_string()], 200);
        assert_eq!(out, "t: []\nhelp[1]: \" h\"");
    }

    #[test]
    fn ac013_u64_and_usize_above_i64_max_are_exact_unsigned_numbers() {
        let v: Value = u64::MAX.into();
        assert_eq!(v, Value::UInt(u64::MAX));
        let v: Value = usize::MAX.into();
        assert_eq!(v, Value::UInt(usize::MAX as u64));
        let v: Value = (i64::MAX as u64).into();
        assert_eq!(v, Value::Int(i64::MAX));
        let v: Value = (i64::MAX as u64 + 1).into();
        assert_eq!(v, Value::UInt(i64::MAX as u64 + 1));
        let out = render("t", &["n".to_string()], &[vec![u64::MAX.into()]], None, &[], 200);
        assert_eq!(out, "t[1]{n}:\n  18446744073709551615");
    }

    #[test]
    fn ac013_numeric_cells_ignore_max_cell_len() {
        let fields = ["n".to_string(), "m".to_string()];
        let out = render("t", &fields, &[vec![u64::MAX.into(), Value::Int(i64::MIN)]], None, &[], 5);
        assert_eq!(out, "t[1]{n,m}:\n  18446744073709551615,-9223372036854775808");
    }

    #[test]
    fn ac014_u64_and_usize_within_range_render_exact_digits() {
        let v: Value = 42u64.into();
        assert_eq!(v, Value::Int(42));
        let v: Value = 42usize.into();
        assert_eq!(v, Value::Int(42));
        let out = render("t", &["n".to_string()], &[vec![42u64.into()]], None, &[], 200);
        assert_eq!(out, "t[1]{n}:\n  42");
    }

    #[test]
    fn ac023_empty_hints_produce_no_help_line() {
        let out = render("t", &[], &[], None, &[], 200);
        assert_eq!(out, "t: []");
    }

    #[test]
    fn ac023_reserved_type_names_are_rejected_but_allowed_as_field_names() {
        for name in ["help", "totalCount", "recovery"] {
            let opts = crate::toon::ToonOptions::new(name, vec![], vec![]);
            let err = crate::toon::render_toon(&opts).unwrap_err();
            assert_eq!(err, crate::toon::ToonError::InvalidTypeName { name: name.to_string() });
        }
        let out = render(
            "t",
            &["help".to_string(), "totalCount".to_string()],
            &[vec![Value::Int(1), Value::Int(2)]],
            None,
            &[],
            200,
        );
        assert_eq!(out, "t[1]{help,totalCount}:\n  1,2");
    }

    #[test]
    fn ac022_type_name_with_a_colon_is_quoted_not_rejected() {
        let out = render("totalCount:", &[], &[], None, &[], 200);
        assert_eq!(out, "\"totalCount:\": []");
    }

    #[test]
    fn ac043_truncation_suffix_format_is_exact() {
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(500))]], None, &[], 200);
        assert!(out.ends_with(" (317 more chars)"), "got: {out:?}");
        let out = render_cell(&"a".repeat(500), 200, Some("full=true"));
        assert!(out.ends_with(" (333 more chars — use full=true)"), "got: {out:?}");
        assert!(out.contains('\u{2014}'), "must use EM DASH U+2014, got: {out:?}");
        assert!(!out.contains('-'), "must not contain a hyphen anywhere in the suffix, got: {out:?}");
    }

    #[test]
    fn ac044_kept_prefix_length_equals_max_cell_len_minus_suffix_chars() {
        let suffix = " (317 more chars)";
        let suffix_chars = suffix.chars().count();
        let max_cell_len = 200usize;
        let expected_kept = max_cell_len - suffix_chars;
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(500))]], None, &[], max_cell_len);
        let cell = out.strip_prefix("t[1]{a}:\n  ").unwrap();
        assert_eq!(cell.chars().count(), max_cell_len, "total cell content must be exactly max_cell_len chars");
        let kept: String = cell.chars().take_while(|c| *c == 'a').collect();
        assert_eq!(kept.chars().count(), expected_kept);

        // Ellipsis fallback (per AC-011c): the suffix does not fit with one kept char, so max_cell_len - 1 chars are
        // kept and the cell ends with `…`, still exactly max_cell_len chars.
        let out10 = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(50))]], None, &[], 10);
        let cell10 = out10.strip_prefix("t[1]{a}:\n  ").unwrap();
        assert_eq!(cell10, format!("{}…", "a".repeat(9)));
        assert_eq!(cell10.chars().count(), 10);
    }

    #[test]
    fn ac036_from_option_string_some_matches_str_none_matches_null() {
        let some_via_option: Value = Some("x".to_string()).into();
        assert_eq!(some_via_option, Value::from("x"));
        let none_via_option: Value = None::<String>.into();
        assert_eq!(none_via_option, Value::Null);

        let out_some = render(
            "t",
            &["a".to_string(), "b".to_string()],
            &[vec![some_via_option, Value::from("y")]],
            None,
            &[],
            200,
        );
        let out_str = render(
            "t",
            &["a".to_string(), "b".to_string()],
            &[vec![Value::from("x"), Value::from("y")]],
            None,
            &[],
            200,
        );
        assert_eq!(out_some, out_str);

        let out_none = render(
            "t",
            &["a".to_string(), "b".to_string()],
            &[vec![none_via_option, Value::from("y")]],
            None,
            &[],
            200,
        );
        let out_null =
            render("t", &["a".to_string(), "b".to_string()], &[vec![Value::Null, Value::from("y")]], None, &[], 200);
        assert_eq!(out_none, out_null);
        assert_eq!(out_null, "t[1]{a,b}:\n  null,y");
    }
}

/// Text-based scanning helpers shared by this crate's structural guard
/// tests. `pub(crate)` (not private) so both `render`'s own guard tests
/// and `lib.rs`'s (e.g. the AC-013 docs guard) can reuse the same,
/// independently-hardened implementation instead of each maintaining its
/// own copy that can drift out of sync or regress individually.
#[cfg(test)]
pub(crate) mod test_scan {
    /// Finds the byte offset, within `src`, of the `{` that opens
    /// `lines[open_line_idx]`'s block -- the first `{` on or after that
    /// line's start.
    fn brace_byte_offset(lines: &[&str], open_line_idx: usize) -> usize {
        let line_start: usize = lines[..open_line_idx].iter().map(|l| l.len() + 1).sum();
        line_start + lines[open_line_idx].find('{').expect("opening line must contain '{'")
    }

    /// Finds the real nesting depth's matching `}` for the `{` at
    /// `open_brace_idx` in `src`, and returns the index of the LINE
    /// containing that `}` (via a byte-offset -> line-index scan over
    /// `lines`).
    ///
    /// Tracks actual lexical structure -- string literals (including ones
    /// that span multiple lines, e.g. inside a `#[doc = "..."]` value),
    /// raw strings with any number of `#`s, char literals (including
    /// `'\u{XXXX}'` escapes), and `//`/`/* */` comments -- rather than
    /// assuming a block's closing brace is always alone on its own
    /// rustfmt-normalized line. That assumption is false: rustfmt does not
    /// reformat the CONTENTS of a string literal, so a multi-line string
    /// (a pasted JSON/TOON example inside a doc comment, for instance)
    /// containing a bare `}` at column 0 previously fooled the
    /// line-alone-ness check into treating it as the block's real close,
    /// silently truncating everything scanned after it -- proven by
    /// exit-gate review, which used exactly that shape to hide a second
    /// constructor inside what this scan believed was `proof`'s full body.
    pub(crate) fn find_closing_line(src: &str, lines: &[&str], open_line_idx: usize, indent: usize) -> usize {
        let _ = indent; // kept in the signature for call-site stability; no longer load-bearing
        let open_brace_idx = brace_byte_offset(lines, open_line_idx);

        #[derive(Clone, Copy, PartialEq)]
        enum State {
            Normal,
            InString,
            InRawString(usize),
            InLineComment,
            InBlockComment,
        }

        let chars: Vec<char> = src[open_brace_idx..].char_indices().map(|(_, c)| c).collect();
        let byte_of: Vec<usize> = src[open_brace_idx..].char_indices().map(|(i, _)| i).collect();
        let mut state = State::Normal;
        let mut depth = 0i32;
        let mut i = 0usize;
        while i < chars.len() {
            let c = chars[i];
            match state {
                State::Normal => {
                    if c == '"' {
                        state = State::InString;
                        i += 1;
                    } else if c == 'r' && matches!(chars.get(i + 1), Some('"') | Some('#')) {
                        let mut hashes = 0usize;
                        let mut j = i + 1;
                        while chars.get(j) == Some(&'#') {
                            hashes += 1;
                            j += 1;
                        }
                        if chars.get(j) == Some(&'"') {
                            state = State::InRawString(hashes);
                            i = j + 1;
                        } else {
                            i += 1;
                        }
                    } else if c == '\'' {
                        // A char literal, not a lifetime, only if a closing
                        // `'` is found either 2 chars later (plain, e.g.
                        // '{') or after a `\...` escape (e.g. '\\', '\'',
                        // '\u{XXXX}'). A lifetime ('a, 'static) never has a
                        // `'` two-or-few characters later in valid syntax.
                        if chars.get(i + 1) == Some(&'\\') {
                            if chars.get(i + 2) == Some(&'u') && chars.get(i + 3) == Some(&'{') {
                                let close = (i + 4..chars.len())
                                    .find(|&k| chars[k] == '}')
                                    .expect("unclosed \\u{ escape in char literal");
                                i = close + 2; // skip past '}' and the closing '
                            } else {
                                i += 4; // ' \ x '
                            }
                        } else if chars.get(i + 2) == Some(&'\'') {
                            i += 3; // ' x '
                        } else {
                            i += 1; // a lifetime; not a char literal
                        }
                    } else if c == '/' && chars.get(i + 1) == Some(&'/') {
                        state = State::InLineComment;
                        i += 2;
                    } else if c == '/' && chars.get(i + 1) == Some(&'*') {
                        state = State::InBlockComment;
                        i += 2;
                    } else if c == '{' {
                        depth += 1;
                        i += 1;
                    } else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            let close_byte = open_brace_idx + byte_of[i];
                            return src[..close_byte].matches('\n').count();
                        }
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
                State::InString => {
                    if c == '\\' {
                        i += 2;
                    } else if c == '"' {
                        state = State::Normal;
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
                State::InRawString(hashes) => {
                    if c == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                        state = State::Normal;
                        i += 1 + hashes;
                    } else {
                        i += 1;
                    }
                }
                State::InLineComment => {
                    if c == '\n' {
                        state = State::Normal;
                    }
                    i += 1;
                }
                State::InBlockComment => {
                    if c == '*' && chars.get(i + 1) == Some(&'/') {
                        state = State::Normal;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        panic!("no matching closing brace found for the block opened at line {open_line_idx}");
    }

    pub(crate) fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Strips every `#[cfg(test)]`/`#[cfg(all(test, ...))]`-gated
    /// `mod ... { ... }` block from `src`, not via a first-occurrence
    /// prefix split -- a prefix split silently stops scanning at the first
    /// test module even if real (non-test) code follows it later in the
    /// file.
    ///
    /// `is_test_cfg_attr` requires `test` as the leading, positive conjunct
    /// of an `all(...)` -- `#[cfg(all(test, ...))]` -- rather than merely
    /// checking whether the attribute's text contains the substring
    /// `"test"` anywhere. A substring check is a real, previously-shipped
    /// bug: `#[cfg(all(not(test), ...))]` also contains `"test"`, so it
    /// would be misclassified as test-only and stripped from this scan,
    /// even though rustc compiles it into every *non*-test build -- the
    /// opposite of test-only. Any other `#[cfg(` line whose text contains
    /// `"test"` and isn't one of the two recognized positive forms trips
    /// the assertion below instead of being silently mishandled in either
    /// direction.
    pub(crate) fn strip_test_modules(src: &str) -> String {
        fn is_recognized_test_cfg_attr(trimmed: &str) -> bool {
            trimmed == "#[cfg(test)]"
                || trimmed.starts_with("#[cfg(all(test,")
                || trimmed.starts_with("#[cfg(all(test)")
        }

        let lines: Vec<&str> = src.lines().collect();
        let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
        let mut i = 0usize;
        while i < lines.len() {
            let trimmed = lines[i].trim_start();
            if trimmed.starts_with("#[cfg(") && trimmed.contains("test") {
                assert!(
                    is_recognized_test_cfg_attr(trimmed),
                    "unrecognized cfg attribute mentioning \"test\": {trimmed:?} -- add it to \
                     is_recognized_test_cfg_attr only if it is genuinely test-only; a `not(test)` \
                     predicate must NOT be treated as test-only"
                );
                let mod_line = i + 1;
                assert!(
                    lines[mod_line].trim_start().contains("mod ") && lines[mod_line].ends_with('{'),
                    "expected a `[pub(...)] mod ... {{` line immediately after a test cfg attribute, got: {:?}",
                    lines[mod_line]
                );
                let close = find_closing_line(src, &lines, mod_line, indent_of(lines[i]));
                i = close + 1;
            } else {
                kept.push(lines[i]);
                i += 1;
            }
        }
        kept.join("\n")
    }
}

#[cfg(test)]
mod invariant_guard_tests {
    use super::test_scan::{find_closing_line, indent_of, strip_test_modules};

    #[test]
    fn sole_constructor_for_toon_document() {
        // ToonDocument's private field is declared inside `mod proof`, a
        // module containing nothing else. Rust's own field-privacy rule --
        // visible only to the declaring module and its descendants -- is
        // what actually prevents every bypass shape three rounds of
        // adversarial review found (a free fn, a second impl block, a
        // From/TryFrom impl, a type alias, and a module gated behind an
        // unanticipated #[cfg(...)]): none of them can compile unless
        // they're written inside `proof` itself, regardless of what shape
        // they take. That guarantee doesn't need a text scan; rustc enforces
        // it the moment anyone tries. What a text scan still needs to pin
        // down is the two things privacy alone doesn't cover: that `proof`
        // stays exactly as narrow as this reasoning requires (nothing else
        // declared inside it, no descendant module of its own), and that no
        // extra method sneaks into ToonDocument's own impl block from
        // within its rightful scope (proven possible in round 3: a
        // `pub fn rebind(&self, opts) -> ToonDocument` inside the impl
        // block itself evaded a fn count that filtered out every
        // self-taking method).
        //
        // This test cannot see through macro_rules! expansion -- an item-
        // position macro invocation inside proof's impl block expands to
        // real code with real field access, but include_str! (which this
        // test and strip_test_modules both read from) captures source
        // exactly as written, before expansion. See `proof`'s own doc
        // comment for why this is an accepted, documented scope boundary
        // rather than a gap this test is meant to close.
        let render_src = strip_test_modules(include_str!("render.rs"));

        let lines: Vec<&str> = render_src.lines().collect();
        let mod_line = lines
            .iter()
            .position(|l| l.trim_end() == "mod proof {")
            .expect("ToonDocument must live in a private (non-pub) `mod proof` in render.rs");
        let close = find_closing_line(&render_src, &lines, mod_line, indent_of(lines[mod_line]));
        let proof_body = lines[mod_line + 1..close].join("\n");

        assert_eq!(
            proof_body.matches("mod ").count(),
            0,
            "proof must declare no descendant module -- a descendant would inherit access to the private field"
        );
        // Exactly two fns may exist anywhere in `proof`: validate and
        // render. This is a total count over every `fn`, with no
        // self-receiver exemption -- the exemption is exactly what let
        // `rebind` (a &self method) through in round 3.
        let fn_names: Vec<&str> =
            proof_body.split("fn ").skip(1).map(|rest| rest.split(['(', '<']).next().unwrap_or(rest).trim()).collect();
        assert_eq!(
            fn_names,
            vec!["validate", "render"],
            "proof's impl block must contain exactly these two fns, in this order; found: {fn_names:?}"
        );
        // fn_names only sees items introduced by the literal token `fn `.
        // A `const`/`static` item, or a function-pointer-typed const
        // (`fn(...)  ->  ToonDocument`, no space before the paren) can
        // produce a ToonDocument without ever containing that token --
        // proven by exit-gate review: `pub const MAKE: for<'x> fn(&'x
        // ToonOptions) -> ToonDocument<'x> = |o| ToonDocument { opts: o };`
        // compiled, passed every check above, and rendered unvalidated
        // data through a crate-internal caller.
        for banned in ["const ", "static ", "fn("] {
            assert_eq!(
                proof_body.matches(banned).count(),
                0,
                "proof may declare no {banned:?} item -- a const/static/fn-pointer can produce \
                 a ToonDocument without ever introducing a `fn ` token for the fn-name check above to see"
            );
        }

        // Module privacy only closes external bypass shapes AS LONG AS the
        // field itself stays bare-private (no `pub`/`pub(in ...)`/
        // `pub(crate)` etc.) -- widening it to `pub(in crate::toon::render)` is a
        // one-token edit that grants field access to all of render.rs,
        // letting a constructor live entirely outside proof while every
        // other assertion here stays green (proven by exit-gate review).
        assert!(
            proof_body.contains("\n        opts: &'a crate::toon::ToonOptions,\n"),
            "ToonDocument's field must stay bare-private (no visibility modifier); \
             found a different declaration in proof_body: {proof_body:?}"
        );
        assert_eq!(
            render_src.matches("pub(in ").count(),
            0,
            "no item in render.rs may use a `pub(in ...)` visibility -- it can widen field access \
             beyond proof without tripping any other check here"
        );

        assert!(
            render_src.contains("\npub use proof::ToonDocument;"),
            "ToonDocument must be re-exported from proof via `pub use`, not by widening proof's own visibility"
        );
        // Defense in depth: pin what immediately follows proof's own
        // closing brace, so even an unanticipated future defect in the
        // string/comment-aware scan above that find_closing_line performs
        // fails loudly here instead of silently accepting a truncated or
        // extended proof_body.
        assert_eq!(
            lines[close + 1],
            "pub use proof::ToonDocument;",
            "the line immediately after proof's closing brace must be its pub use re-export"
        );

        let lib_src = strip_test_modules(include_str!("mod.rs"));
        let escape_src = strip_test_modules(include_str!("escape.rs"));
        for (file, src) in
            [("render.rs", render_src.as_str()), ("lib.rs", lib_src.as_str()), ("escape.rs", escape_src.as_str())]
        {
            let scan = src.to_lowercase().replace("unsafe_code", "");
            for bad in ["unchecked", "unsafe", "raw", "assume"] {
                assert!(!scan.contains(bad), "escape-hatch name pattern {bad:?} must not appear in {file}");
            }
        }
    }

    #[test]
    fn toon_document_render_produces_expected_output() {
        let opts = crate::toon::ToonOptions::new(
            "t",
            vec!["a".to_string(), "b".to_string()],
            vec![vec![crate::toon::Value::from("x"), crate::toon::Value::Int(1)]],
        )
        .total_count(Some(5))
        .hints(vec!["h".to_string()]);
        let doc = super::ToonDocument::validate(&opts).expect("valid options must validate");
        assert_eq!(doc.render(), "t[1]{a,b}:\n  x,1\ntotalCount: 5\nhelp[1]: h");
    }

    #[test]
    fn no_loose_parameter_render_fn() {
        let src = include_str!("render.rs");
        assert!(
            !src.contains("fields: &[String],\n    rows: &[Vec<Value>],"),
            "the loose-parameter render(type_name, fields, rows, ...) function must not exist"
        );
    }

    #[test]
    fn no_profile_dependent_behavior() {
        let non_test = strip_test_modules(include_str!("render.rs"));
        let debug_assert_needle = ["debug", "_", "assert", "!"].concat();
        let debug_assert_eq_needle = ["debug", "_", "assert", "_", "eq", "!"].concat();
        let debug_assert_ne_needle = ["debug", "_", "assert", "_", "ne", "!"].concat();
        let cfg_debug_needle = ["cfg", "(", "debug_assertions", ")"].concat();
        for token in [&debug_assert_needle, &debug_assert_eq_needle, &debug_assert_ne_needle, &cfg_debug_needle] {
            assert!(!non_test.contains(token.as_str()), "found profile-dependent token {token:?} in render.rs");
        }
    }

    #[test]
    fn render_has_no_missing_cell_path() {
        let non_test = strip_test_modules(include_str!("render.rs"));
        let row_get_needle = ["row", ".", "get", "("].concat();
        assert!(
            !non_test.contains(&row_get_needle),
            "ToonDocument::render must iterate cells directly, not via row.get(i)"
        );
    }
}

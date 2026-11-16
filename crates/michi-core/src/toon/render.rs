use compact_str::CompactString;

use super::escape::{escape_key, escape_value, push_f64};

/// A single cell value in a TOON row.
///
/// Rendering follows TOON 4.4: strings are quoted only when §7.2 requires it, numbers use the canonical form of §2,
/// and `Null` is the `null` literal.
///
/// `#[non_exhaustive]`: new value kinds may be added in a minor release, so a `match` outside michi needs a wildcard
/// arm.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Value {
    /// UTF-8 string (stack-inlined for strings <= 24 bytes via `CompactString`). The only variant subject to
    /// `max_cell_len` truncation.
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    Str(CompactString),
    /// Signed integer.
    Int(#[cfg_attr(feature = "schemars", schemars(schema_with = "crate::int_schema::i64"))] i64),
    /// Unsigned integer, used by `From<u64>`/`From<usize>` (and `toon::list()`) for values above `i64::MAX`, so they
    /// stay exact unquoted numbers instead of being clamped, rounded or stringified.
    UInt(#[cfg_attr(feature = "schemars", schemars(schema_with = "crate::int_schema::u64"))] u64),
    /// Floating-point number. NaN and ±Infinity render as `null` (TOON 4.4 §3).
    ///
    /// With the `serde` feature, `Value` itself serializes a non-finite float as the string `"NaN"`, `"Infinity"` or
    /// `"-Infinity"` (finite values stay JSON numbers) and accepts those strings back, so it round-trips exactly. This
    /// is the same wire form as [`crate::kv::KvValue::Float`].
    Float(
        #[cfg_attr(feature = "serde", serde(with = "crate::float_wire"))]
        #[cfg_attr(feature = "schemars", schemars(schema_with = "crate::float_wire::schema"))]
        f64,
    ),
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
    /// Keeps the f32's own shortest digits: `0.1f32` becomes `0.1`, not its exact widened value
    /// `0.10000000149011612`. The f32 is printed with its shortest round-trip form and that decimal is read back as
    /// an f64.
    fn from(f: f32) -> Self {
        if !f.is_finite() {
            return Self::Float(f64::from(f));
        }
        let mut digits = String::with_capacity(16);
        let _ = std::fmt::Write::write_fmt(&mut digits, format_args!("{f}"));
        Self::Float(digits.parse().unwrap_or(f64::from(f)))
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

/// Cut `s` to at most `max_cell_len` chars: `max_cell_len - 1` kept chars and a plain `…`, or the empty string for a
/// cap of 0. Returns the cell and the number of chars cut off, or `None` when `s` fits. The cell never carries a
/// message: the cut is reported in the `truncated` block (see [`super::table::push_truncated`]).
fn cut_cell(s: &str, max_cell_len: usize) -> Option<(String, usize)> {
    // Byte length is always >= char count, so a string that fits in bytes fits in chars: skip the UTF-8 scan.
    if s.len() <= max_cell_len {
        return None;
    }
    let total = s.chars().count();
    if total <= max_cell_len {
        return None;
    }
    let keep = max_cell_len.saturating_sub(1);
    let end = s.char_indices().nth(keep).map_or(s.len(), |(i, _)| i);
    let mut cell = String::with_capacity(end + 3);
    cell.push_str(&s[..end]);
    if max_cell_len > 0 {
        cell.push('\u{2026}');
    }
    Some((cell, total - keep))
}

/// Append one row cell: §7.2 quoting for strings, §2/§3 canonical numbers, the `null` literal. Only strings are cut
/// to `max_cell_len` chars, counted before escaping. Returns the chars cut off a string, `None` when the cell is whole.
fn push_cell(out: &mut String, cell: &Value, max_cell_len: usize) -> Option<usize> {
    use std::fmt::Write as _;
    match cell {
        Value::Str(s) => {
            if let Some((cut, removed)) = cut_cell(s, max_cell_len) {
                out.push_str(&escape_value(&cut));
                return Some(removed);
            }
            out.push_str(&escape_value(s));
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
    None
}

/// Append the list entry in the form TOON 4.4 prescribes for its shape, each line ending in a newline:
/// - no rows: `key: []` (§9.1; the field list is not emitted, an empty array has no shape),
/// - rows with zero fields: `key[N]:` and N bare `-` items, since empty objects may not use tabular form
///   (§9.3, §9.4, §10),
/// - otherwise the tabular form `key[N]{f1,f2}:` with one row per line at depth 1 (§9.3).
///
/// Returns the cells that were cut, as `(row, field index, chars removed)`.
fn push_list(out: &mut String, opts: &crate::toon::ListRef<'_>) -> Vec<(usize, usize, usize)> {
    use std::fmt::Write as _;
    let mut cut = Vec::new();
    if opts.rows.is_empty() {
        out.push_str(&escape_key(opts.type_name));
        out.push_str(": []\n");
        return cut;
    }
    if opts.fields.is_empty() {
        out.push_str(&escape_key(opts.type_name));
        let _ = writeln!(out, "[{}]:", opts.rows.len());
        for _ in opts.rows {
            out.push_str("  -\n");
        }
        return cut;
    }
    super::table::push_header(out, opts.type_name, opts.rows.len(), opts.fields);
    out.push('\n');
    for (r, row) in opts.rows.iter().enumerate() {
        out.push_str("  ");
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            if let Some(removed) = push_cell(out, cell, opts.max_cell_len) {
                cut.push((r, i, removed));
            }
        }
        out.push('\n');
    }
    cut
}

/// What [`ToonDocument::render_list`] produces: the list entry and what it cut, before the blocks that follow a list.
pub(crate) struct RenderedList {
    /// The list entry, with no trailing newline.
    pub(crate) text: String,
    /// Cut cells as `(row, field index, chars removed)`.
    pub(crate) cuts: Vec<(usize, usize, usize)>,
    /// The `help[]` line michi adds when it cut a cell and the caller set a truncation hint.
    pub(crate) extra_hint: Option<String>,
}

/// Holds [`ToonDocument`] alone, so its private field is visible only here and `validate` is the one way to build
/// one. The compiler enforces that; `tests/ui-fail/toon_document_no_field_construction.rs` and
/// `toon_document_no_new_constructor.rs` prove it from outside the crate.
mod proof {
    use crate::toon::ListRef;

    /// Proof that a `ToonOptions` value has passed structural validation.
    ///
    /// The only functions that produce a `ToonDocument` are [`ToonDocument::validate`] and the crate-internal
    /// `validate_list` it calls -- see this module's own doc comment for why no other function anywhere in the crate
    /// can construct one. It borrows the list immutably, so the validated rows cannot be mutated while the proof is
    /// live.
    #[derive(Debug)]
    pub struct ToonDocument<'a> {
        list: ListRef<'a>,
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
        /// reserved root key (see that variant),
        /// [`crate::toon::ToonError::TooManyFields`] past
        /// [`crate::toon::MAX_FIELDS`] fields,
        /// [`crate::toon::ToonError::InvalidFieldName`] if a field name appears
        /// more than once, [`crate::toon::ToonError::TooManyCells`] if
        /// `rows × fields` (at least one cell per row) is past [`crate::toon::MAX_CELLS`], and
        /// [`crate::toon::ToonError::RowLengthMismatch`] if any row's length
        /// differs from `fields.len()`. Every check is linear in the input.
        pub fn validate(opts: &'a crate::toon::ToonOptions) -> Result<Self, crate::toon::ToonError> {
            Self::validate_list(opts.list_ref())
        }

        /// [`validate`](Self::validate) for a borrowed list, which also checks the truncation hint
        /// ([`crate::toon::ToonError::TruncationHintTooLong`], after the cell cap and before row arity).
        pub(crate) fn validate_list(list: ListRef<'a>) -> Result<Self, crate::toon::ToonError> {
            use crate::toon::{ToonError, MAX_CELLS, MAX_FIELDS, MAX_TRUNCATION_HINT_CHARS};
            if crate::toon::is_reserved_root_key(list.type_name) {
                return Err(ToonError::InvalidTypeName { name: list.type_name.to_string() });
            }
            if list.fields.len() > MAX_FIELDS {
                return Err(ToonError::TooManyFields { count: list.fields.len(), max: MAX_FIELDS });
            }
            let mut seen = std::collections::HashSet::with_capacity(list.fields.len());
            for field in list.fields {
                if !seen.insert(field.as_str()) {
                    return Err(ToonError::InvalidFieldName { name: field.clone() });
                }
            }
            if crate::toon::cell_count(list.rows.len(), list.fields.len()) > MAX_CELLS {
                return Err(ToonError::TooManyCells {
                    rows: list.rows.len(),
                    fields: list.fields.len(),
                    max: MAX_CELLS,
                });
            }
            if let Some(hint) = list.truncation_hint {
                // A hint of at most MAX bytes has at most MAX chars, so only a longer one needs counting.
                if hint.len() > MAX_TRUNCATION_HINT_CHARS {
                    let len = hint.chars().count();
                    if len > MAX_TRUNCATION_HINT_CHARS {
                        return Err(ToonError::TruncationHintTooLong { len, max: MAX_TRUNCATION_HINT_CHARS });
                    }
                }
            }
            for (i, row) in list.rows.iter().enumerate() {
                if row.len() != list.fields.len() {
                    return Err(ToonError::RowLengthMismatch {
                        row_index: i,
                        expected: list.fields.len(),
                        actual: row.len(),
                    });
                }
            }
            Ok(Self { list })
        }

        /// Render `self` to a TOON 4.4 list: the list entry, then the `truncated[N]{row,field,removed}:` block when a
        /// cell was cut to `max_cell_len`, with no trailing newline (§12). Infallible and total by construction: row
        /// arity was already proven equal to `fields.len()` by `validate`, so rows and fields are iterated in
        /// lockstep instead of indexed by position, which could fail.
        ///
        /// Cut cells end in `…`. `totalCount`, `help[]` and the truncation hint are not written here:
        /// [`AgentResponse`](crate::AgentResponse) owns them for the whole response.
        #[must_use]
        pub fn render(&self) -> String {
            let list = self.render_list();
            let mut out = list.text;
            let cuts: Vec<crate::toon::table::Cut<'_>> = list
                .cuts
                .iter()
                .map(|&(row, field, removed)| crate::toon::table::Cut {
                    section: self.list.type_name,
                    row,
                    field: self.list.fields.get(field).map_or("", String::as_str),
                    removed,
                })
                .collect();
            crate::toon::table::push_truncated(&mut out, &cuts, false);
            out
        }

        /// The list entry alone, the cells it cut and the `help[]` line michi adds when it cut a cell and the caller
        /// set a truncation hint. [`render`](Self::render) puts the `truncated` block after it;
        /// [`AgentResponse`](crate::AgentResponse) places that block, `totalCount` and `help` itself, so several
        /// sections share one of each.
        pub(crate) fn render_list(&self) -> super::RenderedList {
            let list = &self.list;
            let capacity = 60 + list.rows.len() * (list.fields.len() * 12 + 10);
            let mut text = String::with_capacity(capacity);
            let cuts = super::push_list(&mut text, list);
            if text.ends_with('\n') {
                text.pop();
            }
            let extra_hint = match list.truncation_hint {
                Some(hint) if !cuts.is_empty() && !hint.is_empty() => {
                    Some(format!("Cut cells are listed in the truncated block; use {hint} for the full value"))
                }
                _ => None,
            };
            super::RenderedList { text, cuts, extra_hint }
        }
    }
}
pub use proof::ToonDocument;

#[cfg(test)]
mod tests {
    use super::Value;
    use compact_str::CompactString;

    fn render(type_name: &str, fields: &[String], rows: &[Vec<Value>], max_cell_len: usize) -> String {
        let opts = crate::toon::ToonOptions {
            type_name: type_name.to_string(),
            fields: fields.to_vec(),
            rows: rows.to_vec(),
            max_cell_len,
        };
        crate::toon::ToonDocument::validate(&opts).expect("test render() shim: options must validate").render()
    }

    /// One string cell `content` under field `a`, rendered by a response with `max_cell_len` and an optional
    /// truncation hint: the hint is the response's, so the `help[]` line it adds appears here and not in `render`.
    fn render_cell(content: &str, max_cell_len: usize, hint: Option<&str>) -> String {
        let mut response = crate::AgentResponse::new("t")
            .truncate_cells_at(max_cell_len)
            .items(vec![vec![Value::from(content)]], &["a"]);
        if let Some(hint) = hint {
            response = response.truncation_hint(hint);
        }
        response.render(crate::OutputFormat::Text)
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
            let out = render("t", &["v".to_string()], &[vec![Value::Float(f)]], 200);
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
    fn ac011_truncated_cell_content_is_exact() {
        // 199 chars are kept and a plain `…` fills the 200-char cap; the other 301 are reported in the block.
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(500))]], 200);
        let expected_prefix = "a".repeat(199);
        assert_eq!(
            out,
            format!("t[1]{{a}}:\n  {expected_prefix}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,a,301")
        );
    }

    #[test]
    fn truncated_cell_without_hint_names_no_parameter() {
        let out = render_cell(&"a".repeat(500), 200, None);
        assert!(!out.contains("full=true"), "michi must not invent a hint, got: {out:?}");
        assert!(!out.contains("help"), "no hint means no help line, got: {out:?}");
        assert!(!out.contains("use"), "no hint means no instruction, got: {out:?}");
    }

    #[test]
    fn truncated_cell_carries_the_callers_hint_in_help() {
        let out = render_cell(&"a".repeat(500), 200, Some("full=true"));
        assert_eq!(
            out,
            format!(
                "t[1]{{a}}:\n  {}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,a,301\n\
                 help[1]: Cut cells are listed in the truncated block; use full=true for the full value",
                "a".repeat(199)
            )
        );
        let out = render_cell(&"a".repeat(500), 200, Some("--verbose"));
        assert!(out.ends_with("use --verbose for the full value"), "got: {out:?}");
    }

    #[test]
    fn spec_02_truncated_field_value_example_is_real_output() {
        use crate::{AgentResponse, OutputFormat, Section};
        let section = Section::toon(
            "components",
            vec!["name".to_string(), "description".to_string(), "tokens".to_string()],
            vec![
                vec![
                    Value::from("Button"),
                    Value::from("Primary action element used to submit forms and open dialogs across the app"),
                    Value::Int(12),
                ],
                vec![
                    Value::from("Icon"),
                    Value::from("Scalable vector glyphs that inherit colour and size from surrounding text"),
                    Value::Int(8),
                ],
            ],
        )
        .total_count(Some(84));
        let response = AgentResponse::new("components")
            .section(section)
            .hint("Call get_component with name=<name> and full=true for complete description")
            .truncate_cells_at(40)
            .truncation_hint("full=true");
        assert_eq!(
            response.render(OutputFormat::Text),
            "components[2]{name,description,tokens}:\n  \
             Button,Primary action element used to submit f…,12\n  \
             Icon,Scalable vector glyphs that inherit col…,8\n\
             truncated[2]{row,field,removed}:\n  \
             0,description,36\n  \
             1,description,34\n\
             totalCount: 84\n\
             help[2]: Call get_component with name=<name> and full=true for complete description,\
             Cut cells are listed in the truncated block; use full=true for the full value"
        );
    }

    #[test]
    fn empty_truncation_hint_is_no_hint() {
        assert_eq!(render_cell(&"a".repeat(500), 200, Some("")), render_cell(&"a".repeat(500), 200, None));
    }

    #[test]
    fn ac011_truncated_cell_escapes_newlines_in_kept_prefix() {
        // Truncation runs on the cell value (keeping 199 chars = "a\n" x 99 + "a", 401 removed), then the whole
        // cut cell is quoted and each LF written as `\n`.
        let content = "a\n".repeat(300);
        assert_eq!(content.chars().count(), 600);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content)]], 200);
        let expected_prefix = format!("{}a", "a\\n".repeat(99));
        assert_eq!(
            out,
            format!("t[1]{{a}}:\n  \"{expected_prefix}\u{2026}\"\ntruncated[1]{{row,field,removed}}:\n  0,a,401")
        );
        assert_eq!(out.lines().count(), 4, "no line break survives in the cell, got: {out:?}");
        let cell = out.lines().nth(1).unwrap().trim_start();
        // The cap counts chars before escaping: the escaped, quoted cell is longer than max_cell_len.
        assert_eq!(cell.chars().count(), 99 * 3 + 1 + 1 + 2, "escaped prefix + ellipsis + 2 quotes");
    }

    #[test]
    fn ac011b_max_cell_len_bounds_chars_not_bytes() {
        let content = "日".repeat(100);
        assert_eq!(content.chars().count(), 100);
        assert_eq!(content.len(), 300);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content.clone())]], 200);
        assert_eq!(
            out,
            format!("t[1]{{a}}:\n  {content}"),
            "a 300-byte/100-char cell must not truncate at max_cell_len=200"
        );
    }

    #[test]
    fn ac011c_max_cell_len_smaller_than_suffix_falls_back_to_ellipsis() {
        // The cell is always `max_cell_len - 1` kept chars and an ellipsis, however small the cap.
        let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(50))]], 10);
        assert_eq!(out, "t[1]{a}:\n  aaaaaaaaa…\ntruncated[1]{row,field,removed}:\n  0,a,41");
        let out = render_cell(&"a".repeat(50), 1, None);
        assert_eq!(out, "t[1]{a}:\n  …\ntruncated[1]{row,field,removed}:\n  0,a,50");
        let out = render_cell(&"a".repeat(50), 0, None);
        assert_eq!(out, "t[1]{a}:\n  \"\"\ntruncated[1]{row,field,removed}:\n  0,a,50");
    }

    #[test]
    fn ac008_render_level_comma_cell_is_quoted_when_untruncated() {
        let out = render("t", &["a".to_string()], &[vec![Value::from("x,y")]], 200);
        assert_eq!(out, "t[1]{a}:\n  \"x,y\"");
    }

    #[test]
    fn ac009_render_level_quote_cell_is_wrapped_and_escaped_when_untruncated() {
        let out = render("t", &["a".to_string()], &[vec![Value::from(r#"say "hi""#)]], 200);
        assert_eq!(out, "t[1]{a}:\n  \"say \\\"hi\\\"\"");
    }

    #[test]
    fn ac008b_ac011d_truncated_cell_with_comma_in_kept_prefix_is_quoted() {
        let content = "a,".repeat(300);
        assert_eq!(content.chars().count(), 600);
        let out = render("t", &["a".to_string()], &[vec![Value::from(content)]], 200);
        let expected_kept: String = "a,".repeat(300).chars().take(199).collect();
        let expected =
            format!("t[1]{{a}}:\n  \"{expected_kept}\u{2026}\"\ntruncated[1]{{row,field,removed}}:\n  0,a,401");
        assert_eq!(out, expected);
        let cell = out.lines().nth(1).unwrap().trim_start();
        assert_eq!(cell.chars().count(), 202);
        assert!(cell.starts_with('"') && cell.ends_with('"'));
    }

    #[test]
    fn ac012a_full_render_with_default_max_cell_len() {
        // Constructed via ToonOptions::new + render_toon (not super::render with a
        // literal 200) so a regression to ToonOptions::new's actual default is caught.
        let opts = crate::toon::ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(500))]]);
        let out = crate::toon::render_toon(&opts).unwrap();
        let expected_prefix = "a".repeat(199);
        assert_eq!(
            out,
            format!("t[1]{{a}}:\n  {expected_prefix}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,a,301")
        );
    }

    #[test]
    fn ac012c_toon_options_new_defaults() {
        let opts = crate::toon::ToonOptions::new("t", vec![], vec![]);
        assert_eq!(opts.max_cell_len, 200);
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
        let out = render("t", &["a".to_string()], &[vec![Value::from(" x")]], 200);
        assert_eq!(out, "t[1]{a}:\n  \" x\"");
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
        let out = render("t", &["n".to_string()], &[vec![u64::MAX.into()]], 200);
        assert_eq!(out, "t[1]{n}:\n  18446744073709551615");
    }

    #[test]
    fn ac013_numeric_cells_ignore_max_cell_len() {
        let fields = ["n".to_string(), "m".to_string()];
        let out = render("t", &fields, &[vec![u64::MAX.into(), Value::Int(i64::MIN)]], 5);
        assert_eq!(out, "t[1]{n,m}:\n  18446744073709551615,-9223372036854775808");
    }

    #[test]
    fn ac014_u64_and_usize_within_range_render_exact_digits() {
        let v: Value = 42u64.into();
        assert_eq!(v, Value::Int(42));
        let v: Value = 42usize.into();
        assert_eq!(v, Value::Int(42));
        let out = render("t", &["n".to_string()], &[vec![42u64.into()]], 200);
        assert_eq!(out, "t[1]{n}:\n  42");
    }

    #[test]
    fn ac023_reserved_type_names_are_rejected_but_allowed_as_field_names() {
        for name in ["help", "totalCount", "recovery", "truncated"] {
            let opts = crate::toon::ToonOptions::new(name, vec![], vec![]);
            let err = crate::toon::render_toon(&opts).unwrap_err();
            assert_eq!(err, crate::toon::ToonError::InvalidTypeName { name: name.to_string() });
        }
        let out =
            render("t", &["help".to_string(), "totalCount".to_string()], &[vec![Value::Int(1), Value::Int(2)]], 200);
        assert_eq!(out, "t[1]{help,totalCount}:\n  1,2");
    }

    #[test]
    fn ac022_type_name_with_a_colon_is_quoted_not_rejected() {
        let out = render("totalCount:", &[], &[], 200);
        assert_eq!(out, "\"totalCount:\": []");
    }

    #[test]
    fn ac043_a_cut_cell_carries_no_message_only_an_ellipsis() {
        let out = render_cell(&"a".repeat(500), 200, Some("full=true"));
        let cell = out.lines().nth(1).unwrap().trim_start();
        assert_eq!(cell, format!("{}\u{2026}", "a".repeat(199)), "got: {out:?}");
        assert!(!cell.contains("more chars"), "no in-band marker, got: {cell:?}");
    }

    #[test]
    fn ac044_a_cut_cell_is_exactly_max_cell_len_chars_and_kept_plus_removed_is_the_input() {
        for (len, cap) in [(500usize, 200usize), (50, 10), (11, 10), (3, 2)] {
            let out = render("t", &["a".to_string()], &[vec![Value::from("a".repeat(len))]], cap);
            let cell = out.lines().nth(1).unwrap().trim_start();
            assert_eq!(cell.chars().count(), cap, "len={len} cap={cap}");
            let removed: usize = out.lines().nth(3).unwrap().rsplit(',').next().unwrap().parse().unwrap();
            assert_eq!((cap - 1) + removed, len, "len={len} cap={cap}");
        }
    }

    #[test]
    fn ac036_from_option_string_some_matches_str_none_matches_null() {
        let some_via_option: Value = Some("x".to_string()).into();
        assert_eq!(some_via_option, Value::from("x"));
        let none_via_option: Value = None::<String>.into();
        assert_eq!(none_via_option, Value::Null);

        let out_some =
            render("t", &["a".to_string(), "b".to_string()], &[vec![some_via_option, Value::from("y")]], 200);
        let out_str =
            render("t", &["a".to_string(), "b".to_string()], &[vec![Value::from("x"), Value::from("y")]], 200);
        assert_eq!(out_some, out_str);

        let out_none =
            render("t", &["a".to_string(), "b".to_string()], &[vec![none_via_option, Value::from("y")]], 200);
        let out_null = render("t", &["a".to_string(), "b".to_string()], &[vec![Value::Null, Value::from("y")]], 200);
        assert_eq!(out_none, out_null);
        assert_eq!(out_null, "t[1]{a,b}:\n  null,y");
    }

    #[test]
    fn toon_document_render_produces_expected_output() {
        let opts = crate::toon::ToonOptions::new(
            "t",
            vec!["a".to_string(), "b".to_string()],
            vec![vec![crate::toon::Value::from("x"), crate::toon::Value::Int(1)]],
        );
        let doc = super::ToonDocument::validate(&opts).expect("valid options must validate");
        assert_eq!(doc.render(), "t[1]{a,b}:\n  x,1");
    }

    // The response and the standalone renderer place the `truncated` block themselves, from the same list entry and
    // cut list, so a one-section response renders exactly what `render_toon` does, plus the `totalCount` that only the
    // response writes.
    #[test]
    fn a_one_section_response_renders_what_render_toon_does() {
        use crate::{AgentResponse, OutputFormat, Section};
        let rows = vec![vec![Value::from("a".repeat(50)), Value::Int(1)], vec![Value::from("b,c"), Value::Null]];
        let opts = crate::toon::ToonOptions::new("t", vec!["x".into(), "y".into()], rows.clone()).max_cell_len(10);
        let list = crate::toon::render_toon(&opts).expect("valid");
        for total in [None, Some(9)] {
            let section = Section::toon("t", vec!["x".into(), "y".into()], rows.clone()).total_count(total);
            let r = AgentResponse::new("t").truncate_cells_at(10).section(section);
            let expected = total.map_or_else(|| list.clone(), |n| format!("{list}\ntotalCount: {n}"));
            assert_eq!(r.render(OutputFormat::Text), expected, "{total:?}");
        }
    }
}

#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # TOON
//!
//! Token-Oriented Object Notation (TOON) renderer for michi and AXI.
//!
//! Output conforms to the TOON specification version 4.4 (`toon-spec: 4.4`,
//! <https://github.com/toon-format/spec>): a list document is a root object holding the list in tabular form
//! (`type[N]{f1,f2}:` plus one row per line), `type: []` when empty, an optional `totalCount: N` field and an optional
//! inline `help[N]: …` array, with no trailing newline.

mod escape;
pub(crate) mod render;

pub(crate) use escape::push_f64;
pub use escape::{escape_key, escape_value, escape_value_quoted};
pub use render::{ToonDocument, Value};

/// Most header fields one TOON list may declare. [`ToonOptions::validate()`] returns [`ToonError::TooManyFields`] past
/// it, and `list()` checks it against the union of item keys before building any row.
pub const MAX_FIELDS: usize = 1_000;

/// Most cells (`rows × fields`) one TOON list may hold, where a list with no fields counts one cell per row (an empty
/// row still renders a line). [`ToonOptions::validate()`] returns [`ToonError::TooManyCells`] past it, before looking
/// at any row, and `list()` checks it before building rows, so a sparse item set whose key union is wide cannot
/// expand into a huge table, and neither can a long list of empty items.
pub const MAX_CELLS: usize = 1_000_000;

/// The cells [`MAX_CELLS`] counts: `rows × fields`, with at least one cell per row.
pub(crate) fn cell_count(rows: usize, fields: usize) -> usize {
    rows.saturating_mul(fields.max(1))
}

/// Longest [`ToonOptions::truncation_hint`], in chars. The hint is a short instruction, not content;
/// [`ToonOptions::validate()`] returns [`ToonError::TruncationHintTooLong`] past it.
pub const MAX_TRUNCATION_HINT_CHARS: usize = 256;

/// Error returned by [`ToonOptions::validate()`] when the options cannot form a valid TOON document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToonError {
    /// `type_name` is a root key michi composes next to a list (`totalCount`, `help`, `recovery` or `truncated`); using it would
    /// emit a duplicate key, which TOON 4.4 §14.3 forbids. Any other name is valid and is quoted when §7.3 requires.
    InvalidTypeName {
        /// The offending name.
        name: String,
    },
    /// A field name appears more than once in `fields`; a repeated field name in one header is invalid TOON (§14.2).
    /// Any other name is valid and is quoted when §7.3 requires.
    InvalidFieldName {
        /// The offending field name.
        name: String,
    },
    /// Row `row_index` has `actual` values but `fields` declares `expected`.
    RowLengthMismatch {
        /// Zero-based index of the mismatched row.
        row_index: usize,
        /// Number of fields declared in the header.
        expected: usize,
        /// Actual number of values in the row.
        actual: usize,
    },
    /// An item in `list()` (`serde` feature) could not be rendered as a TOON row.
    ///
    /// Items must serialize to a JSON object (`serde_json::Value::Object`).
    /// Scalars, arrays, and `null` values cannot be represented as TOON rows.
    InvalidItem {
        /// Zero-based index of the invalid item.
        row_index: usize,
        /// Reason why the item is invalid.
        reason: String,
    },
    /// `fields` declares more than [`MAX_FIELDS`] names (for `list()`: the items' keys add up to more).
    TooManyFields {
        /// Number of fields declared.
        count: usize,
        /// The limit, [`MAX_FIELDS`].
        max: usize,
    },
    /// `rows × fields` (at least one cell per row) is more than [`MAX_CELLS`].
    TooManyCells {
        /// Number of rows.
        rows: usize,
        /// Number of fields.
        fields: usize,
        /// The limit, [`MAX_CELLS`].
        max: usize,
    },
    /// [`ToonOptions::truncation_hint`] is longer than [`MAX_TRUNCATION_HINT_CHARS`] chars.
    TruncationHintTooLong {
        /// Length of the hint, in chars.
        len: usize,
        /// The limit, [`MAX_TRUNCATION_HINT_CHARS`].
        max: usize,
    },
}

impl std::fmt::Display for ToonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTypeName { name } => {
                write!(f, "type_name {name:?} is a reserved document key (totalCount, help, recovery, truncated)")
            }
            Self::InvalidFieldName { name } => {
                write!(f, "field {name:?} appears more than once")
            }
            Self::RowLengthMismatch { row_index, expected, actual } => {
                write!(f, "row {row_index} has {actual} values but {expected} fields declared")
            }
            Self::InvalidItem { row_index, reason } => {
                write!(f, "item at index {row_index} cannot be rendered as a TOON row: {reason}")
            }
            Self::TooManyFields { count, max } => write!(f, "fields length {count} exceeds maximum of {max}"),
            Self::TooManyCells { rows, fields, max } => {
                let cells = cell_count(*rows, *fields);
                write!(f, "{rows} rows x {fields} fields = {cells} cells exceeds maximum of {max}")
            }
            Self::TruncationHintTooLong { len, max } => {
                write!(f, "truncation_hint length {len} chars exceeds maximum of {max}")
            }
        }
    }
}

impl std::error::Error for ToonError {}

/// Options for rendering a TOON document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ToonOptions {
    /// Snake_case type name, e.g. `"issue"`, `"component"`: the key of the list entry. Names outside TOON's
    /// unquoted-key pattern are quoted (§7.3); `totalCount`, `help`, `recovery` and `truncated` are reserved.
    pub type_name: String,
    /// Ordered, distinct field names for the header, e.g. `["number", "title", "state"]`, quoted when §7.3 requires.
    /// Not emitted when `rows` is empty (an empty list renders as `type: []`).
    pub fields: Vec<String>,
    /// Rows, each a Vec of values parallel to `fields`. Rows with zero fields are empty objects and render as bare
    /// `-` list items, since TOON forbids a tabular form without fields.
    pub rows: Vec<Vec<Value>>,
    /// Total available count (emitted as `totalCount: N` when `Some`).
    pub total_count: Option<usize>,
    /// Agent-facing usage hints. Emitted as the inline array `help[N]: h1,h2` when non-empty.
    pub hints: Vec<String>,
    /// Max string cell length, in chars, before truncation. Default: 200.
    ///
    /// The cap counts the cell's characters *before* TOON escaping and quoting: a truncated (or untruncated) cell
    /// that needs quotes or escapes (§7.1, §7.2) is written longer than `max_cell_len` on the wire. A longer cell keeps
    /// its first `max_cell_len - 1` chars and ends in a plain `…` (the empty string for a cap of 0). The cell carries
    /// no message: every cut cell is reported in a `truncated[N]{row,field,removed}:` block after the list, which
    /// cell text cannot forge because cells are quoted and escaped.
    pub max_cell_len: usize,
    /// The caller's instruction for getting a truncated cell in full, e.g. `"full=true"` or `"--no-truncate"`.
    /// Default `None`: michi never invents a parameter the calling tool may not have. When set and a cell was cut,
    /// michi adds one `help[]` line naming the `truncated` block and this instruction. It comes from the tool author,
    /// never from data. An empty string is the same as `None`.
    pub truncation_hint: Option<String>,
}

impl ToonOptions {
    /// Create options from a type name, fields and rows, with no total count or hints, the default 200-char cell cap
    /// and no truncation hint.
    #[must_use]
    pub fn new(type_name: impl Into<String>, fields: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self {
            type_name: type_name.into(),
            fields,
            rows,
            total_count: None,
            hints: Vec::new(),
            max_cell_len: 200,
            truncation_hint: None,
        }
    }

    /// Set total count option.
    #[must_use]
    pub fn total_count(mut self, total: Option<usize>) -> Self {
        self.total_count = total;
        self
    }

    /// Set hints option.
    #[must_use]
    pub fn hints(mut self, hints: Vec<String>) -> Self {
        self.hints = hints;
        self
    }

    /// Set the max string cell length in chars, counted before escaping and quoting (see the `max_cell_len` field).
    #[must_use]
    pub fn max_cell_len(mut self, len: usize) -> Self {
        self.max_cell_len = len;
        self
    }

    /// Set the caller's instruction for getting a cut cell in full, e.g. `.truncation_hint("full=true")`. michi
    /// delivers it in `help[]` as `Cut cells are listed in the truncated block; use full=true for the full value`,
    /// and only when a cell was cut. Unset by default; an empty hint does the same. Pass tool-author text, never data.
    #[must_use]
    pub fn truncation_hint(mut self, hint: impl Into<String>) -> Self {
        self.truncation_hint = Some(hint.into());
        self
    }

    /// Validate structural invariants and return the proof required to render.
    ///
    /// Delegates to [`ToonDocument::validate`]. Validation is mandatory:
    /// rendering consumes the [`ToonDocument`] this returns, so there is no
    /// path from an unvalidated `ToonOptions` to rendered output.
    ///
    /// # Errors
    ///
    /// Returns [`ToonError::InvalidTypeName`] if `type_name` is a reserved
    /// root key (`totalCount`, `help`, `recovery` or `truncated`), [`ToonError::TooManyFields`] past [`MAX_FIELDS`] fields,
    /// [`ToonError::InvalidFieldName`] if a field name appears more than once, [`ToonError::TooManyCells`] if
    /// `rows × fields` is past [`MAX_CELLS`], [`ToonError::TruncationHintTooLong`] if the truncation hint is past
    /// [`MAX_TRUNCATION_HINT_CHARS`] chars, or [`ToonError::RowLengthMismatch`]
    /// if any row's length differs from `fields.len()`.
    pub fn validate(&self) -> Result<ToonDocument<'_>, ToonError> {
        ToonDocument::validate(self)
    }
}

impl Default for ToonOptions {
    fn default() -> Self {
        Self {
            type_name: String::new(),
            fields: Vec::new(),
            rows: Vec::new(),
            total_count: None,
            hints: Vec::new(),
            max_cell_len: 200,
            truncation_hint: None,
        }
    }
}

const _: fn(&ToonOptions) -> Result<String, ToonError> = render_toon;

/// Render a TOON 4.4 document to a string (no trailing newline).
///
/// # Errors
///
/// Returns [`ToonError`] if `opts` fails structural validation — see
/// [`ToonOptions::validate`] for the exact conditions.
pub fn render_toon(opts: &ToonOptions) -> Result<String, ToonError> {
    opts.validate().map(|doc| doc.render())
}

#[cfg(feature = "serde")]
fn json_value_to_toon_value(v: Option<&serde_json::Value>) -> Value {
    match v {
        None | Some(serde_json::Value::Null) => Value::Null,
        Some(serde_json::Value::Bool(b)) => Value::Bool(*b),
        Some(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                // Above i64::MAX: an exact unsigned number, not rounded through f64.
                Value::UInt(u)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Str(compact_str::CompactString::new(n.to_string()))
            }
        }
        Some(serde_json::Value::String(s)) => Value::Str(compact_str::CompactString::new(s)),
        Some(other) => Value::Str(compact_str::CompactString::new(other.to_string())),
    }
}

/// Build and render a TOON document from a slice of `Serialize`-able items.
///
/// Returns the rendered TOON string on success. Returns `Err` if any item
/// does not serialize to a JSON object (e.g. a bare integer or string slice)
/// — use only with homogeneous struct slices.
///
/// The header lists every field any item serialized, in first-seen order, so a field skipped on early rows (for
/// example `#[serde(skip_serializing_if = "Option::is_none")]`) still appears. Rows without that field get a `null`
/// cell.
///
/// Host-type normalization (TOON 4.4 §3): JSON integers map to [`Value::Int`], or [`Value::UInt`] above `i64::MAX`;
/// other numbers to [`Value::Float`] (and, only with serde_json's `arbitrary_precision`, a number outside both domains
/// becomes a string of its exact digits); a missing key or JSON `null` to `null`; and a nested array or object to a
/// string holding its compact JSON text, since michi emits flat tables only.
///
/// Requires the `serde` feature.
///
/// # Errors
///
/// Returns [`ToonError::InvalidItem`] if an item fails to serialize or does
/// not produce a JSON object (struct or map required). Returns
/// [`ToonError::InvalidTypeName`] if `type_name` is a reserved root key
/// (`totalCount`, `help`, `recovery` or `truncated`). Returns [`ToonError::TooManyFields`] if the items' keys add up to more than
/// [`MAX_FIELDS`] distinct names, or [`ToonError::TooManyCells`] if `items.len()` times that header width is past
/// [`MAX_CELLS`]; both are checked before any row is built.
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
#[cfg(feature = "serde")]
pub fn list<T: serde::Serialize>(type_name: impl Into<String>, items: &[T]) -> Result<String, ToonError> {
    let type_name = type_name.into();
    let mut fields: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut objects: Vec<serde_json::Map<String, serde_json::Value>> = Vec::with_capacity(items.len());
    for (row_index, item) in items.iter().enumerate() {
        // Through JSON text rather than `serde_json::to_value`: the text writer prints an `f32` with its own shortest
        // digits (`0.1`), while `to_value` widens it to f64 first (`0.10000000149011612`). Reading the text back is
        // exact for every other value (`float_roundtrip` is on).
        let obj = match serde_json::to_vec(item).and_then(|text| serde_json::from_slice(&text)) {
            Ok(serde_json::Value::Object(map)) => map,
            Ok(_) => {
                return Err(ToonError::InvalidItem {
                    row_index,
                    reason: "item is not a JSON object (struct or map required)".into(),
                });
            }
            Err(e) => {
                return Err(ToonError::InvalidItem { row_index, reason: e.to_string() });
            }
        };
        for key in obj.keys() {
            if seen.insert(key.clone()) {
                fields.push(key.clone());
            }
        }
        objects.push(obj);
    }
    // Check the shape before building rows: a sparse item set whose key union is wide would otherwise expand into
    // rows × fields cells here, before validate() could reject it.
    if !render::RESERVED_TYPE_NAMES.contains(&type_name.as_str()) {
        if fields.len() > MAX_FIELDS {
            return Err(ToonError::TooManyFields { count: fields.len(), max: MAX_FIELDS });
        }
        if cell_count(objects.len(), fields.len()) > MAX_CELLS {
            return Err(ToonError::TooManyCells { rows: objects.len(), fields: fields.len(), max: MAX_CELLS });
        }
    }
    let rows: Vec<Vec<Value>> =
        objects.iter().map(|obj| fields.iter().map(|f| json_value_to_toon_value(obj.get(f))).collect()).collect();
    let opts = ToonOptions::new(type_name, fields, rows);
    Ok(opts.validate()?.render())
}

#[cfg(test)]
mod ac032_public_surface_tests {
    // Checks the compiled API, not source text: these imports fail to build if an item is missing or renamed, and
    // `list` only exists with the `serde` feature (tests/ui-fail/serde_gate_toon_list.rs proves it is absent
    // without it).
    #[test]
    #[allow(unused_imports)]
    fn spec_api_surface_items_are_public() {
        use crate::toon::{
            escape_value, escape_value_quoted, render_toon, ToonDocument, ToonError, ToonOptions, Value,
        };
        #[cfg(feature = "serde")]
        let _list: fn(&str, &[serde_json::Value]) -> Result<String, ToonError> = |t, items| crate::toon::list(t, items);
    }

    #[test]
    fn must_use_and_list_signature_match_the_spec() {
        let src = include_str!("mod.rs");
        assert!(
            src.contains("#[must_use]\n    pub fn new("),
            "ToonOptions::new must carry #[must_use] per api_surface"
        );
        assert!(
            src.contains("pub fn render_toon(opts: &ToonOptions) -> Result<String, ToonError>"),
            "render_toon must be the fallible signature per api_surface, with no #[must_use] (Result is already must_use)"
        );
        assert!(
            src.contains("pub fn list<T: serde::Serialize>(type_name: impl Into<String>, items: &[T])"),
            "list's first parameter must be impl Into<String>, not &str, per api_surface"
        );
    }
}

#[cfg(test)]
mod auto_trait_tests {
    use super::*;

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    #[test]
    fn test_auto_traits() {
        assert_send_sync_static::<ToonOptions>();
        assert_send_sync_static::<Value>();
    }
}

#[cfg(test)]
mod validate_tests {
    use super::*;

    fn opts(type_name: &str, fields: &[&str], rows: Vec<Vec<Value>>) -> ToonOptions {
        ToonOptions {
            type_name: type_name.into(),
            fields: fields.iter().map(|f| (*f).to_string()).collect(),
            rows,
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
            truncation_hint: None,
        }
    }

    #[test]
    fn validate_ok_for_valid_options() {
        let o = opts("file", &["path", "size"], vec![vec![Value::from("a.rs"), Value::from(100i64)]]);
        assert!(o.validate().is_ok());
    }

    #[test]
    fn structural_characters_in_names_are_valid_and_quoted() {
        // TOON 4.4 §7.3: any key is encodable; keys outside the unquoted pattern are quoted.
        let o = opts("foo[bar", &["a,b", "c{d}"], vec![vec![Value::Int(1), Value::Int(2)]]);
        assert_eq!(render_toon(&o).unwrap(), "\"foo[bar\"[1]{\"a,b\",\"c{d}\"}:\n  1,2");
    }

    #[test]
    fn validate_rejects_reserved_type_names() {
        for name in ["totalCount", "help"] {
            assert!(matches!(opts(name, &[], vec![]).validate(), Err(ToonError::InvalidTypeName { .. })), "{name}");
        }
    }

    #[test]
    fn ac002_invalid_type_name_error_carries_exact_offending_name() {
        assert_eq!(
            opts("help", &[], vec![]).validate().unwrap_err(),
            ToonError::InvalidTypeName { name: "help".to_string() }
        );
    }

    #[test]
    fn validate_rejects_duplicate_field() {
        assert!(matches!(opts("t", &["a", "a"], vec![]).validate(), Err(ToonError::InvalidFieldName { .. })));
    }

    #[test]
    fn ac003_type_name_violation_takes_precedence_over_field_name_violation() {
        assert_eq!(
            opts("help", &["x", "x"], vec![]).validate().unwrap_err(),
            ToonError::InvalidTypeName { name: "help".to_string() }
        );
    }

    #[test]
    fn ac003_invalid_field_name_error_carries_exact_offending_name() {
        assert_eq!(
            opts("t", &["a", "b", "a"], vec![]).validate().unwrap_err(),
            ToonError::InvalidFieldName { name: "a".to_string() }
        );
    }

    #[test]
    fn ac003_multiple_duplicates_report_the_first_repeat_in_list_order() {
        assert_eq!(
            opts("t", &["c", "d", "d", "c"], vec![]).validate().unwrap_err(),
            ToonError::InvalidFieldName { name: "d".to_string() }
        );
    }

    #[test]
    fn ac004b_multiple_violations_return_only_the_first_in_fixed_order() {
        let o = opts("totalCount", &["a", "a"], vec![vec![Value::from("x")]]);
        assert_eq!(o.validate().unwrap_err(), ToonError::InvalidTypeName { name: "totalCount".to_string() });
    }

    #[test]
    fn ac004b_field_name_violation_takes_precedence_over_row_length_violation() {
        let o = opts("t", &["a", "a"], vec![vec![Value::from("x")]]);
        assert_eq!(o.validate().unwrap_err(), ToonError::InvalidFieldName { name: "a".to_string() });
    }

    #[test]
    fn error_display_names_the_rule() {
        assert_eq!(
            ToonError::InvalidTypeName { name: "help".into() }.to_string(),
            "type_name \"help\" is a reserved document key (totalCount, help, recovery, truncated)"
        );
        assert_eq!(ToonError::InvalidFieldName { name: "a".into() }.to_string(), "field \"a\" appears more than once");
    }

    #[test]
    fn ac035_default_renders_exact_empty_document() {
        let opts = ToonOptions::default();
        // The empty type name is the empty key, which TOON 4.4 §7.3 quotes; an empty list is `key: []` (§9.1).
        assert_eq!(render_toon(&opts).unwrap(), "\"\": []");
        let equivalent = ToonOptions::new(String::new(), Vec::new(), Vec::new());
        assert_eq!(render_toon(&opts).unwrap(), render_toon(&equivalent).unwrap());
    }

    #[test]
    fn ac035b_default_fields_match_new_field_by_field() {
        // Asserted field-by-field, not just via the empty-document render, since
        // the empty render is invariant under max_cell_len and cannot catch a
        // Default-only regression (Default is a separate struct literal, not a
        // delegation to ToonOptions::new).
        let opts = ToonOptions::default();
        assert_eq!(opts.type_name, String::new());
        assert!(opts.fields.is_empty());
        assert!(opts.rows.is_empty());
        assert_eq!(opts.total_count, None);
        assert!(opts.hints.is_empty());
        assert_eq!(opts.max_cell_len, 200);
    }

    #[test]
    fn ac037_direct_field_mutation_affects_render_toon_output() {
        let mut opts = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("x")]]);
        opts.total_count = Some(5);
        opts.hints = vec!["h".to_string()];
        let via_mutation = render_toon(&opts).unwrap();

        let via_builder = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("x")]])
            .total_count(Some(5))
            .hints(vec!["h".to_string()]);
        assert_eq!(via_mutation, render_toon(&via_builder).unwrap());
    }

    #[test]
    fn validate_rejects_row_length_mismatch() {
        let o = opts("t", &["a"], vec![vec![Value::from("x"), Value::from("y")]]);
        assert!(matches!(o.validate(), Err(ToonError::RowLengthMismatch { row_index: 0, expected: 1, actual: 2 })));
    }

    #[test]
    fn ac004_type_name_violation_takes_precedence_over_row_length_violation() {
        let o = opts("help", &["a"], vec![vec![Value::from("x"), Value::from("y")]]);
        assert_eq!(o.validate().unwrap_err(), ToonError::InvalidTypeName { name: "help".to_string() });
    }

    fn distinct_fields(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("f{i}")).collect()
    }

    #[test]
    fn capacity_limits_are_the_documented_values() {
        assert_eq!(MAX_FIELDS, 1_000);
        assert_eq!(MAX_CELLS, 1_000_000);
        assert_eq!(MAX_TRUNCATION_HINT_CHARS, 256);
    }

    #[test]
    fn validate_accepts_exactly_max_fields_and_rejects_one_more() {
        let at_cap = ToonOptions::new("t", distinct_fields(MAX_FIELDS), vec![]);
        assert!(at_cap.validate().is_ok());
        let over = ToonOptions::new("t", distinct_fields(MAX_FIELDS + 1), vec![]);
        assert_eq!(over.validate().unwrap_err(), ToonError::TooManyFields { count: MAX_FIELDS + 1, max: MAX_FIELDS });
    }

    #[test]
    fn too_many_fields_takes_precedence_over_a_duplicate_field() {
        let mut fields = distinct_fields(MAX_FIELDS + 1);
        fields[1] = fields[0].clone();
        let o = ToonOptions::new("t", fields, vec![]);
        assert!(matches!(o.validate(), Err(ToonError::TooManyFields { .. })));
    }

    #[test]
    fn duplicate_check_is_linear_for_the_largest_accepted_header() {
        // The last field repeats the first: the O(F²) scan this replaced did ~500k comparisons here; a set does 1000.
        let mut fields = distinct_fields(MAX_FIELDS);
        fields[MAX_FIELDS - 1] = "f0".to_string();
        let o = ToonOptions::new("t", fields, vec![]);
        assert_eq!(o.validate().unwrap_err(), ToonError::InvalidFieldName { name: "f0".to_string() });
    }

    #[test]
    fn validate_rejects_more_than_max_cells() {
        // 1001 rows x 1000 fields = 1,001,000 cells. Rows are empty: the cell cap is checked before row arity, so
        // an oversized document is rejected without looking at any row.
        let o = ToonOptions::new("t", distinct_fields(1_000), vec![Vec::new(); 1_001]);
        assert_eq!(o.validate().unwrap_err(), ToonError::TooManyCells { rows: 1_001, fields: 1_000, max: MAX_CELLS });
    }

    #[test]
    fn validate_accepts_exactly_max_cells() {
        let o = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::Null]; MAX_CELLS]);
        assert!(o.validate().is_ok());
        let o = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::Null]; MAX_CELLS + 1]);
        assert_eq!(
            o.validate().unwrap_err(),
            ToonError::TooManyCells { rows: MAX_CELLS + 1, fields: 1, max: MAX_CELLS }
        );
    }

    #[test]
    fn validate_counts_an_empty_row_as_one_cell() {
        let o = ToonOptions::new("t", vec![], vec![Vec::new(); MAX_CELLS + 1]);
        let err = o.validate().unwrap_err();
        assert_eq!(err, ToonError::TooManyCells { rows: MAX_CELLS + 1, fields: 0, max: MAX_CELLS });
        assert_eq!(err.to_string(), "1000001 rows x 0 fields = 1000001 cells exceeds maximum of 1000000");
        assert!(ToonOptions::new("t", vec![], vec![Vec::new(); MAX_CELLS]).validate().is_ok());
    }

    #[test]
    fn validate_caps_truncation_hint_in_chars_not_bytes() {
        // 256 two-byte chars (512 bytes) is at the cap; one more char is over it.
        let at_cap = ToonOptions::new("t", vec![], vec![]).truncation_hint("é".repeat(MAX_TRUNCATION_HINT_CHARS));
        assert!(at_cap.validate().is_ok());
        let over = ToonOptions::new("t", vec![], vec![]).truncation_hint("é".repeat(MAX_TRUNCATION_HINT_CHARS + 1));
        assert_eq!(
            over.validate().unwrap_err(),
            ToonError::TruncationHintTooLong { len: MAX_TRUNCATION_HINT_CHARS + 1, max: MAX_TRUNCATION_HINT_CHARS }
        );
    }

    #[test]
    fn capacity_errors_display_the_limit() {
        assert_eq!(
            ToonError::TooManyFields { count: 1001, max: 1000 }.to_string(),
            "fields length 1001 exceeds maximum of 1000"
        );
        assert_eq!(
            ToonError::TooManyCells { rows: 2000, fields: 1000, max: 1_000_000 }.to_string(),
            "2000 rows x 1000 fields = 2000000 cells exceeds maximum of 1000000"
        );
        assert_eq!(
            ToonError::TruncationHintTooLong { len: 300, max: 256 }.to_string(),
            "truncation_hint length 300 chars exceeds maximum of 256"
        );
    }
}

#[cfg(all(test, feature = "serde"))]
mod list_tests {
    use super::*;

    #[test]
    fn list_rejects_non_object_items() {
        let result = list("t", &[1u32, 2, 3]);
        assert!(
            matches!(result, Err(ToonError::InvalidItem { row_index: 0, .. })),
            "non-object items must return Err(InvalidItem), got: {result:?}"
        );
    }

    #[test]
    fn ac025a_builder_chain_exact_output() {
        let opts = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(50))]])
            .total_count(Some(5))
            .hints(vec!["h".to_string()])
            .max_cell_len(10);
        // 9 kept chars and an ellipsis fill the cap; the cut is reported in the `truncated` block.
        assert_eq!(
            render_toon(&opts).unwrap(),
            "t[1]{a}:\n  aaaaaaaaa…\ntruncated[1]{row,field,removed}:\n  0,a,41\ntotalCount: 5\nhelp[1]: h"
        );

        // A hint adds one michi line to `help[]`, after the caller's own hints.
        let opts = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(50))]])
            .hints(vec!["h".to_string()])
            .max_cell_len(30)
            .truncation_hint("x");
        assert_eq!(
            render_toon(&opts).unwrap(),
            format!(
                "t[1]{{a}}:\n  {}…\ntruncated[1]{{row,field,removed}}:\n  0,a,21\n\
                 help[2]: h,Cut cells are listed in the truncated block; use x for the full value",
                "a".repeat(29)
            )
        );
    }

    #[test]
    fn ac025b_builder_order_independence() {
        let forward = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(50))]])
            .total_count(Some(5))
            .hints(vec!["h".to_string()])
            .max_cell_len(10);
        let reversed = ToonOptions::new("t", vec!["a".to_string()], vec![vec![Value::from("a".repeat(50))]])
            .max_cell_len(10)
            .hints(vec!["h".to_string()])
            .total_count(Some(5));
        assert_eq!(render_toon(&forward), render_toon(&reversed));
    }

    #[test]
    fn ac027_invalid_item_reports_index_of_first_non_object_item() {
        let result = list("t", &[serde_json::json!({"a": 1}), serde_json::json!(7)]);
        assert!(
            matches!(result, Err(ToonError::InvalidItem { row_index: 1, .. })),
            "the first item is a valid object; only the second (index 1) is non-object, got: {result:?}"
        );
    }

    #[test]
    fn ac028_missing_key_renders_as_null_cell() {
        let result = list("t", &[serde_json::json!({"a": 1, "b": 2}), serde_json::json!({"a": 3})]);
        assert_eq!(result, Ok("t[2]{a,b}:\n  1,2\n  3,null".to_string()));
    }

    #[test]
    fn ac029_structural_char_in_key_is_quoted() {
        let result = list("t", &[serde_json::json!({"a,b": 1})]);
        assert_eq!(result, Ok("t[1]{\"a,b\"}:\n  1".to_string()));
    }

    #[test]
    fn ac029_reserved_type_name_is_rejected() {
        let result = list("help", &[serde_json::json!({"a": 1})]);
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "help".to_string() }));
    }

    #[test]
    fn ac029_serialization_failure_takes_precedence_over_type_name_violation() {
        let result = list("help", &[serde_json::json!({"a": 1}), serde_json::json!(7)]);
        assert_eq!(
            result,
            Err(ToonError::InvalidItem {
                row_index: 1,
                reason: "item is not a JSON object (struct or map required)".to_string()
            })
        );
    }

    struct FailsToSerialize;

    impl serde::Serialize for FailsToSerialize {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("boom"))
        }
    }

    #[test]
    fn ac027b_serialization_error_reports_reason_and_first_failing_index() {
        let result = list("t", &[FailsToSerialize]);
        assert_eq!(result, Err(ToonError::InvalidItem { row_index: 0, reason: "boom".to_string() }));
    }

    enum MixedItem {
        Fails,
        Num(i32),
    }

    impl serde::Serialize for MixedItem {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                Self::Fails => Err(serde::ser::Error::custom("boom")),
                Self::Num(n) => serializer.serialize_i32(*n),
            }
        }
    }

    #[test]
    fn ac027_serialization_failure_at_lower_index_wins_over_non_object_at_higher_index() {
        let result = list("t", &[MixedItem::Fails, MixedItem::Num(7)]);
        assert_eq!(result, Err(ToonError::InvalidItem { row_index: 0, reason: "boom".to_string() }));
    }

    #[test]
    fn ac027b_non_object_at_lower_index_wins_over_serialization_failure_at_higher_index() {
        let result = list("t", &[MixedItem::Num(7), MixedItem::Fails]);
        assert_eq!(
            result,
            Err(ToonError::InvalidItem {
                row_index: 0,
                reason: "item is not a JSON object (struct or map required)".to_string()
            })
        );
    }

    #[test]
    fn ac038_first_item_empty_later_item_nonempty_fills_a_null_cell() {
        let result = list("t", &[serde_json::json!({}), serde_json::json!({"k": 1})]);
        assert_eq!(result, Ok("t[2]{k}:\n  null\n  1".to_string()));
    }

    #[test]
    fn ac038_header_is_the_union_of_every_items_keys_in_first_seen_order() {
        let result =
            list("t", &[serde_json::json!({}), serde_json::json!({"k": 1}), serde_json::json!({"a": 1, "b": 2})]);
        assert_eq!(result, Ok("t[3]{k,a,b}:\n  null,null,null\n  1,null,null\n  null,1,2".to_string()));
    }

    #[test]
    fn list_rejects_a_key_union_over_max_fields() {
        let items: Vec<serde_json::Value> =
            (0..=MAX_FIELDS).map(|i| serde_json::json!({ format!("k{i}"): 1 })).collect();
        assert_eq!(list("t", &items), Err(ToonError::TooManyFields { count: MAX_FIELDS + 1, max: MAX_FIELDS }));
    }

    #[test]
    fn list_rejects_sparse_items_whose_header_times_rows_exceeds_max_cells() {
        // 2000 items each with one distinct key: only 2000 values, but the union header makes a 2000 x 1000 table.
        // Rejected before any row is materialized (the 10k x 10k case allocated 2.84 GiB).
        let items: Vec<serde_json::Value> =
            (0..2_000).map(|i| serde_json::json!({ format!("k{}", i % 1_000): 1 })).collect();
        assert_eq!(list("t", &items), Err(ToonError::TooManyCells { rows: 2_000, fields: 1_000, max: MAX_CELLS }));
    }

    // Zero fields used to make every row zero cells, so `list()` of 2M empty structs rendered 8 MB unchecked. An empty
    // row still renders a line, so it counts as one cell.
    #[test]
    fn list_counts_rows_of_items_with_no_fields() {
        let items = vec![serde_json::json!({}); MAX_CELLS + 1];
        assert_eq!(list("t", &items), Err(ToonError::TooManyCells { rows: MAX_CELLS + 1, fields: 0, max: MAX_CELLS }));
        assert!(list("t", &items[..MAX_CELLS]).is_ok());
    }

    #[test]
    fn list_reports_reserved_type_name_before_capacity_errors() {
        let items: Vec<serde_json::Value> =
            (0..=MAX_FIELDS).map(|i| serde_json::json!({ format!("k{i}"): 1 })).collect();
        assert_eq!(list("help", &items), Err(ToonError::InvalidTypeName { name: "help".to_string() }));
    }

    #[test]
    fn ac039_empty_items_slice_renders_exact_empty_document() {
        let result = list("t", &([] as [serde_json::Value; 0]));
        assert_eq!(result, Ok("t: []".to_string()));
    }

    #[test]
    fn ac039_reserved_type_name_rejected_even_with_empty_items_slice() {
        let result = list("totalCount", &([] as [serde_json::Value; 0]));
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "totalCount".to_string() }));
    }

    #[test]
    fn ac040_all_empty_object_items_render_as_bare_list_items() {
        // TOON 4.4 §9.3/§9.4/§10: empty objects may not be tabular; each is a bare `-` list item.
        let result = list("t", &[serde_json::json!({}), serde_json::json!({})]);
        assert_eq!(result, Ok("t[2]:\n  -\n  -".to_string()));
    }

    #[test]
    fn ac040_reserved_type_name_rejected_even_with_all_empty_object_items() {
        let result = list("help", &[serde_json::json!({}), serde_json::json!({})]);
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "help".to_string() }));
    }

    #[test]
    fn nested_values_become_quoted_json_text() {
        let result = list("t", &[serde_json::json!({"o": {"x": 1}, "a": [1, 2]})]);
        assert_eq!(result, Ok("t[1]{o,a}:\n  \"{\\\"x\\\":1}\",\"[1,2]\"".to_string()));
    }
}

#[cfg(test)]
mod ac013_docs_guard_tests {
    use crate::toon::render::test_scan::strip_test_modules;

    #[test]
    fn validate_docs_do_not_claim_opt_in() {
        let non_test = strip_test_modules(include_str!("mod.rs"));
        let opt_in_needle = ["Caller", "opt-in"].join(" ");
        assert!(!non_test.contains(&opt_in_needle), "validate's rustdoc must not describe validation as opt-in");
        let principles_needle = ["PRINCIPLES.md", "§1"].join(" ");
        assert!(
            !non_test.contains(&principles_needle),
            "validate's rustdoc must not reference the nonexistent PRINCIPLES.md §1"
        );
    }
}

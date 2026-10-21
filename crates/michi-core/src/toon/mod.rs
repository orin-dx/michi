#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # TOON
//!
//! Token-Optimized Object Notation (TOON) format renderer and parser for michi and AXI.

mod escape;
pub(crate) mod render;

pub use escape::{escape_value, escape_value_quoted};
pub use render::{ToonDocument, Value};

/// Error returned by [`ToonOptions::validate()`] when structural invariants are violated.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToonError {
    /// `type_name` contains a structural character (`[`, `]`, `{`, `}`, `\n`, `\r`).
    InvalidTypeName {
        /// The offending name.
        name: String,
    },
    /// A field name contains a structural character (`,`, `{`, `}`, `\n`, `\r`).
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
    /// An item in [`list()`] could not be rendered as a TOON row.
    ///
    /// Items must serialize to a JSON object (`serde_json::Value::Object`).
    /// Scalars, arrays, and `null` values cannot be represented as TOON rows.
    InvalidItem {
        /// Zero-based index of the invalid item.
        row_index: usize,
        /// Reason why the item is invalid.
        reason: String,
    },
}

impl std::fmt::Display for ToonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTypeName { name } => {
                write!(f, "type_name {name:?} contains a structural character")
            }
            Self::InvalidFieldName { name } => {
                write!(f, "field {name:?} contains a structural character")
            }
            Self::RowLengthMismatch { row_index, expected, actual } => {
                write!(f, "row {row_index} has {actual} values but {expected} fields declared")
            }
            Self::InvalidItem { row_index, reason } => {
                write!(f, "item at index {row_index} cannot be rendered as a TOON row: {reason}")
            }
        }
    }
}

impl std::error::Error for ToonError {}

/// Options for rendering a TOON document.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ToonOptions {
    /// Snake_case type name, e.g. `"issue"`, `"component"`.
    pub type_name: String,
    /// Ordered field names for the header, e.g. `["number", "title", "state"]`.
    pub fields: Vec<String>,
    /// Rows, each a Vec of values parallel to `fields`.
    pub rows: Vec<Vec<Value>>,
    /// Total available count (emitted as `totalCount: N` when `Some`).
    pub total_count: Option<usize>,
    /// Agent-facing usage hints. Emitted as `help[N]:` block when non-empty.
    pub hints: Vec<String>,
    /// Max string cell length before inline truncation. Default: 200.
    pub max_cell_len: usize,
}

impl ToonOptions {
    /// Create a new ToonOptions struct with type_name and fields.
    #[must_use]
    pub fn new(type_name: impl Into<String>, fields: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self { type_name: type_name.into(), fields, rows, total_count: None, hints: Vec::new(), max_cell_len: 200 }
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

    /// Set max cell length option.
    #[must_use]
    pub fn max_cell_len(mut self, len: usize) -> Self {
        self.max_cell_len = len;
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
    /// Returns [`ToonError::InvalidTypeName`] if `type_name` contains a
    /// structural character, [`ToonError::InvalidFieldName`] if any field name
    /// contains a structural character, or [`ToonError::RowLengthMismatch`] if
    /// any row's length differs from `fields.len()`.
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
        }
    }
}

const _: fn(&ToonOptions) -> Result<String, ToonError> = render_toon;

/// Render a TOON document to a string.
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
                // Above i64::MAX: keep the exact digits rather than rounding through f64.
                Value::from(u)
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
/// example `#[serde(skip_serializing_if = "Option::is_none")]`) still appears. Rows without that field get an empty
/// cell.
///
/// Requires the `serde` feature.
///
/// # Errors
///
/// Returns [`ToonError::InvalidItem`] if an item fails to serialize or does
/// not produce a JSON object (struct or map required). Returns
/// [`ToonError::InvalidTypeName`] or [`ToonError::InvalidFieldName`] if
/// `type_name` or a serialized field name contains a structural character
/// (`[`, `]`, `{`, `}`, `,`, `\n`, `\r`).
#[cfg(feature = "serde")]
pub fn list<T: serde::Serialize>(type_name: impl Into<String>, items: &[T]) -> Result<String, ToonError> {
    let type_name = type_name.into();
    let mut fields: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut objects: Vec<serde_json::Map<String, serde_json::Value>> = Vec::with_capacity(items.len());
    for (row_index, item) in items.iter().enumerate() {
        let obj = match serde_json::to_value(item) {
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
    let rows: Vec<Vec<Value>> =
        objects.iter().map(|obj| fields.iter().map(|f| json_value_to_toon_value(obj.get(f))).collect()).collect();
    let opts = ToonOptions { type_name, fields, rows, total_count: None, hints: Vec::new(), max_cell_len: 200 };
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

    #[test]
    fn validate_ok_for_valid_options() {
        let opts = ToonOptions {
            type_name: "file".into(),
            fields: vec!["path".into(), "size".into()],
            rows: vec![vec![Value::from("a.rs"), Value::from(100i64)]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn validate_rejects_type_name_with_bracket() {
        let opts = ToonOptions {
            type_name: "foo[bar".into(),
            fields: vec![],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(opts.validate(), Err(ToonError::InvalidTypeName { .. })));
    }

    #[test]
    fn ac002_invalid_type_name_error_carries_exact_offending_name() {
        let opts = ToonOptions {
            type_name: "foo[bar".into(),
            fields: vec![],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidTypeName { name: "foo[bar".to_string() });
    }

    #[test]
    fn validate_rejects_field_with_comma() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a,b".into()],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(opts.validate(), Err(ToonError::InvalidFieldName { .. })));
    }

    #[test]
    fn ac003_type_name_violation_takes_precedence_over_field_name_violation() {
        let opts = ToonOptions {
            type_name: "a[b".into(),
            fields: vec!["x,y".into()],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidTypeName { name: "a[b".to_string() });
    }

    #[test]
    fn ac003_invalid_field_name_error_carries_exact_offending_name() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a,b".into()],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidFieldName { name: "a,b".to_string() });
    }

    #[test]
    fn ac003_multiple_bad_fields_reports_first_in_list_order() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a,b".into(), "c,d".into()],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidFieldName { name: "a,b".to_string() });
    }

    #[test]
    fn ac004b_multiple_violations_return_only_the_first_in_fixed_order() {
        let opts = ToonOptions {
            type_name: "foo[bar".into(),
            fields: vec!["a,b".into()],
            rows: vec![vec![Value::from("x"), Value::from("y")]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidTypeName { name: "foo[bar".to_string() });
    }

    #[test]
    fn ac004b_field_name_violation_takes_precedence_over_row_length_violation() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a,b".into()],
            rows: vec![vec![Value::from("x"), Value::from("y")]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidFieldName { name: "a,b".to_string() });
    }

    #[test]
    fn ac035_default_renders_exact_empty_document() {
        let opts = ToonOptions::default();
        assert_eq!(render_toon(&opts).unwrap(), "[0]{}:\n");
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
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a".into()],
            rows: vec![vec![Value::from("x"), Value::from("y")]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(opts.validate(), Err(ToonError::RowLengthMismatch { row_index: 0, expected: 1, actual: 2 })));
    }

    #[test]
    fn ac004_type_name_violation_takes_precedence_over_row_length_violation() {
        let opts = ToonOptions {
            type_name: "a[b".into(),
            fields: vec!["a".into()],
            rows: vec![vec![Value::from("x"), Value::from("y")]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert_eq!(opts.validate().unwrap_err(), ToonError::InvalidTypeName { name: "a[b".to_string() });
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
        assert_eq!(render_toon(&opts).unwrap(), "t[1]{a}:\n   (50 chars\ntotalCount: 5\nhelp[1]:\n  h\n");
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
    fn ac028_missing_key_renders_as_empty_cell() {
        let result = list("t", &[serde_json::json!({"a": 1, "b": 2}), serde_json::json!({"a": 3})]);
        assert_eq!(result, Ok("t[2]{a,b}:\n  1,2\n  3,\n".to_string()));
    }

    #[test]
    fn ac029_structural_char_in_first_items_key_rejected_before_rendering() {
        let result = list("t", &[serde_json::json!({"a,b": 1})]);
        assert_eq!(result, Err(ToonError::InvalidFieldName { name: "a,b".to_string() }));
    }

    #[test]
    fn ac029_type_name_violation_takes_precedence_over_field_name_violation() {
        let result = list("t[x", &[serde_json::json!({"a,b": 1})]);
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "t[x".to_string() }));
    }

    #[test]
    fn ac029_serialization_failure_takes_precedence_over_field_name_violation() {
        let result = list("t", &[serde_json::json!({"a,b": 1}), serde_json::json!(7)]);
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
    fn ac038_first_item_empty_later_item_nonempty_fills_an_empty_cell() {
        let result = list("t", &[serde_json::json!({}), serde_json::json!({"k": 1})]);
        assert_eq!(result, Ok("t[2]{k}:\n  \n  1\n".to_string()));
    }

    #[test]
    fn ac038_header_is_the_union_of_every_items_keys_in_first_seen_order() {
        let result =
            list("t", &[serde_json::json!({}), serde_json::json!({"k": 1}), serde_json::json!({"a": 1, "b": 2})]);
        assert_eq!(result, Ok("t[3]{k,a,b}:\n  ,,\n  1,,\n  ,1,2\n".to_string()));
    }

    #[test]
    fn ac039_empty_items_slice_renders_exact_empty_document() {
        let result = list("t", &([] as [serde_json::Value; 0]));
        assert_eq!(result, Ok("t[0]{}:\n".to_string()));
    }

    #[test]
    fn ac039_invalid_type_name_rejected_even_with_empty_items_slice() {
        let result = list("a[b", &([] as [serde_json::Value; 0]));
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "a[b".to_string() }));
    }

    #[test]
    fn ac040_all_empty_object_items_render_exact_blank_rows() {
        let result = list("t", &[serde_json::json!({}), serde_json::json!({})]);
        assert_eq!(result, Ok("t[2]{}:\n  \n  \n".to_string()));
    }

    #[test]
    fn ac040_invalid_type_name_rejected_even_with_all_empty_object_items() {
        let result = list("a[b", &[serde_json::json!({}), serde_json::json!({})]);
        assert_eq!(result, Err(ToonError::InvalidTypeName { name: "a[b".to_string() }));
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

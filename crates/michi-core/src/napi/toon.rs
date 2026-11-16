//! The one-shot document renders: `renderToon`, `renderKv`, `renderStatus`, `emptyState` and `emptyStateWithHints`.

use napi::bindgen_prelude::Unknown;
use napi_derive::napi;

use super::input::{self, Budget, List, MAX_FIELDS, MAX_HINTS, MAX_NAME_BYTES, MAX_ROWS, MAX_STRING_BYTES};
use super::num::JsCount;
use super::value::{JsKvItem, JsToonValue};

/// Options for rendering a TOON document (JavaScript-friendly).
#[napi(object, js_name = "ToonOptions")]
#[derive(Debug, Clone)]
pub struct JsToonOptions {
    /// Snake_case type name, e.g. `"issue"`, `"component"`.
    #[napi(js_name = "typeName")]
    pub type_name: String,
    /// Ordered field names for the header.
    pub fields: Vec<String>,
    /// Rows, each a Vec of values parallel to `fields`. Capped at `MAX_ROWS`
    /// rows and `MAX_FIELDS` values per row.
    // Named explicitly: napi-derive resolves a field's type name only for modules it has already expanded.
    #[napi(ts_type = "Array<Array<ToonValue>>")]
    pub rows: Vec<Vec<JsToonValue>>,
    /// Total available count (may exceed `rows.len()` when paginated).
    #[napi(js_name = "totalCount", ts_type = "number")]
    pub total_count: Option<JsCount>,
    /// Agent-facing usage hints. Capped at `MAX_HINTS` entries.
    pub hints: Vec<String>,
    /// The caller's instruction for getting a cut cell in full (e.g. `"full=true"`). Cut cells end in `…` and are
    /// listed in a `truncated[N]{row,field,removed}:` block; when a cell was cut this adds a `help[]` line that
    /// names the block and the instruction. Omitted: the block still reports each cut. Tool-author text, never data.
    #[napi(js_name = "truncationHint")]
    pub truncation_hint: Option<String>,
}

/// Render a TOON list document from options.
///
/// # Errors
///
/// Returns an error if `rows`, `fields`, `hints`, or any row's value count
/// exceeds this module's hard size limits (`MAX_ROWS`, `MAX_FIELDS`,
/// `MAX_HINTS`) — an unbounded caller-supplied collection here would let a
/// single JS call force unbounded synchronous allocation on Node's event
/// loop, which `#[napi(catch_unwind)]` alone does not guard against. The
/// limits are checked on the JS arrays before anything is converted, and
/// the call's cells (at most 1,000,000) and string bytes (at most 16 MiB in
/// all, 1 MiB per cell or hint, 4 KiB for `typeName`, a field name or
/// `truncationHint`) are counted as they are read. Also
/// returns an error if `ToonOptions::validate()` rejects the input — a row
/// whose value count differs from `fields.len()`, a field name repeated in
/// `fields`, a reserved `type_name` (a root key michi writes itself, such as `help` or `error`), or a
/// document over michi-core's shape caps. Any other type
/// or field name is valid and is quoted when TOON 4.4 §7.3 requires.
/// The error `reason` is `toon_validation_failed: ` followed by the `ToonError` Display text.
#[napi(catch_unwind, js_name = "renderToon", ts_args_type = "opts: ToonOptions")]
pub fn render_toon(opts: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let obj = input::object(opts, "opts")?;
    let rows = input::list_prop(&obj, "rows")?;
    let fields = input::list_prop(&obj, "fields")?;
    let hints = input::list_prop(&obj, "hints")?;
    input::check_len("rows", rows.len(), MAX_ROWS)?;
    input::check_len("fields", fields.len(), MAX_FIELDS)?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let opts = JsToonOptions {
        type_name: budget.string_prop(&obj, "typeName", "typeName", MAX_NAME_BYTES)?,
        total_count: input::typed_prop::<JsCount>(&obj, "totalCount")?,
        truncation_hint: budget.opt_string_prop(&obj, "truncationHint", "truncationHint", MAX_NAME_BYTES)?,
        fields: budget.strings(&fields, "fields", MAX_FIELDS, "field name", MAX_NAME_BYTES)?,
        hints: budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?,
        rows: budget.rows(&rows, MAX_ROWS, MAX_FIELDS, "maximum fields per row")?,
    };
    toon_document(opts)
}

/// The document for options whose collection sizes the caller has already bounded (see [`render_toon`]).
fn toon_document(opts: JsToonOptions) -> napi::Result<String> {
    let rows = opts
        .rows
        .into_iter()
        .map(|row| row.into_iter().map(|v| Ok(v.into_cell()?.into())).collect::<napi::Result<Vec<_>>>())
        .collect::<napi::Result<Vec<_>>>()?;
    let mut toon_opts = crate::toon::ToonOptions::new(opts.type_name, opts.fields, rows)
        .total_count(opts.total_count.map(JsCount::get))
        .hints(opts.hints);
    if let Some(hint) = opts.truncation_hint {
        toon_opts = toon_opts.truncation_hint(hint);
    }
    crate::toon::render_toon(&toon_opts).map_err(|e| napi::Error::from_reason(format!("toon_validation_failed: {e}")))
}

/// Render an explicit empty-state response.
///
/// # Errors
///
/// Returns an error if `type_name` is a reserved root key (one michi writes itself, such as `help`). The reason is
/// `toon_validation_failed: ` followed by the `ToonError` text, as `renderToon` reports it. Also rejects a `typeName`
/// over 4 KiB, as `emptyStateWithHints` does.
#[napi(catch_unwind)]
pub fn empty_state(type_name: String) -> napi::Result<String> {
    input::check_bytes("typeName", &type_name, MAX_NAME_BYTES)?;
    crate::empty::try_empty_state(&type_name)
        .map_err(|e| napi::Error::from_reason(format!("toon_validation_failed: {e}")))
}

/// Render an empty state with a `help[N]: …` block: `{typeName}: []`, `totalCount: 0`, then the hints.
///
/// # Errors
///
/// Returns an error if `type_name` is a reserved root key (one michi writes itself, such as `help`), with the reason
/// `toon_validation_failed: ` followed by the `ToonError` text, as `emptyState` reports it. Also rejects more than
/// 10,000 hints, a `typeName` over 4 KiB, a hint over 1 MiB, and more than 16 MiB of strings per call.
#[napi(catch_unwind, js_name = "emptyStateWithHints", ts_args_type = "typeName: string, hints: Array<string>")]
pub fn empty_state_with_hints(type_name: Unknown<'_>, hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let type_name = budget.string(type_name, "typeName", MAX_NAME_BYTES)?;
    let hints: Vec<crate::hints::Hint> =
        budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?.into_iter().map(Into::into).collect();
    crate::empty::try_empty_state_with_hints(&type_name, &hints)
        .map_err(|e| napi::Error::from_reason(format!("toon_validation_failed: {e}")))
}

/// Render key-value pairs as a TOON 4.4 root object: `key: value` lines, then `totalCount` and `help`.
///
/// # Errors
///
/// Rejects a duplicate key or a reserved root key such as `help` or `message` (`kv_validation_failed: …`),
/// and inputs over the caps: 1,000 items, 10,000 hints, 4 KiB per key, 1 MiB per value or hint, 16 MiB of strings
/// per call.
#[napi(
    catch_unwind,
    js_name = "renderKv",
    ts_args_type = "items: Array<KvItem>, totalCount: number | undefined | null, hints: Array<string>"
)]
pub fn render_kv(items: Unknown<'_>, total_count: Option<JsCount>, hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let items = List::new(items, "items")?;
    let hints = List::new(hints, "hints")?;
    input::check_len("items", items.len(), MAX_FIELDS)?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let items = budget.kv_items(&items, MAX_FIELDS)?;
    let hints = budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?;
    kv_document(items, total_count, hints)
}

fn kv_document(items: Vec<JsKvItem>, total_count: Option<JsCount>, hints: Vec<String>) -> napi::Result<String> {
    let converted = items
        .into_iter()
        .map(|i| Ok(crate::kv::KvItem::new(i.key, i.value.into_cell()?.into())))
        .collect::<napi::Result<Vec<_>>>()?;
    let hints: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    crate::kv::render_kv(&converted, total_count.map(JsCount::get), &hints)
        .map_err(|e| napi::Error::from_reason(format!("kv_validation_failed: {e}")))
}

/// A component status item for `renderStatus`.
#[napi(object, js_name = "StatusItem")]
#[derive(Debug, Clone)]
pub struct JsStatusItem {
    /// Component key.
    pub key: String,
    /// Component value.
    #[napi(ts_type = "ToonValue")]
    pub value: JsToonValue,
    /// Health state: `"ok"`, `"degraded: <reason>"`, or `"error: <reason>"`.
    pub health: Option<String>,
}

/// Render a P8 content-first orientation response: `tool`, `description`, the TOON 4.4 table
/// `items[N]{key,value,health,reason}:` and `help`. An item without `health` renders `ok`.
///
/// # Errors
///
/// Rejects an unknown `health`, more than 1,000 items or 10,000 hints, a `toolName` or key over 4 KiB, a
/// `description`, value, `health` or hint over 1 MiB, and more than 16 MiB of strings per call.
#[napi(
    catch_unwind,
    js_name = "renderStatus",
    ts_args_type = "toolName: string, description: string, items: Array<StatusItem>, hints?: Array<string> | undefined | null"
)]
pub fn render_status(
    tool_name: Unknown<'_>,
    description: Unknown<'_>,
    items: Unknown<'_>,
    hints: Option<Unknown<'_>>,
) -> napi::Result<String> {
    let mut budget = Budget::new();
    let items = List::new(items, "items")?;
    let hints = List::optional(hints, "hints")?;
    input::check_len("items", items.len(), MAX_FIELDS)?;
    input::check_len("hints", hints.as_ref().map_or(0, List::len), MAX_HINTS)?;
    let tool_name = budget.string(tool_name, "toolName", MAX_NAME_BYTES)?;
    let description = budget.string(description, "description", MAX_STRING_BYTES)?;
    let items = budget.status_items(&items, MAX_FIELDS)?;
    let hints = match hints {
        Some(hints) => budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?,
        None => Vec::new(),
    };
    status_document(tool_name, description, items, hints)
}

/// A `health` string as the status table's [`crate::status::Health`].
fn parse_health(health: Option<&str>) -> napi::Result<crate::status::Health> {
    use crate::status::Health;
    match health {
        Some("ok") | None => Ok(Health::Ok),
        Some(h) if h.starts_with("degraded:") => {
            Ok(Health::Degraded(h.trim_start_matches("degraded:").trim().to_string()))
        }
        Some(h) if h.starts_with("error:") => Ok(Health::Error(h.trim_start_matches("error:").trim().to_string())),
        Some(other) => Err(napi::Error::from_reason(format!(
            "unknown health value {other:?}: expected \"ok\", \"degraded: <reason>\", or \"error: <reason>\""
        ))),
    }
}

fn status_document(
    tool_name: String,
    description: String,
    items: Vec<JsStatusItem>,
    hints: Vec<String>,
) -> napi::Result<String> {
    let status_items = items
        .into_iter()
        .map(|i| {
            let health = parse_health(i.health.as_deref())?;
            Ok(crate::status::StatusItem::new(i.key, i.value.into_cell()?.into()).health(health))
        })
        .collect::<napi::Result<Vec<_>>>()?;
    let hints: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    Ok(crate::status::StatusResponse::new(tool_name, description, status_items).with_hints(hints).render())
}

#[cfg(test)]
mod tests {
    use super::super::num::{JsDecimals, JsFloat, JsInt};
    use super::*;

    pub(crate) fn value(t: &str) -> JsToonValue {
        JsToonValue { r#type: t.to_string(), ..JsToonValue::default() }
    }

    fn opts(type_name: &str, fields: &[&str], rows: Vec<Vec<JsToonValue>>) -> JsToonOptions {
        JsToonOptions {
            type_name: type_name.to_string(),
            fields: fields.iter().map(|f| (*f).to_string()).collect(),
            rows,
            total_count: None,
            hints: vec![],
            truncation_hint: None,
        }
    }

    fn str_cell(s: &str) -> JsToonValue {
        JsToonValue { str_val: Some(s.to_string()), ..value("str") }
    }

    fn int_cell(n: f64) -> JsToonValue {
        JsToonValue { int_val: Some(JsInt::try_from(n).expect("a valid int")), ..value("int") }
    }

    #[test]
    fn render_toon_basic() {
        let mut o = opts("issue", &["id"], vec![vec![int_cell(1.0)]]);
        o.total_count = Some(JsCount::try_from(1.0).expect("1 is a valid count"));
        let out = toon_document(o).expect("valid input renders");
        assert!(out.starts_with("issue[1]{id}:"));
    }

    #[test]
    fn render_toon_renders_int_beyond_i32_range() {
        let o = opts("t", &["a"], vec![vec![int_cell(1_755_000_000_000.0)]]);
        assert_eq!(toon_document(o).expect("valid input renders"), "t[1]{a}:\n  1755000000000");
    }

    #[test]
    fn render_toon_rejects_row_length_mismatch() {
        let err = toon_document(opts("t", &["a", "b"], vec![vec![str_cell("x")]])).expect_err("mismatched row");
        assert_eq!(err.reason, "toon_validation_failed: row 0 has 1 values but 2 fields declared");
    }

    #[test]
    fn render_toon_quotes_structural_char_in_type_name() {
        let out = toon_document(opts("a[b]", &["x"], vec![vec![str_cell("v")]])).expect("quoted per TOON 4.4 §7.3");
        assert_eq!(out, "\"a[b]\"[1]{x}:\n  v");
    }

    #[test]
    fn render_toon_rejects_reserved_type_name() {
        let err = toon_document(opts("help", &["x"], vec![])).expect_err("reserved type_name must be rejected");
        assert_eq!(
            err.reason,
            "toon_validation_failed: type_name \"help\" is a reserved document key (totalCount, help, recovery, \
             truncated, budget, sections, provenance, error, message, causes, exit_code)"
        );
    }

    #[test]
    fn render_toon_quotes_structural_char_in_field_name() {
        let out = toon_document(opts("t", &["a,b"], vec![vec![str_cell("v")]])).expect("quoted per TOON 4.4 §7.3");
        assert_eq!(out, "t[1]{\"a,b\"}:\n  v");
    }

    #[test]
    fn render_toon_rejects_duplicate_field_name() {
        let err = toon_document(opts("t", &["a", "a"], vec![])).expect_err("duplicate field name");
        assert_eq!(err.reason, "toon_validation_failed: field \"a\" appears more than once");
    }

    #[test]
    fn render_toon_truncation_hint_is_optional() {
        let long = || vec![vec![str_cell(&"a".repeat(500))]];
        let with = |hint: Option<&str>| JsToonOptions {
            truncation_hint: hint.map(str::to_string),
            ..opts("t", &["a"], long())
        };
        let cut = format!("t[1]{{a}}:\n  {}\u{2026}\ntruncated[1]{{row,field,removed}}:\n  0,a,301", "a".repeat(199));
        assert_eq!(toon_document(with(None)).expect("valid input renders"), cut);
        assert_eq!(
            toon_document(with(Some("full=true"))).expect("valid input renders"),
            format!("{cut}\nhelp[1]: Cut cells are listed in the truncated block; use full=true for the full value")
        );
    }

    #[test]
    fn render_toon_rejects_int_with_missing_int_val() {
        let err = toon_document(opts("t", &["a"], vec![vec![value("int")]])).expect_err("missing intVal");
        assert!(err.reason.contains("intVal"), "error should name the missing field, got: {}", err.reason);
    }

    #[test]
    fn render_toon_rejects_unknown_type_even_with_a_present_payload() {
        let mut v = value("integer");
        v.int_val = Some(JsInt::try_from(42.0).expect("42 is valid"));
        let err = toon_document(opts("t", &["a"], vec![vec![v]])).expect_err("unrecognized type must error");
        assert!(err.reason.contains("integer"), "error should name the offending type, got: {}", err.reason);
    }

    #[test]
    fn render_toon_accepts_explicit_null_with_no_payload() {
        assert!(toon_document(opts("t", &["a"], vec![vec![value("null")]])).is_ok());
    }

    #[test]
    fn empty_state_renders() {
        assert_eq!(empty_state("issue".to_string()).expect("valid name"), "issue: []\ntotalCount: 0");
    }

    // emptyStateWithHints caps typeName at 4 KiB; emptyState took any length.
    #[test]
    fn empty_state_caps_the_type_name_like_empty_state_with_hints() {
        let err = empty_state("t".repeat(MAX_NAME_BYTES + 1)).expect_err("over 4 KiB");
        assert_eq!(
            err.reason,
            format!("typeName byte length {} exceeds maximum of {MAX_NAME_BYTES}", MAX_NAME_BYTES + 1)
        );
        assert!(empty_state("t".repeat(MAX_NAME_BYTES)).is_ok());
    }

    #[test]
    fn empty_state_rejects_a_reserved_type_name_like_render_toon() {
        for name in ["totalCount", "help", "recovery"] {
            let err = empty_state(name.to_string()).expect_err("reserved root key");
            assert!(err.reason.starts_with("toon_validation_failed: "), "{}", err.reason);
            assert!(err.reason.contains(name), "{}", err.reason);
        }
    }

    fn kv_item(k: &str) -> JsKvItem {
        JsKvItem { key: k.to_string(), value: value("null") }
    }

    #[test]
    fn render_kv_rejects_bool_with_missing_bool_val() {
        let items = vec![JsKvItem { key: "k".to_string(), value: value("bool") }];
        let err = kv_document(items, None, vec![]).expect_err("missing boolVal must error, not default to false");
        assert!(err.reason.contains("boolVal"), "error should name the missing field, got: {}", err.reason);
    }

    #[test]
    fn render_kv_rejects_duplicate_and_reserved_keys() {
        let err = kv_document(vec![kv_item("a"), kv_item("a")], None, vec![]).expect_err("duplicate key");
        assert_eq!(err.reason, "kv_validation_failed: key \"a\" appears more than once");
        let err = kv_document(vec![kv_item("help")], None, vec![]).expect_err("reserved key");
        assert_eq!(
            err.reason,
            "kv_validation_failed: key \"help\" is a reserved document key (totalCount, help, recovery, truncated, \
             budget, sections, provenance, error, message, causes, exit_code)"
        );
    }

    // An unrecognized type must reject, not silently discard a possibly-present payload (PROOF-NAPI-001).
    // `"null"` remains the sole legitimate no-payload discriminant.
    #[test]
    fn kv_rejects_unrecognized_value_type_instead_of_defaulting_to_missing() {
        let item = JsKvItem { key: "id".into(), value: value("unrecognized-type") };
        let err = kv_document(vec![item], None, vec![]).expect_err("unrecognized type must error");
        assert!(err.reason.contains("unrecognized-type"), "error should name the offending type, got: {}", err.reason);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn render_kv_napi_float_default_decimals() {
        let item = JsKvItem {
            key: "score".into(),
            value: JsToonValue {
                float_val: Some(JsFloat::try_from(3.14159).expect("3.14159 is finite")),
                ..value("float")
            },
        };
        let out = kv_document(vec![item], None, vec![]).unwrap();
        assert_eq!(out, "score: 3.14159", "the shortest exact decimals by default, then canonical (no padding)");
    }

    #[test]
    fn render_kv_napi_float_without_decimals_keeps_small_and_long_values_exact() {
        let float = |f: f64| JsToonValue { float_val: Some(JsFloat::try_from(f).expect("finite")), ..value("float") };
        for (f, expected) in [
            (1e-7, "x: 1e-7"),
            (0.000_000_123_4, "x: 1.234e-7"),
            (0.123_456_789, "x: 0.123456789"),
            (2.0, "x: 2"),
            (1e21, "x: 1e+21"),
            // Need more than 255 decimal places: these rendered `x: 0`.
            (1e-256, "x: 1e-256"),
            (1e-300, "x: 1e-300"),
            (5e-324, "x: 5e-324"),
            (-1.234_567_890_123_456_8e-250, "x: -1.2345678901234568e-250"),
        ] {
            let out = kv_document(vec![JsKvItem { key: "x".into(), value: float(f) }], None, vec![]).unwrap();
            assert_eq!(out, expected, "{f}");
        }
        let status = status_document(
            "tool".into(),
            "desc".into(),
            vec![JsStatusItem { key: "lag".into(), value: float(1e-7), health: None }],
            vec![],
        )
        .unwrap();
        assert!(status.contains("lag,1e-7,ok,null"), "status items use the same rule: {status}");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn render_kv_napi_float_custom_decimals() {
        let item = JsKvItem {
            key: "score".into(),
            value: JsToonValue {
                float_val: Some(JsFloat::try_from(3.14159).expect("3.14159 is finite")),
                decimals_val: Some(JsDecimals::try_from(2.0).expect("2 is a valid decimals value")),
                ..value("float")
            },
        };
        let out = kv_document(vec![item], None, vec![]).unwrap();
        assert!(out.contains("3.14"), "expected 2 decimal places, got: {out}");
        assert!(!out.contains("3.141"), "must not have more than 2 decimal places, got: {out}");
    }

    fn status(health: &str) -> Vec<JsStatusItem> {
        vec![JsStatusItem { key: "k".into(), value: str_cell("v"), health: Some(health.into()) }]
    }

    #[test]
    fn render_status_health_forms() {
        for (health, tail) in [
            ("ok", "\n  k,v,ok,null"),
            ("degraded: high latency", "\n  k,v,degraded,high latency"),
            ("error: connection refused", "\n  k,v,error,connection refused"),
        ] {
            let out = status_document("my-tool".into(), "desc".into(), status(health), vec![]).unwrap();
            assert!(out.contains("tool:"), "got: {out}");
            assert!(out.ends_with(tail), "got: {out}");
        }
    }

    #[test]
    fn render_status_napi_unknown_health_returns_error() {
        // Missing the colon and reason.
        let err = status_document("t".into(), "d".into(), status("degraded"), vec![]).expect_err("unrecognized health");
        assert_eq!(
            err.reason,
            "unknown health value \"degraded\": expected \"ok\", \"degraded: <reason>\", or \"error: <reason>\""
        );
    }
}

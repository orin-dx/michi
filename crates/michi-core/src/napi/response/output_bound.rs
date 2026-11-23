//! The output bound, measured: for a hostile matrix of responses, every render is at most
//! [`input::MAX_OUTPUT_FACTOR`] times the bytes the response holds, plus [`input::OUTPUT_ENVELOPE`].
//!
//! The matrix varies what a writer can repeat: control characters (the worst escape), 4 KiB type, section and field
//! names that a `truncated` row or a `totalCount` row writes again for every cut cell or section, cut cells, many
//! tiny strings, numbers at their widest, and every outcome section. A case that failed validation would render a
//! short error body and prove nothing, so each case also asserts it is not an error.

use super::super::num::{JsDecimals, JsFloat, JsInt};
use super::*;

/// The bytes a case writes for `unit`: `bytes` UTF-8 bytes, rounded down to whole characters.
fn text(unit: char, bytes: usize) -> String {
    unit.to_string().repeat(bytes / unit.len_utf8())
}

/// One character of `unit`.
fn one(unit: char) -> String {
    unit.to_string()
}

/// A name of about `bytes` bytes that differs from every other name `distinct` makes: the unit, then a letter.
fn name(unit: char, bytes: usize, distinct: char) -> String {
    format!("{}{distinct}", text(unit, bytes - 1))
}

fn count(n: u32) -> JsCount {
    JsCount::try_from(f64::from(n)).expect("a valid count")
}

fn tag(t: &str) -> JsToonValue {
    JsToonValue { r#type: t.to_string(), ..JsToonValue::default() }
}

fn str_cell(s: String) -> JsToonValue {
    JsToonValue { str_val: Some(s), ..tag("str") }
}

fn int_cell(n: f64) -> JsToonValue {
    JsToonValue { int_val: Some(JsInt::try_from(n).expect("an int in range")), ..tag("int") }
}

fn float_cell(f: f64, decimals: Option<f64>) -> JsToonValue {
    JsToonValue {
        float_val: Some(JsFloat::try_from(f).expect("a finite float")),
        decimals_val: decimals.map(|d| JsDecimals::try_from(d).expect("0..=20 decimals")),
        ..tag("float")
    }
}

/// The three outputs a response can produce, as bytes: the text, the JSON, and `toCallToolResult` (its text and its
/// JSON together; the JSON is parsed into an object there, which holds the same strings).
fn output_sizes(r: &JsAgentResponse) -> [usize; 3] {
    let rendered = r.render_text().expect("renders");
    assert!(!rendered.contains("_validation_failed"), "the case failed validation, so it measures nothing");
    let text = rendered.len();
    let json = r.render_json().expect("renders").len();
    r.to_call_tool_result().expect("builds");
    [text, json, text + json]
}

/// Rows of `cells` copies of a string that is cut (201 chars), under `fields` names.
fn cut_rows(unit: char, rows: usize, fields: usize) -> Vec<Vec<JsToonValue>> {
    (0..rows).map(|_| (0..fields).map(|_| str_cell(text(unit, 201 * unit.len_utf8()))).collect()).collect()
}

fn field_names(unit: char, bytes: usize, n: usize) -> Vec<String> {
    (0..n).map(|i| name(unit, bytes, char::from(b'a' + u8::try_from(i).expect("a few fields")))).collect()
}

/// One table whose long field names (and long type name) a `truncated` row writes again for every cut cell.
fn cut_table(unit: char, name_bytes: usize, rows: usize) -> JsAgentResponse {
    named_cut_table(unit, name_bytes, name_bytes, rows)
}

/// [`cut_table`] with the type name and the field names at different lengths, so that charging one cannot pay for the
/// other.
fn named_cut_table(unit: char, type_bytes: usize, field_bytes: usize, rows: usize) -> JsAgentResponse {
    let mut r = JsAgentResponse::new(name(unit, type_bytes, 't')).expect("a valid type name");
    r.add_items(cut_rows(unit, rows, 3), field_names(unit, field_bytes, 3), Some(count(9_000_000))).expect("fits");
    r
}

/// A table with a long type name beside one cut cell of a section with a short name: the two share the `truncated`
/// block, so its `section` column writes the long name for every cut cell of the table.
fn long_name_beside_a_cut_section(unit: char) -> JsAgentResponse {
    let mut r = named_cut_table(unit, MAX_NAME_BYTES, 8, 60);
    let step = JsPipelineStep { id: text(unit, 201), name: "n".into(), status: "completed".into(), reason: None };
    r.add_pipeline("s".into(), vec![step]).expect("fits");
    r
}

/// A long section name that an empty table writes in its `name: []` line and again in the `totalCount` table (two
/// sections report a total, so it is a table with a `section` column), and nothing else repeats. Once for the list
/// that carries the type name, and once for an outcome section.
fn name_in_header_and_total(unit: char) -> Vec<JsAgentResponse> {
    let mut outcome = JsAgentResponse::new("t".into()).expect("a valid type name");
    outcome.add_items(vec![vec![str_cell("x".into())]], vec!["f".into()], Some(count(9))).expect("fits");
    outcome.add_pipeline(name(unit, MAX_NAME_BYTES, 's'), Vec::new()).expect("fits");
    let mut list = JsAgentResponse::new(name(unit, MAX_NAME_BYTES, 't')).expect("a valid type name");
    list.add_items(vec![vec![str_cell("x".into())]], vec!["f".into()], Some(count(9))).expect("fits");
    list.add_pipeline("s".into(), Vec::new()).expect("fits");
    vec![outcome, list]
}

/// Every composed section at once, each with a long name and cut cells, so two or more sections cut and the
/// `truncated` and `totalCount` tables each carry a `section` column.
fn shared_tables(unit: char, name_bytes: usize, rows: u32) -> JsAgentResponse {
    let mut r = named_cut_table(unit, name_bytes, 8, usize::try_from(rows).expect("a few rows"));
    let long = |bytes| text(unit, bytes);
    let completed = (0..rows).map(|i| JsCompletedOp { index: count(i), operation: long(300) }).collect();
    let failed = (0..rows)
        .map(|i| JsFailedOp {
            index: count(rows + i),
            operation: long(300),
            reason: long(300),
            recovery: Some(super::super::hints::JsRecoveryHint { tool: long(100), reason: Some(long(100)) }),
        })
        .collect();
    let skipped = (0..rows)
        .map(|i| JsSkippedOp { index: count(2 * rows + i), operation: long(300), reason: Some(long(300)) })
        .collect();
    r.add_partial_success(name(unit, name_bytes, 'p'), completed, failed, skipped).expect("fits");
    let steps = (0..rows)
        .map(|_| JsPipelineStep { id: long(300), name: long(300), status: "completed".into(), reason: None })
        .collect();
    r.add_pipeline(name(unit, name_bytes, 's'), steps).expect("fits");
    let items = (0..rows)
        .map(|i| JsStatusItem {
            key: name(unit, 300, char::from(b'a' + u8::try_from(i).expect("a few items"))),
            value: str_cell(long(300)),
            health: Some(format!("degraded: {}", long(300))),
        })
        .collect();
    r.add_status(long(300), long(300), items).expect("fits");
    r
}

/// Several KV sections with totals, so the `totalCount` table writes the long type name once per section, and values
/// at their widest.
fn kv_sections(unit: char, name_bytes: usize) -> JsAgentResponse {
    let mut r = JsAgentResponse::new(name(unit, name_bytes, 't')).expect("a valid type name");
    for i in 0..6u8 {
        let key = |j: char| name(unit, name_bytes, j);
        let items = vec![
            JsKvItem { key: key(char::from(b'a' + 4 * i)), value: str_cell(text(unit, 1024)) },
            JsKvItem { key: key(char::from(b'b' + 4 * i)), value: float_cell(f64::MAX, Some(20.0)) },
            JsKvItem { key: key(char::from(b'c' + 4 * i)), value: float_cell(1e-250, None) },
            JsKvItem { key: key(char::from(b'd' + 4 * i)), value: int_cell(-9_007_199_254_740_991.0) },
        ];
        r.add_kv_items(items, Some(count(500))).expect("fits");
    }
    r
}

/// An error with a long message and causes, hints, recovery hints, a truncation hint and human content, beside a cut
/// table that makes the truncation hint render.
fn with_error(unit: char) -> JsAgentResponse {
    let mut r = cut_table(unit, 64, 4);
    r.truncation_hint(text(unit, 200)).expect("fits");
    r.set_error("timeout", text(unit, 5000), (0..20).map(|_| text(unit, 1000)).collect(), None).expect("fits");
    for _ in 0..50 {
        r.hint(text(unit, 1000)).expect("fits");
    }
    for _ in 0..20 {
        r.recovery_hint(text(unit, 100), Some(text(unit, 1000))).expect("fits");
    }
    r.human_content(text(unit, 2000)).expect("fits");
    r.add_already_done(text(unit, 1000), text(unit, 5000)).expect("fits");
    r
}

/// Many tiny strings, at the per-call counts the exports allow: where per-item punctuation (quotes, commas, row
/// breaks, the JSON of a recovery hint) is most of the output.
fn tiny(unit: char) -> JsAgentResponse {
    let one = || one(unit);
    let mut r = JsAgentResponse::new(one()).expect("a valid type name");
    let rows = (0..MAX_ROWS / 4).map(|_| (0..5).map(|_| str_cell(one())).collect()).collect();
    r.add_items(rows, field_names(unit, 2, 5), None).expect("fits");
    for _ in 0..MAX_HINTS {
        r.hint(one()).expect("fits");
        r.recovery_hint(one(), Some(one())).expect("fits");
    }
    r.set_error("not_found", one(), (0..MAX_HINTS).map(|_| one()).collect(), None).expect("fits");
    let kv = (0..26u8).map(|i| JsKvItem { key: format!("{unit}{}", char::from(b'a' + i)), value: str_cell(one()) });
    r.add_kv_items(kv.collect(), None).expect("fits");
    r
}

/// Tiny operations and steps at the caps: every row is mostly punctuation, and each failed one carries a recovery hint.
fn tiny_outcomes(unit: char) -> JsAgentResponse {
    let one = || one(unit);
    let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
    let n = u32::try_from(MAX_STEPS).expect("fits");
    let completed = (0..n).map(|i| JsCompletedOp { index: count(i), operation: one() }).collect();
    let failed = (0..n)
        .map(|i| JsFailedOp {
            index: count(n + i),
            operation: one(),
            reason: one(),
            recovery: Some(super::super::hints::JsRecoveryHint { tool: one(), reason: None }),
        })
        .collect();
    let skipped = (0..n).map(|i| JsSkippedOp { index: count(2 * n + i), operation: one(), reason: None }).collect();
    r.add_partial_success("p".into(), completed, failed, skipped).expect("fits");
    let steps =
        (0..n).map(|_| JsPipelineStep { id: one(), name: one(), status: "completed".into(), reason: None }).collect();
    r.add_pipeline("s".into(), steps).expect("fits");
    r
}

/// How one case fills a response.
type Build<'a> = Box<dyn Fn(&mut JsAgentResponse) + 'a>;

/// One kind of tiny item at its per-call cap, alone, so that nothing else's charge can hide an item that is charged
/// too little.
fn one_kind(unit: char) -> Vec<(String, JsAgentResponse)> {
    let one = || one(unit);
    let n = u32::try_from(MAX_STEPS).expect("fits");
    let recovery = || Some(super::super::hints::JsRecoveryHint { tool: one(), reason: None });
    let builds: Vec<(&str, Build<'_>)> = vec![
        ("hints", Box::new(|r| (0..MAX_HINTS).for_each(|_| r.hint(one()).expect("fits")))),
        ("empty hints", Box::new(|r| (0..MAX_HINTS).for_each(|_| r.hint(String::new()).expect("fits")))),
        ("recovery hints", Box::new(|r| (0..MAX_HINTS).for_each(|_| r.recovery_hint(one(), None).expect("fits")))),
        (
            "recovery hints with reasons",
            Box::new(|r| (0..MAX_HINTS).for_each(|_| r.recovery_hint(one(), Some(one())).expect("fits"))),
        ),
        (
            "causes",
            Box::new(|r| {
                r.set_error("not_found", one(), (0..MAX_HINTS).map(|_| one()).collect(), None).expect("fits");
            }),
        ),
        (
            "kv items",
            Box::new(|r| {
                let items = (0..MAX_FIELDS).map(|i| JsKvItem { key: format!("{unit}{i}"), value: tag("null") });
                r.add_kv_items(items.collect(), None).expect("fits");
            }),
        ),
        (
            "completed operations",
            Box::new(|r| {
                let ops = (0..n).map(|i| JsCompletedOp { index: count(i), operation: one() });
                r.add_partial_success("p".into(), ops.collect(), Vec::new(), Vec::new()).expect("fits");
            }),
        ),
        (
            "failed operations",
            Box::new(|r| {
                let ops =
                    (0..n).map(|i| JsFailedOp { index: count(i), operation: one(), reason: one(), recovery: None });
                r.add_partial_success("p".into(), Vec::new(), ops.collect(), Vec::new()).expect("fits");
            }),
        ),
        (
            "failed operations with recovery",
            Box::new(|r| {
                let ops = (0..n).map(|i| JsFailedOp {
                    index: count(i),
                    operation: one(),
                    reason: one(),
                    recovery: recovery(),
                });
                r.add_partial_success("p".into(), Vec::new(), ops.collect(), Vec::new()).expect("fits");
            }),
        ),
        (
            "skipped operations",
            Box::new(|r| {
                let ops = (0..n).map(|i| JsSkippedOp { index: count(i), operation: one(), reason: Some(one()) });
                r.add_partial_success("p".into(), Vec::new(), Vec::new(), ops.collect()).expect("fits");
            }),
        ),
        (
            "pipeline steps",
            Box::new(|r| {
                let step = || JsPipelineStep { id: one(), name: one(), status: "pending".into(), reason: None };
                r.add_pipeline("s".into(), (0..n).map(|_| step()).collect()).expect("fits");
            }),
        ),
        (
            "status items",
            Box::new(|r| {
                let item = |i: usize| JsStatusItem { key: format!("{unit}{i}"), value: tag("null"), health: None };
                r.add_status(one(), one(), (0..MAX_FIELDS).map(item).collect()).expect("fits");
            }),
        ),
        (
            "rows of one cell",
            Box::new(|r| {
                r.add_items(vec![vec![str_cell(one())]; MAX_ROWS], vec!["a".into()], None).expect("fits");
            }),
        ),
    ];
    builds
        .into_iter()
        .map(|(label, build)| {
            let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
            build(&mut r);
            (label.to_string(), r)
        })
        .collect()
}

/// Tables with no rows: each writes `name: []` and a `totalCount` row that names it again.
fn empty_tables(unit: char, name_bytes: usize) -> JsAgentResponse {
    let mut r = cut_table(unit, name_bytes, 4);
    r.add_partial_success(name(unit, name_bytes, 'p'), Vec::new(), Vec::new(), Vec::new()).expect("fits");
    r.add_pipeline(name(unit, name_bytes, 's'), Vec::new()).expect("fits");
    r
}

/// KV items with one-byte keys and the widest numbers, in a status report and in KV sections.
fn wide_numbers() -> JsAgentResponse {
    let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
    let items = |prefix: &str| {
        (0..MAX_FIELDS / 2)
            .map(|i| JsKvItem {
                key: format!("{prefix}{i}"),
                value: match i % 3 {
                    0 => float_cell(f64::MAX, Some(20.0)),
                    1 => float_cell(1e-250, None),
                    _ => float_cell(-f64::MAX, Some(0.0)),
                },
            })
            .collect::<Vec<_>>()
    };
    r.add_kv_items(items("k"), Some(count(9))).expect("fits");
    let status = items("s").into_iter().map(|i| JsStatusItem { key: i.key, value: i.value, health: None }).collect();
    r.add_status("tool".into(), "desc".into(), status).expect("fits");
    r
}

/// Rows with no cells, and rows of numbers and nulls.
fn bare_cells() -> Vec<JsAgentResponse> {
    let mut empty_rows = JsAgentResponse::new("t".into()).expect("a valid type name");
    empty_rows.add_items(vec![Vec::new(); MAX_ROWS], Vec::new(), None).expect("fits");
    let mut numbers = JsAgentResponse::new("t".into()).expect("a valid type name");
    let row = vec![
        int_cell(-9_007_199_254_740_991.0),
        float_cell(-f64::MAX, None),
        float_cell(-5e-324, None),
        tag("null"),
        JsToonValue { bool_val: Some(false), ..tag("bool") },
    ];
    numbers.add_items(vec![row; MAX_ROWS], ["a", "b", "c", "d", "e"].map(String::from).to_vec(), None).expect("fits");
    vec![empty_rows, numbers, JsAgentResponse::new(String::new()).expect("an empty type name is valid")]
}

/// Every case for `unit`, with the label that names it in a failure.
fn matrix(unit: char) -> Vec<(String, JsAgentResponse)> {
    let cases = vec![
        ("cut table, 4 KiB names".to_string(), cut_table(unit, MAX_NAME_BYTES, 60)),
        ("cut table, 64 B names".to_string(), cut_table(unit, 64, 200)),
        ("cut table, long fields".to_string(), named_cut_table(unit, 8, MAX_NAME_BYTES, 60)),
        ("cut table, long type name".to_string(), named_cut_table(unit, MAX_NAME_BYTES, 8, 60)),
        ("long type name beside a cut section".to_string(), long_name_beside_a_cut_section(unit)),
        ("outcome name in header and total".to_string(), name_in_header_and_total(unit).remove(0)),
        ("type name in header and total".to_string(), name_in_header_and_total(unit).remove(1)),
        ("every section cut, 4 KiB names".to_string(), shared_tables(unit, MAX_NAME_BYTES, 8)),
        ("kv sections with totals".to_string(), kv_sections(unit, MAX_NAME_BYTES)),
        ("error, hints, recovery".to_string(), with_error(unit)),
        ("empty tables, 4 KiB names".to_string(), empty_tables(unit, MAX_NAME_BYTES)),
        ("tiny strings".to_string(), tiny(unit)),
        ("tiny operations".to_string(), tiny_outcomes(unit)),
    ];
    cases
}

/// The cases that do not depend on a character.
fn fixed_cases() -> Vec<(String, JsAgentResponse)> {
    let mut cases = vec![("wide numbers".to_string(), wide_numbers())];
    cases.extend(bare_cells().into_iter().enumerate().map(|(i, r)| (format!("bare cells {i}"), r)));
    cases
}

/// The worst `output / held` ratio (as a percentage) over `cases`, as `(percent, label)`. Panics naming every case
/// whose output is over [`input::MAX_OUTPUT_FACTOR`] times the bytes held plus [`input::OUTPUT_ENVELOPE`].
fn worst_ratio(cases: impl Iterator<Item = (String, JsAgentResponse)>) -> (usize, String) {
    let mut worst: (usize, String) = (0, String::new());
    let mut broken: Vec<String> = Vec::new();
    for (label, r) in cases {
        let held = r.held.total();
        for (surface, out) in ["text", "json", "toCallToolResult"].into_iter().zip(output_sizes(&r)) {
            let limit = input::MAX_OUTPUT_FACTOR * held + input::OUTPUT_ENVELOPE;
            let ratio = out * 100 / held.max(1);
            if out > limit {
                broken.push(format!("{label}, {surface}: {out} bytes for {held} held ({}x)", ratio / 100));
            }
            if ratio > worst.0 {
                worst = (ratio, format!("{label}, {surface}"));
            }
        }
    }
    broken.dedup();
    assert!(
        broken.is_empty(),
        "output over {}x the bytes held + {}:\n{}",
        input::MAX_OUTPUT_FACTOR,
        input::OUTPUT_ENVELOPE,
        broken.join("\n")
    );
    worst
}

fn unit_cases(unit: char) -> impl Iterator<Item = (String, JsAgentResponse)> {
    matrix(unit).into_iter().chain(one_kind(unit)).map(move |(l, r)| (format!("{l}, unit {unit:?}"), r))
}

/// One test per character (every control character and escape trigger, plain text, and multi-byte characters, which
/// cost fewer output bytes per held byte, never more), so the matrix runs in parallel (about 17 s in one test in a debug build, under 3 s as
/// the slowest of these) and a failure names its character.
macro_rules! bound_test {
    ($($name:ident: $unit:expr),* $(,)?) => {$(
        #[test]
        fn $name() {
            worst_ratio(unit_cases($unit));
        }
    )*};
}

bound_test! {
    every_render_of_control_unit_separator_is_within_the_factor: '\u{1f}',
    every_render_of_a_quote_is_within_the_factor: '"',
    every_render_of_a_backslash_is_within_the_factor: '\\',
    every_render_of_a_newline_is_within_the_factor: '\n',
    every_render_of_a_comma_is_within_the_factor: ',',
    every_render_of_plain_text_is_within_the_factor: 'a',
    every_render_of_two_byte_text_is_within_the_factor: '\u{e9}',
    every_render_of_four_byte_text_is_within_the_factor: '\u{1f600}',
}

#[test]
fn the_fixed_cases_are_within_the_factor() {
    worst_ratio(fixed_cases().into_iter());
}

/// `U+0001` is the worst escape (six JSON bytes for one held), and with it the factor is reached, not just an upper
/// bound nobody comes near: a loosened `MAX_OUTPUT_FACTOR` fails here.
#[test]
fn the_worst_character_reaches_the_documented_factor() {
    let worst = worst_ratio(unit_cases('\u{1}'));
    assert!(
        worst.0 > 1500,
        "worst ratio {}/100 ({}): the documented factor of {} is loose",
        worst.0,
        worst.1,
        input::MAX_OUTPUT_FACTOR
    );
}

/// The amplification the review found: a 4 KiB control-character field name that a `truncated` row writes for every
/// cut cell, charged once. 1,000 rows of one 201-char cell held about 261 KB and rendered to 95x in text.
#[test]
fn a_long_field_name_is_charged_for_every_cell_it_can_be_repeated_in() {
    let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
    let field = text('\u{1}', MAX_NAME_BYTES);
    r.add_items(cut_rows('a', 1000, 1), vec![field], None).expect("fits");
    let [text_out, json_out, _] = output_sizes(&r);
    let held = r.held.total();
    assert!(held > 1000 * MAX_NAME_BYTES, "{held} bytes held: each cut cell can write the field name again");
    assert!(text_out <= input::MAX_OUTPUT_FACTOR * held, "{text_out} text bytes for {held} held");
    assert!(json_out <= input::MAX_OUTPUT_FACTOR * held, "{json_out} JSON bytes for {held} held");
}

/// The same, for a section name: a multi-section `truncated` row writes it for every cut cell.
#[test]
fn a_long_section_name_is_charged_for_every_cell_it_can_be_repeated_in() {
    let mut r = cut_table('a', 16, 1);
    r.add_pipeline(
        text('\u{1}', MAX_NAME_BYTES),
        (0..500)
            .map(|_| JsPipelineStep { id: text('a', 201), name: "n".into(), status: "completed".into(), reason: None })
            .collect(),
    )
    .expect("fits");
    let held = r.held.total();
    assert!(held > 500 * MAX_NAME_BYTES, "{held} bytes held: each cut cell can write the section name again");
    assert!(output_sizes(&r)[0] <= input::MAX_OUTPUT_FACTOR * held);
}

/// The cap on what a response holds is what stops a hostile table: 60,000 rows of the review's reproduction used to
/// fit under the cap and then fail to render.
/// An error report as large as one call may carry (10,000 causes, under the 16 MiB string cap) fits a fresh builder:
/// the report must not be lost while handling the failure it describes.
#[test]
fn a_full_error_report_fits_a_fresh_builder() {
    let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
    let causes: Vec<String> = (0..MAX_HINTS).map(|_| text('a', 1_660)).collect();
    r.set_error("external_failure", text('a', 100), causes, None).expect("one call's worth of causes fits");
    let [text_len, json_len, mcp_len] = output_sizes(&r);
    let bound = input::MAX_OUTPUT_FACTOR * input::MAX_INPUT_BYTES + input::OUTPUT_ENVELOPE;
    assert!(text_len.max(json_len).max(mcp_len) <= bound, "rendered past the output bound");
}

#[test]
fn the_reviews_reproduction_is_rejected_before_rendering() {
    let mut r = JsAgentResponse::new("t".into()).expect("a valid type name");
    let field = text('\u{1}', MAX_NAME_BYTES);
    let err = r.add_items(cut_rows('a', 60_000, 1), vec![field], None).expect_err("over the cap");
    assert!(err.reason.starts_with("cumulative byte total"), "{}", err.reason);
}

use michi_core::toon::{render_toon, ToonOptions, Value};
use proptest::prelude::*;

mod support;
use support::toon_parser::{parse, value_to_string};

/// Cell strategy mixing plain alphanumeric text with strings drawn from a
/// charset that includes comma, double-quote, and newline/CR — the exact
/// characters `escape_value` treats specially — so the round-trip test
/// regularly exercises quoting/escaping, not just the happy path.
fn cell_strategy() -> impl Strategy<Value = String> {
    let escaping_char = prop_oneof![
        Just(','),
        Just('"'),
        Just('\\'),
        Just('\n'),
        Just('\r'),
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
    ];
    prop_oneof![
        "[a-zA-Z0-9 ]{0,20}",
        proptest::collection::vec(escaping_char, 0..20).prop_map(|chars| chars.into_iter().collect()),
    ]
}

/// Row/field counts and the `total_count` option are generated first, then
/// the cell grid is generated depending on them (`field_count` columns ×
/// `row_count` rows) via `prop_flat_map` — the `proptest!` macro's `x in
/// strategy` bindings can't reference each other directly, so the dependent
/// composition has to happen outside it.
fn doc_strategy() -> impl Strategy<Value = (String, usize, usize, Option<usize>, Vec<Vec<String>>)> {
    ("[a-z][a-z0-9_]{0,10}", 1usize..4, 0usize..5, proptest::option::of(0usize..10_000)).prop_flat_map(
        |(type_name, field_count, row_count, total_count)| {
            proptest::collection::vec(proptest::collection::vec(cell_strategy(), field_count), row_count)
                .prop_map(move |cells| (type_name.clone(), field_count, row_count, total_count, cells))
        },
    )
}

proptest! {
    #[test]
    fn render_toon_output_is_grammar_valid(
        (type_name, field_count, row_count, total_count, cells) in doc_strategy(),
    ) {
        let fields: Vec<String> = (0..field_count).map(|i| format!("f{i}")).collect();
        // Each cell is generated independently per row×field position (rather
        // than reusing one shared value everywhere), so a bug that
        // transposed rows or misaligned columns would actually be caught by
        // the per-position comparison below.
        let rows: Vec<Vec<Value>> =
            cells.iter().map(|row| row.iter().map(|c| Value::Str(c.clone().into())).collect()).collect();
        let opts = ToonOptions::new(type_name.clone(), fields.clone(), rows.clone()).total_count(total_count);
        let rendered = render_toon(&opts).expect("doc_strategy only generates arity-matched, structural-char-free options");

        let parsed = parse(&rendered).expect("render_toon output must be parseable by the grammar");
        prop_assert_eq!(parsed.type_name, type_name);
        prop_assert_eq!(parsed.fields, fields);
        prop_assert_eq!(parsed.rows.len(), row_count);
        prop_assert_eq!(parsed.total_count, total_count);
        for (parsed_row, original_row) in parsed.rows.iter().zip(rows.iter()) {
            for (parsed_cell, original_value) in parsed_row.iter().zip(original_row.iter()) {
                // Every value round-trips exactly: line breaks, quotes and backslashes are escaped inside quotes.
                prop_assert_eq!(parsed_cell, &value_to_string(original_value));
            }
        }
    }
}

proptest! {
    #[test]
    fn every_rendered_row_has_exactly_field_count_cells(
        (type_name, field_count, row_count, total_count, cells) in doc_strategy(),
    ) {
        let fields: Vec<String> = (0..field_count).map(|i| format!("f{i}")).collect();
        let rows: Vec<Vec<Value>> = cells.iter().map(|row| row.iter().map(|c| Value::Str(c.clone().into())).collect()).collect();
        let opts = ToonOptions::new(type_name, fields, rows).total_count(total_count);
        let rendered = render_toon(&opts).expect("doc_strategy only generates arity-matched, structural-char-free options");
        let parsed = parse(&rendered).expect("render_toon output must be parseable by the grammar");
        for parsed_row in &parsed.rows {
            prop_assert_eq!(parsed_row.len(), field_count);
        }
        prop_assert_eq!(parsed.rows.len(), row_count);
    }
}

fn mismatched_doc_strategy() -> impl Strategy<Value = (String, Vec<String>, Vec<Vec<String>>)> {
    let name_strategy = prop_oneof!["[a-z][a-z0-9_]{0,10}", "[a-z]\\[[a-z]{0,5}"];
    (name_strategy.clone(), proptest::collection::vec(name_strategy, 1usize..3)).prop_flat_map(|(type_name, fields)| {
        let field_count = fields.len();
        proptest::collection::vec(proptest::collection::vec(cell_strategy(), 0usize..field_count + 2), 0usize..4)
            .prop_map(move |rows| (type_name.clone(), fields.clone(), rows))
    })
}

proptest! {
    #[test]
    fn render_toon_succeeds_iff_invariants_hold(
        (type_name, fields, rows) in mismatched_doc_strategy(),
    ) {
        let structural = [',', '[', ']', '{', '}', '\n', '\r'];
        let type_name_ok = !type_name.chars().any(|c| structural.contains(&c));
        let fields_ok = fields.iter().all(|f| !f.chars().any(|c| structural.contains(&c)));
        let rows_ok = rows.iter().all(|r| r.len() == fields.len());
        let expected_ok = type_name_ok && fields_ok && rows_ok;

        let rows_values: Vec<Vec<Value>> = rows.iter().map(|row| row.iter().map(|c| Value::Str(c.clone().into())).collect()).collect();
        let opts = ToonOptions::new(type_name, fields, rows_values);
        let actual_ok = render_toon(&opts).is_ok();
        prop_assert_eq!(actual_ok, expected_ok);
    }
}

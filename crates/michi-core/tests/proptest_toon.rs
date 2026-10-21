use michi_core::toon::{render_toon, ToonOptions, Value};
use proptest::prelude::*;

mod support;
use support::toon_parser::{parse, value_to_prim, Prim};

/// Cell strategy mixing plain alphanumeric text, strings drawn from a charset of the characters TOON quoting and
/// escaping treat specially (delimiter, quote, backslash, colon, brackets, hyphen, hash, whitespace, controls), and
/// arbitrary Unicode — so the round-trip tests exercise every §7.1/§7.2 path, not just the happy path.
fn cell_strategy() -> impl Strategy<Value = String> {
    let escaping_char = prop_oneof![
        Just(','),
        Just('"'),
        Just('\\'),
        Just('\n'),
        Just('\r'),
        Just('\t'),
        Just(':'),
        Just('['),
        Just('}'),
        Just('-'),
        Just('#'),
        Just(' '),
        Just('\u{1}'),
        Just('\u{7f}'),
        proptest::char::range('a', 'z'),
        proptest::char::range('0', '9'),
    ];
    prop_oneof![
        "[a-zA-Z0-9 ]{0,20}",
        proptest::collection::vec(escaping_char, 0..20).prop_map(|chars| chars.into_iter().collect()),
        "(?s).{0,20}",
        Just("true".to_string()),
        Just("null".to_string()),
        Just("-1.5e3".to_string()),
    ]
}

/// Header names: plain keys, keys that must be quoted (§7.3), and arbitrary Unicode.
fn name_strategy() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z][a-z0-9_]{0,10}", "[a-z]\\[[a-z]{0,5}", "(?s).{0,8}"]
}

/// Row/field counts and the `total_count` option are generated first, then the cell grid depending on them
/// (`field_count` columns × `row_count` rows) via `prop_flat_map`.
/// `(type_name, fields, row_count, total_count, cells, hints)`.
type Doc = (String, Vec<String>, usize, Option<usize>, Vec<Vec<String>>, Vec<String>);

fn doc_strategy() -> impl Strategy<Value = Doc> {
    (
        name_strategy().prop_filter("reserved type name", |t| t != "totalCount" && t != "help"),
        proptest::collection::btree_set(name_strategy(), 0..4),
        0usize..5,
        proptest::option::of(0usize..10_000),
        proptest::collection::vec(cell_strategy(), 0..3),
    )
        .prop_flat_map(|(type_name, fields, row_count, total_count, hints)| {
            let fields: Vec<String> = fields.into_iter().collect();
            let field_count = fields.len();
            proptest::collection::vec(proptest::collection::vec(cell_strategy(), field_count), row_count).prop_map(
                move |cells| (type_name.clone(), fields.clone(), row_count, total_count, cells, hints.clone()),
            )
        })
}

proptest! {
    // Every rendered document is canonical TOON 4.4 encoder output (the strict decoder rejects anything else) and
    // decodes back to exactly the input: type name, fields, every cell, totalCount and hints.
    #[test]
    fn render_toon_output_is_conformant_and_round_trips(
        (type_name, fields, row_count, total_count, cells, hints) in doc_strategy(),
    ) {
        let rows: Vec<Vec<Value>> =
            cells.iter().map(|row| row.iter().map(|c| Value::Str(c.clone().into())).collect()).collect();
        let opts = ToonOptions::new(type_name.clone(), fields.clone(), rows.clone())
            .total_count(total_count)
            .hints(hints.clone());
        let rendered = render_toon(&opts).expect("doc_strategy only generates valid options");

        let parsed = parse(&rendered).map_err(|e| TestCaseError::fail(format!("{e}\n{rendered:?}")))?;
        prop_assert_eq!(parsed.type_name, type_name);
        if row_count > 0 {
            prop_assert_eq!(&parsed.fields, &fields);
        }
        prop_assert_eq!(parsed.rows.len(), row_count);
        prop_assert_eq!(parsed.total_count, total_count);
        prop_assert_eq!(parsed.hints, hints);
        for (parsed_row, original_row) in parsed.rows.iter().zip(rows.iter()) {
            prop_assert_eq!(parsed_row.len(), fields.len());
            for (parsed_cell, original_value) in parsed_row.iter().zip(original_row.iter()) {
                prop_assert_eq!(parsed_cell, &value_to_prim(original_value));
            }
        }
    }
}

proptest! {
    // Numeric cells always render as canonical numbers that decode to the same value (§2, §3).
    #[test]
    fn numeric_cells_round_trip(i in any::<i64>(), u in any::<u64>(), f in any::<f64>()) {
        let opts = ToonOptions::new(
            "n",
            vec!["i".into(), "u".into(), "f".into()],
            vec![vec![Value::Int(i), Value::from(u), Value::Float(f)]],
        );
        let rendered = render_toon(&opts).expect("renders");
        let parsed = parse(&rendered).map_err(|e| TestCaseError::fail(format!("{e}\n{rendered:?}")))?;
        prop_assert_eq!(&parsed.rows[0][0], &Prim::Num(i.to_string()));
        prop_assert_eq!(&parsed.rows[0][1], &Prim::Num(u.to_string()));
        match &parsed.rows[0][2] {
            Prim::Null => prop_assert!(!f.is_finite()),
            Prim::Num(text) => {
                let back: f64 = text.parse().expect("JSON number parses");
                prop_assert!(back == f || (back == 0.0 && f == 0.0), "{} -> {}", f, text);
            }
            other => prop_assert!(false, "float decoded as {:?}", other),
        }
    }
}

fn mismatched_doc_strategy() -> impl Strategy<Value = (String, Vec<String>, Vec<Vec<String>>)> {
    let type_strategy = prop_oneof![name_strategy(), Just("help".to_string()), Just("totalCount".to_string())];
    let field_strategy = prop_oneof!["[ab]", name_strategy()];
    (type_strategy, proptest::collection::vec(field_strategy, 1usize..4)).prop_flat_map(|(type_name, fields)| {
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
        let type_name_ok = type_name != "help" && type_name != "totalCount";
        let fields_ok = fields.iter().enumerate().all(|(i, f)| !fields[..i].contains(f));
        let rows_ok = rows.iter().all(|r| r.len() == fields.len());
        let expected_ok = type_name_ok && fields_ok && rows_ok;

        let rows_values: Vec<Vec<Value>> = rows.iter().map(|row| row.iter().map(|c| Value::Str(c.clone().into())).collect()).collect();
        let opts = ToonOptions::new(type_name, fields, rows_values);
        let actual_ok = render_toon(&opts).is_ok();
        prop_assert_eq!(actual_ok, expected_ok);
    }
}

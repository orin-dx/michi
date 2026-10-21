//! TOON 4.4 encoder conformance, one test per applicable MUST/SHOULD rule, grouped by spec section.
//!
//! Target: TOON specification Version 4.4 (Working Draft, 2026-10-08), https://github.com/toon-format/spec/blob/main/SPEC.md.
//! Every expected value below was checked against the spec text, not copied from michi's output. Rules that cannot
//! apply to michi's output (keyed tabular form §9.5, nested field groups §9.3, arrays of arrays §9.2, non-comma
//! delimiters §11, root primitives/arrays §5, decoder rules §4/§14) are listed in docs/spec/02-toon-format.md.
//!
//! The vendored upstream fixtures run separately in `tests/toon_spec_fixtures.rs`.

use michi_core::pipeline::{Pipeline, PipelineStep, StepStatus};
use michi_core::response::{AgentResponse, OutputFormat};
use michi_core::toon::{escape_key, escape_value, render_toon, ToonError, ToonOptions, Value};
use michi_core::{empty_state, empty_state_with_hints, render_hints, Hint};

mod support;
use support::toon_parser::{decode, parse, Node, Prim};

fn one_cell(v: Value) -> String {
    let out = render_toon(&ToonOptions::new("t", vec!["v".into()], vec![vec![v]])).expect("renders");
    decode(&out).unwrap_or_else(|e| panic!("non-conformant output {out:?}: {e}"));
    out.strip_prefix("t[1]{v}:\n  ").expect("single-cell table").to_string()
}

fn cell(s: &str) -> String {
    one_cell(Value::from(s))
}

#[track_caller]
fn assert_conformant(out: &str) {
    if let Err(e) = decode(out) {
        panic!("not conformant TOON 4.4 encoder output: {e}\n{out:?}");
    }
}

// §1.2 / §12 — line separator is LF; encoders MUST NOT use CRLF.
#[test]
fn s1_2_lines_are_separated_by_lf_only() {
    let opts = ToonOptions::new("t", vec!["a".into()], vec![vec![Value::from("x")], vec![Value::from("y\r\nz")]])
        .total_count(Some(2))
        .hints(vec!["h".into()]);
    let out = render_toon(&opts).unwrap();
    assert!(!out.contains('\r'), "{out:?}");
    assert_eq!(out, "t[2]{a}:\n  x\n  \"y\\r\\nz\"\ntotalCount: 2\nhelp[1]: h");
}

// §1.2 — a syntax character matches as one scalar; a combining mark after it does not change it.
#[test]
fn s1_2_delimiter_hyphen_or_hash_followed_by_combining_mark_is_still_quoted() {
    assert_eq!(cell("a,\u{301}b"), "\"a,\u{301}b\"");
    assert_eq!(cell("-\u{301}"), "\"-\u{301}\"");
    assert_eq!(cell("#\u{301}"), "\"#\u{301}\"");
}

// §2 — finite numbers in the canonical range have no exponent.
#[test]
fn s2_canonical_range_numbers_have_no_exponent() {
    assert_eq!(one_cell(Value::Float(1e6)), "1000000");
    assert_eq!(one_cell(Value::Float(1e-6)), "0.000001");
    assert_eq!(one_cell(Value::Float(1e20)), "100000000000000000000");
}

// §2 — no trailing fractional zeros; integral values print as integers.
#[test]
fn s2_no_trailing_zeros_and_integral_floats_print_as_integers() {
    assert_eq!(one_cell(Value::Float(1.5)), "1.5");
    assert_eq!(one_cell(Value::Float(1.0)), "1");
    assert_eq!(one_cell(Value::Float(-7.0)), "-7");
}

// §2 — -0 MUST be normalized to 0.
#[test]
fn s2_negative_zero_is_normalized() {
    assert_eq!(one_cell(Value::Float(-0.0)), "0");
}

// §2 — the fewest significant digits that round-trip.
#[test]
fn s2_floats_use_shortest_round_trip_digits() {
    assert_eq!(one_cell(Value::Float(0.1 + 0.2)), "0.30000000000000004");
    assert_eq!(one_cell(Value::Float(1.0 / 3.0)), "0.3333333333333333");
    assert_eq!(one_cell(Value::Float(-0.000001)), "-0.000001");
}

// §2 — among shortest candidates the one closest to x is emitted (upstream fixture primitives#38: the binary64 value
// is exactly 123456789012345683968).
#[test]
fn s2_large_integral_float_uses_closest_shortest_digits() {
    assert_eq!(one_cell(Value::Float(123_456_789_012_345_683_968.0)), "123456789012345680000");
}

// §2 — outside the canonical range exponent notation MAY be used; SHOULD use lowercase `e` and an explicit sign.
#[test]
fn s2_out_of_range_numbers_use_lowercase_signed_exponent() {
    assert_eq!(one_cell(Value::Float(1e21)), "1e+21");
    assert_eq!(one_cell(Value::Float(1.5e300)), "1.5e+300");
    assert_eq!(one_cell(Value::Float(1e-7)), "1e-7");
    assert_eq!(one_cell(Value::Float(-2.5e-10)), "-2.5e-10");
}

// §2 — integers are numbers in their exact decimal form, including u64 values above i64::MAX (michi's numeric domain
// is i64 ∪ u64 ∪ f64, so these are in-domain and MUST NOT be stringified).
#[test]
fn s2_integers_including_u64_above_i64_max_are_unquoted_numbers() {
    assert_eq!(one_cell(Value::Int(i64::MIN)), "-9223372036854775808");
    assert_eq!(one_cell(Value::from(u64::MAX)), "18446744073709551615");
    assert_eq!(Value::from(u64::MAX), Value::UInt(u64::MAX));
    assert_eq!(Value::from(usize::MAX), Value::UInt(usize::MAX as u64));
}

// §2 — numeric cells are never truncated (the cell limit applies to strings only).
#[test]
fn s2_large_unsigned_integer_is_never_truncated() {
    let opts = ToonOptions::new("t", vec!["n".into()], vec![vec![Value::from(u64::MAX)]]).max_cell_len(5);
    assert_eq!(render_toon(&opts).unwrap(), "t[1]{n}:\n  18446744073709551615");
}

// §2 — booleans and null are lowercase literals.
#[test]
fn s2_booleans_and_null_are_lowercase_literals() {
    assert_eq!(one_cell(Value::Bool(true)), "true");
    assert_eq!(one_cell(Value::Bool(false)), "false");
    assert_eq!(one_cell(Value::Null), "null");
}

// §3 — NaN and ±Infinity normalize to null.
#[test]
fn s3_non_finite_floats_become_null() {
    assert_eq!(one_cell(Value::Float(f64::NAN)), "null");
    assert_eq!(one_cell(Value::Float(f64::INFINITY)), "null");
    assert_eq!(one_cell(Value::Float(f64::NEG_INFINITY)), "null");
}

// §5 — a michi list document is a root object (type entry, totalCount, help), never a root array; the empty type
// name is the empty key, quoted per §7.3.
#[test]
fn s5_document_is_a_root_object_even_with_an_empty_type_name() {
    assert_eq!(render_toon(&ToonOptions::default()).unwrap(), "\"\": []");
    let out = render_toon(&ToonOptions::new("", vec!["a".into()], vec![vec![Value::Int(1)]])).unwrap();
    assert_eq!(out, "\"\"[1]{a}:\n  1");
    assert_conformant(&out);
}

// §5.1 — encoders MUST NOT emit comment lines: values and keys starting with `#` are quoted.
#[test]
fn s5_1_no_line_can_start_with_hash() {
    assert_eq!(cell("#x"), "\"#x\"");
    assert_eq!(escape_key("#x"), "\"#x\"");
    let out = render_toon(&ToonOptions::new("#t", vec!["#f".into()], vec![vec![Value::from("#v")]])).unwrap();
    assert_eq!(out, "\"#t\"[1]{\"#f\"}:\n  \"#v\"");
    assert!(!out.lines().any(|l| l.trim_start().starts_with('#')));
}

// §6 — tabular header shape `key[N]{f1,f2}:`; the comma delimiter is declared by the absence of a delimiter symbol.
#[test]
fn s6_tabular_header_shape_with_implicit_comma_delimiter() {
    let opts =
        ToonOptions::new("issues", vec!["id".into(), "state".into()], vec![vec![Value::Int(1), Value::from("open")]]);
    assert_eq!(render_toon(&opts).unwrap(), "issues[1]{id,state}:\n  1,open");
}

// §6 — a field list MUST contain at least one field; `{}` is a syntax error (§14.2).
#[test]
fn s6_no_empty_field_list_is_ever_emitted() {
    for opts in [
        ToonOptions::new("t", vec![], vec![]),
        ToonOptions::new("t", vec![], vec![vec![], vec![]]),
        ToonOptions::new("t", vec!["a".into()], vec![]),
    ] {
        let out = render_toon(&opts).unwrap();
        assert!(!out.contains("{}"), "{out:?}");
        assert_conformant(&out);
    }
}

// §6 / §13.1 — declared lengths match the actual row, item and value counts.
#[test]
fn s6_declared_lengths_match_counts() {
    let rows: Vec<Vec<Value>> = (0..3).map(|i| vec![Value::Int(i)]).collect();
    let opts = ToonOptions::new("t", vec!["n".into()], rows).hints(vec!["a".into(), "b".into()]);
    let parsed = parse(&render_toon(&opts).unwrap()).unwrap();
    assert_eq!(parsed.rows.len(), 3);
    assert_eq!(parsed.hints.len(), 2);
}

// §7.1 — `\\`, `\"`, `\n`, `\r`, `\t` escapes.
#[test]
fn s7_1_named_escapes() {
    assert_eq!(cell("a\\b,"), "\"a\\\\b,\"");
    assert_eq!(cell("say \"hi\""), "\"say \\\"hi\\\"\"");
    assert_eq!(cell("l1\nl2"), "\"l1\\nl2\"");
    assert_eq!(cell("l1\rl2"), "\"l1\\rl2\"");
    assert_eq!(cell("a\tb"), "\"a\\tb\"");
}

// §7.1 — other C0 controls are `\uXXXX` with lowercase hex.
#[test]
fn s7_1_other_c0_controls_use_lowercase_unicode_escapes() {
    assert_eq!(cell("a\u{0}b"), "\"a\\u0000b\"");
    assert_eq!(cell("a\u{4}\u{8}\u{c}b"), "\"a\\u0004\\u0008\\u000cb\"");
    assert_eq!(cell("x\u{1f}y"), "\"x\\u001fy\"");
    assert_eq!(cell("\u{1b}[31m"), "\"\\u001b[31m\"");
}

// §7.1 — DEL, C1 controls, line/paragraph separators and supplementary scalars are emitted literally.
#[test]
fn s7_1_everything_else_is_literal_utf8() {
    assert_eq!(cell("a:\u{7f}\u{85}\u{2028}b"), "\"a:\u{7f}\u{85}\u{2028}b\"");
    assert_eq!(cell("a,🚀"), "\"a,🚀\"");
    assert_eq!(cell("🚀"), "🚀");
}

// §7.2 — empty string is quoted.
#[test]
fn s7_2_empty_string_is_quoted() {
    assert_eq!(cell(""), "\"\"");
}

// §7.2 — leading or trailing SP/HTAB is quoted.
#[test]
fn s7_2_leading_or_trailing_whitespace_is_quoted() {
    assert_eq!(cell(" x"), "\" x\"");
    assert_eq!(cell("x "), "\"x \"");
    assert_eq!(cell("  "), "\"  \"");
    assert_eq!(cell("\tx"), "\"\\tx\"");
    // NBSP is ordinary content (§1.2), not whitespace.
    assert_eq!(cell("\u{a0}x"), "\u{a0}x");
}

// §7.2 — exactly `true`, `false`, `null` (case-sensitive) are quoted.
#[test]
fn s7_2_literal_lookalikes_are_quoted_case_sensitively() {
    assert_eq!(cell("true"), "\"true\"");
    assert_eq!(cell("false"), "\"false\"");
    assert_eq!(cell("null"), "\"null\"");
    assert_eq!(cell("True"), "True");
    assert_eq!(cell("NULL"), "NULL");
}

// §7.2 — numeric-like strings (`/^[+-]?[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i`, ASCII digits) are quoted.
#[test]
fn s7_2_numeric_like_strings_are_quoted() {
    for s in ["42", "-3.14", "05", "+1", "1e-6", "1E6", "0", "007.50"] {
        assert_eq!(cell(s), format!("\"{s}\""), "{s}");
    }
    for s in [".5", "1.", "NaN", "Infinity", "0x10", "1_000", "٤٢", "1e", "--1"] {
        assert_eq!(cell(s), if s.starts_with('-') { format!("\"{s}\"") } else { s.to_string() }, "{s}");
    }
}

// §7.2 — `:` `"` `\` are quoted.
#[test]
fn s7_2_colon_quote_backslash_are_quoted() {
    assert_eq!(cell("a:b"), "\"a:b\"");
    assert_eq!(cell("http://x"), "\"http://x\"");
    assert_eq!(cell("a\"b"), "\"a\\\"b\"");
    assert_eq!(cell("C:\\src"), "\"C:\\\\src\"");
    assert_eq!(cell("a\\b"), "\"a\\\\b\"");
}

// §7.2 — brackets and braces are quoted.
#[test]
fn s7_2_brackets_and_braces_are_quoted() {
    for s in ["a[test]", "[5", "x]", "{key}", "y}"] {
        assert_eq!(cell(s), format!("\"{s}\""), "{s}");
    }
}

// §7.2 — control characters U+0000..U+001F are quoted.
#[test]
fn s7_2_control_characters_are_quoted() {
    assert_eq!(cell("a\u{7}b"), "\"a\\u0007b\"");
}

// §7.2 / §11.1 — the document delimiter (comma) is quoted in every position; other delimiter characters are not.
#[test]
fn s7_2_document_delimiter_is_quoted_and_pipe_is_not() {
    assert_eq!(cell("a,b"), "\"a,b\"");
    assert_eq!(cell("a|b"), "a|b");
    assert_eq!(render_hints(&[Hint::new("a, b")]), "help[1]: \"a, b\"");
}

// §7.2 — strings starting with `-` or `#` are quoted; interior ones are not.
#[test]
fn s7_2_leading_hyphen_or_hash_is_quoted() {
    assert_eq!(cell("-"), "\"-\"");
    assert_eq!(cell("-x"), "\"-x\"");
    assert_eq!(cell("- item"), "\"- item\"");
    assert_eq!(cell("#hello"), "\"#hello\"");
    assert_eq!(cell("a-b #c"), "a-b #c");
}

// §7.2 — otherwise strings MUST be unquoted (Unicode, emoji, interior spaces are safe).
#[test]
fn s7_2_safe_strings_are_unquoted() {
    for s in ["hello", "Ada_99", "café", "hello 👋 world", "Fix login redirect", "id=<id>", "a.b", "\u{feff}x"] {
        assert_eq!(cell(s), s, "{s}");
    }
    assert_eq!(escape_value("plain"), "plain");
}

// §7.3 — keys matching `^[A-Za-z_][A-Za-z0-9_.]*$` are unquoted; all others are quoted and escaped.
#[test]
fn s7_3_key_quoting() {
    for k in ["issue", "_x", "user.name", "a1", "totalCount", "__proto__", "true"] {
        assert_eq!(escape_key(k), k, "{k}");
    }
    for (k, want) in [
        ("my-key", "\"my-key\""),
        ("1abc", "\"1abc\""),
        ("", "\"\""),
        ("full name", "\"full name\""),
        ("order:id", "\"order:id\""),
        ("a,b", "\"a,b\""),
        ("[index]", "\"[index]\""),
        ("café", "\"café\""),
        ("line\n", "\"line\\n\""),
        ("tab\there", "\"tab\\there\""),
        ("a\u{4}b", "\"a\\u0004b\""),
        ("he said \"hi\"", "\"he said \\\"hi\\\"\""),
    ] {
        assert_eq!(escape_key(k), want, "{k:?}");
    }
}

// §7.3 — keys requiring quotes are quoted in array headers too, for the type name and every field name.
#[test]
fn s7_3_header_type_and_field_names_are_quoted_when_required() {
    let opts = ToonOptions::new(
        "my-items",
        vec!["order:id".into(), "full name".into(), "a,b".into(), "x{y}".into()],
        vec![vec![Value::Int(1), Value::from("Ada"), Value::Int(2), Value::Int(3)]],
    );
    let out = render_toon(&opts).unwrap();
    assert_eq!(out, "\"my-items\"[1]{\"order:id\",\"full name\",\"a,b\",\"x{y}\"}:\n  1,Ada,2,3");
    assert_conformant(&out);
}

// §8 — primitive fields are `key: value` with a single space (the totalCount line).
#[test]
fn s8_total_count_is_a_key_value_line_with_one_space() {
    let out = render_toon(&ToonOptions::new("t", vec![], vec![]).total_count(Some(47))).unwrap();
    assert_eq!(out, "t: []\ntotalCount: 47");
    assert_eq!(decode(&out).unwrap().get("totalCount"), Some(&Node::Prim(Prim::Num("47".into()))));
}

// §9.1 — empty arrays MUST be `key: []`; the legacy `key[0]:` header MUST NOT be emitted.
#[test]
fn s9_1_empty_list_is_key_colon_brackets() {
    let opts = ToonOptions::new("issues", vec!["id".into()], vec![]).total_count(Some(0)).hints(vec!["Try x".into()]);
    assert_eq!(render_toon(&opts).unwrap(), "issues: []\ntotalCount: 0\nhelp[1]: Try x");
    assert_eq!(empty_state("issue"), "issue: []\ntotalCount: 0");
    assert_eq!(empty_state_with_hints("issue", &[Hint::new("Try x")]), "issue: []\ntotalCount: 0\nhelp[1]: Try x");
    let empty_pipeline = Pipeline { id: "p".into(), steps: vec![] };
    assert_eq!(empty_pipeline.render(), "step: []");
}

// §9.1 / §1.4 — a non-empty primitive array (the hints) is emitted inline: `help[N]: v1,v2`, one space after the
// colon. The form follows from the value's shape, so list form would be non-conformant.
#[test]
fn s9_1_help_block_is_an_inline_primitive_array() {
    let opts = ToonOptions::new("t", vec![], vec![]).hints(vec!["Call get_issue".into(), "id=<id>, then retry".into()]);
    let out = render_toon(&opts).unwrap();
    assert_eq!(out, "t: []\nhelp[2]: Call get_issue,\"id=<id>, then retry\"");
    assert_eq!(parse(&out).unwrap().hints, vec!["Call get_issue", "id=<id>, then retry"]);
}

// §9.3 — tabular rows sit at depth +1 (two spaces), cells joined by the delimiter, width = field count.
#[test]
fn s9_3_rows_are_indented_once_and_full_width() {
    let opts = ToonOptions::new(
        "items",
        vec!["sku".into(), "qty".into(), "price".into()],
        vec![
            vec![Value::from("A1"), Value::Int(2), Value::Float(9.99)],
            vec![Value::from("B2"), Value::Int(1), Value::Float(14.5)],
        ],
    );
    assert_eq!(render_toon(&opts).unwrap(), "items[2]{sku,qty,price}:\n  A1,2,9.99\n  B2,1,14.5");
}

// §9.3 — null cells are the `null` literal; empty strings are `""`, so the two stay distinct.
#[test]
fn s9_3_null_and_empty_string_cells_are_distinct() {
    let opts = ToonOptions::new("t", vec!["a".into(), "b".into()], vec![vec![Value::Null, Value::from("")]]);
    let out = render_toon(&opts).unwrap();
    assert_eq!(out, "t[1]{a,b}:\n  null,\"\"");
    assert_eq!(parse(&out).unwrap().rows[0], vec![Prim::Null, Prim::str("")]);
}

// §9.3 / §9.4 / §10 — rows with zero fields are empty objects, which MUST NOT use tabular form: they are list items
// carrying the bare `-` marker.
#[test]
fn s9_4_zero_field_rows_render_as_list_of_empty_objects() {
    let out = render_toon(&ToonOptions::new("t", vec![], vec![vec![], vec![]]).total_count(Some(2))).unwrap();
    assert_eq!(out, "t[2]:\n  -\n  -\ntotalCount: 2");
    assert_eq!(decode(&out).unwrap().get("t"), Some(&Node::EmptyObjects(2)));
}

// §12 — no trailing newline, no trailing spaces, no blank lines, 2-space indentation, no BOM.
#[test]
fn s12_whitespace_invariants_hold_for_every_document_producer() {
    let full = ToonOptions::new("t", vec!["a".into()], vec![vec![Value::from(" x ")]])
        .total_count(Some(1))
        .hints(vec!["h ".into()]);
    let pipeline = Pipeline {
        id: "p".into(),
        steps: vec![PipelineStep { id: "fetch".into(), name: "Fetch, then parse".into(), status: StepStatus::Failed }],
    };
    for out in [
        render_toon(&full).unwrap(),
        render_toon(&ToonOptions::default()).unwrap(),
        empty_state("issue"),
        empty_state_with_hints("x-y", &[Hint::new("a"), Hint::new("")]),
        pipeline.render(),
        AgentResponse::new("t")
            .items(vec![vec![Value::Int(1)]], &["n"])
            .total_count(9)
            .hint("h")
            .render(OutputFormat::Text),
        AgentResponse::new("t").render(OutputFormat::Text),
    ] {
        assert!(!out.ends_with('\n'), "{out:?}");
        assert!(!out.starts_with('\u{feff}'), "{out:?}");
        assert_conformant(&out);
    }
}

// §14.3 (encoder side) — the output never contains duplicate sibling keys: `totalCount` and `help` are reserved
// root keys, so they cannot be a type name.
#[test]
fn s14_3_reserved_root_keys_are_rejected_as_type_names() {
    for name in ["totalCount", "help"] {
        let err = render_toon(&ToonOptions::new(name, vec![], vec![])).unwrap_err();
        assert_eq!(err, ToonError::InvalidTypeName { name: name.to_string() });
    }
}

// §14.2 (encoder side) — a field name repeated in one field list is a header defect, so it is rejected.
#[test]
fn s14_2_duplicate_field_names_are_rejected() {
    let opts = ToonOptions::new("t", vec!["a".into(), "b".into(), "a".into()], vec![]);
    assert_eq!(render_toon(&opts), Err(ToonError::InvalidFieldName { name: "a".to_string() }));
}

// §16 — no locale-dependent number formatting.
#[test]
fn s16_numbers_have_no_thousands_separators() {
    assert_eq!(one_cell(Value::Int(1_234_567)), "1234567");
    assert_eq!(one_cell(Value::Float(1_234_567.25)), "1234567.25");
}

// docs/spec/02-toon-format.md's "Quoted keys, nulls and numbers" example is exactly what michi emits.
#[test]
fn docs_example_quoted_keys_nulls_and_numbers() {
    let opts = ToonOptions::new(
        "pull-requests",
        vec!["id".into(), "head ref".into(), "merged_at".into(), "score".into()],
        vec![
            vec![Value::from(u64::MAX), Value::from("feature/x:y"), Value::Null, Value::Float(0.000001)],
            vec![Value::Int(7), Value::from("main"), Value::from("2026-10-21T09:00:00Z"), Value::Float(1e21)],
        ],
    );
    let out = render_toon(&opts).unwrap();
    assert_eq!(
        out,
        "\"pull-requests\"[2]{id,\"head ref\",merged_at,score}:\n  18446744073709551615,\"feature/x:y\",null,0.000001\n  7,main,\"2026-10-21T09:00:00Z\",1e+21"
    );
    assert_conformant(&out);
}

// Composite: the canonical michi document from the spec docs decodes to exactly the input data.
#[test]
fn composite_list_document_round_trips() {
    let opts = ToonOptions::new(
        "issues",
        vec!["number".into(), "title".into(), "state".into()],
        vec![
            vec![Value::Int(42), Value::from("Fix login redirect"), Value::from("open")],
            vec![Value::Int(44), Value::from("Update deps, bump major"), Value::Null],
        ],
    )
    .total_count(Some(47))
    .hints(vec!["Call get_issue with number=<number>".into()]);
    let out = render_toon(&opts).unwrap();
    assert_eq!(
        out,
        "issues[2]{number,title,state}:\n  42,Fix login redirect,open\n  44,\"Update deps, bump major\",null\ntotalCount: 47\nhelp[1]: Call get_issue with number=<number>"
    );
    let parsed = parse(&out).unwrap();
    assert_eq!(parsed.rows[1], vec![Prim::Num("44".into()), Prim::str("Update deps, bump major"), Prim::Null]);
    assert_eq!(parsed.total_count, Some(47));
}

#[cfg(feature = "serde")]
mod serde_list {
    use super::*;

    // §3 — michi's documented host normalization for `list()`: a key missing from an item is null, u64 above
    // i64::MAX stays a number, nested values become JSON-text strings (quoted per §7.2).
    #[test]
    fn s3_list_normalization_is_conformant() {
        let items =
            [serde_json::json!({"id": 1, "tags": ["a", "b"]}), serde_json::json!({"id": u64::MAX, "extra": ""})];
        let out = michi_core::toon::list("rows", &items).unwrap();
        assert_eq!(
            out,
            "rows[2]{id,tags,extra}:\n  1,\"[\\\"a\\\",\\\"b\\\"]\",null\n  18446744073709551615,null,\"\""
        );
        assert_conformant(&out);
    }

    #[test]
    fn s9_list_of_empty_objects_and_empty_slice() {
        assert_eq!(
            michi_core::toon::list("t", &[serde_json::json!({}), serde_json::json!({})]).unwrap(),
            "t[2]:\n  -\n  -"
        );
        assert_eq!(michi_core::toon::list("t", &([] as [serde_json::Value; 0])).unwrap(), "t: []");
    }
}

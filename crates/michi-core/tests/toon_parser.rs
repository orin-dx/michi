//! Unit tests for the shared test-only strict TOON decoder at `tests/support/toon_parser.rs` (see that file for what
//! it's for and why it lives under `tests/support/`). These pin the oracle itself: it must accept canonical TOON 4.4
//! and reject each non-canonical shape, otherwise the conformance tests that rely on it prove nothing.

mod support;
use support::toon_parser::{decode, parse, Node, Prim};

#[test]
fn parses_basic_document() {
    let input = "issues[2]{number,title,state}:\n  42,Fix login redirect,open\n  43,Add dark mode,open\ntotalCount: 47\nhelp[1]: Call get_issue";
    let parsed = parse(input).unwrap();
    assert_eq!(parsed.type_name, "issues");
    assert_eq!(parsed.fields, vec!["number", "title", "state"]);
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows[0], vec![Prim::Num("42".into()), Prim::str("Fix login redirect"), Prim::str("open")]);
    assert_eq!(parsed.total_count, Some(47));
    assert_eq!(parsed.hints, vec!["Call get_issue"]);
}

#[test]
fn parses_quoted_comma_value() {
    let parsed = parse("t[1]{a}:\n  \"x,y\"").unwrap();
    assert_eq!(parsed.rows[0], vec![Prim::str("x,y")]);
}

#[test]
fn parses_empty_array_and_list_of_empty_objects() {
    assert_eq!(decode("t: []").unwrap().get("t"), Some(&Node::EmptyArray));
    assert_eq!(decode("t[2]:\n  -\n  -").unwrap().get("t"), Some(&Node::EmptyObjects(2)));
}

#[test]
fn parses_quoted_keys_and_escapes() {
    let doc = decode("\"my-key\"[1]{\"a b\",c}:\n  \"x\\ty\",\"\\u0001\"").unwrap();
    assert_eq!(
        doc.get("my-key"),
        Some(&Node::Table {
            fields: vec!["a b".into(), "c".into()],
            rows: vec![vec![Prim::str("x\ty"), Prim::str("\u{1}")]]
        })
    );
}

// The non-list blocks (KV, errors, status, recovery, partial success) are root objects mixing primitive fields,
// inline arrays and tables; the oracle decodes them with their types.
#[test]
fn parses_mixed_root_objects_of_the_other_blocks() {
    let doc = decode(
        "error: not_found\nmessage: \"a: b\"\nexit_code: 1\ncompleted[2]: a,\"b, c\"\nrecovery[1]{tool,params,reason}:\n  \
         t,\"{\\\"k\\\":1}\",null",
    )
    .unwrap();
    assert_eq!(doc.get("message"), Some(&Node::Prim(Prim::str("a: b"))));
    assert_eq!(doc.get("exit_code"), Some(&Node::Prim(Prim::Num("1".into()))));
    assert_eq!(doc.get("completed"), Some(&Node::Inline(vec![Prim::str("a"), Prim::str("b, c")])));
    assert_eq!(
        doc.get("recovery"),
        Some(&Node::Table {
            fields: vec!["tool".into(), "params".into(), "reason".into()],
            rows: vec![vec![Prim::str("t"), Prim::str("{\"k\":1}"), Prim::Null]]
        })
    );
}

// The pre-TOON shapes of those blocks must fail, or the block tests would prove nothing.
#[test]
fn rejects_the_legacy_block_formats() {
    for (bad, why) in [
        ("id:    1\ntitle: x", "KV column alignment"),
        ("k: line1\\nline2", "unquoted backslash"),
        ("recovery[1]:\n  t: suggestedParams: { k: v } — why", "recovery item lines"),
        ("completed[2]:\n  a\n  b", "bare list item lines"),
        ("partial_success: 2 completed, 1 failed", "unquoted delimiter in a field value"),
        ("x: warm  [DEGRADED: slow]", "brackets and colon unquoted"),
        ("help[1]: a\nhelp[1]: b", "duplicate key from composition"),
        ("k: v\n", "block trailing newline"),
    ] {
        assert!(decode(bad).is_err(), "{why}: {bad:?} must be rejected");
    }
}

#[test]
fn rejects_non_canonical_encoder_output() {
    for (bad, why) in [
        ("t[1]{a}:\n  x\n", "trailing newline"),
        ("t[1]{a}:\r\n  x", "CRLF"),
        ("t[1]{a}:\n  x ", "trailing space"),
        ("t[0]{}:", "legacy empty header with empty field list"),
        ("t[0]:", "legacy empty header"),
        ("t[1]{}:\n  ", "empty field list"),
        ("t[2]{a}:\n  x", "row count below N"),
        ("t[1]{a}:\n  x\n  y", "row count above N"),
        ("t[1]{a,b}:\n  x", "row width"),
        ("t[1]{a,a}:\n  x,y", "duplicate field"),
        ("t: []\nt: []", "duplicate key"),
        ("t[1]{a}:\n  \"x\"", "needless quotes"),
        ("t[1]{a}:\n  true", "fine: a boolean"),
        ("t[1]{a}:\n  -x", "leading hyphen unquoted"),
        ("t[1]{a}:\n  \"\\u000A\"", "\\u escape where \\n is required"),
        ("t[1]{a}:\n  \"\\u001F\"", "uppercase hex"),
        ("t[1]{a}:\n  1.50", "trailing fractional zero"),
        ("t[1]{a}:\n  -0", "negative zero"),
        ("t[1]{a}:\n  1e6", "exponent inside canonical range"),
        ("t[1]{a}:\n  1e21", "exponent without sign"),
        ("\"t\": []", "needlessly quoted key"),
        ("my-key: []", "key that must be quoted"),
        ("t:  x", "two spaces after colon"),
        ("help[1]:  x", "two spaces after inline header"),
        ("\u{feff}t: []", "byte-order mark"),
        ("t: []\n\nu: []", "blank line"),
    ] {
        let result = decode(bad);
        if why.starts_with("fine") {
            assert!(result.is_ok(), "{why}: {bad:?} -> {result:?}");
        } else {
            assert!(result.is_err(), "{why}: {bad:?} must be rejected, got {result:?}");
        }
    }
}

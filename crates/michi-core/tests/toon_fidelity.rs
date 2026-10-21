//! Data-fidelity regressions: values that must survive render → parse unchanged.

use michi_core::toon::{escape_value, escape_value_quoted, render_toon, ToonOptions, Value};

mod support;
use support::toon_parser::parse;

fn round_trip_cell(value: &str) -> String {
    let opts = ToonOptions::new("item", vec!["v".into()], vec![vec![Value::from(value)]]);
    let out = render_toon(&opts).expect("single-column row renders");
    let parsed = parse(&out).expect("rendered TOON parses");
    parsed.rows[0][0].clone()
}

#[test]
fn quoted_windows_path_keeps_its_backslashes() {
    assert_eq!(round_trip_cell(r"C:\src\a,b"), r"C:\src\a,b");
}

#[test]
fn quoted_value_ending_in_backslash_round_trips() {
    assert_eq!(round_trip_cell(r#"say "hi" \"#), r#"say "hi" \"#);
}

#[test]
fn unquoted_backslash_is_left_literal() {
    assert_eq!(escape_value(r"C:\src\a"), r"C:\src\a");
    assert_eq!(round_trip_cell(r"C:\src\a"), r"C:\src\a");
}

#[test]
fn quoting_doubles_backslashes() {
    assert_eq!(escape_value(r"C:\src\a,b"), r#""C:\\src\\a,b""#);
    assert_eq!(escape_value_quoted(r"a\b"), r#""a\\b""#);
}

#[test]
fn integers_above_i64_max_keep_their_exact_digits() {
    let opts = ToonOptions::new(
        "id",
        vec!["u64".into(), "usize".into()],
        vec![vec![Value::from(u64::MAX), Value::from(usize::MAX)]],
    );
    let expected_usize = usize::MAX.to_string();
    assert_eq!(
        render_toon(&opts).expect("renders"),
        format!("id[1]{{u64,usize}}:\n  18446744073709551615,{expected_usize}\n")
    );
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct Counter {
    hits: u64,
}

#[cfg(feature = "serde")]
#[test]
fn list_keeps_exact_digits_for_u64_above_i64_max() {
    let out = michi_core::toon::list("counter", &[Counter { hits: u64::MAX }]).expect("renders");
    assert_eq!(out, "counter[1]{hits}:\n  18446744073709551615\n");
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize)]
struct Link {
    id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[cfg(feature = "serde")]
#[test]
fn list_keeps_a_field_first_present_in_a_later_row() {
    let rows = [Link { id: 1, url: None }, Link { id: 2, url: Some("https://example.com".into()) }];
    let out = michi_core::toon::list("link", &rows).expect("homogeneous rows render");
    assert_eq!(out, "link[2]{id,url}:\n  1,\n  2,https://example.com\n");
}

//! A response says one thing on every surface (text, JSON, `render_for`, `to_call_tool_result`): the caller's error is
//! never hidden by a validation failure, a validation failure is the one `error` object shape, a section's
//! `totalCount` comes from one rule, and no hint source drops another's hints.

use michi_core::kv::{KvItem, KvValue};
use michi_core::toon::Value;
use michi_core::{
    truncate_by_count, AgentResponse, Audience, BudgetSummary, DomainError, ErrorCode, Health, Hint, OutputFormat,
    Section, StatusItem, StatusResponse,
};

mod support;
use support::toon_parser::decode;

fn json(r: &AgentResponse) -> serde_json::Value {
    let text = r.render(OutputFormat::Json);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{text}"))
}

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

/// A list whose one row has two cells for one field: it fails validation.
fn bad_list() -> Section {
    Section::toon("bad", fields(&["a"]), vec![vec![Value::Int(1), Value::Int(2)]])
}

fn ok_list() -> Section {
    Section::toon("good", fields(&["a"]), vec![vec![Value::Int(1)]])
}

/// Every surface carries the same text, the same JSON and `isError` true, and the text is valid TOON.
fn assert_same_on_every_surface(r: &AgentResponse) -> (String, serde_json::Value) {
    let text = r.render(OutputFormat::Text);
    decode(&text).unwrap_or_else(|e| panic!("invalid TOON ({e}): {text}"));
    assert_eq!(r.render_for(Audience::Assistant), text);
    assert_eq!(r.render_for(Audience::User), text);
    let result = r.to_call_tool_result();
    assert_eq!(result.content[0].text, text);
    let v = json(r);
    let structured: serde_json::Value =
        serde_json::from_str(result.structured_content.as_ref().map_or("", |j| j.as_str())).expect("JSON");
    assert_eq!(structured, v);
    assert!(r.is_error() && result.is_error, "{text}");
    assert_eq!(v["isError"], true);
    (text, v)
}

fn bad_list_named(name: &str) -> Section {
    Section::toon(name, fields(&["a"]), vec![vec![Value::Int(1), Value::Int(2)]])
}

const BAD_MESSAGE: &str = "row 0 has 2 values but 1 fields declared";

// ---- Two failed sections follow the same rule as every other collision ----

#[test]
fn two_failed_sections_are_one_error_object_with_the_second_failure_as_its_cause() {
    let r = AgentResponse::new("t").section(bad_list_named("one")).section(ok_list()).section(bad_list_named("two"));
    let (text, v) = assert_same_on_every_surface(&r);
    assert_eq!(text.matches("error:").count(), 1, "{text}");
    assert_eq!(
        v["error"],
        serde_json::json!({
            "code": "toon_validation_failed",
            "message": BAD_MESSAGE,
            "causes": [format!("toon_validation_failed: {BAD_MESSAGE}")],
            "retryable": false,
        })
    );
    assert!(text.starts_with("error: toon_validation_failed\nmessage: "), "{text}");
    assert!(text.contains("causes[1]: "), "{text}");
    // The valid section still renders.
    assert!(text.contains("good[1]{a}:"), "{text}");
}

#[test]
fn many_failed_sections_and_an_error_leave_the_callers_error_first_and_every_failure_a_cause() {
    let e = DomainError::new(ErrorCode::ExternalFailure, "upload failed").cause("timeout");
    let r = AgentResponse::new("t")
        .section(bad_list_named("one"))
        .section(bad_list_named("two"))
        .section(bad_list_named("three"))
        .error(e);
    let (text, v) = assert_same_on_every_surface(&r);
    assert_eq!(text.matches("error:").count(), 1, "{text}");
    assert_eq!(v["error"]["code"], "external_failure");
    assert_eq!(v["error"]["message"], "upload failed");
    let failure = format!("toon_validation_failed: {BAD_MESSAGE}");
    assert_eq!(v["error"]["causes"], serde_json::json!(["timeout", failure, failure, failure]));
}

#[test]
fn a_failed_section_and_a_root_key_collision_both_appear_among_the_causes() {
    let r = AgentResponse::new("t")
        .section(bad_list_named("one"))
        .section(Section::text("a", "x"))
        .section(Section::text("a", "y"));
    let (text, v) = assert_same_on_every_surface(&r);
    assert_eq!(text.matches("error:").count(), 1, "{text}");
    assert_eq!(v["error"]["message"], BAD_MESSAGE);
    assert_eq!(
        v["error"]["causes"],
        serde_json::json!([
            "toon_validation_failed: duplicate root key 'a': give every section and KV key a distinct name"
        ])
    );
}

// ---- Finding 1: the caller's error is never hidden ----

#[test]
fn an_error_and_an_invalid_section_report_both_and_lead_with_the_callers_error() {
    let r = AgentResponse::new("t")
        .section(ok_list())
        .section(bad_list())
        .error(DomainError::new(ErrorCode::ExternalFailure, "upload failed"));
    let (text, v) = assert_same_on_every_surface(&r);
    assert_eq!(
        text,
        "error: external_failure\nmessage: upload failed\n\
         causes[1]: \"toon_validation_failed: row 0 has 2 values but 1 fields declared\"\nexit_code: 1\n\
         good[1]{a}:\n  1"
    );
    assert_eq!(
        v["error"],
        serde_json::json!({
            "code": "external_failure",
            "message": "upload failed",
            "causes": ["toon_validation_failed: row 0 has 2 values but 1 fields declared"],
            "retryable": DomainError::new(ErrorCode::ExternalFailure, "x").is_retryable(),
        })
    );
}

#[test]
fn an_error_and_a_root_key_collision_report_both_and_keep_the_errors_causes_hints_and_recovery() {
    let e = DomainError::new(ErrorCode::NotFound, "gone")
        .cause("the index was rebuilt")
        .hint("list issues first")
        .recovery(michi_core::RecoveryHint::new("list_issues"));
    let r = AgentResponse::new("t").section(Section::text("a", "x")).section(Section::text("a", "y")).error(e);
    let (text, v) = assert_same_on_every_surface(&r);
    assert_eq!(
        text,
        "error: not_found\nmessage: gone\ncauses[2]: the index was rebuilt,\"toon_validation_failed: duplicate root \
         key 'a': give every section and KV key a distinct name\"\nexit_code: 1\nhelp[1]: list issues first\n\
         recovery[1]{tool,params,reason}:\n  list_issues,\"{}\",null"
    );
    assert_eq!(v["error"]["code"], "not_found");
    assert_eq!(v["error"]["message"], "gone");
    assert_eq!(
        v["error"]["causes"],
        serde_json::json!([
            "the index was rebuilt",
            "toon_validation_failed: duplicate root key 'a': give every section and KV key a distinct name"
        ])
    );
}

#[test]
fn an_error_and_an_invalid_kv_section_name_the_kv_code() {
    let dup = vec![KvItem::new("k", KvValue::Int(1)), KvItem::new("k", KvValue::Int(2))];
    let r = AgentResponse::new("t").kv_items(dup).error(DomainError::new(ErrorCode::Conflict, "stale"));
    let (text, v) = assert_same_on_every_surface(&r);
    assert!(text.starts_with("error: conflict\nmessage: stale\ncauses[1]: \"kv_validation_failed: "), "{text}");
    assert_eq!(v["error"]["code"], "conflict");
    assert!(v["error"]["causes"][0].as_str().is_some_and(|c| c.starts_with("kv_validation_failed: ")), "{v}");
}

// ---- Finding 2: one error JSON shape ----

#[test]
fn a_validation_failure_is_an_error_object_with_the_same_keys() {
    let dup = vec![KvItem::new("k", KvValue::Int(1)), KvItem::new("k", KvValue::Int(2))];
    let cases = [
        (
            AgentResponse::new("t").section(bad_list()),
            "toon_validation_failed",
            "row 0 has 2 values but 1 fields declared",
        ),
        (
            AgentResponse::new("t").section(ok_list()).section(ok_list()),
            "toon_validation_failed",
            "duplicate root key 'good': give every section and KV key a distinct name",
        ),
        (AgentResponse::new("t").kv_items(dup), "kv_validation_failed", "key \"k\" appears more than once"),
    ];
    for (r, code, message) in cases {
        let (text, v) = assert_same_on_every_surface(&r);
        // The text is unchanged: the error lines lead the body.
        assert!(text.starts_with(&format!("error: {code}\nmessage: ")), "{text}");
        assert_eq!(v["error"], serde_json::json!({"code": code, "message": message, "causes": [], "retryable": false}));
        let keys: Vec<&str> = v["error"].as_object().expect("object").keys().map(String::as_str).collect();
        assert_eq!(keys, ["code", "message", "causes", "retryable"]);
    }
}

// ---- Finding 3: one rule for a section's totalCount ----

/// The `totalCount` the text reports for a one-section response: the inline line, if any.
fn text_total(text: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix("totalCount: ")).map(|n| n.parse().expect("a count"))
}

#[test]
fn text_and_json_report_the_same_total_for_every_section_kind() {
    let row = || vec![Value::Int(1)];
    let kv = |n: usize| (0..n).map(|i| KvItem::new(format!("k{i}"), KvValue::Int(1))).collect::<Vec<_>>();
    let mut cases = Vec::new();
    for total in [None, Some(0), Some(1), Some(2), Some(5)] {
        for n in [0usize, 1, 2] {
            cases.push(Section::toon("t", fields(&["a"]), vec![row(); n]).total_count(total));
            cases.push(Section::kv("k", kv(n)).total_count(total));
            cases.push(Section::text_with_count("n", "x", n).total_count(total));
        }
    }
    for s in cases {
        let r = AgentResponse::new("t").section(s.clone());
        let text = r.render(OutputFormat::Text);
        let v = json(&r);
        let in_json = v["sections"][0].get("totalCount").map(|n| n.as_u64().expect("a count"));
        assert_eq!(in_json, text_total(&text), "{s:?}\ntext: {text}\njson: {v}");
        // The roll-up's total column says the same.
        let two = AgentResponse::new("t").section(s.clone()).section(Section::text("other", "y"));
        let rollup = two.budget(BudgetSummary::new()).render(OutputFormat::Text);
        let cell = rollup.lines().find_map(|l| l.strip_prefix(&format!("  {},", s.name()))).expect("roll-up row");
        let total_cell = cell.split(',').nth(1).expect("total column");
        assert_eq!(total_cell, in_json.map_or("null".to_string(), |n| n.to_string()), "{s:?}\n{rollup}");
    }
}

#[test]
fn a_total_below_the_rows_and_an_empty_list_match_across_surfaces() {
    let two_rows = Section::toon("t", fields(&["a"]), vec![vec![Value::Int(1)], vec![Value::Int(2)]]);
    let below = AgentResponse::new("t").section(two_rows.total_count(Some(1)));
    assert_eq!(below.render(OutputFormat::Text), "t[2]{a}:\n  1\n  2");
    assert!(json(&below)["sections"][0].get("totalCount").is_none(), "{}", json(&below));

    let empty = AgentResponse::new("t").section(Section::toon("t", fields(&["a"]), vec![]));
    assert_eq!(empty.render(OutputFormat::Text), "t: []\ntotalCount: 0");
    assert_eq!(json(&empty)["sections"][0]["totalCount"], 0);
}

// ---- Finding 5: hint sources compose ----

fn status() -> StatusResponse {
    StatusResponse::new(
        "tool",
        "desc",
        vec![StatusItem::new("index", KvValue::Text("ready".into())).health(Health::Ok)],
    )
    .hints(vec!["call get_status for detail".into()])
}

#[test]
fn hints_replaces_only_the_callers_own_hints() {
    let r = AgentResponse::new("t").hint("dropped").status(status()).hints(vec![Hint::new("mine")]);
    let help = r.render_hints_only();
    assert_eq!(help, "help[2]: call get_status for detail,mine");
    let v = json(&r);
    assert_eq!(v["hints"], serde_json::json!(["call get_status for detail", "mine"]));
    assert!(r.render(OutputFormat::Text).ends_with(&help));
}

#[test]
fn hints_keeps_the_showing_n_of_m_line_of_a_cut_section() {
    let outcome = truncate_by_count(vec![1, 2, 3, 4], 2);
    let r = AgentResponse::new("t")
        .truncated_section("t", fields(&["n"]), &outcome, |n| vec![Value::Int(*n)], "pass --limit 4")
        .hints(vec![Hint::new("mine")]);
    assert_eq!(
        r.render(OutputFormat::Text),
        "t[2]{n}:\n  1\n  2\ntotalCount: 4\nhelp[2]: showing 2 of 4; pass --limit 4,mine"
    );
}

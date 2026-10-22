//! Every agent-facing block michi renders is TOON 4.4 encoder output (docs/spec/06-decisions.md, "Every agent-facing
//! block is TOON 4.4"). Each block has an exact-output test, and every output is checked by the strict, independent
//! validator in `tests/support/toon_parser.rs`, which also rejects duplicate root keys (§14.3).

use std::time::Duration;

use michi_core::kv::KvError;
use michi_core::resilience::render_already_done;
use michi_core::response::{AgentResponse, OutputFormat};
use michi_core::toon::Value;
use michi_core::{
    append_hints, append_recovery, empty_state_with_hints, render_hints, render_kv, render_recovery, DomainError,
    ErrorCode, FailedOp, Health, Hint, KvItem, KvValue, PartialSuccess, RecoveryHint, StatusItem, StatusResponse,
};

mod support;
use support::toon_parser::{decode, Doc, Node, Prim};

#[track_caller]
fn conformant(out: &str) -> Doc {
    decode(out).unwrap_or_else(|e| panic!("not conformant TOON 4.4 encoder output: {e}\n{out}"))
}

fn kv(key: &str, value: KvValue) -> KvItem {
    KvItem::new(key, value)
}

fn kv_doc(items: &[KvItem]) -> String {
    render_kv(items, None, &[]).expect("valid KV items")
}

// --- KV block --------------------------------------------------------------------------------------------------------

#[test]
fn kv_block_is_a_root_object_with_one_space_after_each_colon() {
    let items = [
        kv("id", KvValue::Text("abc-123".into())),
        kv("title", KvValue::Text("Fix login".into())),
        kv("count", KvValue::Int(3)),
        kv("ratio", KvValue::Float(0.5, 2)),
        kv("ok", KvValue::Bool(true)),
        kv("wait", KvValue::Duration(Duration::from_millis(1500))),
        kv("gone", KvValue::Missing),
    ];
    let out = render_kv(&items, Some(10), &[Hint::new("a, b")]).expect("valid");
    assert_eq!(
        out,
        "id: abc-123\ntitle: Fix login\ncount: 3\nratio: 0.5\nok: true\nwait: 1.5s\ngone: null\ntotalCount: 10\n\
         help[1]: \"a, b\""
    );
    let doc = conformant(&out);
    assert_eq!(doc.get("count"), Some(&Node::Prim(Prim::Num("3".into()))));
    assert_eq!(doc.get("ok"), Some(&Node::Prim(Prim::Bool(true))));
    assert_eq!(doc.get("gone"), Some(&Node::Prim(Prim::Null)));
    assert_eq!(doc.get("wait"), Some(&Node::Prim(Prim::str("1.5s"))));
}

#[test]
fn kv_keys_and_text_values_are_quoted_per_7_2_and_7_3() {
    let items = [
        kv("my key", KvValue::Text("-x".into())),
        kv("n", KvValue::Text("42".into())),
        kv("b", KvValue::Text("true".into())),
        kv("e", KvValue::Text(String::new())),
        kv("c", KvValue::Text("a: b".into())),
        kv("u", KvValue::Text("café ünïcode".into())),
    ];
    let out = kv_doc(&items);
    assert_eq!(out, "\"my key\": \"-x\"\nn: \"42\"\nb: \"true\"\ne: \"\"\nc: \"a: b\"\nu: café ünïcode");
    let doc = conformant(&out);
    assert_eq!(doc.get("my key"), Some(&Node::Prim(Prim::str("-x"))));
    assert_eq!(doc.get("n"), Some(&Node::Prim(Prim::str("42"))));
}

// KvValue::Float(f, decimals): `f` is rounded to `decimals` places, then the rounded value is written in canonical
// TOON number form (§2), so the decimals bound the precision but never pad with trailing zeros.
#[test]
#[allow(clippy::approx_constant)]
fn kv_floats_round_to_the_given_decimals_then_render_canonically() {
    let cases: [(f64, u8, &str); 10] = [
        (3.14159, 2, "3.14"),
        (3.14159, 0, "3"),
        (1.5, 3, "1.5"),
        (2.0, 2, "2"),
        (-0.001, 2, "0"),
        (f64::NAN, 2, "null"),
        (f64::INFINITY, 0, "null"),
        (1e21, 0, "1e+21"),
        (1e-7, 10, "1e-7"),
        (0.1 + 0.2, 20, "0.30000000000000004"),
    ];
    for (f, decimals, expected) in cases {
        let out = kv_doc(&[kv("x", KvValue::Float(f, decimals))]);
        assert_eq!(out, format!("x: {expected}"), "Float({f}, {decimals})");
        conformant(&out);
    }
}

#[test]
fn kv_duplicate_key_is_rejected() {
    let items = [kv("a", KvValue::Int(1)), kv("a", KvValue::Int(2))];
    assert_eq!(render_kv(&items, None, &[]), Err(KvError::DuplicateKey { key: "a".into() }));
    assert_eq!(KvError::DuplicateKey { key: "a".into() }.to_string(), "key \"a\" appears more than once");
}

#[test]
fn kv_reserved_root_keys_are_rejected_even_when_unused() {
    for key in ["totalCount", "help", "recovery"] {
        assert_eq!(
            render_kv(&[kv(key, KvValue::Int(1))], None, &[]),
            Err(KvError::ReservedKey { key: key.into() }),
            "{key}"
        );
    }
    assert_eq!(
        KvError::ReservedKey { key: "help".into() }.to_string(),
        "key \"help\" is a reserved document key (totalCount, help, recovery)"
    );
}

#[test]
fn kv_empty_items_is_the_empty_document() {
    assert_eq!(render_kv(&[], Some(3), &[Hint::new("h")]), Ok(String::new()));
}

// --- help block and composition --------------------------------------------------------------------------------------

#[test]
fn help_block_is_a_finished_document_without_trailing_newline() {
    let out = render_hints(&[Hint::new("first"), Hint::new("a, b")]);
    assert_eq!(out, "help[2]: first,\"a, b\"");
    conformant(&out);
}

#[test]
fn append_helpers_join_documents_with_one_lf_and_leave_no_trailing_newline() {
    let mut out = "t: []".to_string();
    append_hints(&mut out, &[Hint::new("h")]);
    append_recovery(&mut out, &[RecoveryHint::new("r")]);
    assert_eq!(out, "t: []\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  r,\"{}\",null");
    conformant(&out);

    // A buffer that already ends in a newline gets no second one.
    let mut out = "t: []\n".to_string();
    append_hints(&mut out, &[Hint::new("h")]);
    assert_eq!(out, "t: []\nhelp[1]: h");

    // An empty buffer gets no leading newline.
    let mut out = String::new();
    append_recovery(&mut out, &[RecoveryHint::new("r")]);
    assert_eq!(out, render_recovery(&[RecoveryHint::new("r")]));
}

#[test]
fn empty_state_with_hints_is_conformant() {
    let out = empty_state_with_hints("issue", &[Hint::new("Try list_issues with a broader filter")]);
    assert_eq!(out, "issue: []\ntotalCount: 0\nhelp[1]: Try list_issues with a broader filter");
    conformant(&out);
}

// --- recovery block --------------------------------------------------------------------------------------------------

#[test]
fn recovery_block_is_a_tabular_array_with_params_as_compact_json_text() {
    let hints = [
        RecoveryHint::new("create_item")
            .param("project", KvValue::Text("PROJ".into()))
            .param("type", KvValue::Text("Task".into()))
            .reason("project is required"),
        RecoveryHint::new("retry").param("attempt", KvValue::Int(2)),
        RecoveryHint::new("reload_cache"),
    ];
    let out = render_recovery(&hints);
    assert_eq!(
        out,
        "recovery[3]{tool,params,reason}:\n  \
         create_item,\"{\\\"project\\\":\\\"PROJ\\\",\\\"type\\\":\\\"Task\\\"}\",project is required\n  \
         retry,\"{\\\"attempt\\\":2}\",null\n  \
         reload_cache,\"{}\",null"
    );
    let doc = conformant(&out);
    let Some(Node::Table { fields, rows }) = doc.get("recovery") else { panic!("recovery table: {doc:?}") };
    assert_eq!(fields, &["tool", "params", "reason"]);
    let Prim::Str(params) = &rows[0][1] else { panic!("params must decode as a string: {rows:?}") };
    let params: serde_json::Value = serde_json::from_str(params).expect("params cell is JSON text");
    assert_eq!(params, serde_json::json!({"project": "PROJ", "type": "Task"}));
    assert_eq!(rows[1][2], Prim::Null, "an absent reason is null, distinct from the empty string");
}

#[test]
fn recovery_params_json_matches_the_structured_companion() {
    let r = RecoveryHint::new("t")
        .param("missing", KvValue::Missing)
        .param("flag", KvValue::Bool(false))
        .param("ratio", KvValue::Float(0.25, 2))
        .param("wait", KvValue::Duration(Duration::from_millis(1500)));
    let text = render_recovery(std::slice::from_ref(&r));
    let doc = conformant(&text);
    let Some(Node::Table { rows, .. }) = doc.get("recovery") else { panic!("{doc:?}") };
    let Prim::Str(params) = &rows[0][1] else { panic!("{rows:?}") };
    let from_text: serde_json::Value = serde_json::from_str(params).expect("JSON text");
    let companion: serde_json::Value =
        serde_json::from_str(&AgentResponse::new("x").recovery_hint(r).render(OutputFormat::Json)).expect("JSON");
    assert_eq!(from_text, companion["recovery"][0]["params"]);
}

#[test]
fn empty_recovery_is_the_empty_document() {
    assert_eq!(render_recovery(&[]), "");
}

// --- status ----------------------------------------------------------------------------------------------------------

#[test]
fn status_items_are_a_tabular_array_with_health_and_reason_columns() {
    let resp = StatusResponse::new(
        "my-tool",
        "does things",
        vec![
            StatusItem::new("index", KvValue::Text("ready".into())).health(Health::Ok),
            StatusItem::new("cache", KvValue::Text("warm".into())).health(Health::Degraded("near limit".into())),
            StatusItem::new("queue", KvValue::Text("down".into())).health(Health::Error("disconnected".into())),
            StatusItem::new("workers", KvValue::Int(4)),
        ],
    )
    .with_hints(vec![Hint::new("Call get_status for detail")]);
    let out = resp.render();
    assert_eq!(
        out,
        "tool: my-tool\ndescription: does things\nitems[4]{key,value,health,reason}:\n  \
         index,ready,ok,null\n  cache,warm,degraded,near limit\n  queue,down,error,disconnected\n  \
         workers,4,null,null\nhelp[1]: Call get_status for detail"
    );
    conformant(&out);
}

#[test]
fn status_without_items_has_an_empty_items_array() {
    let out = StatusResponse::new("t", "d", vec![]).render();
    assert_eq!(out, "tool: t\ndescription: d\nitems: []");
    conformant(&out);
}

// Item keys are cells, not root keys, so any key — even a repeated one or `help` — keeps the document valid.
#[test]
fn status_item_keys_cannot_collide_with_root_keys() {
    let items = ["tool", "help", "help"].into_iter().map(|k| StatusItem::new(k, KvValue::Missing)).collect();
    let out = StatusResponse::new("t", "a, b", items).with_hints(vec![Hint::new("h")]).render();
    assert_eq!(
        out,
        "tool: t\ndescription: \"a, b\"\nitems[3]{key,value,health,reason}:\n  tool,null,null,null\n  \
         help,null,null,null\n  help,null,null,null\nhelp[1]: h"
    );
    conformant(&out);
}

// --- already done ----------------------------------------------------------------------------------------------------

#[test]
fn already_done_is_a_root_object() {
    let out = render_already_done("delete_item", "item 42 was deleted in run abc", &["use get_item".to_string()]);
    assert_eq!(
        out,
        "operation: delete_item\nstatus: already_done\nsummary: item 42 was deleted in run abc\nhelp[1]: use get_item"
    );
    conformant(&out);
    let out = render_already_done("op", "done: 3 items, 0 errors", &[]);
    assert_eq!(out, "operation: op\nstatus: already_done\nsummary: \"done: 3 items, 0 errors\"");
    conformant(&out);
}

// --- errors ----------------------------------------------------------------------------------------------------------

#[test]
fn domain_error_is_a_root_object_with_help_and_recovery() {
    let e = DomainError::new(ErrorCode::NotFound, "Issue #9999 does not exist: check the number")
        .hint("Call list_issues to see available numbers")
        .recovery(RecoveryHint::new("list_issues").param("state", KvValue::Text("open".into())));
    let out = e.render();
    assert_eq!(
        out,
        "error: not_found\nmessage: \"Issue #9999 does not exist: check the number\"\nexit_code: 1\n\
         help[1]: Call list_issues to see available numbers\nrecovery[1]{tool,params,reason}:\n  \
         list_issues,\"{\\\"state\\\":\\\"open\\\"}\",null"
    );
    conformant(&out);
}

#[test]
fn domain_error_without_hints_or_recovery() {
    let out = DomainError::new(ErrorCode::RateLimited, "slow down").render();
    assert_eq!(out, "error: rate_limited\nmessage: slow down\nexit_code: 1");
    conformant(&out);
}

#[test]
fn unified_error_variants_use_the_domain_error_shape() {
    let out = michi_core::Error::InvalidInput("field 'name' is required".into()).render();
    assert_eq!(out, "error: invalid_input\nmessage: field 'name' is required\nexit_code: 1");
    conformant(&out);
    let out = michi_core::Error::NotFound("issue 42".into()).render();
    assert_eq!(out, "error: not_found\nmessage: issue 42\nexit_code: 1");
    conformant(&out);
}

// --- partial success -------------------------------------------------------------------------------------------------

#[test]
fn partial_success_full_example() {
    let ps = PartialSuccess::new(
        vec!["create_issue".into(), "add_label".into()],
        vec![FailedOp::new("assign_user", "User 'ghost' not found")
            .recovery(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into())))],
        vec!["notify_team".into()],
    );
    let out = ps.render();
    assert_eq!(
        out,
        "partial_success: \"2 completed, 1 failed, 1 skipped\"\ncompleted[2]: create_issue,add_label\n\
         failed[1]{operation,reason}:\n  assign_user,User 'ghost' not found\nskipped[1]: notify_team\n\
         recovery[1]{tool,params,reason}:\n  assign_user,\"{\\\"user\\\":\\\"alice\\\"}\",null"
    );
    conformant(&out);
}

#[test]
fn partial_success_empty_is_just_the_summary() {
    let ps = PartialSuccess::new(vec![], vec![], vec![]);
    let out = ps.render();
    assert_eq!(out, "partial_success: \"0 completed, 0 failed, 0 skipped\"");
    conformant(&out);
}

#[test]
fn partial_success_operation_names_are_quoted_in_inline_arrays() {
    let ps = PartialSuccess::new(vec!["a, b".into(), "-x".into()], vec![], vec!["".into()]);
    let out = ps.render();
    assert_eq!(
        out,
        "partial_success: \"2 completed, 0 failed, 1 skipped\"\ncompleted[2]: \"a, b\",\"-x\"\nskipped[1]: \"\""
    );
    conformant(&out);
}

// --- AgentResponse composition ---------------------------------------------------------------------------------------

#[test]
fn agent_response_kv_body_composes_with_help_and_recovery() {
    let r = AgentResponse::new("t")
        .kv_items(vec![kv("k", KvValue::Missing), kv("n", KvValue::Float(1.0, 3))])
        .total_count(2)
        .hint("h")
        .recovery_hint(RecoveryHint::new("r").reason("why"));
    let out = r.render(OutputFormat::Text);
    assert_eq!(out, "k: null\nn: 1\ntotalCount: 2\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  r,\"{}\",why");
    conformant(&out);
}

#[test]
fn agent_response_toon_body_composes_with_help_and_recovery() {
    let r = AgentResponse::new("issues")
        .items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"])
        .hint("h")
        .recovery_hint(RecoveryHint::new("r"));
    let out = r.render(OutputFormat::Text);
    assert_eq!(out, "issues[1]{id,state}:\n  1,open\nhelp[1]: h\nrecovery[1]{tool,params,reason}:\n  r,\"{}\",null");
    conformant(&out);
}

#[test]
fn agent_response_kv_duplicate_or_reserved_key_is_a_validation_error() {
    let dup = AgentResponse::new("t").kv_items(vec![kv("a", KvValue::Int(1)), kv("a", KvValue::Int(2))]);
    let out = dup.render(OutputFormat::Text);
    assert_eq!(out, "error: kv_validation_failed\nmessage: \"key \\\"a\\\" appears more than once\"");
    assert!(dup.is_error());
    conformant(&out);

    let reserved = AgentResponse::new("t").kv_items(vec![kv("recovery", KvValue::Int(1))]).hint("h");
    let out = reserved.render(OutputFormat::Text);
    assert_eq!(
        out,
        "error: kv_validation_failed\nmessage: \"key \\\"recovery\\\" is a reserved document key (totalCount, help, \
         recovery)\"\nhelp[1]: h"
    );
    assert!(reserved.to_call_tool_result().is_error);
    conformant(&out);
}

#[test]
fn agent_response_type_name_recovery_is_reserved() {
    let r = AgentResponse::new("recovery")
        .items(vec![vec![Value::from("x")]], &["a"])
        .recovery_hint(RecoveryHint::new("r"));
    let out = r.render(OutputFormat::Text);
    assert!(out.starts_with("error: toon_validation_failed\nmessage: "), "{out}");
    assert!(r.is_error());
    conformant(&out);
}

#[test]
fn agent_response_hints_only_is_a_finished_document() {
    assert_eq!(AgentResponse::new("t").hint("do this").render_hints_only(), "help[1]: do this");
}

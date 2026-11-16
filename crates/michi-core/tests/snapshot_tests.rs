use michi_core::error::{DomainError, ErrorCode};
use michi_core::idempotency::{CompletedOp, FailedOp, PartialSuccess, SkippedOp};
use michi_core::kv::{render_kv, KvItem, KvValue};
use michi_core::recovery::RecoveryHint;
use michi_core::response::{AgentResponse, OutputFormat};
use michi_core::section::Section;
use michi_core::status::{Health, StatusItem, StatusResponse};
use michi_core::toon::Value;

#[test]
fn snapshot_toon_basic_list() {
    let section = Section::toon(
        "issue",
        vec!["number".into(), "title".into(), "state".into()],
        vec![
            vec![Value::Int(42), Value::Str("Fix login redirect".into()), Value::Str("open".into())],
            vec![Value::Int(43), Value::Str("Add dark mode".into()), Value::Str("open".into())],
            vec![Value::Int(44), Value::Str("Update deps, bump major".into()), Value::Str("closed".into())],
        ],
    )
    .total_count(Some(47));
    let response = AgentResponse::new("issue")
        .section(section)
        .hint("Call get_issue with number=<number> for full detail")
        .hint("Call list_issues with state=open to filter");
    insta::assert_snapshot!(response.render(OutputFormat::Text));
}

#[test]
fn snapshot_toon_empty_state() {
    let out = AgentResponse::new("issue").hint("Try list_issues with a broader filter").render(OutputFormat::Text);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_kv_single_item() {
    let items = vec![
        KvItem::new("id", KvValue::Text("abc-123".into())),
        KvItem::new("title", KvValue::Text("Fix login".into())),
        KvItem::new("state", KvValue::Text("open".into())),
        KvItem::new("count", KvValue::Int(3)),
    ];
    insta::assert_snapshot!(render_kv(&items).expect("valid items"));
}

#[test]
fn snapshot_kv_mixed_values() {
    let items = vec![
        KvItem::new("id", KvValue::Int(1)),
        KvItem::new("description", KvValue::Text("A longer field value".into())),
        KvItem::new("x", KvValue::Bool(true)),
    ];
    insta::assert_snapshot!(render_kv(&items).expect("valid items"));
}

#[test]
fn snapshot_status_mixed_health() {
    let resp = StatusResponse::new(
        "my-tool",
        "does things",
        vec![
            StatusItem::new("index", KvValue::Text("ready".into())).health(Health::Ok),
            StatusItem::new("cache", KvValue::Text("warm".into())).health(Health::Degraded("near limit".into())),
            StatusItem::new("queue", KvValue::Text("down".into())).health(Health::Error("disconnected".into())),
        ],
    );
    insta::assert_snapshot!(resp.render());
}

#[test]
fn snapshot_agent_response_full() {
    let out = AgentResponse::new("issue")
        .items(vec![], &[])
        .hint("Try list_issues with state=open")
        .hint("Try list_issues with a different label")
        .render(OutputFormat::Text);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_call_tool_result_kv() {
    let r = AgentResponse::new("issue")
        .kv_items(vec![
            KvItem::new("id", KvValue::Text("abc-123".into())),
            KvItem::new("state", KvValue::Text("open".into())),
        ])
        .human_content("Issue abc-123 is currently open.");
    let result = r.to_call_tool_result();
    insta::assert_debug_snapshot!(result);
}

#[test]
fn snapshot_partial_success_full() {
    let ps = PartialSuccess::try_new(
        vec![CompletedOp::new(0, "create_issue"), CompletedOp::new(1, "add_label")],
        vec![FailedOp::new(2, "assign_user", "User 'ghost' not found")
            .recovery(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into())))],
        vec![SkippedOp::new(3, "notify_team").reason("budget_exhausted")],
    )
    .expect("distinct indices");
    insta::assert_snapshot!(ps.render());
}

#[test]
fn error_structured_content_schemas_are_compatible() {
    let domain_json = DomainError::new(ErrorCode::NotFound, "missing item").render(OutputFormat::Json);
    let response_json = AgentResponse::new("items").as_error().render(OutputFormat::Json);

    // Both must be JSON objects starting with '{'
    assert!(domain_json.starts_with('{'), "DomainError JSON must be an object, got: {domain_json}");
    assert!(response_json.starts_with('{'), "AgentResponse JSON must be an object, got: {response_json}");

    // Both must include isError and hints — the fields callers depend on
    assert!(domain_json.contains("\"isError\""), "DomainError JSON must have isError, got: {domain_json}");
    assert!(domain_json.contains("\"hints\""), "DomainError JSON must have hints, got: {domain_json}");
    assert!(response_json.contains("\"isError\""), "AgentResponse JSON must have isError, got: {response_json}");
    assert!(response_json.contains("\"hints\""), "AgentResponse JSON must have hints, got: {response_json}");

    // DomainError always sets isError:true; AgentResponse.as_error() must also set true
    assert!(domain_json.contains("\"isError\":true"), "DomainError isError must be true, got: {domain_json}");
    assert!(
        response_json.contains("\"isError\":true"),
        "AgentResponse.as_error() isError must be true, got: {response_json}"
    );
}

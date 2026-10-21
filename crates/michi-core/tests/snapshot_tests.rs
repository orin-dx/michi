use michi_core::empty::empty_state_with_hints;
use michi_core::error::{DomainError, ErrorCode};
use michi_core::hints::Hint;
use michi_core::idempotency::{FailedOp, PartialSuccess};
use michi_core::kv::{render_kv, KvItem, KvValue};
use michi_core::recovery::RecoveryHint;
use michi_core::response::{AgentResponse, OutputFormat};
use michi_core::status::{Health, StatusItem, StatusResponse};
use michi_core::toon::{render_toon, ToonOptions, Value};

#[test]
fn snapshot_toon_basic_list() {
    let opts = ToonOptions::new(
        "issue",
        vec!["number".into(), "title".into(), "state".into()],
        vec![
            vec![Value::Int(42), Value::Str("Fix login redirect".into()), Value::Str("open".into())],
            vec![Value::Int(43), Value::Str("Add dark mode".into()), Value::Str("open".into())],
            vec![Value::Int(44), Value::Str("Update deps, bump major".into()), Value::Str("closed".into())],
        ],
    )
    .total_count(Some(47))
    .hints(vec![
        "Call get_issue with number=<number> for full detail".to_string(),
        "Call list_issues with state=open to filter".to_string(),
    ]);
    insta::assert_snapshot!(render_toon(&opts).unwrap());
}

#[test]
fn snapshot_toon_empty_state() {
    let out = empty_state_with_hints("issue", &[Hint::new("Try list_issues with a broader filter")]);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_kv_single_item() {
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Text("abc-123".into()) },
        KvItem { key: "title".into(), value: KvValue::Text("Fix login".into()) },
        KvItem { key: "state".into(), value: KvValue::Text("open".into()) },
        KvItem { key: "count".into(), value: KvValue::Int(3) },
    ];
    insta::assert_snapshot!(render_kv(&items, None, &[]).expect("valid items"));
}

#[test]
fn snapshot_kv_mixed_values() {
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Int(1) },
        KvItem { key: "description".into(), value: KvValue::Text("A longer field value".into()) },
        KvItem { key: "x".into(), value: KvValue::Bool(true) },
    ];
    insta::assert_snapshot!(render_kv(&items, None, &[]).expect("valid items"));
}

#[test]
fn snapshot_status_mixed_health() {
    let resp = StatusResponse::new(
        "my-tool",
        "does things",
        vec![
            StatusItem { key: "index".into(), value: KvValue::Text("ready".into()), health: Some(Health::Ok) },
            StatusItem {
                key: "cache".into(),
                value: KvValue::Text("warm".into()),
                health: Some(Health::Degraded("near limit".into())),
            },
            StatusItem {
                key: "queue".into(),
                value: KvValue::Text("down".into()),
                health: Some(Health::Error("disconnected".into())),
            },
        ],
    );
    insta::assert_snapshot!(resp.render());
}

#[test]
fn snapshot_agent_response_full() {
    let out = AgentResponse::new("issue")
        .items(vec![], &[])
        .total_count(0)
        .hint("Try list_issues with state=open")
        .hint("Try list_issues with a different label")
        .render(OutputFormat::Text);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_call_tool_result_kv() {
    let r = AgentResponse::new("issue")
        .kv_items(vec![
            KvItem { key: "id".into(), value: KvValue::Text("abc-123".into()) },
            KvItem { key: "state".into(), value: KvValue::Text("open".into()) },
        ])
        .human_content("Issue abc-123 is currently open.");
    let result = r.to_call_tool_result();
    insta::assert_debug_snapshot!(result);
}

#[test]
fn snapshot_partial_success_full() {
    let ps = PartialSuccess {
        completed: vec!["create_issue".into(), "add_label".into()],
        failed: vec![FailedOp {
            operation: "assign_user".into(),
            reason: "User 'ghost' not found".into(),
            recovery: Some(RecoveryHint::new("assign_user").param("user", KvValue::Text("alice".into()))),
        }],
        skipped: vec!["notify_team".into()],
    };
    insta::assert_snapshot!(ps.render());
}

#[test]
fn error_structured_content_schemas_are_compatible() {
    let domain_json = DomainError::new(ErrorCode::NotFound, "missing item").render_json();
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

//! Generated JSON Schemas must describe what serde actually puts on the wire, checked against real serialized values.
#![cfg(all(feature = "serde", feature = "schemars"))]

use michi_core::kv::KvValue;
use michi_core::{Audience, CallToolResult, ContentBlock, DomainError, ErrorCode, RecoveryHint};
use std::collections::BTreeSet;

/// Resolve a `$ref` against the root schema's definitions, or return the schema itself.
fn resolve<'a>(root: &'a serde_json::Value, schema: &'a serde_json::Value) -> &'a serde_json::Value {
    // A `$ref` next to `default` is wrapped as `allOf: [{"$ref": …}]`.
    if let Some([only]) = schema.get("allOf").and_then(serde_json::Value::as_array).map(Vec::as_slice) {
        return resolve(root, only);
    }
    match schema.get("$ref").and_then(serde_json::Value::as_str) {
        Some(r) => {
            let name = r.rsplit('/').next().expect("ref has a name");
            &root["definitions"][name]
        }
        None => schema,
    }
}

fn keys(v: &serde_json::Value) -> BTreeSet<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// Every key serde emits is a schema property, and every required property is emitted.
#[track_caller]
fn assert_object_matches(root: &serde_json::Value, schema: &serde_json::Value, value: &serde_json::Value) {
    let schema = resolve(root, schema);
    let props = keys(&schema["properties"]);
    assert_eq!(props, keys(value), "schema properties vs serialized keys\nschema: {schema:#}\nvalue: {value:#}");
    for req in schema["required"].as_array().into_iter().flatten() {
        let req = req.as_str().expect("required entries are strings");
        assert!(value.get(req).is_some(), "required `{req}` missing from serialized value {value:#}");
    }
}

#[test]
fn call_tool_result_schema_matches_its_wire_form() {
    let result = CallToolResult::new(
        vec![ContentBlock::new("body", vec![Audience::Assistant])],
        false,
        r#"{"body":"x","isError":false}"#,
    );
    let value = serde_json::to_value(&result).expect("serializes");
    let root = serde_json::to_value(schemars::schema_for!(CallToolResult)).expect("schema serializes");

    assert_object_matches(&root, &root, &value);
    let structured = &resolve(&root, &root)["properties"]["structuredContent"];
    assert_ne!(structured["type"], "string", "structuredContent is parsed JSON on the wire, not a string");

    let content_items = &resolve(&root, &root)["properties"]["content"]["items"];
    assert_object_matches(&root, content_items, &value["content"][0]);
    let annotations = &resolve(&root, content_items)["properties"]["annotations"];
    assert_object_matches(&root, annotations, &value["content"][0]["annotations"]);
}

#[test]
fn call_tool_result_schema_requires_only_what_input_requires() {
    let root = serde_json::to_value(schemars::schema_for!(CallToolResult)).expect("schema serializes");
    assert_eq!(root["required"], serde_json::json!(["content"]), "isError and structuredContent are optional");
    assert_eq!(resolve(&root, &root)["properties"]["structuredContent"]["type"], "object");
    let block = serde_json::to_value(schemars::schema_for!(ContentBlock)).expect("schema serializes");
    assert_eq!(block["required"], serde_json::json!(["text", "type"]), "annotations are optional");
}

fn domain_errors() -> Vec<DomainError> {
    vec![
        DomainError::new(ErrorCode::NotFound, "missing"),
        DomainError::new(ErrorCode::RateLimited, "slow down")
            .hint("wait")
            .hint("then retry")
            .retry_after(std::time::Duration::from_secs(3))
            .recovery(
                RecoveryHint::new("list_items")
                    .param("limit", KvValue::Int(10))
                    .param("ratio", KvValue::Float(0.25, 2))
                    .param("nan", KvValue::Float(f64::NAN, 2))
                    .param("d", KvValue::Duration(std::time::Duration::from_millis(1500)))
                    .param("m", KvValue::Missing)
                    .reason("why"),
            ),
    ]
}

/// The schema advertised for MCP `outputSchema` must describe what `render_json()` emits.
#[test]
fn domain_error_schema_matches_render_json() {
    let root = serde_json::to_value(schemars::schema_for!(DomainError)).expect("schema serializes");
    let codes: Vec<&str> = [
        ErrorCode::InvalidInput,
        ErrorCode::NotFound,
        ErrorCode::Unauthorized,
        ErrorCode::Forbidden,
        ErrorCode::Conflict,
        ErrorCode::RateLimited,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ExternalFailure,
    ]
    .iter()
    .map(ErrorCode::label)
    .collect();
    let error_schema = resolve(&root, &resolve(&root, &root)["properties"]["error"]);
    // Documented variants become `oneOf` single-value enums; collect them either way.
    let values: Vec<&serde_json::Value> = match error_schema["oneOf"].as_array() {
        Some(alts) => alts.iter().flat_map(|a| a["enum"].as_array().into_iter().flatten()).collect(),
        None => error_schema["enum"].as_array().into_iter().flatten().collect(),
    };
    let allowed: Vec<&str> = values.into_iter().filter_map(serde_json::Value::as_str).collect();
    assert_eq!(allowed, codes, "error codes are the snake_case labels\n{error_schema:#}");

    for err in domain_errors() {
        let value: serde_json::Value = serde_json::from_str(&err.render_json()).expect("render_json is JSON");
        assert_object_matches(&root, &root, &value);
        let recovery = &resolve(&root, &root)["properties"]["recovery"];
        if !value["recovery"].is_null() {
            let recovery_schema = recovery["anyOf"]
                .as_array()
                .and_then(|alts| alts.iter().find(|a| a.get("$ref").is_some()))
                .unwrap_or(recovery);
            assert_object_matches(&root, recovery_schema, &value["recovery"]);
        }
    }
}

/// Serializing a `DomainError` and calling `render_json()` produce the same JSON, so one schema covers both.
#[test]
fn domain_error_serializes_to_its_render_json_form() {
    let mut dup = RecoveryHint::new("t");
    dup.params.push(("x".to_string(), KvValue::Int(1)));
    dup.params.push(("x".to_string(), KvValue::Int(2)));
    let mut errors = domain_errors();
    errors.push(DomainError::new(ErrorCode::Conflict, "a\nb \"q\"").retryable(true).recovery(dup));
    for err in errors {
        let rendered: serde_json::Value = serde_json::from_str(&err.render_json()).expect("render_json is JSON");
        assert_eq!(serde_json::to_value(&err).expect("serializes"), rendered);
        assert_eq!(rendered["isError"], true);
    }
}

#[test]
fn content_block_rejects_a_non_text_type_instead_of_coercing_it() {
    let json = r#"{"type":"image","text":"x","annotations":{"audience":["user"]}}"#;
    assert!(serde_json::from_str::<ContentBlock>(json).is_err(), "an image block must not become a text block");
    let ok = r#"{"type":"text","text":"x","annotations":{"audience":["user"]}}"#;
    assert!(serde_json::from_str::<ContentBlock>(ok).is_ok());
}

#[test]
fn content_block_schema_matches_its_wire_form() {
    let value = serde_json::to_value(ContentBlock::new("t", vec![Audience::User])).expect("serializes");
    let root = serde_json::to_value(schemars::schema_for!(ContentBlock)).expect("schema serializes");
    assert_object_matches(&root, &root, &value);
}

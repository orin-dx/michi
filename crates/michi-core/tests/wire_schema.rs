//! Generated JSON Schemas must describe what serde actually puts on the wire, checked against real serialized values.
#![cfg(all(feature = "serde", feature = "schemars"))]

use michi_core::{Audience, CallToolResult, ContentBlock};
use std::collections::BTreeSet;

/// Resolve a `$ref` against the root schema's definitions, or return the schema itself.
fn resolve<'a>(root: &'a serde_json::Value, schema: &'a serde_json::Value) -> &'a serde_json::Value {
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
fn content_block_schema_matches_its_wire_form() {
    let value = serde_json::to_value(ContentBlock::new("t", vec![Audience::User])).expect("serializes");
    let root = serde_json::to_value(schemars::schema_for!(ContentBlock)).expect("schema serializes");
    assert_object_matches(&root, &root, &value);
}

//! A response whose TOON body fails validation must read as an error on every output surface.

use michi_core::toon::Value;
use michi_core::{AgentResponse, OutputFormat};

/// One cell declared against two fields: fails `ToonOptions::validate()`.
fn invalid_rows() -> AgentResponse {
    AgentResponse::new("issues").items(vec![vec![Value::Int(1)]], &["id", "state"])
}

fn structured_is_error(json: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(json).expect("structured content is valid JSON");
    v["isError"].clone()
}

#[test]
fn invalid_rows_render_an_error_body() {
    let text = invalid_rows().render(OutputFormat::Text);
    assert!(text.starts_with("error: toon_validation_failed\n"), "got: {text}");
}

#[test]
fn invalid_rows_set_is_error_on_the_mcp_result() {
    let result = invalid_rows().to_call_tool_result();
    assert!(result.is_error, "MCP isError must be true when the body is an error");
    assert_eq!(structured_is_error(&result.structured_content), serde_json::json!(true));
}

#[test]
fn invalid_rows_set_is_error_in_the_json_render() {
    assert_eq!(structured_is_error(&invalid_rows().render(OutputFormat::Json)), serde_json::json!(true));
}

#[test]
fn valid_rows_stay_a_success() {
    let response = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::from("open")]], &["id", "state"]);
    let result = response.to_call_tool_result();
    assert!(!result.is_error);
    assert_eq!(structured_is_error(&result.structured_content), serde_json::json!(false));
}

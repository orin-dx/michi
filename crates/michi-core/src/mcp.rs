//! MCP `CallToolResult` mapping — the shape a tool call returns to an MCP client.

use crate::audience::Audience;

/// One text content block. Wire-conformant with MCP's text content shape.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "ContentBlockWire", from = "ContentBlockWire"))]
#[non_exhaustive]
pub struct ContentBlock {
    /// The block's text content.
    pub text: String,
    /// Which surface(s) this block is meant for.
    pub audience: Vec<Audience>,
}

impl ContentBlock {
    /// Create a new ContentBlock.
    pub fn new(text: impl Into<String>, audience: Vec<Audience>) -> Self {
        Self { text: text.into(), audience }
    }
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "Annotations"))]
#[serde(rename_all = "camelCase")]
struct AnnotationsWire {
    audience: Vec<Audience>,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "ContentBlock"))]
struct ContentBlockWire {
    #[serde(rename = "type")]
    kind: TextKind,
    text: String,
    annotations: AnnotationsWire,
}

/// The only content type `ContentBlock` models. Any other `type` (e.g. `"image"`) fails to deserialize instead of
/// silently becoming a text block.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
enum TextKind {
    #[serde(rename = "text")]
    Text,
}

#[cfg(feature = "serde")]
impl From<ContentBlock> for ContentBlockWire {
    fn from(b: ContentBlock) -> Self {
        Self { kind: TextKind::Text, text: b.text, annotations: AnnotationsWire { audience: b.audience } }
    }
}

#[cfg(feature = "serde")]
impl From<ContentBlockWire> for ContentBlock {
    fn from(w: ContentBlockWire) -> Self {
        Self { text: w.text, audience: w.annotations.audience }
    }
}

/// The MCP `CallToolResult` shape: what a tool call returns to a client.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "CallToolResultWire", from = "CallToolResultWire"))]
#[non_exhaustive]
pub struct CallToolResult {
    /// Text content blocks.
    pub content: Vec<ContentBlock>,
    /// Whether this is a tool execution error.
    pub is_error: bool,
    /// Machine-readable JSON companion to the text in `content`. This is
    /// typically a richer, typed representation (e.g. `AgentResponse::render_json()`
    /// or `DomainError::render_json()`) — it is NOT a copy of `content[0].text`.
    pub structured_content: String,
}

impl CallToolResult {
    /// Create a new CallToolResult.
    pub fn new(content: Vec<ContentBlock>, is_error: bool, structured_content: impl Into<String>) -> Self {
        Self { content, is_error, structured_content: structured_content.into() }
    }
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "CallToolResult"))]
#[serde(rename_all = "camelCase")]
struct CallToolResultWire {
    content: Vec<ContentBlock>,
    is_error: bool,
    structured_content: serde_json::Value,
}

#[cfg(feature = "serde")]
impl From<CallToolResult> for CallToolResultWire {
    fn from(r: CallToolResult) -> Self {
        // structured_content is always produced by render_json() which emits valid JSON.
        // If parsing fails, preserve the raw value as a JSON string rather than silently
        // replacing it with null — bad JSON from render_json() is a bug, not an expected case.
        let structured_content = serde_json::from_str(&r.structured_content)
            .unwrap_or_else(|_| serde_json::Value::String(r.structured_content));
        Self { content: r.content, is_error: r.is_error, structured_content }
    }
}

#[cfg(feature = "serde")]
impl From<CallToolResultWire> for CallToolResult {
    fn from(w: CallToolResultWire) -> Self {
        let structured_content = serde_json::to_string(&w.structured_content).unwrap_or_default();
        Self { content: w.content, is_error: w.is_error, structured_content }
    }
}

// The schemas describe the wire form (`type`/`annotations.audience`, camelCase keys, parsed JSON
// `structuredContent`), not the Rust field layout; `schemars` enables `serde` so the wire DTOs always exist.
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ContentBlock {
    fn schema_name() -> String {
        ContentBlockWire::schema_name()
    }

    fn json_schema(generator: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        ContentBlockWire::json_schema(generator)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for CallToolResult {
    fn schema_name() -> String {
        CallToolResultWire::schema_name()
    }

    fn json_schema(generator: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        CallToolResultWire::json_schema(generator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_tool_result_is_constructible() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: vec![Audience::Assistant] }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        assert_eq!(r.content.len(), 1);
        assert!(!r.is_error);
    }

    #[test]
    fn ac027_new_preserves_text_and_audience_order() {
        let block = ContentBlock::new("body", vec![Audience::User, Audience::Assistant]);
        assert_eq!(block.text, "body");
        assert_eq!(block.audience, vec![Audience::User, Audience::Assistant]);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac028_type_field_is_always_the_literal_text_with_audience_nested() {
        let block = ContentBlock::new("body", vec![Audience::Assistant]);
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["annotations"]["audience"], serde_json::json!(["assistant"]));
        assert!(json.get("audience").is_none(), "audience must not be top-level");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac028a_type_field_must_be_text() {
        let image = r#"{"type":"image","text":"t","annotations":{"audience":["user"]}}"#;
        assert!(serde_json::from_str::<ContentBlock>(image).is_err(), "a non-text block must not be coerced to text");
        let text = r#"{"type":"text","text":"t","annotations":{"audience":["user"]}}"#;
        let block: ContentBlock = serde_json::from_str(text).unwrap();
        assert_eq!(block, ContentBlock { text: "t".to_string(), audience: vec![Audience::User] });
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac028b_missing_annotations_or_text_key_is_a_deserialization_error() {
        let missing_annotations = r#"{"type":"text","text":"t"}"#;
        assert!(serde_json::from_str::<ContentBlock>(missing_annotations).is_err());
        let missing_text = r#"{"type":"text","annotations":{"audience":[]}}"#;
        assert!(serde_json::from_str::<ContentBlock>(missing_text).is_err());
    }

    #[test]
    fn ac029a_new_accepts_str_and_string_via_into_string() {
        let from_str = CallToolResult::new(vec![], false, "raw");
        let from_string = CallToolResult::new(vec![], false, "raw".to_string());
        assert_eq!(from_str.structured_content, "raw");
        assert_eq!(from_string.structured_content, "raw");
        assert_eq!(from_str.content, vec![]);
        assert!(!from_str.is_error);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac030_well_formed_json_structured_content_becomes_a_parsed_value() {
        let result = CallToolResult::new(vec![], false, r#"{"a":1}"#);
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["structuredContent"], serde_json::json!({"a": 1}));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac031_malformed_json_structured_content_becomes_a_json_string_not_dropped() {
        let result = CallToolResult::new(vec![], false, "not json");
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["structuredContent"], serde_json::json!("not json"));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032_round_trip_preserves_structured_content_modulo_formatting() {
        let original = CallToolResult::new(vec![], false, r#"{"a":1,"b":[1,2,3]}"#);
        let wire_json = serde_json::to_string(&original).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire_json).unwrap();
        let original_value: serde_json::Value = serde_json::from_str(&original.structured_content).unwrap();
        let round_tripped_value: serde_json::Value = serde_json::from_str(&round_tripped.structured_content).unwrap();
        assert_eq!(original_value, round_tripped_value);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032a_round_trip_is_lossy_for_non_json_structured_content() {
        let empty = CallToolResult::new(vec![], false, "");
        let wire = serde_json::to_string(&empty).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire).unwrap();
        assert_eq!(round_tripped.structured_content, "\"\"");

        let not_json = CallToolResult::new(vec![], false, "not json");
        let wire = serde_json::to_string(&not_json).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire).unwrap();
        assert_eq!(round_tripped.structured_content, "\"not json\"");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac030a_wire_keys_are_exactly_camel_case_content_iserror_structuredcontent() {
        let result = CallToolResult::new(vec![], true, "{}");
        let json = serde_json::to_value(&result).unwrap();
        let obj = json.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["content", "isError", "structuredContent"]);
        assert_eq!(json["isError"], serde_json::json!(true));
        assert!(obj.get("is_error").is_none(), "snake_case is_error must not be present");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032b_missing_field_deserialization_is_an_error() {
        for payload in [
            r#"{"content":[],"isError":false}"#,
            r#"{"content":[],"structuredContent":{}}"#,
            r#"{"isError":false,"structuredContent":{}}"#,
        ] {
            assert!(serde_json::from_str::<CallToolResult>(payload).is_err(), "payload: {payload}");
        }
    }
}

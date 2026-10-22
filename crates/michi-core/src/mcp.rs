//! MCP `CallToolResult` mapping — the shape a tool call returns to an MCP client.

use crate::audience::Audience;

/// One text content block. Wire-conformant with MCP's text content shape.
///
/// With the `serde` feature it serializes as `{"type":"text","text":…,"annotations":{"audience":[…]}}`. On input
/// `annotations` and `annotations.audience` are optional, as in MCP, and default to no audience; other annotation
/// fields (such as `priority`) are ignored. A `type` other than `"text"` is rejected.
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
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "Annotations"))]
#[serde(rename_all = "camelCase")]
struct AnnotationsWire {
    #[serde(default)]
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
    #[serde(default)]
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
///
/// With the `serde` feature it serializes as `{"content":[…],"isError":…,"structuredContent":{…}}`, with
/// `structuredContent` the parsed JSON object and omitted when `structured_content` is empty. Serialization fails
/// when `structured_content` is neither empty nor the text of a JSON object, because MCP requires an object there.
/// On input only `content` is required: `isError` defaults to `false`, and an absent (or `null`)
/// `structuredContent` becomes the empty string; a `structuredContent` that is not an object is rejected.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "CallToolResultWire"))]
#[non_exhaustive]
pub struct CallToolResult {
    /// Text content blocks.
    pub content: Vec<ContentBlock>,
    /// Whether this is a tool execution error.
    pub is_error: bool,
    /// Machine-readable JSON companion to the text in `content`: the text of a JSON object, or empty for none. This is
    /// typically a richer, typed representation (e.g. `AgentResponse::render_json()`
    /// or `DomainError::render_json()`) — it is NOT a copy of `content[0].text`.
    pub structured_content: String,
}

impl CallToolResult {
    /// Create a new CallToolResult.
    ///
    /// `structured_content` is not checked here: the caller is responsible for passing the text of a JSON object or
    /// the empty string. Anything else makes serialization fail. Use `CallToolResult::try_new` (`serde` feature) to
    /// check it up front.
    pub fn new(content: Vec<ContentBlock>, is_error: bool, structured_content: impl Into<String>) -> Self {
        Self { content, is_error, structured_content: structured_content.into() }
    }

    /// Create a new CallToolResult, checking that `structured_content` is empty or the text of a JSON object.
    ///
    /// Caller opt-in — `new` does not call this; serialization performs the same check.
    ///
    /// # Errors
    ///
    /// [`StructuredContentError::InvalidJson`] when `structured_content` is not JSON, and
    /// [`StructuredContentError::NotAnObject`] when it is JSON but not an object.
    #[cfg(feature = "serde")]
    #[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
    pub fn try_new(
        content: Vec<ContentBlock>,
        is_error: bool,
        structured_content: impl Into<String>,
    ) -> Result<Self, StructuredContentError> {
        let structured_content = structured_content.into();
        parse_structured_content(&structured_content)?;
        Ok(Self { content, is_error, structured_content })
    }
}

/// The parsed `structuredContent` object, or `None` for the empty string.
#[cfg(feature = "serde")]
fn parse_structured_content(
    text: &str,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, StructuredContentError> {
    if text.is_empty() {
        return Ok(None);
    }
    match serde_json::from_str(text) {
        Ok(serde_json::Value::Object(map)) => Ok(Some(map)),
        Ok(other) => Err(StructuredContentError::NotAnObject {
            found: match other {
                serde_json::Value::Array(_) => "array",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Null | serde_json::Value::Object(_) => "null",
            },
        }),
        Err(e) => Err(StructuredContentError::InvalidJson { reason: e.to_string() }),
    }
}

/// Error returned when `structured_content` is not empty and not the text of a JSON object, which MCP requires.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StructuredContentError {
    /// The text is not valid JSON.
    #[error("structuredContent is not valid JSON: {reason}")]
    InvalidJson {
        /// The JSON parser's message.
        reason: String,
    },
    /// The text is valid JSON but not an object.
    #[error("structuredContent must be a JSON object, got {found}")]
    NotAnObject {
        /// The JSON type found instead: `array`, `string`, `number`, `boolean` or `null`.
        found: &'static str,
    },
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(rename = "CallToolResult"))]
#[serde(rename_all = "camelCase")]
struct CallToolResultWire {
    content: Vec<ContentBlock>,
    #[serde(default)]
    is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(schema_with = "object_schema"))]
    structured_content: Option<serde_json::Map<String, serde_json::Value>>,
}

/// `structuredContent` is a JSON object (MCP); michi never emits `null` for it.
#[cfg(feature = "schemars")]
fn object_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({ "type": "object", "additionalProperties": true }))
        .unwrap_or(schemars::schema::Schema::Bool(true))
}

#[cfg(feature = "serde")]
impl serde::Serialize for CallToolResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let structured_content =
            parse_structured_content(&self.structured_content).map_err(serde::ser::Error::custom)?;
        CallToolResultWire { content: self.content.clone(), is_error: self.is_error, structured_content }
            .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl From<CallToolResultWire> for CallToolResult {
    fn from(w: CallToolResultWire) -> Self {
        let structured_content =
            w.structured_content.map(|map| serde_json::Value::Object(map).to_string()).unwrap_or_default();
        Self { content: w.content, is_error: w.is_error, structured_content }
    }
}

// The schemas describe the wire form (`type`/`annotations.audience`, camelCase keys, parsed JSON
// `structuredContent`), not the Rust field layout; `schemars` enables `serde` so the wire DTOs always exist.
#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl schemars::JsonSchema for ContentBlock {
    fn schema_name() -> String {
        ContentBlockWire::schema_name()
    }

    fn json_schema(generator: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        ContentBlockWire::json_schema(generator)
    }
}

#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
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
    fn ac028b_missing_text_key_is_a_deserialization_error() {
        let missing_text = r#"{"type":"text","annotations":{"audience":[]}}"#;
        assert!(serde_json::from_str::<ContentBlock>(missing_text).is_err());
        let missing_type = r#"{"text":"t"}"#;
        assert!(serde_json::from_str::<ContentBlock>(missing_type).is_err());
    }

    // MCP marks `annotations` and `annotations.audience` optional: a block without them targets no specific audience.
    #[test]
    #[cfg(feature = "serde")]
    fn ac028c_annotations_and_audience_are_optional_on_input() {
        let expected = ContentBlock { text: "t".to_string(), audience: vec![] };
        for json in [
            r#"{"type":"text","text":"t"}"#,
            r#"{"type":"text","text":"t","annotations":{}}"#,
            r#"{"type":"text","text":"t","annotations":{"priority":0.5}}"#,
        ] {
            assert_eq!(serde_json::from_str::<ContentBlock>(json).expect(json), expected, "{json}");
        }
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
    fn ac031_non_object_structured_content_fails_to_serialize_instead_of_emitting_invalid_mcp() {
        // MCP requires `structuredContent` to be a JSON object. Anything else is a caller bug, reported as a
        // serialization error rather than sent as a string or number (it used to become `"not json"` / `42`).
        for bad in ["not json", "42", "[1]", "\"s\"", "null", " "] {
            let result = CallToolResult::new(vec![], false, bad);
            let err = serde_json::to_string(&result).expect_err(bad).to_string();
            assert!(err.contains("structuredContent"), "{bad:?}: {err}");
        }
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac031a_empty_structured_content_means_absent_and_is_omitted() {
        let result = CallToolResult::new(vec![], false, "");
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire, serde_json::json!({"content": [], "isError": false}));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac031b_try_new_checks_structured_content_up_front() {
        assert!(CallToolResult::try_new(vec![], false, r#"{"a":1}"#).is_ok());
        assert_eq!(CallToolResult::try_new(vec![], false, "").map(|r| r.structured_content), Ok(String::new()));
        assert_eq!(
            CallToolResult::try_new(vec![], false, "42"),
            Err(StructuredContentError::NotAnObject { found: "number" })
        );
        assert_eq!(
            CallToolResult::try_new(vec![], false, "[1]"),
            Err(StructuredContentError::NotAnObject { found: "array" })
        );
        assert!(matches!(
            CallToolResult::try_new(vec![], false, "not json"),
            Err(StructuredContentError::InvalidJson { .. })
        ));
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
    fn ac032a_absent_structured_content_round_trips_as_absent() {
        let empty = CallToolResult::new(vec![], false, "");
        let wire = serde_json::to_string(&empty).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire).unwrap();
        assert_eq!(round_tripped, empty);
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
    fn ac032b_only_content_is_required_on_input() {
        // MCP: `isError` defaults to false and `structuredContent` is optional.
        let r: CallToolResult = serde_json::from_str(r#"{"content":[{"type":"text","text":"hi"}]}"#).unwrap();
        assert_eq!(r, CallToolResult::new(vec![ContentBlock::new("hi", vec![])], false, ""));
        let r: CallToolResult = serde_json::from_str(r#"{"content":[],"structuredContent":{"a":1}}"#).unwrap();
        assert!(!r.is_error);
        assert_eq!(r.structured_content, r#"{"a":1}"#);
        let r: CallToolResult = serde_json::from_str(r#"{"content":[],"isError":true}"#).unwrap();
        assert!(r.is_error);
        assert_eq!(r.structured_content, "");

        assert!(serde_json::from_str::<CallToolResult>(r#"{"isError":false,"structuredContent":{}}"#).is_err());
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032c_non_object_structured_content_is_rejected_on_input() {
        for payload in [
            r#"{"content":[],"isError":false,"structuredContent":42}"#,
            r#"{"content":[],"isError":false,"structuredContent":"s"}"#,
        ] {
            assert!(serde_json::from_str::<CallToolResult>(payload).is_err(), "payload: {payload}");
        }
    }
}

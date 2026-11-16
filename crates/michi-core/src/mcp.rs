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

/// The text of one JSON object (RFC 8259), checked when it is built: what MCP requires for `structuredContent`.
///
/// michi's own renderers build it from their JSON writers; anything else goes through [`JsonObject::try_new`] (or
/// `parse()`), in every build, with or without the `serde` feature. The check is strict: one object, nothing after it
/// but whitespace, no lone surrogate escapes, every number finite as an f64, and at most 127 levels of nesting. Those
/// are the limits `serde_json` applies, so a value that passes here also serializes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonObject(String);

impl JsonObject {
    /// Check that `text` is one JSON object.
    ///
    /// # Errors
    ///
    /// [`StructuredContentError::InvalidJson`] when `text` is not JSON, and [`StructuredContentError::NotAnObject`]
    /// when it is JSON but not an object.
    pub fn try_new(text: impl Into<String>) -> Result<Self, StructuredContentError> {
        let text = text.into();
        check_json_object(&text)?;
        Ok(Self(text))
    }

    /// Wrap text one of michi's JSON writers produced. Those writers always emit one object; the tests check each.
    pub(crate) fn from_trusted(text: String) -> Self {
        Self(text)
    }

    /// The JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The JSON text, without copying it.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for JsonObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for JsonObject {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for JsonObject {
    type Err = StructuredContentError;

    /// [`JsonObject::try_new`] on a copy of `s`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}

impl From<JsonObject> for String {
    fn from(j: JsonObject) -> Self {
        j.0
    }
}

/// The MCP `CallToolResult` shape: what a tool call returns to a client.
///
/// With the `serde` feature it serializes as `{"content":[…],"isError":…,"structuredContent":{…}}`, with
/// `structuredContent` the parsed JSON object and omitted when `structured_content` is `None`. On input only `content`
/// is required: `isError` defaults to `false`, and an absent (or `null`) `structuredContent` becomes `None`; a
/// `structuredContent` that is not an object is rejected.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "CallToolResultWire"))]
#[non_exhaustive]
pub struct CallToolResult {
    /// Text content blocks.
    pub content: Vec<ContentBlock>,
    /// Whether this is a tool execution error.
    pub is_error: bool,
    /// Machine-readable JSON companion to the text in `content`, or `None`. This is typically a richer, typed
    /// representation (e.g. `AgentResponse::render(OutputFormat::Json)` or `DomainError::render(OutputFormat::Json)`)
    /// — it is NOT a copy of `content[0].text`. A [`JsonObject`] is always an object, so every result serializes.
    pub structured_content: Option<JsonObject>,
}

impl CallToolResult {
    /// Create a new CallToolResult.
    #[must_use]
    pub fn new(content: Vec<ContentBlock>, is_error: bool, structured_content: Option<JsonObject>) -> Self {
        Self { content, is_error, structured_content }
    }
}

/// Error returned when a `structuredContent` text is not the text of a JSON object, which MCP requires.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StructuredContentError {
    /// The text is not valid JSON.
    #[error("structuredContent is not valid JSON: {reason}")]
    InvalidJson {
        /// What is wrong, and the byte offset where it was found.
        reason: String,
    },
    /// The text is valid JSON but not an object.
    #[error("structuredContent must be a JSON object, got {found}")]
    NotAnObject {
        /// The JSON type found instead: `array`, `string`, `number`, `boolean` or `null`.
        found: &'static str,
    },
}

/// Deepest nesting [`check_json_object`] accepts; `serde_json` rejects the 128th level.
const MAX_JSON_DEPTH: usize = 127;

/// Check that `text` is exactly one JSON object, surrounded by optional whitespace.
fn check_json_object(text: &str) -> Result<(), StructuredContentError> {
    let mut p = JsonChecker { bytes: text.as_bytes(), pos: 0 };
    p.skip_ws();
    let found = match p.peek() {
        Some(b'{') => None,
        Some(b'[') => Some("array"),
        Some(b'"') => Some("string"),
        Some(b't' | b'f') => Some("boolean"),
        Some(b'n') => Some("null"),
        Some(b'-' | b'0'..=b'9') => Some("number"),
        _ => return Err(p.fail("expected a JSON value")),
    };
    p.value(0)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(p.fail("trailing characters"));
    }
    match found {
        None => Ok(()),
        Some(found) => Err(StructuredContentError::NotAnObject { found }),
    }
}

/// A validating JSON reader over bytes: no values are built, only the grammar of RFC 8259 is checked.
struct JsonChecker<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl JsonChecker<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn fail(&self, what: &str) -> StructuredContentError {
        StructuredContentError::InvalidJson { reason: format!("{what} at byte {}", self.pos) }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        let hit = self.peek() == Some(b);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), StructuredContentError> {
        if self.bytes[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(self.fail("expected a JSON value"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<(), StructuredContentError> {
        match self.peek() {
            Some(b'{') => self.container(depth, b'}', true),
            Some(b'[') => self.container(depth, b']', false),
            Some(b'"') => self.string(),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.fail("expected a JSON value")),
        }
    }

    /// An object (`keyed`) or array, whose opening bracket is at `pos`.
    fn container(&mut self, depth: usize, close: u8, keyed: bool) -> Result<(), StructuredContentError> {
        if depth >= MAX_JSON_DEPTH {
            return Err(self.fail("nesting too deep"));
        }
        self.pos += 1;
        self.skip_ws();
        if self.eat(close) {
            return Ok(());
        }
        loop {
            if keyed {
                if self.peek() != Some(b'"') {
                    return Err(self.fail("expected a string key"));
                }
                self.string()?;
                self.skip_ws();
                if !self.eat(b':') {
                    return Err(self.fail("expected `:`"));
                }
                self.skip_ws();
            }
            self.value(depth + 1)?;
            self.skip_ws();
            if self.eat(close) {
                return Ok(());
            }
            if !self.eat(b',') {
                return Err(self.fail("expected `,` or a closing bracket"));
            }
            self.skip_ws();
        }
    }

    fn string(&mut self) -> Result<(), StructuredContentError> {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return Err(self.fail("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.pos += 1,
                        Some(b'u') => {
                            self.pos += 1;
                            let unit = self.hex4()?;
                            if (0xDC00..0xE000).contains(&unit) {
                                return Err(self.fail("lone trailing surrogate"));
                            }
                            if (0xD800..0xDC00).contains(&unit) {
                                if !(self.eat(b'\\') && self.eat(b'u')) {
                                    return Err(self.fail("lone leading surrogate"));
                                }
                                if !(0xDC00..0xE000).contains(&self.hex4()?) {
                                    return Err(self.fail("lone leading surrogate"));
                                }
                            }
                        }
                        _ => return Err(self.fail("invalid escape")),
                    }
                }
                Some(b) if b < 0x20 => return Err(self.fail("control character in string")),
                // The input is a `&str`, so multi-byte UTF-8 sequences are already valid.
                Some(_) => self.pos += 1,
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, StructuredContentError> {
        let digits = self.bytes.get(self.pos..self.pos + 4).ok_or_else(|| self.fail("invalid \\u escape"))?;
        let mut unit = 0;
        for &d in digits {
            let v = char::from(d).to_digit(16).ok_or_else(|| self.fail("invalid \\u escape"))?;
            unit = unit * 16 + v;
        }
        self.pos += 4;
        Ok(unit)
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<(), StructuredContentError> {
        let start = self.pos;
        self.eat(b'-');
        if self.eat(b'0') {
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.fail("leading zero in number"));
            }
        } else if self.digits() == 0 {
            return Err(self.fail("expected a digit"));
        }
        if self.eat(b'.') && self.digits() == 0 {
            return Err(self.fail("expected a digit after `.`"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if self.digits() == 0 {
                return Err(self.fail("expected an exponent digit"));
            }
        }
        // The slice is ASCII digits and signs, so it is valid UTF-8 and `f64` parses it; past f64's range it reads
        // as infinity, which `serde_json` rejects as out of range.
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
        match text.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(()),
            _ => Err(StructuredContentError::InvalidJson { reason: format!("number out of range at byte {start}") }),
        }
    }
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
fn object_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({ "type": "object", "additionalProperties": true })
}

#[cfg(feature = "serde")]
impl serde::Serialize for CallToolResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // A `JsonObject` was checked against the limits serde_json applies, so this parse does not fail in practice;
        // if it ever did, it is reported as a serialization error, never a panic.
        let structured_content = self
            .structured_content
            .as_ref()
            .map(|j| serde_json::from_str(j.as_str()))
            .transpose()
            .map_err(serde::ser::Error::custom)?;
        CallToolResultWire { content: self.content.clone(), is_error: self.is_error, structured_content }
            .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl From<CallToolResultWire> for CallToolResult {
    fn from(w: CallToolResultWire) -> Self {
        let structured_content =
            w.structured_content.map(|map| JsonObject::from_trusted(serde_json::Value::Object(map).to_string()));
        Self { content: w.content, is_error: w.is_error, structured_content }
    }
}

// The schemas describe the wire form (`type`/`annotations.audience`, camelCase keys, parsed JSON
// `structuredContent`), not the Rust field layout; `schemars` enables `serde` so the wire DTOs always exist.
#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl schemars::JsonSchema for ContentBlock {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        ContentBlockWire::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        ContentBlockWire::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ContentBlockWire::json_schema(generator)
    }
}

#[cfg(feature = "schemars")]
#[cfg_attr(docsrs, doc(cfg(feature = "schemars")))]
impl schemars::JsonSchema for CallToolResult {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        CallToolResultWire::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        CallToolResultWire::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        CallToolResultWire::json_schema(generator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(text: &str) -> Option<JsonObject> {
        Some(JsonObject::try_new(text).expect("a JSON object"))
    }

    #[test]
    fn call_tool_result_is_constructible() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: vec![Audience::Assistant] }],
            is_error: false,
            structured_content: obj("{}"),
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
    fn json_object_accepts_objects_only() {
        for good in [
            "{}",
            " {\"a\":1} ",
            "{\"a\":[1,-2.5e+3,true,false,null,\"s\\u00e9\\n\"],\"b\":{\"c\":{}}}",
            "{\"\\ud83d\\ude00\":0}",
            "{\"a\":0.0,\"b\":-0,\"c\":1E5}",
        ] {
            assert_eq!(JsonObject::try_new(good).map(JsonObject::into_string), Ok(good.to_string()), "{good}");
        }
        for (bad, found) in
            [("[1]", "array"), ("\"s\"", "string"), ("42", "number"), ("true", "boolean"), ("null", "null")]
        {
            assert_eq!(JsonObject::try_new(bad), Err(StructuredContentError::NotAnObject { found }), "{bad}");
        }
        for bad in [
            "",
            " ",
            "not json",
            "{",
            "{\"a\"}",
            "{\"a\":}",
            "{\"a\":1,}",
            "{a:1}",
            "{\"a\":01}",
            "{\"a\":1.}",
            "{\"a\":1e}",
            "{\"a\":-}",
            "{\"a\":\"\\x\"}",
            "{\"a\":\"\\ud800\"}",
            "{\"a\":\"\\udc00\"}",
            "{\"a\":\"\\ud800\\u0041\"}",
            "{\"a\":\"tab\there\"}",
            "{\"a\":1e400}",
            "{} {}",
            "{\"a\":nul}",
        ] {
            assert!(
                matches!(JsonObject::try_new(bad), Err(StructuredContentError::InvalidJson { .. })),
                "{bad:?}: {:?}",
                JsonObject::try_new(bad)
            );
        }
    }

    #[test]
    fn json_object_nesting_stops_where_serde_json_does() {
        let nested = |n: usize| format!("{{\"a\":{}1{}}}", "[".repeat(n - 1), "]".repeat(n - 1));
        assert!(JsonObject::try_new(nested(MAX_JSON_DEPTH)).is_ok());
        assert!(JsonObject::try_new(nested(MAX_JSON_DEPTH + 1)).is_err());
    }

    // The checker and serde_json agree on every fixture, including the limits, so a checked object always serializes.
    #[test]
    #[cfg(feature = "serde")]
    fn json_object_check_agrees_with_serde_json() {
        let nested = |n: usize| format!("{{\"a\":{}1{}}}", "[".repeat(n - 1), "]".repeat(n - 1));
        let fixtures = [
            "{}".to_string(),
            "{\"a\":1e400}".into(),
            "{\"a\":1e308}".into(),
            "{\"a\":\"\\ud800\"}".into(),
            "{\"\\ud83d\\ude00\":0}".into(),
            "{\"a\":1,}".into(),
            "{\"a\":01}".into(),
            nested(MAX_JSON_DEPTH),
            nested(MAX_JSON_DEPTH + 1),
        ];
        for text in fixtures {
            let ours = JsonObject::try_new(text.as_str()).is_ok();
            let theirs = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text).is_ok();
            assert_eq!(ours, theirs, "{text:.80}");
        }
    }

    #[test]
    fn json_object_parses_and_displays_its_text() {
        let j: JsonObject = "{\"a\":1}".parse().expect("an object");
        assert_eq!(j.to_string(), "{\"a\":1}");
        assert_eq!(j.as_ref(), "{\"a\":1}");
        assert_eq!(String::from(j), "{\"a\":1}");
        assert!("[]".parse::<JsonObject>().is_err());
    }

    // michi's own writers are the only callers of `from_trusted`; each must produce what `new` accepts.
    #[test]
    fn michi_writers_produce_checked_objects() {
        let err = crate::DomainError::new(crate::ErrorCode::Timeout, "a \"q\"\n")
            .hint("h")
            .recovery(crate::RecoveryHint::new("t").param("k", crate::KvValue::Int(-1)));
        let resp = crate::AgentResponse::new("t").hint("h").recovery_hint(crate::RecoveryHint::new("r"));
        for result in [err.to_call_tool_result(), resp.to_call_tool_result(), resp.as_error().to_call_tool_result()] {
            let text = result.structured_content.expect("set").into_string();
            assert!(JsonObject::try_new(text.as_str()).is_ok(), "{text}");
        }
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
    #[cfg(feature = "serde")]
    fn ac030_well_formed_json_structured_content_becomes_a_parsed_value() {
        let result = CallToolResult::new(vec![], false, obj(r#"{"a":1}"#));
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["structuredContent"], serde_json::json!({"a": 1}));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac031a_absent_structured_content_is_omitted() {
        let result = CallToolResult::new(vec![], false, None);
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire, serde_json::json!({"content": [], "isError": false}));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032_round_trip_preserves_structured_content_modulo_formatting() {
        let original = CallToolResult::new(vec![], false, obj(r#"{"a":1, "b":[1,2,3]}"#));
        let wire_json = serde_json::to_string(&original).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire_json).unwrap();
        let parse = |r: &CallToolResult| -> serde_json::Value {
            serde_json::from_str(r.structured_content.as_ref().expect("set").as_str()).unwrap()
        };
        assert_eq!(parse(&original), parse(&round_tripped));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac032a_absent_structured_content_round_trips_as_absent() {
        let empty = CallToolResult::new(vec![], false, None);
        let wire = serde_json::to_string(&empty).unwrap();
        let round_tripped: CallToolResult = serde_json::from_str(&wire).unwrap();
        assert_eq!(round_tripped, empty);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac030a_wire_keys_are_exactly_camel_case_content_iserror_structuredcontent() {
        let result = CallToolResult::new(vec![], true, obj("{}"));
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
        assert_eq!(r, CallToolResult::new(vec![ContentBlock::new("hi", vec![])], false, None));
        let r: CallToolResult = serde_json::from_str(r#"{"content":[],"structuredContent":{"a":1}}"#).unwrap();
        assert!(!r.is_error);
        assert_eq!(r.structured_content, obj(r#"{"a":1}"#));
        let r: CallToolResult = serde_json::from_str(r#"{"content":[],"isError":true}"#).unwrap();
        assert!(r.is_error);
        assert_eq!(r.structured_content, None);
        let r: CallToolResult = serde_json::from_str(r#"{"content":[],"structuredContent":null}"#).unwrap();
        assert_eq!(r.structured_content, None);

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

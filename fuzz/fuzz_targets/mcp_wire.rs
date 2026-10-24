//! Untrusted JSON into every `Deserialize` type michi accepts off the wire: never a panic, and anything accepted
//! serializes to a form that deserializes to the same thing. Also pins the tolerant-input rules from
//! docs/spec/06-decisions.md ("MCP wire input is tolerant where MCP is").
//!
//! The NAPI input module (`crates/michi-core/src/napi/input.rs`) converts live JS values and only compiles with the
//! `napi` feature against a Node runtime, so it is not reachable from here.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::pipeline::{NotRunReason, SkipReason, StepStatus, UnknownCause};
use michi_core::toon::Value;
use michi_core::{
    render_kv, render_recovery, Audience, CallToolResult, ContentBlock, Hint, KvItem, KvValue, RecoveryHint,
};
use michi_pipeline::{
    AttemptRecord, Effect, ExecutionError, PlanFingerprint, ReplayClass, RunId, RunRecord, StepId, StepRecord,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Accepts `bytes` as a `T` or rejects them cleanly; an accepted value must survive serialize -> parse -> serialize
/// unchanged. Compares the JSON text, since a float field makes `T: PartialEq` unusable for NaN.
fn round_trip<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Option<T> {
    let value: T = serde_json::from_slice(bytes).ok()?;
    let first = serde_json::to_string(&value).expect("an accepted value serializes");
    let again: T = serde_json::from_str(&first).unwrap_or_else(|e| panic!("own output rejected: {e}\n{first}"));
    let second = serde_json::to_string(&again).expect("serializes again");
    assert_eq!(first, second, "round trip changed the value");
    Some(value)
}

#[derive(Arbitrary, Debug)]
struct WireResult {
    text: String,
    is_error: Option<bool>,
    audience: Option<Vec<bool>>,
    /// `Some` is an object with these string entries.
    structured: Option<Vec<(String, String)>>,
    structured_null: bool,
    extra_key: String,
    extra_value: i64,
}

fuzz_target!(|data: &[u8]| {
    // Raw bytes into every wire type.
    round_trip::<Value>(data);
    round_trip::<KvValue>(data);
    round_trip::<Hint>(data);
    round_trip::<Audience>(data);
    round_trip::<StepStatus>(data);
    round_trip::<SkipReason>(data);
    round_trip::<NotRunReason>(data);
    round_trip::<UnknownCause>(data);
    round_trip::<StepId>(data);
    round_trip::<RunId>(data);
    round_trip::<PlanFingerprint>(data);
    round_trip::<ReplayClass>(data);
    round_trip::<Effect>(data);
    round_trip::<ExecutionError>(data);
    round_trip::<AttemptRecord>(data);
    round_trip::<StepRecord>(data);
    round_trip::<RunRecord>(data);

    if let Some(block) = round_trip::<ContentBlock>(data) {
        let again: ContentBlock = serde_json::from_str(&serde_json::to_string(&block).unwrap()).unwrap();
        assert_eq!(block, again);
    }
    if let Some(result) = round_trip::<CallToolResult>(data) {
        let again: CallToolResult = serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
        assert_eq!(result, again);
        // Whatever it accepted, `structuredContent` is empty or the text of a JSON object.
        if !result.structured_content.is_empty() {
            let parsed: serde_json::Value = serde_json::from_str(&result.structured_content).unwrap();
            assert!(parsed.is_object(), "structuredContent {:?}", result.structured_content);
        }
        CallToolResult::try_new(result.content.clone(), result.is_error, result.structured_content.clone())
            .expect("an accepted result passes try_new");
    }

    // Accepted wire values go through the renderers: line breaks in caller text must not make lines.
    if let Some(value) = round_trip::<KvValue>(data) {
        if let Ok(out) = render_kv(&[KvItem::new("k", value)], None, &[]) {
            assert!(!out.contains('\r'), "raw CR in {out:?}");
            assert_eq!(out.matches('\n').count(), 0, "one field is one line: {out:?}");
        }
    }
    if let Some(hint) = round_trip::<RecoveryHint>(data) {
        let out = render_recovery(&[hint]);
        assert!(!out.contains('\r'), "raw CR in {out:?}");
        assert_eq!(out.matches('\n').count(), 1, "header plus one row: {out:?}");
    }

    // The tolerant rules, from structured input: this shape is always accepted, and read back exactly.
    let Ok(r) = arbitrary::Unstructured::new(data).arbitrary::<WireResult>() else { return };
    let mut block = serde_json::json!({ "type": "text", "text": r.text });
    if let Some(audience) = &r.audience {
        let names: Vec<&str> = audience.iter().map(|&a| if a { "assistant" } else { "user" }).collect();
        block["annotations"] = serde_json::json!({ "audience": names, "priority": 0.5 });
    }
    let mut wire = serde_json::json!({ "content": [block], r.extra_key.as_str(): r.extra_value });
    if let Some(is_error) = r.is_error {
        wire["isError"] = is_error.into();
    }
    let object: Option<serde_json::Map<String, serde_json::Value>> =
        r.structured.as_ref().map(|e| e.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect());
    if let Some(object) = &object {
        wire["structuredContent"] = serde_json::Value::Object(object.clone());
    } else if r.structured_null {
        wire["structuredContent"] = serde_json::Value::Null;
    }
    // `extra_key` may collide with a real key; the accepted result is then not the one asserted below.
    if ["content", "isError", "structuredContent"].contains(&r.extra_key.as_str()) {
        return;
    }
    let parsed: CallToolResult =
        serde_json::from_value(wire.clone()).unwrap_or_else(|e| panic!("rejected {wire}: {e}"));
    assert_eq!(parsed.content.len(), 1);
    assert_eq!(parsed.content[0].text, r.text);
    assert_eq!(parsed.is_error, r.is_error.unwrap_or(false));
    let want_audience: Vec<Audience> =
        r.audience.iter().flatten().map(|&a| if a { Audience::Assistant } else { Audience::User }).collect();
    assert_eq!(parsed.content[0].audience, want_audience);
    match &object {
        Some(object) => {
            let back: serde_json::Value = serde_json::from_str(&parsed.structured_content).unwrap();
            assert_eq!(back, serde_json::Value::Object(object.clone()));
        }
        None => assert_eq!(parsed.structured_content, ""),
    }

    // And these are rejected: a non-text block, a non-object structuredContent.
    let mut bad = wire.clone();
    bad["content"][0]["type"] = "image".into();
    assert!(serde_json::from_value::<CallToolResult>(bad).is_err());
    let mut bad = wire;
    bad["structuredContent"] = serde_json::json!([r.extra_value]);
    assert!(serde_json::from_value::<CallToolResult>(bad).is_err());
});

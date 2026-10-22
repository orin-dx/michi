#![cfg(feature = "serde")]

use michi_core::audience::Audience;
use michi_core::mcp::{CallToolResult, ContentBlock};
use proptest::prelude::*;

fn audience_strategy() -> impl Strategy<Value = Audience> {
    prop_oneof![Just(Audience::Assistant), Just(Audience::User)]
}

fn content_block_strategy() -> impl Strategy<Value = ContentBlock> {
    ("[a-zA-Z0-9 ]{0,40}", proptest::collection::vec(audience_strategy(), 1..3))
        .prop_map(|(text, audience)| ContentBlock::new(text, audience))
}

fn call_tool_result_strategy() -> impl Strategy<Value = CallToolResult> {
    (proptest::collection::vec(content_block_strategy(), 1..3), any::<bool>())
        .prop_map(|(content, is_error)| CallToolResult::new(content, is_error, "{}"))
}

proptest! {
    #[test]
    fn call_tool_result_round_trips_through_json(r in call_tool_result_strategy()) {
        let json = serde_json::to_string(&r).expect("serializes");
        let back: CallToolResult = serde_json::from_str(&json).expect("deserializes");
        prop_assert_eq!(r, back);
    }

    #[test]
    fn call_tool_result_wire_json_is_mcp_conformant(r in call_tool_result_strategy()) {
        let json = serde_json::to_string(&r).expect("serializes");
        prop_assert!(json.contains(r#""type":"text""#), "missing type discriminator: {json}");
        prop_assert!(json.contains("\"isError\""), "missing camelCase isError: {json}");
        prop_assert!(json.contains("\"structuredContent\""), "missing camelCase structuredContent: {json}");
        prop_assert!(!json.contains("\"is_error\""), "leaked snake_case is_error: {json}");
    }

    #[test]
    fn call_tool_result_with_non_object_structured_content_fails_to_serialize(
        content in proptest::collection::vec(content_block_strategy(), 1..3),
        is_error in any::<bool>(),
        garbage in "[^{][^}]{0,40}", // never a JSON object: does not start with `{`
    ) {
        // MCP requires `structuredContent` to be an object, so anything else is a serialization error (never a
        // panic, never a string or number on the wire), and `try_new` reports the same thing up front.
        let r = CallToolResult::new(content.clone(), is_error, &garbage);
        prop_assert!(serde_json::to_string(&r).is_err(), "{garbage:?} must not serialize");
        prop_assert!(CallToolResult::try_new(content, is_error, &garbage).is_err());
    }
}

//! AC-027/AC-029: `#[non_exhaustive]` on `ContentBlock`/`CallToolResult`/`StatusResponse`
//! prevents struct-literal construction from outside this crate, and on `Audience`
//! rejects an exhaustive match without a wildcard arm (E0004).
//! AC-036: `Health`, `StatusItem` and `StatusResponse` have no `Serialize` or `Deserialize` impl even with the
//! `serde` feature on.
//! error.rs AC-001/AC-002/AC-023: `ErrorClass`/`ErrorCode`/`Error` are
//! `#[non_exhaustive]`, so an exhaustive match without a wildcard arm fails
//! to compile (E0004).
//! error.rs AC-009a: `DomainError`'s builder methods are `#[must_use]`, so
//! discarding their return value under `#![deny(unused_must_use)]` fails.
//! `StatusResponse`, `StatusItem`, `Health`, `ToonOptions`, `ErrorClass`, `KvItem`, `OutputFormat`, `Pipeline` and
//! `PipelineStep` have no `JsonSchema` impl even with the `schemars` feature on: nothing serializes them, so a
//! schema would describe no real wire form.
//! pipeline/mod.rs AC-003: `StepStatus` is `#[non_exhaustive]`, so an
//! exhaustive match without a wildcard arm fails to compile (E0004).
//! 0.4 API hygiene: `FailedOp`, `PartialSuccess`, `KvItem`, `StatusItem`, `RecoveryHint`, `PipelineStep` and
//! `Pipeline` reject struct literals (E0639), and `toon::Value`, `KvValue` and `RetryConfigError` reject a match
//! without a wildcard arm (E0004).

#[test]
fn non_exhaustive_and_no_serde_types_reject_the_forbidden_construction() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/content_block_struct_literal.rs");
    t.compile_fail("tests/ui-fail/call_tool_result_struct_literal.rs");
    t.compile_fail("tests/ui-fail/status_response_struct_literal.rs");
    t.compile_fail("tests/ui-fail/audience_exhaustive_match.rs");
    t.compile_fail("tests/ui-fail/error_class_exhaustive_match.rs");
    t.compile_fail("tests/ui-fail/error_code_exhaustive_match.rs");
    t.compile_fail("tests/ui-fail/error_exhaustive_match.rs");
    t.compile_fail("tests/ui-fail/domain_error_must_use.rs");
    t.compile_fail("tests/ui-fail/step_status_exhaustive_match.rs");
    t.compile_fail("tests/ui-fail/api_hygiene_struct_literals.rs");
    t.compile_fail("tests/ui-fail/api_hygiene_exhaustive_match.rs");
    #[cfg(feature = "serde")]
    {
        t.compile_fail("tests/ui-fail/health_no_serde.rs");
        t.compile_fail("tests/ui-fail/status_item_no_serde.rs");
        t.compile_fail("tests/ui-fail/status_response_no_serde.rs");
        t.compile_fail("tests/ui-fail/health_no_deserialize.rs");
        t.compile_fail("tests/ui-fail/status_item_no_deserialize.rs");
        t.compile_fail("tests/ui-fail/status_response_no_deserialize.rs");
    }
    #[cfg(feature = "schemars")]
    t.compile_fail("tests/ui-fail/unserialized_types_no_schema.rs");
}

//! AC-008: `#[non_exhaustive]` and private fields on `RetryConfig` prevent struct-literal construction from outside
//! this crate.

#[test]
fn retry_config_rejects_struct_literal() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/resilience_*.rs");
}

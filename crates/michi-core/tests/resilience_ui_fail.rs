//! AC-008: `#[non_exhaustive]` on `RetryConfig` prevents struct-literal construction from outside this crate.

#[test]
fn non_exhaustive_types_reject_struct_literal() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/resilience_*.rs");
}

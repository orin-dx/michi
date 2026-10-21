//! AC-008: `#[non_exhaustive]` on `RetryConfig` and `AlreadyDone` prevents
//! both struct-literal construction and non-`..` exhaustive matching from
//! outside this crate.

#[test]
fn non_exhaustive_types_reject_struct_literal_and_exhaustive_match() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/resilience_*.rs");
}

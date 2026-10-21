//! AC-011: `#[non_exhaustive]` on `Truncated` prevents both struct-literal
//! construction and non-`..` exhaustive matching from outside this crate.

#[test]
fn non_exhaustive_truncated_rejects_struct_literal_and_exhaustive_match() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/truncate_*.rs");
}

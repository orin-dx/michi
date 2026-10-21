#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui-fail/toon_*.rs");
    #[cfg(not(feature = "serde"))]
    t.compile_fail("tests/ui-fail/serde_gate_toon_list.rs");
}

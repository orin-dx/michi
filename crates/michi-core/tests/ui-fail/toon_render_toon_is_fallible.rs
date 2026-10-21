fn main() {
    let o = michi_core::toon::ToonOptions::new("t", vec![], vec![]);
    // AC-004b: render_toon returns Result<String, ToonError>, not String --
    // this must fail with E0308 (mismatched types).
    let s: String = michi_core::toon::render_toon(&o);
    let _ = s;
}

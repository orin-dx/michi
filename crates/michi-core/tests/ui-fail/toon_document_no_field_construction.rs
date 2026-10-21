fn main() {
    let o = michi_core::toon::ToonOptions::new("t", vec![], vec![]);

    // AC-001a: the field is private -- struct-literal construction must fail
    // with E0451 (field `opts` of struct `ToonDocument` is private).
    let _direct = michi_core::toon::ToonDocument { opts: &o };
}

fn main() {
    // AC-001a: the field is private -- struct-literal construction must fail
    // (cannot construct with struct literal syntax due to private fields).
    let _direct = michi_core::toon::ToonDocument {};
}

// Without the `serde` feature, `toon::list` is not part of the compiled API.
fn main() {
    let _ = michi_core::toon::list("t", &[0u8; 0]);
}

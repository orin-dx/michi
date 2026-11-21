#![deny(unused_must_use)]

// `ToonOptions::new` builds a value and does nothing else, so discarding it is a mistake the compiler must catch.
fn main() {
    michi_core::toon::ToonOptions::new("t", vec![], vec![]);
}

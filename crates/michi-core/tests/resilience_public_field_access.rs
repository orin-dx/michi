//! AC-048: `IdempotencyKey`'s inner field is public and externally
//! constructible/mutable. This must live in a real integration test
//! (compiled as a separate crate) -- a test inside `lib.rs`'s own
//! `#[cfg(test)] mod tests` cannot observe a field-privacy regression,
//! since it has crate-internal access regardless of `pub`.

use michi_core::resilience::IdempotencyKey;

#[test]
fn inner_field_is_public_and_mutable_from_outside_the_crate() {
    assert_eq!(IdempotencyKey("manual".to_string()).as_str(), "manual");
    let mut k = IdempotencyKey::new("a");
    k.0 = "b".to_string();
    assert_eq!(k.as_str(), "b");
}

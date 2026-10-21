#![cfg(feature = "napi")]

#[test]
fn js_ranged_is_generic_over_bounds() {
    let decimals = michi_core::napi::num::JsRanged::<0, 20>::try_from(6.0).expect("6 is in [0, 20]");
    assert_eq!(decimals.get_u8(), 6);

    let status_like = michi_core::napi::num::JsRanged::<100, 599>::try_from(429.0).expect("429 is in [100, 599]");
    assert_eq!(status_like.get(), 429i64);
}

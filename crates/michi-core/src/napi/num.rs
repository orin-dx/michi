//! Validating newtypes for the NAPI numeric boundary.
//!
//! Every JavaScript number that crosses into a `#[napi]` export in
//! [`crate::napi`] is read as an `f64` via `napi_get_value_double` (a
//! non-coercing read — `napi_get_value_int32`'s `ToInt32` wraparound is
//! never in the path) and then validated through one of this module's
//! `TryFrom<f64>` impls before any michi code runs. A `TryFrom<f64>`
//! failure becomes a thrown `napi::Error` before the wrapped function body
//! runs, so there is no coercion step left at any call site to reimplement
//! inconsistently. This is the shared kernel for the whole NAPI numeric
//! boundary: `JsRanged` (added in a later commit) is generic over its
//! bounds so a future resilience-domain module can define its own aliases
//! over the same type without redefining it.
#![allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]

use napi::bindgen_prelude::{sys, FromNapiValue, ToNapiValue, TypeName, ValidateNapiValue};

/// A JavaScript number accepted only when it is finite, has no fractional
/// part, is `>= 0`, and is `<= 9007199254740991`
/// (`Number.MAX_SAFE_INTEGER`). Stores the validated value as `usize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsCount(usize);

impl JsCount {
    /// Returns the validated count.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl TryFrom<f64> for JsCount {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        if v.is_finite() && v.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&v) {
            Ok(Self(v as usize))
        } else {
            Err(format!("expected a non-negative integer no greater than 9007199254740991, got {v}"))
        }
    }
}

impl TypeName for JsCount {
    fn type_name() -> &'static str {
        "JsCount"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl ValidateNapiValue for JsCount {}

impl FromNapiValue for JsCount {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl ToNapiValue for JsCount {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0 as f64) }
    }
}

/// A JavaScript number accepted only when it is finite, has no fractional
/// part, and lies within `[-9007199254740991, 9007199254740991]` — the
/// range over which a JS number and an `i64` agree exactly. Stores `i64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsInt(i64);

impl JsInt {
    /// Returns the validated integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<f64> for JsInt {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        if v.is_finite() && v.fract() == 0.0 && (-9_007_199_254_740_991.0..=9_007_199_254_740_991.0).contains(&v) {
            Ok(Self(v as i64))
        } else {
            Err(format!("expected an integer in [-9007199254740991, 9007199254740991], got {v}"))
        }
    }
}

impl TypeName for JsInt {
    fn type_name() -> &'static str {
        "JsInt"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl ValidateNapiValue for JsInt {}

impl FromNapiValue for JsInt {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl ToNapiValue for JsInt {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0 as f64) }
    }
}

/// The shared kernel type for every bounded-integer position on the NAPI
/// boundary, in either domain. Accepts a JS number only when it is finite,
/// has no fractional part, and satisfies `MIN <= v <= MAX`. Generic and
/// reusable — not specialized to any single domain's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsRanged<const MIN: i64, const MAX: i64>(i64);

impl<const MIN: i64, const MAX: i64> JsRanged<MIN, MAX> {
    const U8_FITS: () = assert!(MIN >= 0 && MAX <= u8::MAX as i64, "JsRanged bounds do not fit in u8");

    /// Returns the validated integer.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Returns the validated integer narrowed to `u8`. Only valid when
    /// `MIN >= 0 && MAX <= u8::MAX`; instantiating this function with
    /// bounds that don't fit `u8` is a compile error (`E0080`) via
    /// `U8_FITS`.
    // NOT `const fn`: `u8::try_from` cannot be called inside a const fn on
    // this workspace's pinned rustc 1.96 (E0658 — `TryFrom` is not yet a
    // const trait).
    #[must_use]
    pub fn get_u8(self) -> u8 {
        let () = Self::U8_FITS;
        u8::try_from(self.0).unwrap_or(u8::MAX)
    }

    const U16_FITS: () = assert!(MIN >= 0 && MAX <= u16::MAX as i64, "JsRanged bounds do not fit in u16");

    /// Returns the validated integer narrowed to `u16`. Only valid when
    /// `MIN >= 0 && MAX <= u16::MAX`; instantiating this function with
    /// bounds that don't fit `u16` is a compile error (`E0080`) via
    /// `U16_FITS`.
    #[must_use]
    pub fn get_u16(self) -> u16 {
        let () = Self::U16_FITS;
        u16::try_from(self.0).unwrap_or(u16::MAX)
    }

    const U32_FITS: () = assert!(MIN >= 0 && MAX <= u32::MAX as i64, "JsRanged bounds do not fit in u32");

    /// Returns the validated integer narrowed to `u32`. Only valid when
    /// `MIN >= 0 && MAX <= u32::MAX`; instantiating this function with
    /// bounds that don't fit `u32` is a compile error (`E0080`) via
    /// `U32_FITS`.
    #[must_use]
    pub fn get_u32(self) -> u32 {
        let () = Self::U32_FITS;
        u32::try_from(self.0).unwrap_or(u32::MAX)
    }
}

impl<const MIN: i64, const MAX: i64> TryFrom<f64> for JsRanged<MIN, MAX> {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        if v.is_finite() && v.fract() == 0.0 && v >= MIN as f64 && v <= MAX as f64 {
            Ok(Self(v as i64))
        } else {
            Err(format!("expected an integer in [{MIN}, {MAX}], got {v}"))
        }
    }
}

impl<const MIN: i64, const MAX: i64> TypeName for JsRanged<MIN, MAX> {
    fn type_name() -> &'static str {
        "JsRanged"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl<const MIN: i64, const MAX: i64> ValidateNapiValue for JsRanged<MIN, MAX> {}

impl<const MIN: i64, const MAX: i64> FromNapiValue for JsRanged<MIN, MAX> {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl<const MIN: i64, const MAX: i64> ToNapiValue for JsRanged<MIN, MAX> {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0 as f64) }
    }
}

/// The rendering-domain alias used at `JsToonValue.decimalsVal`. Consumed
/// via `get_u8()`, which compiles because `[0, 20]` fits `u8`.
pub type JsDecimals = JsRanged<0, 20>;

/// The resilience-domain alias matching `crate::resilience::RetryConfig`'s
/// `max_retries: u32` domain and `next_retry_delay`'s `attempt: u32`
/// domain exactly. Consumed via `get_u32()`.
pub type JsRetryCount = JsRanged<0, 4_294_967_295>;

/// The resilience-domain alias for the conventional HTTP status-code
/// domain. Consumed via `get_u16()`.
pub type JsHttpStatus = JsRanged<100, 599>;

/// A JavaScript number accepted only when `f64::is_finite` holds — `NaN`,
/// `Infinity`, and `-Infinity` are rejected. Stores `f64`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JsFloat(f64);

impl JsFloat {
    /// Returns the validated float.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for JsFloat {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        if v.is_finite() {
            Ok(Self(v))
        } else {
            Err(format!("expected a finite number, got {v}"))
        }
    }
}

impl TypeName for JsFloat {
    fn type_name() -> &'static str {
        "JsFloat"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl ValidateNapiValue for JsFloat {}

impl FromNapiValue for JsFloat {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl ToNapiValue for JsFloat {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0) }
    }
}

/// A JavaScript number accepted only when it is finite and lies within
/// `[0.0, 1.0]`. Delegates its finiteness check to [`JsFloat::try_from`]
/// (both share `type Error = String`), so a non-finite input is rejected
/// with `JsFloat`'s own message verbatim, not a `JsUnitInterval`-specific
/// string; only a finite value then receives this type's own range check.
/// Used for `next_retry_delay`'s `jitter_factor` and `jitter_seed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JsUnitInterval(f64);

impl JsUnitInterval {
    /// Returns the validated value.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for JsUnitInterval {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        let v = JsFloat::try_from(v)?.get();
        if (0.0..=1.0).contains(&v) {
            Ok(Self(v))
        } else {
            Err(format!("expected a finite number in [0.0, 1.0], got {v}"))
        }
    }
}

impl TypeName for JsUnitInterval {
    fn type_name() -> &'static str {
        "JsUnitInterval"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl ValidateNapiValue for JsUnitInterval {}

impl FromNapiValue for JsUnitInterval {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl ToNapiValue for JsUnitInterval {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0) }
    }
}

/// A JavaScript number, in milliseconds, accepted only when it is finite,
/// `>= 0.0`, and its seconds-equivalent (`v / 1000.0`) is strictly less
/// than `u64::MAX as f64` (`2^64`, one greater than the true `u64::MAX`)
/// so [`Self::as_duration`] can never panic. Delegates its finiteness
/// check to [`JsFloat::try_from`] (both share `type Error = String`), so a
/// non-finite input is rejected with `JsFloat`'s own message verbatim.
/// Used for `next_retry_delay`'s `base_delay_ms`, `max_delay_ms`, and
/// `retry_after_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JsDelayMillis(f64);

impl JsDelayMillis {
    /// Returns the validated value, in milliseconds.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Converts to a [`std::time::Duration`]. Infallible: the `TryFrom<f64>`
    /// bound already guarantees `self.0 / 1000.0 < u64::MAX as f64`.
    #[must_use]
    pub fn as_duration(self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.0 / 1000.0)
    }
}

impl TryFrom<f64> for JsDelayMillis {
    type Error = String;

    fn try_from(v: f64) -> Result<Self, Self::Error> {
        let v = JsFloat::try_from(v)?.get();
        if v >= 0.0 && v / 1000.0 < u64::MAX as f64 {
            Ok(Self(v))
        } else {
            Err(format!(
                "expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got {v}"
            ))
        }
    }
}

impl TypeName for JsDelayMillis {
    fn type_name() -> &'static str {
        "JsDelayMillis"
    }

    fn value_type() -> napi::ValueType {
        napi::ValueType::Number
    }
}

impl ValidateNapiValue for JsDelayMillis {}

impl FromNapiValue for JsDelayMillis {
    unsafe fn from_napi_value(env: sys::napi_env, napi_val: sys::napi_value) -> napi::Result<Self> {
        let v = unsafe { f64::from_napi_value(env, napi_val)? };
        Self::try_from(v).map_err(|msg| napi::Error::new(napi::Status::InvalidArg, msg))
    }
}

impl ToNapiValue for JsDelayMillis {
    unsafe fn to_napi_value(env: sys::napi_env, val: Self) -> napi::Result<sys::napi_value> {
        unsafe { f64::to_napi_value(env, val.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_count_accepts_valid_domain() {
        for (v, expected) in
            [(0.0, 0usize), (1.0, 1), (42.0, 42), (1e6, 1_000_000), (9_007_199_254_740_991.0, 9_007_199_254_740_991)]
        {
            assert_eq!(JsCount::try_from(v).map(JsCount::get), Ok(expected), "input {v}");
        }
    }

    #[test]
    fn js_count_rejects_out_of_domain() {
        for v in [-1.0, -0.5, 0.5, 1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 9_007_199_254_740_992.0] {
            let err = JsCount::try_from(v).expect_err("out-of-domain value must be rejected");
            assert_eq!(err, format!("expected a non-negative integer no greater than 9007199254740991, got {v}"));
        }
    }

    #[test]
    fn js_int_accepts_safe_integers() {
        for (v, expected) in [
            (0.0, 0i64),
            (-1.0, -1),
            (1.0, 1),
            (-9_007_199_254_740_991.0, -9_007_199_254_740_991),
            (9_007_199_254_740_991.0, 9_007_199_254_740_991),
            (-1_755_000_000_000.0, -1_755_000_000_000),
        ] {
            assert_eq!(JsInt::try_from(v).map(JsInt::get), Ok(expected), "input {v}");
        }
    }

    #[test]
    fn js_int_rejects_fractional_and_unsafe() {
        for v in
            [1.5, -1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 9_007_199_254_740_992.0, -9_007_199_254_740_992.0]
        {
            let err = JsInt::try_from(v).expect_err("out-of-domain value must be rejected");
            assert_eq!(err, format!("expected an integer in [-9007199254740991, 9007199254740991], got {v}"));
        }
    }

    #[test]
    fn js_count_type_name_is_js_count() {
        assert_eq!(JsCount::type_name(), "JsCount");
    }

    #[test]
    fn js_int_type_name_is_js_int() {
        assert_eq!(JsInt::type_name(), "JsInt");
    }

    #[test]
    fn js_ranged_type_name_is_js_ranged() {
        assert_eq!(JsRanged::<0, 20>::type_name(), "JsRanged");
    }

    #[test]
    fn js_float_type_name_is_js_float() {
        assert_eq!(JsFloat::type_name(), "JsFloat");
    }

    #[test]
    fn js_decimals_accepts_zero_through_twenty() {
        for (v, expected) in [(0.0, 0u8), (6.0, 6), (20.0, 20)] {
            let d = JsDecimals::try_from(v).expect("in-domain decimals value");
            assert_eq!(d.get_u8(), expected, "input {v}");
        }
    }

    #[test]
    fn js_decimals_rejects_out_of_range() {
        for v in [-1.0, 21.0, 6.5, f64::NAN, f64::INFINITY] {
            let err = JsDecimals::try_from(v).expect_err("out-of-range decimals value must be rejected");
            assert_eq!(err, format!("expected an integer in [0, 20], got {v}"));
        }
    }

    #[test]
    fn js_float_accepts_finite_rejects_nan_and_infinity() {
        for v in [0.0, -0.0, 1.5, -1.5, f64::MAX, f64::MIN_POSITIVE] {
            let f = JsFloat::try_from(v).expect("finite value must be accepted");
            assert_eq!(f.get().to_bits(), v.to_bits(), "input {v}");
        }
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = JsFloat::try_from(v).expect_err("non-finite value must be rejected");
            assert_eq!(err, format!("expected a finite number, got {v}"));
        }
    }

    #[test]
    fn js_unit_interval_accepts_domain_boundaries() {
        for v in [0.0, 0.5, 1.0] {
            let u = JsUnitInterval::try_from(v).expect("in-domain unit interval value");
            assert_eq!(u.get(), v, "input {v}");
        }
    }

    #[test]
    fn js_unit_interval_rejects_out_of_range() {
        for v in [1.5, -0.1] {
            let err = JsUnitInterval::try_from(v).expect_err("out-of-range value must be rejected");
            assert_eq!(err, format!("expected a finite number in [0.0, 1.0], got {v}"));
        }
    }

    #[test]
    fn js_unit_interval_rejects_non_finite_via_delegated_message() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = JsUnitInterval::try_from(v).expect_err("non-finite value must be rejected");
            assert_eq!(err, format!("expected a finite number, got {v}"));
        }
    }

    #[test]
    fn js_delay_millis_accepts_domain_boundaries() {
        for v in [0.0, 1000.0, 1.844674407370955e22] {
            let d = JsDelayMillis::try_from(v).expect("in-domain delay value");
            assert_eq!(d.get(), v, "input {v}");
        }
    }

    #[test]
    fn js_delay_millis_rejects_negative_and_overflow() {
        for v in [-1.0, 2e22, 1.8446744073709552e22] {
            let err = JsDelayMillis::try_from(v).expect_err("out-of-domain value must be rejected");
            assert_eq!(
                err,
                format!(
                    "expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got {v}"
                )
            );
        }
    }

    #[test]
    fn js_delay_millis_rejects_non_finite_via_delegated_message() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = JsDelayMillis::try_from(v).expect_err("non-finite value must be rejected");
            assert_eq!(err, format!("expected a finite number, got {v}"));
        }
    }

    #[test]
    fn js_delay_millis_as_duration_computes_seconds_equivalent() {
        let d = JsDelayMillis::try_from(1500.0).expect("1500 is in-domain");
        assert_eq!(d.as_duration(), std::time::Duration::from_secs_f64(1.5));
    }

    #[test]
    fn js_retry_count_get_u32_round_trips_domain_boundaries() {
        for (v, expected) in [(0.0, 0u32), (4_294_967_295.0, u32::MAX)] {
            let r = JsRetryCount::try_from(v).expect("in-domain retry count");
            assert_eq!(r.get_u32(), expected, "input {v}");
        }
    }

    #[test]
    fn js_retry_count_rejects_out_of_domain() {
        for v in [-1.0, 4_294_967_296.0, 2.5] {
            let err = JsRetryCount::try_from(v).expect_err("out-of-domain value must be rejected");
            assert_eq!(err, format!("expected an integer in [0, 4294967295], got {v}"));
        }
    }

    #[test]
    fn js_http_status_get_u16_round_trips_domain_boundaries() {
        for (v, expected) in [(100.0, 100u16), (599.0, 599u16)] {
            let s = JsHttpStatus::try_from(v).expect("in-domain status");
            assert_eq!(s.get_u16(), expected, "input {v}");
        }
    }

    #[test]
    fn js_http_status_rejects_out_of_domain() {
        for v in [50.0, 70000.0, 429.5] {
            let err = JsHttpStatus::try_from(v).expect_err("out-of-domain value must be rejected");
            assert_eq!(err, format!("expected an integer in [100, 599], got {v}"));
        }
    }

    #[test]
    fn unit_interval_and_delay_millis_delegate_finiteness_to_js_float() {
        let src = include_str!("num.rs");
        for type_name in ["JsUnitInterval", "JsDelayMillis"] {
            let marker = format!("impl TryFrom<f64> for {type_name} {{");
            let start = src.find(&marker).unwrap_or_else(|| panic!("missing impl TryFrom<f64> for {type_name}"));
            let body_start = start + marker.len();
            let mut depth = 1;
            let mut end = body_start;
            for (i, c) in src[body_start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = body_start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &src[body_start..end];
            assert!(
                body.contains("JsFloat::try_from"),
                "{type_name}'s TryFrom<f64> impl does not delegate to JsFloat::try_from"
            );
        }
    }

    #[test]
    fn deleted_jitter_tests_do_not_reappear_in_napi_rs() {
        let napi_src = include_str!("../napi.rs");
        for deleted in [
            "next_retry_delay_rejects_nan_jitter_factor",
            "next_retry_delay_rejects_nan_jitter_seed",
            ".contains(\"jitter_factor\")",
            ".contains(\"jitter_seed\")",
        ] {
            assert!(!napi_src.contains(deleted), "deleted test artifact `{deleted}` reappeared in src/napi.rs");
        }
    }

    #[test]
    fn js_unit_interval_type_name_is_js_unit_interval() {
        assert_eq!(JsUnitInterval::type_name(), "JsUnitInterval");
    }

    #[test]
    fn js_delay_millis_type_name_is_js_delay_millis() {
        assert_eq!(JsDelayMillis::type_name(), "JsDelayMillis");
    }

    #[test]
    fn module_denies_cast_lints() {
        let src = include_str!("../napi.rs");
        // Whitespace-stripped: rustfmt wraps this attribute's ~147-char
        // single-line form across multiple lines under this project's
        // pinned `max_width = 120`, so the substring check tolerates
        // rustfmt's line-wrapping while still pinning the exact lint set
        // and order.
        let stripped: String = src.chars().filter(|c| !c.is_whitespace()).collect();
        let expected: String = "#![deny(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)]"
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(stripped.contains(&expected), "expected the module-level deny attribute for cast lints");
    }

    #[test]
    fn napi_numeric_docs_are_consistent() {
        let Some(docs) = crate::test_support::workspace_file("docs/spec/04-mcp-and-napi.md") else { return };
        assert!(!docs.contains("clamped non-negative"), "docs still describe clamping");
        assert!(!docs.contains("n.max(0) as usize"), "docs still show the deleted clamp expression");
        assert!(!docs.contains("silently lose precision"), "docs still describe silent precision loss");
        for field in [
            "totalCount",
            "decimalsVal",
            "maxChars",
            "intVal",
            "floatVal",
            "baseDelayMs",
            "maxDelayMs",
            "jitterFactor",
            "jitterSeed",
            "retryAfterMs",
            "maxRetries",
            "attempt",
            "status",
        ] {
            assert!(docs.contains(field), "numeric boundary docs missing rejection contract for {field}");
        }
    }

    #[test]
    fn boundary_carries_exactly_one_cast_allow_and_no_residual_coercion() {
        let napi_src = include_str!("../napi.rs");
        assert_eq!(
            napi_src.matches("#[allow(clippy::cast_possible_truncation").count(),
            0,
            "expected zero cast-allow attributes to remain in src/napi.rs"
        );
        for residual in [".max(0)", "usize::try_from", ".clamp(0, 20)", "u16::try_from", ".clamp(0.0, 1.0)"] {
            assert!(!napi_src.contains(residual), "residual coercion expression `{residual}` found in src/napi.rs");
        }
    }
}

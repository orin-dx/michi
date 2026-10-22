//! `KvValue` serde must round-trip exactly, including non-finite floats, and its schema must describe that wire form.
#![cfg(feature = "serde")]

use michi_core::kv::KvValue;

fn round_trip(v: &KvValue) -> (String, KvValue) {
    let json = serde_json::to_string(v).expect("serializes");
    let back = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json} must deserialize: {e}"));
    (json, back)
}

#[test]
fn non_finite_floats_serialize_as_named_strings_and_round_trip() {
    let (json, back) = round_trip(&KvValue::Float(f64::NAN, 2));
    assert_eq!(json, r#"{"Float":["NaN",2]}"#);
    assert!(matches!(back, KvValue::Float(f, 2) if f.is_nan()), "{back:?}");

    for (f, token) in [(f64::INFINITY, "Infinity"), (f64::NEG_INFINITY, "-Infinity")] {
        let (json, back) = round_trip(&KvValue::Float(f, 3));
        assert_eq!(json, format!(r#"{{"Float":["{token}",3]}}"#));
        assert_eq!(back, KvValue::Float(f, 3));
    }
}

#[test]
fn finite_floats_stay_json_numbers() {
    let (json, back) = round_trip(&KvValue::Float(1.25, 2));
    assert_eq!(json, r#"{"Float":[1.25,2]}"#);
    assert_eq!(back, KvValue::Float(1.25, 2));
    let (_, back) = round_trip(&KvValue::Float(-0.0, 0));
    assert!(matches!(back, KvValue::Float(f, 0) if f == 0.0 && f.is_sign_negative()), "{back:?}");
}

#[test]
fn every_variant_round_trips() {
    for v in [
        KvValue::Text("t".into()),
        KvValue::Int(-7),
        KvValue::Float(f64::MAX, 255),
        KvValue::Bool(true),
        KvValue::Duration(std::time::Duration::from_nanos(1)),
        KvValue::Missing,
    ] {
        assert_eq!(round_trip(&v).1, v);
    }
}

#[test]
fn other_strings_are_rejected_for_the_float() {
    for bad in [r#"{"Float":["nan",2]}"#, r#"{"Float":["inf",2]}"#, r#"{"Float":["1.5",2]}"#, r#"{"Float":[null,2]}"#] {
        assert!(serde_json::from_str::<KvValue>(bad).is_err(), "{bad} must be rejected");
    }
}

#[cfg(feature = "schemars")]
#[test]
fn schema_allows_the_named_non_finite_strings_for_the_float() {
    let root = serde_json::to_value(schemars::schema_for!(KvValue)).expect("schema serializes");
    let float = root["oneOf"]
        .as_array()
        .expect("externally tagged enum")
        .iter()
        .find_map(|s| s["properties"].get("Float"))
        .expect("Float variant");
    let number = &float["items"][0];
    let alternatives =
        number["anyOf"].as_array().unwrap_or_else(|| panic!("f64 is number or named string: {number:#}"));
    assert!(alternatives.iter().any(|a| a["type"] == "number"), "{number:#}");
    assert!(
        alternatives.iter().any(|a| a["enum"] == serde_json::json!(["NaN", "Infinity", "-Infinity"])),
        "{number:#}"
    );
}

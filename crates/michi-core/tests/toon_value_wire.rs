//! `toon::Value` serde must round-trip exactly, including non-finite floats (same wire form as `KvValue`), while the
//! rendered TOON keeps `null` for them. f32-origin floats render with f32's shortest digits.

use michi_core::toon::{render_toon, ToonOptions, Value};

#[cfg(feature = "serde")]
fn round_trip(v: &Value) -> (String, Value) {
    let json = serde_json::to_string(v).expect("serializes");
    let back = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json} must deserialize: {e}"));
    (json, back)
}

#[cfg(feature = "serde")]
#[test]
fn non_finite_floats_serialize_as_named_strings_and_round_trip() {
    let (json, back) = round_trip(&Value::Float(f64::NAN));
    assert_eq!(json, r#"{"Float":"NaN"}"#);
    assert!(matches!(back, Value::Float(f) if f.is_nan()), "{back:?}");
    for (f, token) in [(f64::INFINITY, "Infinity"), (f64::NEG_INFINITY, "-Infinity")] {
        let (json, back) = round_trip(&Value::Float(f));
        assert_eq!(json, format!(r#"{{"Float":"{token}"}}"#));
        assert_eq!(back, Value::Float(f));
    }
    let (json, back) = round_trip(&Value::Float(1.25));
    assert_eq!(json, r#"{"Float":1.25}"#, "finite floats stay JSON numbers");
    assert_eq!(back, Value::Float(1.25));
    assert!(serde_json::from_str::<Value>(r#"{"Float":"nan"}"#).is_err());
}

#[test]
fn non_finite_floats_still_render_as_null() {
    let opts =
        ToonOptions::new("t", vec!["v".into()], vec![vec![Value::Float(f64::NAN)], vec![Value::Float(f64::INFINITY)]]);
    assert_eq!(render_toon(&opts).unwrap(), "t[2]{v}:\n  null\n  null");
}

#[cfg(feature = "schemars")]
#[test]
fn schema_allows_the_named_non_finite_strings_for_the_float() {
    let root = serde_json::to_value(schemars::schema_for!(Value)).expect("schema serializes");
    let float = root["oneOf"]
        .as_array()
        .expect("externally tagged enum")
        .iter()
        .find_map(|s| s["properties"].get("Float"))
        .unwrap_or_else(|| panic!("Float variant: {root:#}"));
    let alternatives = float["anyOf"].as_array().unwrap_or_else(|| panic!("number or named string: {float:#}"));
    assert!(alternatives.iter().any(|a| a["type"] == "number"), "{float:#}");
    assert!(alternatives.iter().any(|a| a["enum"] == serde_json::json!(["NaN", "Infinity", "-Infinity"])), "{float:#}");
}

#[test]
fn f32_values_render_with_f32_shortest_digits() {
    assert_eq!(Value::from(0.1f32).to_string(), "0.1");
    assert_eq!(Value::from(1.1f32).to_string(), "1.1");
    assert_eq!(Value::from(16_777_217f32).to_string(), "16777216");
    assert_eq!(Value::from(f32::NAN).to_string(), "null");
    assert_eq!(Value::from(f32::MAX).to_string(), "3.4028235e+38");
    assert_eq!(Value::from(1e-7f32).to_string(), "1e-7");
}

#[cfg(feature = "serde")]
#[test]
fn list_renders_f32_fields_with_f32_shortest_digits() {
    #[derive(serde::Serialize)]
    struct Row {
        ratio: f32,
    }
    let out = michi_core::toon::list("row", &[Row { ratio: 0.1 }, Row { ratio: 2.5 }]).unwrap();
    assert_eq!(out, "row[2]{ratio}:\n  0.1\n  2.5");
}

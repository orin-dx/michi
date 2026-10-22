//! `RecoveryHint` has one JSON shape: serde, the schema and both hand-written structured companions agree.
#![cfg(feature = "serde")]

use michi_core::{AgentResponse, DomainError, ErrorCode, KvValue, OutputFormat, RecoveryHint};

fn hints() -> Vec<RecoveryHint> {
    vec![
        RecoveryHint::new("create_item").param("project", KvValue::Text("PROJ".into())).param("count", KvValue::Int(3)),
        RecoveryHint::new("list_items").reason("item may have moved"),
    ]
}

fn companion_recovery(json: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(json).expect("structured content is JSON");
    v["recovery"].clone()
}

#[test]
fn serde_matches_the_agent_response_companion() {
    let mut response = AgentResponse::new("t");
    for h in hints() {
        response = response.recovery_hint(h);
    }
    let serde_form = serde_json::to_value(hints()).expect("serializes");
    assert_eq!(serde_form, companion_recovery(&response.render(OutputFormat::Json)));
    assert_eq!(serde_form[0]["params"], serde_json::json!({"project": "PROJ", "count": 3}));
    assert!(serde_form[0].get("reason").is_none(), "absent reason is omitted, not null");
}

#[test]
fn serde_matches_the_domain_error_companion() {
    let hint = hints().remove(0);
    let err = DomainError::new(ErrorCode::NotFound, "m").recovery(hint.clone());
    let companion: serde_json::Value = serde_json::from_str(&err.render_json()).expect("JSON");
    assert_eq!(serde_json::to_value(&hint).expect("serializes"), companion["recovery"]);
}

#[test]
fn wire_form_round_trips_text_int_bool_and_missing() {
    let hint = RecoveryHint::new("t")
        .param("s", KvValue::Text("x".into()))
        .param("n", KvValue::Int(-4))
        .param("b", KvValue::Bool(true))
        .param("m", KvValue::Missing)
        .reason("r");
    let json = serde_json::to_string(&hint).expect("serializes");
    assert_eq!(serde_json::from_str::<RecoveryHint>(&json).expect("deserializes"), hint);
}

#[test]
fn small_and_fractional_floats_round_trip_exactly() {
    for json in [
        r#"{"tool":"t","params":{"x":0.000001}}"#,
        r#"{"tool":"t","params":{"x":1e-6}}"#,
        r#"{"tool":"t","params":{"x":2.5}}"#,
    ] {
        let hint: RecoveryHint = serde_json::from_str(json).expect("deserializes");
        let back = serde_json::to_value(&hint).expect("serializes");
        let original: serde_json::Value = serde_json::from_str(json).expect("json");
        assert_eq!(back["params"]["x"].as_f64(), original["params"]["x"].as_f64(), "value changed for {json}: {back}");
    }
}

#[test]
fn values_the_model_cannot_represent_are_rejected_not_rewritten() {
    for json in [r#"{"tool":"t","params":{"x":[1,2]}}"#, r#"{"tool":"t","params":{"x":{"a":1}}}"#] {
        assert!(serde_json::from_str::<RecoveryHint>(json).is_err(), "must reject, not coerce: {json}");
    }
}

// Every JSON number is accepted, as the schema says: one outside i64 is a float with the value any JSON reader gives
// it, never rounded further. 2^63 is exact; tiny values need more than 255 places and used to be rejected.
#[test]
fn numbers_outside_i64_and_tiny_floats_keep_their_value() {
    for (json, expected) in [
        (r#"{"tool":"t","params":{"x":9223372036854775808}}"#, 9_223_372_036_854_775_808.0_f64),
        (r#"{"tool":"t","params":{"x":-9223372036854775809}}"#, -9_223_372_036_854_775_808.0),
        (r#"{"tool":"t","params":{"x":1e-300}}"#, 1e-300),
        (r#"{"tool":"t","params":{"x":5e-324}}"#, 5e-324),
    ] {
        let hint: RecoveryHint = serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"));
        let KvValue::Float(f, _) = hint.params[0].1 else { panic!("{json} read as {:?}", hint.params[0].1) };
        assert_eq!(f.to_bits(), expected.to_bits(), "{json}");
    }
}

#[cfg(feature = "schemars")]
#[test]
fn schema_describes_the_wire_form() {
    let schema = serde_json::to_value(schemars::schema_for!(RecoveryHint)).expect("schema serializes");
    let props = schema["properties"].as_object().expect("object schema");
    assert_eq!(props["params"]["type"], "object", "params is a JSON object on the wire: {schema:#}");
    let required: Vec<&str> =
        schema["required"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).collect();
    assert!(!required.contains(&"reason"), "reason is optional on the wire: {schema:#}");
}

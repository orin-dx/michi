//! Generated JSON Schemas must describe what serde actually puts on the wire, checked against real serialized values.
#![cfg(all(feature = "serde", feature = "schemars"))]

use michi_core::kv::KvValue;
use michi_core::{Audience, CallToolResult, ContentBlock, DomainError, ErrorCode, RecoveryHint};
use std::collections::BTreeSet;

/// Resolve a `$ref` against the root schema's definitions, or return the schema itself.
fn resolve<'a>(root: &'a serde_json::Value, schema: &'a serde_json::Value) -> &'a serde_json::Value {
    // A `$ref` next to `default` is wrapped as `allOf: [{"$ref": …}]`.
    if let Some([only]) = schema.get("allOf").and_then(serde_json::Value::as_array).map(Vec::as_slice) {
        return resolve(root, only);
    }
    match schema.get("$ref").and_then(serde_json::Value::as_str) {
        Some(r) => {
            let name = r.rsplit('/').next().expect("ref has a name");
            &root["definitions"][name]
        }
        None => schema,
    }
}

fn keys(v: &serde_json::Value) -> BTreeSet<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// Every key serde emits is a schema property, and every required property is emitted.
#[track_caller]
fn assert_object_matches(root: &serde_json::Value, schema: &serde_json::Value, value: &serde_json::Value) {
    let schema = resolve(root, schema);
    let props = keys(&schema["properties"]);
    assert_eq!(props, keys(value), "schema properties vs serialized keys\nschema: {schema:#}\nvalue: {value:#}");
    for req in schema["required"].as_array().into_iter().flatten() {
        let req = req.as_str().expect("required entries are strings");
        assert!(value.get(req).is_some(), "required `{req}` missing from serialized value {value:#}");
    }
}

#[test]
fn call_tool_result_schema_matches_its_wire_form() {
    let result = CallToolResult::new(
        vec![ContentBlock::new("body", vec![Audience::Assistant])],
        false,
        r#"{"body":"x","isError":false}"#,
    );
    let value = serde_json::to_value(&result).expect("serializes");
    let root = serde_json::to_value(schemars::schema_for!(CallToolResult)).expect("schema serializes");

    assert_object_matches(&root, &root, &value);
    let structured = &resolve(&root, &root)["properties"]["structuredContent"];
    assert_ne!(structured["type"], "string", "structuredContent is parsed JSON on the wire, not a string");

    let content_items = &resolve(&root, &root)["properties"]["content"]["items"];
    assert_object_matches(&root, content_items, &value["content"][0]);
    let annotations = &resolve(&root, content_items)["properties"]["annotations"];
    assert_object_matches(&root, annotations, &value["content"][0]["annotations"]);
}

#[test]
fn call_tool_result_schema_requires_only_what_input_requires() {
    let root = serde_json::to_value(schemars::schema_for!(CallToolResult)).expect("schema serializes");
    assert_eq!(root["required"], serde_json::json!(["content"]), "isError and structuredContent are optional");
    assert_eq!(resolve(&root, &root)["properties"]["structuredContent"]["type"], "object");
    let block = serde_json::to_value(schemars::schema_for!(ContentBlock)).expect("schema serializes");
    assert_eq!(block["required"], serde_json::json!(["text", "type"]), "annotations are optional");
}

fn domain_errors() -> Vec<DomainError> {
    vec![
        DomainError::new(ErrorCode::NotFound, "missing"),
        DomainError::new(ErrorCode::RateLimited, "slow down")
            .cause("upstream returned 429")
            .cause("quota window: 60s\nremaining: 0")
            .hint("wait")
            .hint("then retry")
            .retry_after(std::time::Duration::from_secs(3))
            .recovery(
                RecoveryHint::new("list_items")
                    .param("limit", KvValue::Int(10))
                    .param("ratio", KvValue::Float(0.25, 2))
                    .param("nan", KvValue::Float(f64::NAN, 2))
                    .param("d", KvValue::Duration(std::time::Duration::from_millis(1500)))
                    .param("m", KvValue::Missing)
                    .reason("why"),
            ),
    ]
}

/// The schema advertised for MCP `outputSchema` must describe what `render_json()` emits.
#[test]
fn domain_error_schema_matches_render_json() {
    let root = serde_json::to_value(schemars::schema_for!(DomainError)).expect("schema serializes");
    let codes: Vec<&str> = [
        ErrorCode::InvalidInput,
        ErrorCode::NotFound,
        ErrorCode::Unauthorized,
        ErrorCode::Forbidden,
        ErrorCode::Conflict,
        ErrorCode::RateLimited,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ExternalFailure,
    ]
    .iter()
    .map(ErrorCode::label)
    .collect();
    let error_schema = resolve(&root, &resolve(&root, &root)["properties"]["error"]);
    // Documented variants become `oneOf` single-value enums; collect them either way.
    let values: Vec<&serde_json::Value> = match error_schema["oneOf"].as_array() {
        Some(alts) => alts.iter().flat_map(|a| a["enum"].as_array().into_iter().flatten()).collect(),
        None => error_schema["enum"].as_array().into_iter().flatten().collect(),
    };
    let allowed: Vec<&str> = values.into_iter().filter_map(serde_json::Value::as_str).collect();
    assert_eq!(allowed, codes, "error codes are the snake_case labels\n{error_schema:#}");

    for err in domain_errors() {
        let value: serde_json::Value = serde_json::from_str(&err.render_json()).expect("render_json is JSON");
        assert_object_matches(&root, &root, &value);
        let recovery = &resolve(&root, &root)["properties"]["recovery"];
        if !value["recovery"].is_null() {
            let recovery_schema = recovery["anyOf"]
                .as_array()
                .and_then(|alts| alts.iter().find(|a| a.get("$ref").is_some()))
                .unwrap_or(recovery);
            assert_object_matches(&root, recovery_schema, &value["recovery"]);
        }
    }
}

/// Serializing a `DomainError` and calling `render_json()` produce the same JSON, so one schema covers both.
#[test]
fn domain_error_serializes_to_its_render_json_form() {
    let mut dup = RecoveryHint::new("t");
    dup.params.push(("x".to_string(), KvValue::Int(1)));
    dup.params.push(("x".to_string(), KvValue::Int(2)));
    let mut errors = domain_errors();
    errors.push(DomainError::new(ErrorCode::Conflict, "a\nb \"q\"").retryable(true).recovery(dup));
    for err in errors {
        let rendered: serde_json::Value = serde_json::from_str(&err.render_json()).expect("render_json is JSON");
        assert_eq!(serde_json::to_value(&err).expect("serializes"), rendered);
        assert_eq!(rendered["isError"], true);
    }
}

#[test]
fn content_block_rejects_a_non_text_type_instead_of_coercing_it() {
    let json = r#"{"type":"image","text":"x","annotations":{"audience":["user"]}}"#;
    assert!(serde_json::from_str::<ContentBlock>(json).is_err(), "an image block must not become a text block");
    let ok = r#"{"type":"text","text":"x","annotations":{"audience":["user"]}}"#;
    assert!(serde_json::from_str::<ContentBlock>(ok).is_ok());
}

#[test]
fn content_block_schema_matches_its_wire_form() {
    let value = serde_json::to_value(ContentBlock::new("t", vec![Audience::User])).expect("serializes");
    let root = serde_json::to_value(schemars::schema_for!(ContentBlock)).expect("schema serializes");
    assert_object_matches(&root, &root, &value);
}

/// Whether a JSON Schema `type` (one name or a list) admits `value`. `integer` is any number with no fractional part,
/// as JSON Schema defines it; `number` is any number.
fn type_admits(schema: &serde_json::Value, value: &serde_json::Value) -> bool {
    let names: Vec<&str> = match &schema["type"] {
        serde_json::Value::String(s) => vec![s.as_str()],
        serde_json::Value::Array(a) => a.iter().filter_map(serde_json::Value::as_str).collect(),
        _ => panic!("expected a `type` keyword in {schema:#}"),
    };
    names.iter().any(|name| match (*name, value) {
        ("string", serde_json::Value::String(_))
        | ("boolean", serde_json::Value::Bool(_))
        | ("null", serde_json::Value::Null)
        | ("number", serde_json::Value::Number(_))
        | ("object", serde_json::Value::Object(_))
        | ("array", serde_json::Value::Array(_)) => true,
        ("integer", serde_json::Value::Number(n)) => {
            n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        _ => false,
    })
}

/// The `params` value schema and the deserializer accept exactly the same values, at every numeric boundary.
#[test]
fn recovery_hint_param_schema_and_deserializer_agree() {
    let root = serde_json::to_value(schemars::schema_for!(RecoveryHint)).expect("schema serializes");
    let value_schema = &resolve(&root, &root)["properties"]["params"]["additionalProperties"];
    for param in [
        "0",
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
        "9223372036854775809",
        "18446744073709551615",
        "18446744073709551616",
        "-9223372036854775809",
        "1e19",
        "0.1",
        "1e-300",
        "5e-324",
        "-5e-324",
        "1.7976931348623157e308",
        "\"s\"",
        "true",
        "null",
        "[1]",
        "{\"a\":1}",
    ] {
        let json = format!(r#"{{"tool":"t","params":{{"x":{param}}}}}"#);
        let value: serde_json::Value = serde_json::from_str(param).expect("valid JSON");
        let parsed = serde_json::from_str::<RecoveryHint>(&json);
        assert_eq!(
            type_admits(value_schema, &value),
            parsed.is_ok(),
            "schema and deserializer disagree on {param}: {parsed:?}\nschema: {value_schema:#}"
        );
        if let Ok(hint) = parsed {
            // Whatever is accepted keeps the value a JSON reader sees, and survives another round trip with it.
            let back = serde_json::to_string(&hint).expect("serializes");
            let again: RecoveryHint = serde_json::from_str(&back).expect("its own output deserializes");
            let again = serde_json::to_value(&again).expect("serializes");
            match value.as_f64() {
                Some(expected) => {
                    let got = again["params"]["x"].as_f64().expect("still a number");
                    assert_eq!(got.to_bits(), expected.to_bits(), "{param} came back as {back}");
                }
                None => assert_eq!(again["params"]["x"], value, "{param} came back as {back}"),
            }
        }
    }
}

/// Every float a `RecoveryHint` can hold serializes to JSON its own deserializer accepts, with the same value.
#[test]
fn recovery_hint_float_params_round_trip() {
    // Integral values above i64::MAX print as plain integer digits at 0 decimals; the rest need their places.
    for (f, decimals) in [
        (1e19, 0),
        (9_223_372_036_854_775_808.0, 0),
        (1.5e300, 0),
        (-2.5, 1),
        (0.25, 2),
        (1e-300, KvValue::FULL_PRECISION),
        (5e-324, KvValue::FULL_PRECISION),
    ] {
        let hint = RecoveryHint::new("t")
            .param("x", KvValue::Float(f, decimals))
            .param("y", KvValue::Float(f, KvValue::FULL_PRECISION));
        let json = serde_json::to_string(&hint).expect("serializes");
        let back: RecoveryHint = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json} rejected: {e}"));
        for (key, value) in &back.params {
            let KvValue::Float(got, _) = value else { panic!("{key} of {json} read back as {value:?}") };
            assert_eq!(got.to_bits(), f.to_bits(), "{key} of {json}");
        }
    }
}

/// The schema of variant `name` in an externally tagged enum's `oneOf`.
fn variant<'a>(root: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    root["oneOf"]
        .as_array()
        .and_then(|alts| alts.iter().find(|a| a["properties"].get(name).is_some()))
        .map(|a| &a["properties"][name])
        .unwrap_or_else(|| panic!("no variant {name} in {root:#}"))
}

/// One integer position: its schema, the bounds serde enforces, and a probe that deserializes a number there.
struct IntCase<'a> {
    schema: &'a serde_json::Value,
    min: &'static str,
    max: &'static str,
    below: &'static str,
    above: &'static str,
    accepts: fn(&str) -> bool,
}

/// Whether an integer schema's bounds admit `n`, compared exactly (the bounds are powers of two or small integers, so
/// their f64 form is exact).
fn bounds_admit(schema: &serde_json::Value, n: &str) -> bool {
    let n: i128 = n.parse().expect("integer");
    #[allow(clippy::cast_possible_truncation)]
    let bound = |key: &str| schema[key].as_f64().map(|f| f as i128);
    bound("minimum").is_none_or(|b| n >= b)
        && bound("maximum").is_none_or(|b| n <= b)
        && bound("exclusiveMaximum").is_none_or(|b| n < b)
}

/// Integer bounds in the schema are exactly the ones serde enforces: the bounds pass, one past them fails, in both
/// the schema and serde. Without bounds, the schema admitted `{"Int": 9223372036854775808}` and a decimals value of
/// 256, which serde rejects.
#[test]
fn integer_schemas_carry_the_bounds_serde_enforces() {
    let kv = serde_json::to_value(schemars::schema_for!(KvValue)).expect("schema serializes");
    let value = serde_json::to_value(schemars::schema_for!(michi_core::toon::Value)).expect("schema serializes");
    let (i64_min, i64_max) = ("-9223372036854775808", "9223372036854775807");
    let (i64_below, i64_above) = ("-9223372036854775809", "9223372036854775808");
    let cases = [
        IntCase {
            schema: variant(&kv, "Int"),
            min: i64_min,
            max: i64_max,
            below: i64_below,
            above: i64_above,
            accepts: |n| serde_json::from_str::<KvValue>(&format!(r#"{{"Int":{n}}}"#)).is_ok(),
        },
        IntCase {
            schema: &variant(&kv, "Float")["items"][1],
            min: "0",
            max: "255",
            below: "-1",
            above: "256",
            accepts: |n| serde_json::from_str::<KvValue>(&format!(r#"{{"Float":[1.5,{n}]}}"#)).is_ok(),
        },
        IntCase {
            schema: variant(&value, "Int"),
            min: i64_min,
            max: i64_max,
            below: i64_below,
            above: i64_above,
            accepts: |n| serde_json::from_str::<michi_core::toon::Value>(&format!(r#"{{"Int":{n}}}"#)).is_ok(),
        },
        IntCase {
            schema: variant(&value, "UInt"),
            min: "0",
            max: "18446744073709551615",
            below: "-1",
            above: "18446744073709551616",
            accepts: |n| serde_json::from_str::<michi_core::toon::Value>(&format!(r#"{{"UInt":{n}}}"#)).is_ok(),
        },
    ];
    for c in cases {
        let schema = resolve(&kv, c.schema);
        for n in [c.min, c.max, c.below, c.above] {
            assert_eq!(bounds_admit(schema, n), (c.accepts)(n), "schema and serde disagree on {n}: {schema}");
        }
        assert!((c.accepts)(c.min) && (c.accepts)(c.max), "serde accepts the bounds {}..={}", c.min, c.max);
        assert!(!(c.accepts)(c.below) && !(c.accepts)(c.above), "serde rejects {} and {}", c.below, c.above);
    }
}

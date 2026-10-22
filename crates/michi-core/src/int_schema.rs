//! JSON Schemas for integer fields that state the bounds serde enforces. schemars' defaults give only a `format`
//! (`int64`, `uint8`), which validators treat as a note, so the schema admitted values serde rejects (`2^63` for an
//! `i64`, `256` for a `u8`).
//!
//! schemars keeps bounds as f64, which cannot hold `i64::MAX` or `u64::MAX`, so the upper bound of those two is the
//! exclusive power of two above them (`exclusiveMaximum: 2^63`), which f64 holds exactly.

fn bounded(format: &str, bounds: serde_json::Value) -> schemars::schema::Schema {
    let mut schema = serde_json::json!({ "type": "integer", "format": format });
    if let (Some(schema), Some(bounds)) = (schema.as_object_mut(), bounds.as_object()) {
        schema.extend(bounds.clone());
    }
    serde_json::from_value(schema).unwrap_or(schemars::schema::Schema::Bool(true))
}

/// `[-2^63, 2^63)`.
pub(crate) fn i64(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    bounded(
        "int64",
        serde_json::json!({ "minimum": -9_223_372_036_854_775_808.0, "exclusiveMaximum": 9_223_372_036_854_775_808.0 }),
    )
}

/// `[0, 2^64)`.
pub(crate) fn u64(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    bounded("uint64", serde_json::json!({ "minimum": 0, "exclusiveMaximum": 18_446_744_073_709_551_616.0 }))
}

/// `[0, 255]`.
pub(crate) fn u8(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    bounded("uint8", serde_json::json!({ "minimum": 0, "maximum": 255 }))
}

//! Serde form of a float that may be NaN or infinite, shared by `KvValue::Float` and `toon::Value::Float`: a JSON
//! number when finite, else one of the strings `"NaN"`, `"Infinity"` or `"-Infinity"`, which JSON numbers cannot
//! express. Rendered output (TOON and the JSON companions) still writes `null` for a non-finite float; this form is
//! only for persisting the value itself, so it round-trips exactly.

use serde::{Deserialize as _, Serialize as _};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum Wire {
    Number(f64),
    Named(NonFinite),
}

#[derive(serde::Serialize, serde::Deserialize)]
enum NonFinite {
    #[serde(rename = "NaN")]
    NaN,
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    NegInfinity,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's `with` passes the field by reference
pub(crate) fn serialize<S: serde::Serializer>(f: &f64, s: S) -> Result<S::Ok, S::Error> {
    let wire = if f.is_nan() {
        Wire::Named(NonFinite::NaN)
    } else if *f == f64::INFINITY {
        Wire::Named(NonFinite::Infinity)
    } else if *f == f64::NEG_INFINITY {
        Wire::Named(NonFinite::NegInfinity)
    } else {
        Wire::Number(*f)
    };
    wire.serialize(s)
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Wire::deserialize(d)? {
        Wire::Number(f) => f,
        Wire::Named(NonFinite::NaN) => f64::NAN,
        Wire::Named(NonFinite::Infinity) => f64::INFINITY,
        Wire::Named(NonFinite::NegInfinity) => f64::NEG_INFINITY,
    })
}

/// The schema of the wire form: `anyOf` a number or one of the three named strings.
#[cfg(feature = "schemars")]
pub(crate) fn schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({
        "anyOf": [
            { "type": "number", "format": "double" },
            { "type": "string", "enum": ["NaN", "Infinity", "-Infinity"] }
        ]
    }))
    .unwrap_or(schemars::schema::Schema::Bool(true))
}

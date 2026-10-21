//! Runs the official TOON 4.4 encode fixtures (vendored under `tests/fixtures/toon-spec/`, see its README for source,
//! commit and license) against michi, for every case whose input falls inside the shapes michi can emit.
//!
//! michi is not a general JSON-to-TOON encoder, so each in-subset case is mapped onto the michi API that produces
//! that shape:
//! - `{ key: [] }` and `{ key: [uniform flat objects] }` and `{ key: [{}, …] }` → [`render_toon`] with `key` as the
//!   type name: the real document renderer, compared byte for byte.
//! - `{ key: [strings…] }` → [`render_hints`] (the inline primitive array michi emits as `help[N]: …`), with the
//!   fixture's key in place of `help`.
//! - root primitives, primitive arrays and flat objects of primitives → michi's cell encoder (a one-row table) and
//!   [`escape_key`], the exact encoders michi uses for cells, the `totalCount` line, type names and field names.
//!
//! Everything else (nested objects, keyed tabular, nested field groups, arrays of arrays, mixed lists, non-default
//! delimiters or indent sizes, root-only rules) is skipped with the reason recorded, and the test asserts a floor on
//! the number of cases actually executed so a mapping regression cannot silently skip everything.

use michi_core::toon::{escape_key, render_toon, ToonOptions, Value};
use michi_core::{render_hints, Hint};
use serde_json::Value as Json;

const FIXTURES: &[(&str, &str)] = &[
    ("arrays-nested", include_str!("fixtures/toon-spec/encode/arrays-nested.json")),
    ("arrays-objects", include_str!("fixtures/toon-spec/encode/arrays-objects.json")),
    ("arrays-primitive", include_str!("fixtures/toon-spec/encode/arrays-primitive.json")),
    ("arrays-tabular", include_str!("fixtures/toon-spec/encode/arrays-tabular.json")),
    ("delimiters", include_str!("fixtures/toon-spec/encode/delimiters.json")),
    ("objects-keyed", include_str!("fixtures/toon-spec/encode/objects-keyed.json")),
    ("objects", include_str!("fixtures/toon-spec/encode/objects.json")),
    ("primitives", include_str!("fixtures/toon-spec/encode/primitives.json")),
    ("whitespace", include_str!("fixtures/toon-spec/encode/whitespace.json")),
];

fn prim(v: &Json) -> Option<Value> {
    match v {
        Json::Null => Some(Value::Null),
        Json::Bool(b) => Some(Value::Bool(*b)),
        Json::Number(n) => {
            n.as_i64().map(Value::Int).or_else(|| n.as_u64().map(Value::from)).or_else(|| n.as_f64().map(Value::Float))
        }
        Json::String(s) => Some(Value::from(s.as_str())),
        Json::Array(_) | Json::Object(_) => None,
    }
}

/// michi's cell encoding of each value, joined by the comma delimiter, via a real one-row document render.
fn cells(values: Vec<Value>) -> String {
    let fields: Vec<String> = (0..values.len()).map(|i| format!("f{i}")).collect();
    let header = format!("t[1]{{{}}}:\n  ", fields.join(","));
    let out = render_toon(&ToonOptions::new("t", fields, vec![values]).max_cell_len(usize::MAX)).expect("renders");
    out.strip_prefix(&header).expect("one-row table").to_string()
}

fn encode(input: &Json) -> Result<String, &'static str> {
    match input {
        Json::String(s) if s.starts_with('\u{feff}') => {
            Err("root-primitive-only U+FEFF rule (§7.2); michi never emits a root primitive")
        }
        Json::Array(items) if items.is_empty() => Err("root empty array; michi documents are root objects"),
        Json::Array(items) => {
            let values = items.iter().map(prim).collect::<Option<Vec<_>>>().ok_or("root array of non-primitives")?;
            Ok(format!("[{}]: {}", values.len(), cells(values)))
        }
        Json::Object(map) if map.is_empty() => Err("empty root object; michi never emits an empty document"),
        Json::Object(map) if map.len() == 1 && map.values().all(Json::is_array) => {
            let Some((key, Json::Array(items))) = map.iter().next() else { unreachable!() };
            list_entry(key, items)
        }
        Json::Object(map) => {
            let mut lines = Vec::with_capacity(map.len());
            for (k, v) in map {
                let value = prim(v).ok_or("object with non-primitive values")?;
                lines.push(format!("{}: {}", escape_key(k), cells(vec![value])));
            }
            Ok(lines.join("\n"))
        }
        scalar => Ok(cells(vec![prim(scalar).ok_or("unreachable")?])),
    }
}

fn list_entry(key: &str, items: &[Json]) -> Result<String, &'static str> {
    if items.is_empty() {
        return Ok(render_toon(&ToonOptions::new(key, vec![], vec![])).expect("renders"));
    }
    if let Some(strings) = items.iter().map(Json::as_str).collect::<Option<Vec<_>>>() {
        let hints: Vec<Hint> = strings.into_iter().map(Hint::new).collect();
        let block = render_hints(&hints);
        let tail = block.strip_prefix("help").and_then(|b| b.strip_suffix('\n')).expect("help block");
        return Ok(format!("{}{tail}", escape_key(key)));
    }
    if let Some(values) = items.iter().map(prim).collect::<Option<Vec<_>>>() {
        return Ok(format!("{}[{}]: {}", escape_key(key), values.len(), cells(values)));
    }
    let objects = items.iter().map(Json::as_object).collect::<Option<Vec<_>>>().ok_or("mixed list (§9.4)")?;
    if objects.iter().all(|o| o.is_empty()) {
        let rows = vec![Vec::new(); objects.len()];
        return Ok(render_toon(&ToonOptions::new(key, vec![], rows)).expect("renders"));
    }
    let fields: Vec<String> = objects[0].keys().cloned().collect();
    let mut rows = Vec::with_capacity(objects.len());
    for obj in &objects {
        if obj.len() != fields.len() || !fields.iter().all(|f| obj.contains_key(f)) {
            return Err("non-uniform objects (§9.4 list form)");
        }
        let row =
            fields.iter().map(|f| prim(&obj[f])).collect::<Option<Vec<_>>>().ok_or("nested columns or list form")?;
        rows.push(row);
    }
    Ok(render_toon(&ToonOptions::new(key, fields, rows).max_cell_len(usize::MAX)).expect("renders"))
}

/// Cases the harness cannot reproduce, with the reason; each one's rule is covered natively in `tests/toon_spec.rs`.
const HARNESS_LIMITS: &[(&str, &str)] = &[(
    "primitives#38",
    "serde_json's default float parser is not correctly rounded (that needs its `float_roundtrip` feature), so the \
     input 123456789012345680000 parses one ULP off; covered by toon_spec::s2_large_integral_float_uses_closest_shortest_digits",
)];

#[test]
fn official_encode_fixtures_in_michis_subset_pass() {
    let mut ran = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (file, text) in FIXTURES {
        let suite: Json = serde_json::from_str(text).expect("fixture JSON parses");
        assert_eq!(suite["category"], "encode", "{file}");
        for (index, case) in suite["tests"].as_array().expect("tests array").iter().enumerate() {
            let id = format!("{file}#{index}");
            if let Some((_, reason)) = HARNESS_LIMITS.iter().find(|(known, _)| *known == id) {
                skipped.push(format!("{id}: {reason}"));
                continue;
            }
            let options = case.get("options").and_then(Json::as_object);
            let default_options = options
                .is_none_or(|o| o.iter().all(|(k, v)| (k == "delimiter" && v == ",") || (k == "indentSize" && v == 2)));
            if !default_options {
                skipped.push(format!("{id}: non-default encoder option"));
                continue;
            }
            let expected = case["expected"].as_str().expect("encode fixtures expect a string");
            match encode(&case["input"]) {
                Err(reason) => skipped.push(format!("{id}: {reason}")),
                Ok(actual) => {
                    ran += 1;
                    if actual != expected {
                        failures.push(format!("{id} ({}): expected {expected:?}, got {actual:?}", case["name"]));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} fixture failures:\n{}", failures.len(), failures.join("\n"));
    assert!(ran >= 87, "only {ran} in-subset fixtures ran; skipped:\n{}", skipped.join("\n"));
}

//! Render every corpus/<dataset>_<size>.json to renders/toon/<dataset>_<size>.toon with michi-core.
//! kind "list" -> michi_core::toon::list(type_name, rows); kind "kv" -> michi_core::kv::render_kv(items, None, &[]).
//! No totalCount / help blocks are appended: only the data itself is compared across formats.
use michi_core::kv::{render_kv, KvItem, KvValue};
use serde_json::Value;
use std::{env, fs, path::Path};

fn kv_value(v: &Value) -> KvValue {
    match v {
        Value::Null => KvValue::Missing,
        Value::Bool(b) => KvValue::Bool(*b),
        Value::Number(n) if n.is_i64() => KvValue::Int(n.as_i64().unwrap_or_default()),
        Value::Number(n) => KvValue::Float(n.as_f64().unwrap_or(f64::NAN), 6),
        Value::String(s) => KvValue::Text(s.clone()),
        other => KvValue::Text(other.to_string()),
    }
}

fn main() {
    let root = env::args().nth(1).unwrap_or_else(|| ".".into());
    let corpus = Path::new(&root).join("corpus");
    let out = Path::new(&root).join("renders/toon");
    fs::create_dir_all(&out).expect("create renders/toon");
    let mut entries: Vec<_> = fs::read_dir(&corpus).expect("read corpus").flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let doc: Value = serde_json::from_str(&fs::read_to_string(&p).expect("read")).expect("parse");
        let type_name = doc["type_name"].as_str().expect("type_name");
        let rows = doc["rows"].as_array().expect("rows");
        let text = match doc["kind"].as_str() {
            Some("kv") => {
                let items: Vec<KvItem> = rows
                    .iter()
                    .map(|r| KvItem::new(r["key"].as_str().expect("key"), kv_value(&r["value"])))
                    .collect();
                render_kv(&items, None, &[]).expect("render_kv")
            }
            _ => michi_core::toon::list(type_name, rows).expect("toon::list"),
        };
        assert!(!text.contains("more chars)"), "unexpected truncation in {}", p.display());
        let stem = p.file_stem().and_then(|s| s.to_str()).expect("stem");
        fs::write(out.join(format!("{stem}.toon")), &text).expect("write");
    }
}

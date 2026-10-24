//! Render the shapes michi can emit to renders/toon_michi/<shape>.toon with michi-core.
//!
//! michi emits flat tables and key-value blocks only. Corpus files with `"michi": "list"` go through the same table
//! `michi_core::toon::list(wrapper, rows)` builds, but via `render_toon` with the 200-char cell cap lifted (see
//! `list_lossless`); `"michi": "kv"` goes through `michi_core::kv::render_kv`. Shapes with
//! `"michi": null` (nested records, trees) are skipped: michi has no renderer for them, so the results use the
//! reference encoder for those cells. No totalCount or help blocks are added: only the data is compared.
use michi_core::kv::{render_kv, KvItem, KvValue};
use michi_core::toon::{render_toon, ToonOptions, Value as Cell};
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

fn cell(v: Option<&Value>) -> Cell {
    match v {
        None | Some(Value::Null) => Cell::Null,
        Some(Value::Bool(b)) => Cell::Bool(*b),
        Some(Value::Number(n)) if n.is_i64() => Cell::Int(n.as_i64().unwrap_or_default()),
        Some(Value::Number(n)) => Cell::Float(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => Cell::from(s.as_str()),
        Some(other) => Cell::from(other.to_string()),
    }
}

/// Same table `toon::list` builds (header = union of keys in first-seen order, missing key = null), but with the
/// 200-char cell cap lifted so long free-text fields stay lossless. The default cap is reported in the write-up.
fn list_lossless(type_name: &str, rows: &[Value]) -> String {
    let mut fields: Vec<String> = Vec::new();
    for r in rows {
        for k in r.as_object().expect("row object").keys() {
            if !fields.contains(k) {
                fields.push(k.clone());
            }
        }
    }
    let cells = rows.iter().map(|r| fields.iter().map(|f| cell(r.get(f))).collect()).collect();
    let opts = ToonOptions::new(type_name, fields, cells).max_cell_len(1_000_000);
    render_toon(&opts).expect("render_toon")
}

fn main() {
    let root = env::args().nth(1).unwrap_or_else(|| ".".into());
    let corpus = Path::new(&root).join("corpus");
    let out = Path::new(&root).join("renders/toon_michi");
    fs::create_dir_all(&out).expect("create renders/toon_michi");
    let mut entries: Vec<_> = fs::read_dir(&corpus).expect("read corpus").flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let doc: Value = serde_json::from_str(&fs::read_to_string(&p).expect("read")).expect("parse");
        let text = match doc["michi"].as_str() {
            Some("list") => {
                let rows = doc["data"].as_array().expect("data array");
                list_lossless(doc["wrapper"].as_str().expect("wrapper"), rows)
            }
            Some("kv") => {
                let items: Vec<KvItem> = doc["data"]
                    .as_object()
                    .expect("data object")
                    .iter()
                    .map(|(k, v)| KvItem::new(k.as_str(), kv_value(v)))
                    .collect();
                render_kv(&items, None, &[]).expect("render_kv")
            }
            _ => continue,
        };
        assert!(!text.contains("more chars)"), "unexpected truncation in {}", p.display());
        let stem = p.file_stem().and_then(|s| s.to_str()).expect("stem");
        fs::write(out.join(format!("{stem}.toon")), &text).expect("write");
    }
}

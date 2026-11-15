//! One storage model: `items()` and `kv_items()` append sections, so mixing them never drops a body.

use michi_core::kv::{KvItem, KvValue};
use michi_core::toon::Value;
use michi_core::{AgentResponse, OutputFormat};

fn kv(key: &str, n: i64) -> KvItem {
    KvItem::new(key, KvValue::Int(n))
}

fn rows(n: i64) -> Vec<Vec<Value>> {
    vec![vec![Value::Int(n)]]
}

#[test]
fn items_and_kv_items_together_render_both_in_call_order() {
    let r = AgentResponse::new("t").items(rows(1), &["a"]).kv_items(vec![kv("k", 2)]);
    assert_eq!(r.render(OutputFormat::Text), "t[1]{a}:\n  1\nk: 2");
    let flipped = AgentResponse::new("t").kv_items(vec![kv("k", 2)]).items(rows(1), &["a"]);
    assert_eq!(flipped.render(OutputFormat::Text), "k: 2\nt[1]{a}:\n  1", "call order is output order");
}

#[test]
fn items_called_twice_keeps_both_lists() {
    let r = AgentResponse::new("t").items(rows(1), &["a"]).items(rows(2), &["b"]);
    assert_eq!(r.render(OutputFormat::Text), "t[1]{a}:\n  1\nt[1]{b}:\n  2");
}

#[test]
fn the_sugar_is_a_section_named_for_the_response() {
    let r = AgentResponse::new("issues").items(rows(1), &["a"]);
    let json = r.render(OutputFormat::Json);
    assert!(json.contains("\"sections\":[{\"name\":\"issues\",\"kind\":\"toon\""), "{json}");
    let kv_json = AgentResponse::new("receipt").kv_items(vec![kv("k", 1)]).render(OutputFormat::Json);
    assert!(kv_json.contains("\"sections\":[{\"name\":\"receipt\",\"kind\":\"kv\""), "{kv_json}");
}

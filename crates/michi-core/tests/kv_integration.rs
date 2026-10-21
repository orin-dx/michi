use michi_core::kv::{render_kv, KvItem, KvValue};

#[test]
fn single_item_renders() {
    let items = vec![KvItem { key: "id".into(), value: KvValue::Text("abc-123".into()) }];
    assert_eq!(render_kv(&items, None, &[]), "id: abc-123\n");
}

#[test]
fn multiple_items_render_in_order() {
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Text("abc-123".into()) },
        KvItem { key: "title".into(), value: KvValue::Text("Fix login".into()) },
        KvItem { key: "state".into(), value: KvValue::Text("open".into()) },
        KvItem { key: "count".into(), value: KvValue::Int(3) },
    ];
    let out = render_kv(&items, None, &[]);
    assert_eq!(out, "id:    abc-123\ntitle: Fix login\nstate: open\ncount: 3\n");
}

#[test]
fn missing_value_renders_as_em_dash() {
    let items = vec![KvItem { key: "assignee".into(), value: KvValue::Missing }];
    assert_eq!(render_kv(&items, None, &[]), "assignee: —\n");
}

#[test]
fn bool_values_render_as_words() {
    let items = vec![
        KvItem { key: "active".into(), value: KvValue::Bool(true) },
        KvItem { key: "archived".into(), value: KvValue::Bool(false) },
    ];
    assert_eq!(render_kv(&items, None, &[]), "active:   true\narchived: false\n");
}

#[test]
fn empty_slice_returns_empty_string() {
    assert_eq!(render_kv(&[], None, &[]), "");
}

use michi_core::kv::{render_kv, KvItem, KvValue};

fn kv(items: &[KvItem]) -> String {
    render_kv(items, None, &[]).expect("valid items")
}

#[test]
fn single_item_renders() {
    let items = vec![KvItem::new("id", KvValue::Text("abc-123".into()))];
    assert_eq!(kv(&items), "id: abc-123");
}

#[test]
fn multiple_items_render_in_order_with_one_space_after_each_colon() {
    let items = vec![
        KvItem::new("id", KvValue::Text("abc-123".into())),
        KvItem::new("title", KvValue::Text("Fix login".into())),
        KvItem::new("state", KvValue::Text("open".into())),
        KvItem::new("count", KvValue::Int(3)),
    ];
    assert_eq!(kv(&items), "id: abc-123\ntitle: Fix login\nstate: open\ncount: 3");
}

#[test]
fn missing_value_renders_as_null() {
    let items = vec![KvItem::new("assignee", KvValue::Missing)];
    assert_eq!(kv(&items), "assignee: null");
}

#[test]
fn bool_values_render_as_words() {
    let items = vec![KvItem::new("active", KvValue::Bool(true)), KvItem::new("archived", KvValue::Bool(false))];
    assert_eq!(kv(&items), "active: true\narchived: false");
}

#[test]
fn empty_slice_returns_empty_string() {
    assert_eq!(kv(&[]), "");
}

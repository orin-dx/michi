fn main() {
    let item = michi_core::StatusItem::new("k", michi_core::KvValue::Int(1));
    let _ = serde_json::to_string(&item);
}

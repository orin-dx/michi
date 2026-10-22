use divan::Bencher;
use michi_core::kv::{render_kv, KvItem, KvValue};

fn main() {
    divan::main();
}

#[divan::bench(args = [1, 5, 20])]
fn render_n_items(b: Bencher, n: usize) {
    let items: Vec<KvItem> =
        (0..n).map(|i| KvItem::new(format!("field_{i}"), KvValue::Text(format!("value_{i}")))).collect();
    b.bench(|| render_kv(&items, None, &[]));
}

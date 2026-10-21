use divan::Bencher;
use michi_core::toon::{render_toon, ToonOptions, Value};

fn main() {
    divan::main();
}

fn make_opts(n: usize) -> ToonOptions {
    ToonOptions::new(
        "issue",
        vec!["number".into(), "title".into(), "state".into()],
        (0..n)
            .map(|i| vec![Value::Int(i as i64), Value::from(format!("Issue title number {i}")), Value::from("open")])
            .collect(),
    )
    .total_count(Some(1000))
    .hints(vec!["Call get_issue with number=<number>".to_string()])
}

#[divan::bench(args = [1, 10, 100, 1000])]
fn render_n_rows(b: Bencher, n: usize) {
    let opts = make_opts(n);
    b.bench(|| render_toon(&opts).unwrap());
}

#[divan::bench]
fn render_with_comma_escaping(b: Bencher) {
    let opts = ToonOptions::new(
        "item",
        vec!["name".into()],
        (0..100).map(|i| vec![Value::from(format!("Item {i}, with comma"))]).collect(),
    );
    b.bench(|| render_toon(&opts).unwrap());
}

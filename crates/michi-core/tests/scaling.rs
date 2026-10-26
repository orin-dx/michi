//! Complexity-class checks: work must grow about linearly with input size.
//!
//! These catch an accidental O(n²) (a `contains` in a loop, a rescan per row), which is the regression that matters for a
//! library whose inputs come from tools and can be large. They do not track constant factors: wall time on a shared
//! runner moves by tens of percent, so a threshold on absolute time or on a percentage change would be either flaky
//! or too loose to mean anything. A tenfold input makes linear code take about 10x as long and quadratic code about
//! 100x, so a 30x bound separates them with a threefold margin for noise.

use std::hint::black_box;
use std::time::{Duration, Instant};

use michi_core::kv::{render_kv, KvItem, KvValue};
use michi_core::toon::{render_toon, ToonOptions, Value};
use michi_core::truncate::truncate;
use michi_core::truncation::truncate_by_size;

/// Input growth between the two measured sizes.
const GROWTH: u32 = 10;
/// Linear work takes ~10x as long for 10x the input; quadratic ~100x. Anything past this is not linear.
const MAX_RATIO: f64 = 30.0;
/// Each size is timed this many times and the fastest run counts, which discards scheduler and cache noise.
const RUNS: usize = 5;

fn best_of(mut f: impl FnMut()) -> Duration {
    (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// Times `work(n)` at `n` and `n * GROWTH`, building the input outside the timed region.
fn assert_linear<I>(label: &str, n: usize, build: impl Fn(usize) -> I, work: impl Fn(&I)) {
    let (small, large) = (build(n), build(n * GROWTH as usize));
    let t_small = best_of(|| work(black_box(&small)));
    let t_large = best_of(|| work(black_box(&large)));
    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(1e-9);
    assert!(
        ratio < MAX_RATIO,
        "{label}: {GROWTH}x the input took {ratio:.1}x as long ({t_small:?} -> {t_large:?}); linear is ~{GROWTH}x"
    );
}

#[test]
fn render_toon_is_linear_in_rows() {
    assert_linear(
        "render_toon rows",
        1_000,
        |n| {
            ToonOptions::new(
                "issue",
                vec!["number".into(), "title".into(), "state".into()],
                (0..n)
                    .map(|i| vec![Value::Int(i as i64), Value::from(format!("Issue, number {i}")), Value::from("open")])
                    .collect(),
            )
        },
        |opts| {
            black_box(render_toon(opts).expect("valid options"));
        },
    );
}

#[test]
fn render_toon_is_linear_in_cell_length() {
    assert_linear(
        "render_toon long cell",
        10_000,
        |n| ToonOptions::new("t", vec!["body".into()], vec![vec![Value::from("a, \"b\"\n".repeat(n))]]),
        |opts| {
            black_box(render_toon(opts).expect("valid options"));
        },
    );
}

#[test]
fn render_kv_is_linear_in_items() {
    assert_linear(
        "render_kv items",
        1_000,
        |n| (0..n).map(|i| KvItem::new(format!("key{i}"), KvValue::Text(format!("value {i}")))).collect::<Vec<_>>(),
        |items| {
            black_box(render_kv(items, None, &[]).expect("valid items"));
        },
    );
}

#[test]
fn truncate_is_linear_in_string_length() {
    assert_linear(
        "truncate long text",
        20_000,
        |n| "é".repeat(n),
        |s| {
            black_box(truncate(s, s.chars().count() / 2));
        },
    );
}

#[test]
fn truncate_by_size_is_linear_in_items() {
    assert_linear(
        "truncate_by_size items",
        20_000,
        |n| (0..n).collect::<Vec<usize>>(),
        |items| {
            black_box(truncate_by_size(items.clone(), items.len() * 2, |i| i % 7 + 1));
        },
    );
}

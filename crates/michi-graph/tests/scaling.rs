//! Complexity-class checks: work must grow about linearly (V + E) with graph size.
//!
//! These catch an accidental O(n²), such as a node list searched per edge. They do not track constant factors: wall time
//! on a shared runner moves by tens of percent, so a threshold on absolute time would be flaky or too loose to mean
//! anything. A tenfold graph makes linear code take about 10x as long and quadratic code about 100x, so a 30x bound
//! separates them with a threefold margin for noise. The same shape of test lives in `michi-core/tests/scaling.rs`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use michi_graph::{Dag, Order};

const GROWTH: usize = 10;
const MAX_RATIO: f64 = 30.0;
/// Each size is timed this many times and the fastest run counts, which discards scheduler and cache noise.
const RUNS: usize = 5;

type Nodes = Vec<(String, Vec<String>)>;

fn chain(n: usize) -> Nodes {
    (0..n).map(|i| (format!("n{i}"), if i == 0 { vec![] } else { vec![format!("n{}", i - 1)] })).collect()
}

/// `layers` layers of 20 nodes; each node depends on three nodes of the previous layer.
fn layered(layers: usize) -> Nodes {
    (0..layers)
        .flat_map(|l| {
            (0..20).map(move |w| {
                let deps =
                    if l == 0 { vec![] } else { (0..3).map(|p| format!("l{}w{}", l - 1, (w + p) % 20)).collect() };
                (format!("l{l}w{w}"), deps)
            })
        })
        .collect()
}

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

fn assert_linear(label: &str, n: usize, build: impl Fn(usize) -> Nodes, work: impl Fn(&Nodes)) {
    let (small, large) = (build(n), build(n * GROWTH));
    let t_small = best_of(|| work(black_box(&small)));
    let t_large = best_of(|| work(black_box(&large)));
    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(1e-9);
    assert!(
        ratio < MAX_RATIO,
        "{label}: {GROWTH}x the input took {ratio:.1}x as long ({t_small:?} -> {t_large:?}); linear is ~{GROWTH}x"
    );
}

#[test]
fn validating_a_chain_is_linear_in_declaration_order() {
    assert_linear("chain, declaration order", 1_000, chain, |nodes| {
        black_box(Dag::from_nodes(nodes.clone(), Order::Declaration).expect("a chain is acyclic"));
    });
}

#[test]
fn validating_a_chain_is_linear_in_key_order() {
    assert_linear("chain, key order", 1_000, chain, |nodes| {
        black_box(Dag::from_nodes(nodes.clone(), Order::Key).expect("a chain is acyclic"));
    });
}

#[test]
fn ready_and_layers_are_linear() {
    assert_linear(
        "ready + layers",
        200,
        layered,
        // Validation is included: it is linear too, and rebuilding keeps the closure `Fn`.
        |nodes| {
            let dag = Dag::from_nodes(nodes.clone(), Order::Declaration).expect("layered graph is acyclic");
            black_box((dag.ready(|_| false, |_| true).count(), dag.layers().len()));
        },
    );
}

#[test]
fn reporting_a_cycle_is_linear() {
    assert_linear(
        "cycle report",
        1_000,
        |n| (0..n).map(|i| (format!("n{i}"), vec![format!("n{}", (i + 1) % n)])).collect(),
        |nodes| {
            black_box(Dag::from_nodes(nodes.clone(), Order::Declaration).is_err());
        },
    );
}

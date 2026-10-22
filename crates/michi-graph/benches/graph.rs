//! Build and validate costs at sizes beyond real dependency graphs.

use michi_graph::{Dag, GraphBuilder, Order, UnknownTargets};

fn main() {
    divan::main();
}

fn chain(n: usize) -> Vec<(String, Vec<String>)> {
    (0..n).map(|i| (format!("n{i}"), if i == 0 { vec![] } else { vec![format!("n{}", i - 1)] })).collect()
}

/// `width` nodes per layer, each depending on every node of the previous layer.
fn layered(layers: usize, width: usize) -> Vec<(String, Vec<String>)> {
    (0..layers)
        .flat_map(|l| {
            (0..width).map(move |w| {
                let deps = if l == 0 { vec![] } else { (0..width).map(|p| format!("l{}w{p}", l - 1)).collect() };
                (format!("l{l}w{w}"), deps)
            })
        })
        .collect()
}

#[divan::bench(args = [1_000, 100_000])]
fn validate_chain(bencher: divan::Bencher, n: usize) {
    bencher.with_inputs(|| chain(n)).bench_values(|nodes| Dag::from_nodes(nodes, Order::Declaration));
}

#[divan::bench]
fn validate_layered_10k_declaration(bencher: divan::Bencher) {
    bencher.with_inputs(|| layered(100, 100)).bench_values(|nodes| Dag::from_nodes(nodes, Order::Declaration));
}

#[divan::bench]
fn validate_layered_10k_key(bencher: divan::Bencher) {
    bencher.with_inputs(|| layered(100, 100)).bench_values(|nodes| Dag::from_nodes(nodes, Order::Key));
}

#[divan::bench]
fn report_100k_cycle(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| {
            let mut b = GraphBuilder::<u32>::with_capacity(100_000);
            for i in 0..100_000u32 {
                let node = b.node(i);
                b.depends_on(node, (i + 1) % 100_000, ());
            }
            b.build(UnknownTargets::Error)
        })
        .bench_refs(|g| g.validate(Order::Declaration).is_err());
}

#[divan::bench]
fn ready_and_layers_10k(bencher: divan::Bencher) {
    let dag = Dag::from_nodes(layered(100, 100), Order::Declaration).unwrap();
    bencher.bench(|| (dag.ready(|_| false, |_| true).count(), dag.layers().len()));
}

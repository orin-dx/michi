//! Compile-time check that the public types stay `Send + Sync`, so a future non-thread-safe field fails the build.
use michi_graph::{
    Cycle, Dag, Dangling, Graph, GraphBuilder, Layers, NodeIx, Order, UnknownTargets, Violation, Violations,
};

fn is_send_sync<T: Send + Sync>() {}

#[test]
fn public_types_are_send_and_sync() {
    is_send_sync::<GraphBuilder<String, u8>>();
    is_send_sync::<Graph<String, u8>>();
    is_send_sync::<Dag<String, u8>>();
    is_send_sync::<Dangling<String, u8>>();
    is_send_sync::<Cycle<String>>();
    is_send_sync::<Violation<String>>();
    is_send_sync::<Violations<String>>();
    is_send_sync::<Layers>();
    is_send_sync::<NodeIx>();
    is_send_sync::<Order>();
    is_send_sync::<UnknownTargets>();
}

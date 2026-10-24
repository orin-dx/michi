# michi-graph

Deterministic dependency graphs for dev tools: steps in a pipeline, operations in a release, packages in a workspace, tasks in a plan. MIT-licensed, no dependencies, `no_std` + `alloc`.

- **Every problem at once.** Validation reports duplicate nodes, unknown dependencies, self-dependencies and cycles in one list, in a fixed order. Each cycle names all of its members and one concrete loop.
- **Deterministic.** Ties break by declaration order or by key, your choice, and every output follows it: the topological order (the smallest valid one), layers, ready sets, closures and cycle reports.
- **No recursion.** A 100,000-node chain or cycle validates without touching the stack limit.
- **Edge kinds.** Give edges a kind (runtime, dev, …) and choose per validation which kinds count, so one graph yields several dags. A `Dag<K, E>` keeps each edge's kind (`prerequisite_edges`), and `ready_by_edge(satisfied, eligible)` takes the same arguments as `ready(satisfied, eligible)` but judges each edge by its kind (`satisfied(prerequisite, kind)`), for graphs where some kinds are satisfied earlier than others.

```rust
use michi_graph::{Dag, Order};

let dag = Dag::from_nodes(
    [("build", vec![]), ("test", vec!["build"]), ("lint", vec![]), ("ship", vec!["test", "lint"])],
    Order::Declaration,
)
.unwrap();
assert_eq!(dag.keys_in_order().copied().collect::<Vec<_>>(), ["build", "test", "lint", "ship"]);

let err = Dag::from_nodes([("a", vec!["b"]), ("b", vec!["a"])], Order::Key).unwrap_err();
assert_eq!(err.to_string(), "dependency cycle: a depends on b, which depends on a");
```

With edge kinds:

```rust
use michi_graph::{GraphBuilder, Order, UnknownTargets};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind { Runtime, Dev }

let mut b = GraphBuilder::<&str, Kind>::new();
let app = b.node("app");
let lib = b.node("lib");
b.depends_on(app, "lib", Kind::Runtime);
b.depends_on(lib, "app", Kind::Dev); // a dev-only cycle
let graph = b.build(UnknownTargets::Error);

let runtime = graph.validate_filtered(|e| *e.kind == Kind::Runtime, Order::Key).unwrap();
assert_eq!(runtime.keys_in_order().copied().collect::<Vec<_>>(), ["lib", "app"]);
assert!(graph.validate(Order::Key).is_err());
```

Part of [michi](https://github.com/orin-dx/michi); `michi-pipeline` builds its step plans on it. API reference: [docs.rs/michi-graph](https://docs.rs/michi-graph). Design: [`docs/spec/09-graph.md`](https://github.com/orin-dx/michi/blob/main/docs/spec/09-graph.md).

## License

MIT. See [`LICENSE`](LICENSE).

---
michi-graph: minor
---

michi-graph: first release. An MIT, dependency-free (`no_std` + `alloc`) crate for deterministic dependency graphs: build with `GraphBuilder`, validate into a `Dag` that reports every problem at once (duplicate nodes, unknown dependencies, self-dependencies, and cycles with members and one shortest loop), then query the topological order, layers, ready sets, ancestors and descendants. Ties break by declaration order or by key, and every output follows the choice. Edge kinds and per-validation edge filters let one graph produce several dags. See `docs/spec/09-graph.md`.

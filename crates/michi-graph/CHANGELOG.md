# michi-graph

## 0.7.1

- Docs and doc comments match the 0.7 code: the READMEs, crate and module docs, and the Node package's API tables list every export (`parseRetryAfter`, `nextRetryDelay`, `isRetryableStatus`, `AgentResponse.renderFor`, `.hasHumanContent`), and several comments that described removed behavior are corrected. No behavior changes.

## 0.7.0

- michi-graph: a `Dag` keeps its edges' kinds. `Dag<K, E = ()>` gains `prerequisite_edges(ix)` (every kept edge to a prerequisite with its kind) and `ready_by_edge(eligible, satisfied)`, which judges each edge with its kind, for graphs where some kinds are satisfied earlier than others (a sibling publish needs a publish issued, a tag needs it confirmed). `Graph::validate` and `validate_filtered` now return `Dag<K, E>` and need `E: Clone`; code naming `Dag<K>` for graphs without kinds is unchanged.

## 0.6.0

- Released together with the `michi` fixed group.

## 0.5.0

- michi-graph: first release. An MIT, dependency-free (`no_std` + `alloc`) crate for deterministic dependency graphs: build with `GraphBuilder`, validate into a `Dag` that reports every problem at once (duplicate nodes, unknown dependencies, self-dependencies, and cycles with members and one shortest loop), then query the topological order, layers, ready sets, ancestors and descendants. Ties break by declaration order or by key, and every output follows the choice. Edge kinds and per-validation edge filters let one graph produce several dags. See `docs/spec/09-graph.md`.
- Joined the `michi` group at this version.


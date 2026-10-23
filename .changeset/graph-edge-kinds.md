---
michi-graph: minor
---

michi-graph: a `Dag` keeps its edges' kinds. `Dag<K, E = ()>` gains `prerequisite_edges(ix)` (every kept edge to a prerequisite with its kind) and `ready_by_edge(eligible, satisfied)`, which judges each edge with its kind, for graphs where some kinds are satisfied earlier than others (a sibling publish needs a publish issued, a tag needs it confirmed). `Graph::validate` and `validate_filtered` now return `Dag<K, E>` and need `E: Clone`; code naming `Dag<K>` for graphs without kinds is unchanged.

---
michi-graph: minor
---

Breaking: `Dag::ready_by_edge` now takes `satisfied` first and `eligible` second, the same order as `Dag::ready`. In 0.7.0 it took `ready_by_edge(eligible, satisfied)` while `ready` took `ready(satisfied, eligible)`, an easy swap to make. Migration: swap the two arguments, `dag.ready_by_edge(eligible, satisfied)` becomes `dag.ready_by_edge(satisfied, eligible)`. A call left in the old order fails to compile because the closures' shapes differ (`satisfied` takes the edge kind too). `ready`'s signature is unchanged; its docs now name each closure's role.

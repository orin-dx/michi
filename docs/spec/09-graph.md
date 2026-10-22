# Dependency graphs (`michi-graph`)

`michi-graph` validates and orders dependency graphs: steps in a pipeline, operations in a release, packages in a workspace, tasks in an agent's plan. It is MIT-licensed, has no dependencies (it is `no_std` with `alloc`, and needs pointer-sized atomics, which every mainstream target has; CI builds it for `thumbv7em-none-eabihf`), and depends on nothing else in michi, so permissive crates such as `callisto-model` can use it. `michi-pipeline` builds its `Plan` on it.

It is a small, deterministic graph model, not a general graph library. There are no node or edge payloads, no mutation after a graph is built, no path finding and no execution policy. The reasons for each choice are in [06-decisions.md](06-decisions.md), "michi-graph: an MIT leaf crate, versioned on its own".

## Vocabulary

- A **node** has a key `K: Ord + Clone` chosen by the caller (`StepId`, `PackageId`, `String`).
- An **edge** says that a **dependent** depends on a **prerequisite**: the prerequisite comes first. The API never says "from/to" or "in/out", because those words mean opposite things in different tools.
- Every edge has a **kind** `E: Ord`, which defaults to `()`. Callisto uses kinds such as runtime, dev and peer; michi and wisp don't use kinds.
- A `NodeIx` is a node's position among the graph's nodes in declaration order: the first node declared is 0. Duplicate declarations are not nodes and don't count, so in a graph that validates it is exactly the declaration position. It has no public constructor. Every method that takes one returns an empty slice or `None` for an index the graph does not have, so an index from another graph never panics.

## Building

```rust
let mut b = GraphBuilder::<StepId>::with_capacity(steps.len());
for step in &steps {
    let node = b.node(step.id().clone());
    for prerequisite in step.prerequisites() {
        b.depends_on(node, prerequisite, ());
    }
}
let graph = b.build(UnknownTargets::Error);
```

- `node(key)` declares a node and returns a `Decl` token. Edges hang off the token, not the key. A token only works on its own builder; a clone of the builder accepts tokens made before the clone, not after, so a token can never name another builder's node at the same position. A later node reusing a key is recorded as a duplicate, and its edges are dropped with it, so they can never land on the first node and cause false errors.
- `depends_on(decl, prerequisite, kind)` takes the prerequisite key, which may be declared before or after. An edge repeated with the same kind collapses into one.
- `build(policy)` never fails. It sets aside what it cannot use and records why, so every problem can be reported later in one list:
  - a duplicate node;
  - an edge to a key no node has (a **dangling** edge). It is always kept in `Graph::dangling()`. The edge filter sees it too (with no `prerequisite_ix`), so an unknown dev dependency is not a problem when dev edges are filtered out. For a kept dangling edge, the `UnknownTargets` policy decides: `Error` reports it as `UnknownDependency`, and `Ignore` doesn't (Callisto ignores dependencies on packages outside the workspace). One missing key under several kept kinds is reported once;
  - more than `u32::MAX - 1` nodes.
- A node that depends on itself is kept as an ordinary edge, and the edge filter decides whether it counts (a dev-only self-dependency is legal in Cargo).
- `Dag::from_nodes(iter, order)` builds and validates in one call for the common case: no kinds and unknown targets are errors.

## Graph and Dag

A `Graph<K, E>` may contain cycles. A `Dag<K>` is proven acyclic and carries no kinds, because validation has already chosen which edges count.

- `graph.validate(order)` uses every edge; `graph.validate_filtered(keep, order)` keeps only the edges for which `keep(EdgeRef)` returns true. `EdgeRef` has named fields: `dependent`, `prerequisite`, `kind` and both `NodeIx`s. Keys are shared between the graph and its dags, so validating does not copy them.
- One graph can produce several dags. Callisto validates its package graph once without dev edges for cascade order, and once with them for publish order.
- Validation returns the `Dag` or a `Violations` list with every problem at once. Cycles made only of filtered-out edges are not problems: you validate the graph you will run.
- When several kinds connect the same pair and more than one passes the filter, the dag holds one edge. `prerequisites` and `dependents` never repeat a node.
- `graph.cycles(keep, order)` reports the cycles of a graph that is allowed to have them (lumen's agent trajectories).

## Order

The caller chooses how ties are broken, once, at validation:

- `Order::Declaration`: the earliest-declared node first. Steps run in the order they were written. michi uses this.
- `Order::Key`: the smallest key first. Shuffling the declarations changes nothing. Callisto uses this, because it seals its release plan with a digest.

Everything the crate outputs follows that one order: the topological order, each layer, `ready`, `ancestors`, `descendants`, cycle members and the order of the violations' cycles. The topological order is the lexicographically smallest valid order under that rule, and that is part of the contract, not an implementation detail. The order never comes from hash maps or traversal accidents.

## Queries on a Dag

| Method | Returns |
| --- | --- |
| `order()` | `&[NodeIx]` in topological order, computed at validation |
| `keys_in_order()` | the keys in that order |
| `prerequisites(ix)`, `dependents(ix)` | `&[NodeIx]` neighbours, in the chosen order |
| `layers()` | layer 0 has no prerequisites; a node's layer is one more than its deepest prerequisite's. Nodes in one layer don't depend on each other, so they can run together |
| `ready(satisfied, eligible)` | nodes for which `eligible(key)` holds and `satisfied(p)` holds for every prerequisite, in order |
| `ancestors(roots)`, `descendants(roots)` | every node upstream or downstream of `roots`, not including them |
| `key(ix)`, `index_of(&key)`, `node_at(position)`, `len()` | lookups; `node_at` maps a declaration position to its node, for callers that keep per-node data in declaration order |
| `canonical()` | each node's key with its prerequisites' keys, nodes and prerequisites sorted by key, for callers that hash the structure with their own hasher. Isolated nodes are included |

`ready` takes two predicates because callers track more than "done". A node behind a failure must not become ready, and a running or failed node is not eligible to start. When `satisfied` holds only for nodes that succeeded, checking direct prerequisites is enough: a node whose prerequisite failed can never become ready.

`ready` is one pass over the graph, O(V + E). There is no incremental ready tracker: michi's scheduler keeps its own counters because what unblocks a step depends on the outcome, and Callisto's release plans are small. A tracker can be added later without a breaking change if a consumer measures a need.

## Violations

`Violation<K>` is `#[non_exhaustive]` and holds keys, cloned only on the error path:

- `DuplicateNode { key }`
- `UnknownDependency { dependent, prerequisite }`
- `SelfDependency { key }`: reported once per node, never as a cycle.
- `Cycle(Cycle<K>)`: `members()` lists every node of one strongly connected component, in the chosen order. `path()` is one shortest closed path through the first member, so a message can show a real loop.
- `TooManyNodes`

`Violations<K>` lists them in a fixed order: duplicates, then `TooManyNodes`, then edge problems, then cycles. Under `Order::Declaration`, duplicates are in declaration order and edge problems go by the dependent's declaration position, then the edge's. Under `Order::Key`, duplicates are sorted by key and edge problems go by the dependent's key, then the missing key (a self-dependency sorts as the node's own key), so shuffling the declarations changes nothing. Cycles follow the chosen order either way. The same input always gives the same list.

`Display` exists when `K: Display`, and `display_with(|key, f| ...)` covers keys without it. A cycle reads as `a depends on b, which depends on c, which depends on a`. The printed path stops after 8 steps with `… and N more`; the struct keeps the whole path. `Violations<K>` implements `core::error::Error` when `K: Debug + Display`.

## Implementation rules

- No recursion: Kahn's algorithm for the order and an iterative Kosaraju for strongly connected components. A 100,000-node chain and a 100,000-node cycle are tested.
- Kahn's leftovers are always an error: nodes on or behind a cycle never disappear silently. Strongly connected components are computed only on those leftovers, so a valid graph doesn't pay for them.
- Adjacency is `u32` indices in sorted vectors; keys live once in an `Arc<[K]>`, and a key-sorted index permutation answers `index_of` by binary search.
- No `unwrap`, `expect`, unchecked indexing or `unsafe` (`#![forbid(unsafe_code)]`).

## How michi-pipeline uses it

- `Plan::new` declares one node per step, builds with `UnknownTargets::Error` and validates with `Order::Declaration`. Violations map one to one onto `PlanViolation`. Cycles stay a single `PlanViolation::Cycle` listing every step on any cycle in declaration order, and `TooManyNodes` becomes `PlanViolation::TooManySteps`. One change: a step stuck between two cycles (depending on one, depended on by another) lies on no cycle and is no longer listed.
- The scheduler reads `order()`, `prerequisites` and `dependents`. Node indices are declaration positions, so they match `Plan::steps()` and `RunRecord` without translation. Its ready counters, failure skipping and resume seeding stay in michi-pipeline: they are execution policy.
- `PlanFingerprint` stays michi's own 64-bit FNV-1a with its current framing, computed over `canonical()`. A record saved before the change still resumes.
- No michi-graph type appears in michi-pipeline's public API. michi-pipeline depends on it through the same exact `=` pin as michi-core, since they release together.

## Licensing and release

- `license = "MIT"` in the crate's own manifest and a crate `LICENSE`; the rest of the workspace stays FSL-1.1-MIT.
- CI walks the normal and build dependencies of `michi-graph` with `cargo metadata` and fails on any license outside the permissive allowlist, so no FSL crate can enter its tree.
- It releases with the rest of michi: one version for every michi package, in the Callisto fixed group `michi`, checked by CI's version-sync. Its first release is 0.5.0.

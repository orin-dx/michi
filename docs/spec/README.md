# michi spec

The technical reference for `michi-core`, michi's sync primitives crate — what it does, how it's shaped, and why.

## Reading order

| Doc | Covers |
| --- | --- |
| [01-overview-and-setup.md](01-overview-and-setup.md) | What michi is, why it exists, what's deliberately out of scope, who consumes it, `Cargo.toml`, crate layout |
| [02-toon-format.md](02-toon-format.md) | The TOON grammar — the agent-facing list format |
| [03-rust-api.md](03-rust-api.md) | Every module: `toon`, `kv`, `hints`, `truncate`, `empty`, `error`, `idempotency`, `resilience`, `status`, `recovery`, `response` |
| [04-mcp-and-napi.md](04-mcp-and-napi.md) | The `mcp` module, the NAPI/npm boundary, and the builder-across-FFI problem |
| [05-scope-and-quality.md](05-scope-and-quality.md) | What's formalized vs. what stays in your app, feature flags, versioning, performance, testing |
| [06-decisions.md](06-decisions.md) | Open questions and the reasoning behind everything that isn't obvious from the code |
| [07-wisp-and-monokl-envelope.md](07-wisp-and-monokl-envelope.md) | The multi-section envelope Wisp briefings and monokl batch results need: `Section`, michi's own `BudgetSummary` and `ProvenanceBlock` slots, index-bearing per-op outcomes, `structuredContent` and `outputSchema` |
| [08-pipeline-execution.md](08-pipeline-execution.md) | `michi-pipeline`: step plans, the replay rule, the status model, the run record, scheduling, cancellation, keyed breakers, the retry budget, the observer and resume |

New to the crate? Read 01 → 02 → 03 in order — that's the whole mental model. 04–08 are reference, read as needed.

## Known gaps

**TOON vs. Markdown-KV retrieval accuracy hasn't been measured.**

- The token-count win is real.
- Whether it holds up on retrieval accuracy across model sizes is untested.
- See [06-decisions.md](06-decisions.md).

**No real consumer depends on michi yet.** crates.io publish is gated on a count, not a roster: at least one consumer outside this repository integrating against a tagged commit, whichever lands first (see [05-scope-and-quality.md](05-scope-and-quality.md) for current status). Consumers are not enumerated in advance and the gate does not name them.

- What an integration needs from michi is specified in [07-wisp-and-monokl-envelope.md](07-wisp-and-monokl-envelope.md): repeatable sections, a `BudgetSummary` slot, a `ProvenanceBlock` slot, index-bearing per-op outcomes, and the extended `structuredContent`/`outputSchema`. None of it is built yet. All of it is michi's own types — michi takes no dependency on any consumer to hold them.
- The gate runs one way. **michi publishes first.** No consumer's own publish proceeds until michi has a **versioned release** it can depend on, for any consumer using a local path dependency during coordinated development; the integration that earns michi's publish is exercised against that path dependency before the release, not after it. Nothing in michi's manifest waits on a consumer's crate. A tagged git dependency is the documented interim step.

**`cli` isn't a Cargo feature of this crate at all.** Reserved name for future terminal-aware rendering, in a separate crate, not implemented.

**Plan 2 (the async execution layer) doesn't exist as code in this crate — but `pipeline`'s half has landed as its own crate.**

- `pipeline` has a real, tested data type and `render()` in `michi-core` — always compiled.
- The actual executor exists too, just not in this crate: `crates/michi-pipeline` ships `Plan`, `execute`/`resume` returning a `RunRecord`, a keyed `BreakerRegistry` and `CancellationToken`. It depends on `michi-core` (for `StepStatus`, the `Pipeline` view and `resilience` retry config and budget) and isn't re-exported through it.
- `fuzzy`, `cache`, and `cli` don't exist anywhere in the workspace at all — not even as stubs. Earlier placeholder stub files were deliberately deleted; see [`ARCHITECTURE.md`](../../ARCHITECTURE.md).
- Each remaining piece lands as its own crate when it's actually built, per the same pattern `michi-pipeline` followed.
- 01–07 cover Plan 1, the pure-primitives crate; `michi-pipeline`'s API is [08-pipeline-execution.md](08-pipeline-execution.md).

## Companion docs

- [`../../PRINCIPLES.md`](../../PRINCIPLES.md) — what belongs in michi and why, plus how work here gets done
- [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) — the design in prose, one level up from this spec's API detail
- [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md) — how to actually send a PR

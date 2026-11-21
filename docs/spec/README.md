# michi spec

The technical reference for `michi-core`, michi's sync primitives crate — what it does, how it's shaped, and why.

See the [documentation map](../README.md) for the role of this reference and related design records.

## Reading order

| Doc | Covers |
| --- | --- |
| [01-overview-and-setup.md](01-overview-and-setup.md) | What michi is, why it exists, what's deliberately out of scope, who consumes it, `Cargo.toml`, crate layout |
| [02-toon-format.md](02-toon-format.md) | The TOON grammar — the agent-facing list format |
| [03-rust-api.md](03-rust-api.md) | Every `michi-core` module: `toon`, `kv`, `hints`, `truncate`, `truncation`, `error`, `idempotency`, `resilience`, `redact`, `status`, `recovery`, `response`, `section`, `budget`, `provenance`, `pipeline`, `audience`, `telemetry` |
| [04-mcp-and-napi.md](04-mcp-and-napi.md) | The `mcp` module, the NAPI/npm boundary, and the builder-across-FFI problem |
| [05-scope-and-quality.md](05-scope-and-quality.md) | What's formalized vs. what stays in your app, feature flags, versioning, performance, testing |
| [06-decisions.md](06-decisions.md) | Open questions and API design rationale |
| [07-wisp-and-monokl-envelope.md](07-wisp-and-monokl-envelope.md) | Multi-section response envelope: `Section`, `BudgetSummary`, `ProvenanceBlock`, per-op outcomes, `structuredContent` and `outputSchema`. Accepted ([ADR 10](../adr/0010-response-envelope-accepted.md)). Slices 1-5 are implemented here (sections, budget, provenance, the per-op index in `PartialSuccess`, JSON envelope); the published `outputSchema` is not built |
| [08-pipeline-execution.md](08-pipeline-execution.md) | `michi-pipeline`: step plans, the replay rule, the status model, the run record, scheduling, cancellation, keyed breakers, the retry budget, the observer and resume |
| [09-graph.md](09-graph.md) | `michi-graph`: the MIT dependency-graph crate — building, validation, order, layers, ready sets, violations, and how `michi-pipeline` uses it |
| [10-pipeline-effects.md](10-pipeline-effects.md) | Amendment to 08, implemented in 0.5: what happened to a failed step's effect (not applied or unknown), so resume never repeats one blindly; an opt-in precheck that adopts effects already in place; durable starts that close the crash gap. Where it differs from 08, 08 is current |

New to the crate? Read 01 → 02 → 03 in order — that's the whole mental model. Read 04–07 and 08 as needed.

## Known gaps

**TOON retrieval accuracy is measured on Claude and one OpenAI model.** On Claude Haiku, Sonnet and Opus 5.5 and GPT-6 Sol TOON matches JSON and Markdown; Gemini and open-weight models, much longer lists and open-ended tasks are untested. See [TOON benchmarks](../toon-benchmarks.md) and [the research note](../research/toon-retrieval-accuracy.md).

**One external consumer has integrated, and 1.0 needs two.** Monokl depends on the published `michi-core` 0.7.1. [ADR 9](../adr/0009-v1-positioning-and-criteria.md) asks for two independent consumers on a published release for several weeks before 1.0, and Wisp consumes Monokl, so they do not count as two. The envelope in [07](07-wisp-and-monokl-envelope.md) is built here ahead of either consumer using it; treat its API as unproven until one does.

- A consumer may use a tagged Git dependency during development, then validate against the published crate. Any issues can inform a follow-up release.

**`cli` isn't a Cargo feature of this crate at all.** Reserved name for future terminal-aware rendering, in a separate crate, not implemented.

**Plan 2 (the async execution layer) doesn't exist as code in this crate — but `pipeline`'s half has landed as its own crate.**

- `pipeline` has a real, tested data type and `render()` in `michi-core` — always compiled.
- The actual executor exists too, just not in this crate: `crates/michi-pipeline` ships `Plan`, `execute`/`resume` returning a `RunRecord`, a keyed `BreakerRegistry` and `CancellationToken`. It depends on `michi-core` (for `StepStatus`, the `Pipeline` view and `resilience` retry config and budget) and `michi-graph` (dependency validation and order), and isn't re-exported through `michi-core`.
- `fuzzy`, `cache`, and `cli` don't exist anywhere in the workspace at all — not even as stubs. Earlier placeholder stub files were deliberately deleted; see [`ARCHITECTURE.md`](../../ARCHITECTURE.md).
- Each remaining piece lands as its own crate when it's actually built, per the same pattern `michi-pipeline` followed.
- 01–06 cover Plan 1, the pure-primitives crate, and 07 extends it with the response envelope; `michi-pipeline`'s API is [08-pipeline-execution.md](08-pipeline-execution.md) and `michi-graph`'s is [09-graph.md](09-graph.md).

## Companion docs

- [`../../PRINCIPLES.md`](../../PRINCIPLES.md) — what belongs in michi and why, plus how work here gets done
- [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) — the design in prose, one level up from this spec's API detail
- [`../../CONTRIBUTING.md`](../../CONTRIBUTING.md) — how to actually send a PR
- [`../adr/README.md`](../adr/README.md) — durable architectural decisions
- [`../research/README.md`](../research/README.md) — open questions that need evidence

# Overview & Setup

michi is a Rust library (`michi-core`) of pure agentic response primitives — the formatting and signaling conventions that make tools ergonomic for LLM agents, regardless of protocol or language.

## What it is

AXI (Agent eXperience Interface) is a set of ten design principles for agent-ergonomic tooling that treats token budget as a first-class constraint.

- **Core claim:** "MCP vs. CLI" is the wrong frame — the real question is which design principles make _any_ interface effective for an LLM agent.
- A well-designed interface following these principles measurably beats both naive CLIs and MCP on task success rate, cost, duration, and turn count.

michi encodes the subset of AXI that's pure, language-agnostic computation — the parts worth one canonical, tested implementation instead of ad hoc re-derivation in every tool.

**Seven of the ten principles, as typed Rust:**

| Principle                           | Module(s)                                             |
| ----------------------------------- | ----------------------------------------------------- |
| P1 — Token-Efficient Output         | `toon`, `kv`                                          |
| P3 — Content Truncation             | `truncate`                                            |
| P4 — Pre-Computed Aggregates        | `ToonOptions::total_count`, `status` health summaries |
| P5 — Definitive Empty States        | `empty`                                               |
| P6 — Structured Errors & Exit Codes | `error`, `idempotency`                                |
| P8 — Content First                  | `status`                                              |
| P9 — Contextual Disclosure          | `hints`, `recovery`                                   |

The other three stay out of scope on purpose.

- **P2 (Minimal Default Schemas)** is supported — callers pass exactly the fields they want — but not enforced.
- **P7 (Ambient Context)** and **P10 (Consistent Help)** are session-hook and CLI-framework concerns; they belong in the consuming tool, not a rendering crate.

No protocol knowledge, no async runtime. Pure computation: data in, strings and types out.

- Rust consumers take a direct crates.io or git dependency.
- TypeScript consumers reach it through the `@orin-dx/michi` npm package.

## Why this exists

AXI's principles usually get implemented ad hoc, scattered across tools instead of shared:

- Scattered TypeScript conventions in MCP servers
- Implicit CLI patterns
- Per-tool string formatting that drifts over time

With michi:

- TOON has one canonical, tested implementation shared by every consumer
- `help[]` hints, `totalCount` formatting, truncation signals, and recovery shapes are defined once and can't drift between tools
- Any agent-facing Rust CLI imports the primitives directly
- Any TypeScript MCP server or CLI reaches them through the npm package

Non-agentic tools — build scripts, infrastructure CLIs — never touch michi. The package boundary keeps agentic concepts out of code that doesn't need them.

## What's out of scope, deliberately

- **Display-format Markdown.** michi is the `audience: ["assistant"]` compact surface. Display rendering for `audience: ["user"]` stays in your MCP SDK or application layer.
- **Full MCP protocol knowledge.** No JSON-RPC, no tool registration, no server bootstrapping, no `outputSchema` validation. `AgentResponse::to_call_tool_result()` assembles the `content[]`/`isError`/`structuredContent` shape from an already-built response — see [04-mcp-and-napi.md](04-mcp-and-napi.md) — but nothing beyond that one assembly step.
- **CLI framework.** No argument parsing, no stdin/stdout handling.
- **HTTP client.** No auth, no request construction — just retry-delay primitives you plug into your own client.
- **Async runtime.** Zero tokio, zero async-std. Every function is sync and pure; you own the retry loop.
- **Full retry implementation.** michi gives you `RetryConfig`, `parse_retry_after()`, `next_retry_delay()`. The sleep-and-re-execute loop is yours.
- **Caching.** Use `moka` or equivalent.
- **Logging / telemetry.** Use `tracing` independently.
- **Schema validation.** Your concern, your type system.
- **MCP server bootstrapping.** `server.tool()`, tool discovery, deferred loading — stays in your SDK.

## Who consumes this, and how

```
Any Rust CLI binary
  Cargo.toml dep on michi-core (crates.io or git)
  Render --format toon via michi_core::toon::render_toon()
  Full access to every module — no NAPI overhead

Any TypeScript CLI
  npm dep on @orin-dx/michi
  --format toon dispatch calls into the NAPI wrapper
  Same output; the NAPI boundary is transparent
  render_for()/renderFor() picks agent vs. human output

Any TypeScript MCP server
  npm dep on @orin-dx/michi
  Renders TOON for the audience:["assistant"] content block
  Assembles MCP content[] from the returned string
```

## Cargo.toml

`michi` is a Cargo **workspace** with a virtual root manifest — there is no root package and no `michi` facade crate (the `michi` name on crates.io belongs to an unrelated project). It publishes two crates, `michi-core` and `michi-pipeline`, plus the npm package built from `packages/michi-node` (see [ARCHITECTURE.md](../../ARCHITECTURE.md) for the full component table, and [06-decisions.md](06-decisions.md) for why there are two crates and one version). The root `Cargo.toml`:

```toml
[workspace]
members  = [
  "crates/michi-core",
  "crates/michi-pipeline",
  "packages/michi-node"
]
resolver = "2"

[workspace.package]
# One version for every crate: Callisto releases michi-core, michi-pipeline and the npm package as a fixed group.
version      = "0.3.0"

[workspace.dependencies]
# Exact pin: workspace crates release in lockstep, so a mixed set must never resolve.
michi-core  = { path = "crates/michi-core", version = "=0.3.0" }
thiserror   = "2"
compact_str = "0.8"
serde       = { version = "1.0.228", features = ["derive"] }
serde_json  = { version = "1.0.150", features = ["preserve_order"] }
schemars    = "0.8"
miette      = "7"
tokio = { version = "1", features = ["time"] }
```

`crates/michi-core/Cargo.toml`, where every feature flag lives:

```toml
[package]
name = "michi-core"
version.workspace = true

[dependencies]
compact_str = { workspace = true, features = ["serde"] }
thiserror.workspace = true

serde      = { workspace = true, optional = true }
serde_json = { workspace = true, optional = true }
schemars   = { workspace = true, optional = true }
miette     = { workspace = true, optional = true }

# napi feature
napi        = { version = "3", features = ["napi6", "serde-json"], optional = true }
napi-derive = { version = "3", optional = true }

[features]
default  = []
serde    = ["dep:serde", "dep:serde_json"]
schemars = ["dep:schemars"]
miette   = ["dep:miette"]
napi     = ["dep:napi", "dep:napi-derive", "serde"]
```

(Elided: the remaining `[workspace.package]` metadata, `[profile.release]`/`[profile.bench]`, `[workspace.lints]`, `[dev-dependencies]`, `[[bench]]` entries — see the real files for those.)

Default features add no runtime dependencies beyond `thiserror` and `compact_str`.

- `serde_json` only enters `michi-core`'s tree through `napi` (typed `structuredContent`, wire-conformant MCP types) or the `serde` feature — never unconditionally. (`michi-core` unconditionally enables `compact_str`'s own `serde` Cargo feature regardless of its own `serde` feature, so `serde` the _crate_ is transitively present in the default dependency graph even though no default-feature type derives `Serialize`/`Deserialize` — see `crates/michi-core/Cargo.toml`.)
- `kv::KvValue` fills the role `serde_json::Value` would have for every consumer who doesn't opt in, at zero dependency cost.
- `preserve_order` keeps `toon::list()`'s field order matching each struct's declared order instead of alphabetizing it.
- `schemars` derives `JsonSchema` on DTO types; `miette` implements `miette::Diagnostic` for `DomainError`.

Consumers who want to write `use michi::...` rename the dependency: `michi = { package = "michi-core", version = "0.3" }`.

`pipeline`/`fuzzy`/`cache`/`cli` **execution** is not a Cargo feature of `michi-core` at all, and never will be.

- That async execution layer (Plan 2) lands as genuinely separate crates when actually built, never as features gated on `michi-core`'s `Cargo.toml`. `pipeline` has landed this way: `crates/michi-pipeline` (`Plan`, `execute`/`resume`, `RunRecord`, `BreakerRegistry`, `CancellationToken`; see [08-pipeline-execution.md](08-pipeline-execution.md)). `fuzzy`, `cache`, and `cli` remain unbuilt.
- `pipeline`'s pure **data model** (`Pipeline`, `PipelineStep`, `StepStatus`, `.render()`) lives in `michi-core` — unconditionally compiled, no feature gate. Orchestration/execution is the separate `michi-pipeline` crate, not a feature of this one. See [ARCHITECTURE.md](../../ARCHITECTURE.md) for the crate-boundary layout and [06-decisions.md](06-decisions.md) for the decision rule behind the split.
- `serde` isn't part of that Plan 2 set, despite historically sitting next to it in the feature table — it gates `Serialize`/`Deserialize` on Plan 1's own types.

`napi-build` lives in `packages/michi-node/Cargo.toml`, not here — that's the cdylib crate the actual napi-rs build step compiles.

## Crate layout

```
michi/                           (workspace root — virtual manifest, no package)
  Cargo.toml

  crates/
    michi-core/
      Cargo.toml
      src/
        lib.rs                   # module declarations, explicit root re-exports (incl. resilience and truncate items), Result<T>
        audience.rs              # Audience — always compiled
        empty.rs                 # empty_state(), empty_state_with_hints()
        error.rs                 # Error, ErrorCode, DomainError, ErrorClass, Sensitive<T>
        hints.rs                 # Hint, render_hints(), append_hints()
        idempotency.rs           # FailedOp, PartialSuccess — NOT idempotency keys/already-done
                                  # (that's resilience::AlreadyDone/already_done(); the
                                  # module name is a known, tracked naming staleness)
        kv/mod.rs                # render_kv(), KvItem, KvValue
        mcp.rs                   # ContentBlock, CallToolResult — always compiled
        napi.rs                  # #[napi] exports (napi feature only)
        napi/
          num.rs                 # JsRanged/JsFloat/JsCount/... numeric-boundary newtype kernel
        pipeline/mod.rs          # Pipeline, PipelineStep, StepStatus — data model + render only
        recovery.rs              # RecoveryHint, render_recovery()
        resilience.rs            # RetryConfig, next_retry_delay(), parse_retry_after(),
                                  # is_retryable_status(), AlreadyDone, IdempotencyKey
        resilience/budget.rs     # RetryBudget
        response.rs              # AgentResponse builder, OutputFormat
        status.rs                # StatusItem, StatusResponse, Health
        telemetry/mod.rs         # NoopProvider — zero-cost, always compiled
        toon/
          mod.rs                 # ToonOptions, ToonError, render_toon(), list() (serde feature)
          escape.rs              # comma/quote/newline escaping, header-token sanitizing
          render.rs              # Value, ToonDocument; string assembly with pre-allocated capacity
        truncate.rs              # Truncated, truncate(), truncate_inline()
      benches/
        toon_render.rs, kv_render.rs
      examples/
        basic_toon.rs, agent_response.rs, domain_errors.rs   # cargo run -p michi-core --example <name>
      tests/
        toon_integration.rs, kv_integration.rs, napi_num_reuse.rs
        snapshot_tests.rs        # insta snapshots
        proptest_toon.rs, proptest_truncate.rs, proptest_resilience.rs, proptest_mcp.rs
        toon_parser.rs, support/ # test-only TOON parser for round-trip property tests
        ui_fail.rs, ui-fail/     # trybuild compile-fail tests

    michi-pipeline/
      src/
        lib.rs                   # crate docs and re-exports
        id.rs                    # StepId, RunId
        plan.rs                  # ExecStep, Plan, PlanError, PlanFingerprint
        step.rs                  # Step, Replay, StepContext, step_fn
        exec.rs                  # ExecOptions, execute(), resume(), StepObserver
        scheduler.rs             # the JoinSet scheduler loop and per-step retry loop
        record.rs                # RunRecord, StepRecord, RunOutcome
        breaker.rs               # CircuitBreaker, BreakerRegistry
        cancel.rs, error.rs      # CancellationToken, ExecutionError
      benches/scheduler.rs       # divan: 500-step plan validation and execution
      tests/                     # plan, execute, replay, resume, serde

  packages/michi-node/           # NAPI wrapper (npm: @orin-dx/michi), publish = false
    Cargo.toml                   # cdylib; depends on michi-core with the napi feature
    build.rs                     # napi-build setup
    package.json                 # name: "@orin-dx/michi"
    index.js                     # platform binary loader + TS fallback
    index.d.ts                   # TypeScript types (auto-generated)
    src/lib.rs                   # pub use michi_core::napi::*;
    __test__/                    # node:test NAPI integration tests
```

# michi — Architecture Design

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).
> orin-dx · 2026-07-03

This document records the architectural decisions made during the July 2026
brainstorming session. It supersedes and extends any open questions in
`01-spec.md` (which predates this session). Implementation plans should treat
this doc as authoritative on the topics it covers.

---

## 1. One crate, feature-gated

**Decision: one `michi` crate with optional features. No separate execution crate.**

The original spec positioned michi as a pure sync rendering library with no
async runtime. The question was whether pipeline execution (tokio, moka, nucleo)
should live in a companion crate (`kata`) or in michi itself behind features.

Feature gates give the same guarantee that crate boundaries give — a consumer
who adds `michi` with default features pulls in zero tokio, zero moka, zero
nucleo. The "pure computation" principle is enforced by cargo, not by convention.

The suite tools (Monokl, Firkin, Lumen, Pulse) all need both rendering primitives
and pipeline execution. One crate means one dep, one version pin, one mental model.
Two crates would require cross-crate type conversions and coordinated releases with
no architectural benefit.

The NAPI wrapper (`packages/michi-node`) remains a separate Cargo crate because
`crate-type = ["cdylib"]` cannot coexist with a regular `[lib]` in one
`Cargo.toml`. This is a build-artifact boundary, not a conceptual one.

---

## 2. Feature taxonomy

```toml
[features]
default  = []

# Async execution layer: PipelineExecutor, Step, CheckpointStore, OutputSink,
# CircuitBreaker, with_resilience(). Adds: tokio, tokio-util, async-trait, uuid.
# Note: thiserror is a regular (non-optional) dep used by all features.
pipeline = [
    "dep:tokio",
    "dep:tokio-util",
    "dep:async-trait",
    "dep:uuid",
]

# Fuzzy matching: FuzzyMatcher, FuzzyResolver, Disambiguator trait.
# Implies pipeline (Resolution<T> is used in pipeline context).
# Adds: nucleo-matcher.
fuzzy = ["dep:nucleo-matcher", "pipeline"]

# Two-tier cache: moka async LRU + disk LRU with tag invalidation +
# stale-while-revalidate. Adds: moka, sha2.
cache = ["dep:moka", "dep:sha2", "pipeline"]

# CLI surface adapter: CliSink (indicatif spinners), DiskCheckpointStore,
# TtyDisambiguator, install_ctrl_c_handler.
# Adds: indicatif, inquire, crossterm, ctrlc.
# TTY detection uses std::io::IsTerminal (stable since Rust 1.70, well below
# our MSRV) instead of the `atty` crate, which carries RUSTSEC-2021-0145.
cli = [
    "dep:indicatif",
    "dep:inquire",
    "dep:crossterm",
    "dep:ctrlc",
    "pipeline",
]

# MCP surface adapter: McpSink, MemoryCheckpointStore, McpDisambiguator.
# Pure logic — no additional non-tokio deps.
mcp = ["pipeline"]

# NAPI npm wrapper surface. Used by packages/michi-node only.
napi = ["dep:napi", "dep:napi-derive"]

# Everything except napi (napi is build-artifact-level, not feature-level).
full = ["pipeline", "fuzzy", "cache", "cli", "mcp"]
```

Typical consumer configs:

| Consumer | Features |
|---|---|
| Pure rendering (Rust CLI, no pipeline) | `michi` (defaults) |
| Rust CLI that runs pipelines | `michi = { features = ["cli"] }` |
| TypeScript MCP server via npm | `@orin-dx/michi` npm package (default) |
| MCP server that runs pipelines | `michi = { features = ["mcp"] }` |
| Test/bench harness | `michi = { features = ["full"] }` |
| `packages/michi-node` Cargo.toml | `michi = { features = ["napi"] }` |

---

## 3. Module map

Modules in the left column are always compiled (default features). Modules in
the right column are gated.

```
src/
  lib.rs

  ── Always compiled ──────────────────────────────────────────────────────────

  toon/
    mod.rs          render_toon(), ToonOptions, Value
    escape.rs       comma/quote/null escaping
    render.rs       string assembly with pre-allocated capacity

  kv/
    mod.rs          render_kv(), KvItem, KvValue

  hints.rs          Hint, render_hints(), append_hints()
  truncate.rs       Truncated, truncate(), truncate_inline()
  empty.rs          empty_state(), empty_state_with_hints()
  error.rs          Error (unified — see §4), ErrorClass, Sensitive<T>
  idempotency.rs    IdempotencyKey, already_done(), PartialSuccess
  recovery.rs       RecoveryHint, render_recovery()
  status.rs         StatusItem, StatusResponse, Health
  response.rs       AgentResponse builder, OutputFormat

  resilience/
    mod.rs          RetryConfig, next_retry_delay(), parse_retry_after()  ← always
    policy.rs       with_resilience()                                      ← pipeline
    circuit.rs      CircuitBreaker, CircuitState                          ← pipeline

  pipeline/
    mod.rs          Pipeline (pure data type + render())                   ← always
    executor.rs     PipelineExecutor, PipelineContext                     ← pipeline
    step.rs         Step, StepFn, StepContext, SkipReason                 ← pipeline
    graph.rs        assign_levels(), cycle detection                      ← pipeline
    result.rs       PipelineResult, RunStatus                             ← pipeline
    checkpoint.rs   Checkpoint, CheckpointStore, NoopCheckpointStore      ← pipeline

  sink/
    mod.rs          OutputSink, AgentEvent, NoopSink                      ← pipeline

  telemetry/
    mod.rs          TelemetryProvider, NoopProvider                       ← always (zero-cost)

  ── Optional features ────────────────────────────────────────────────────────

  fuzzy/
    mod.rs          FuzzyMatcher, FuzzyResolver, Resolution<T>            ← fuzzy
    matcher.rs      nucleo-matcher wrapper, MatcherConfig                 ← fuzzy
    resolver.rs     Disambiguator trait, resolve()                        ← fuzzy

  cache/
    mod.rs          Cache, CachePolicy, CacheEntry                        ← cache
    memory.rs       moka::future::Cache wrapper                           ← cache
    disk.rs         DiskCache, tag invalidation                           ← cache
    policy.rs       CachePolicyBuilder, stale-while-revalidate            ← cache

  adapters/
    cli/
      mod.rs        CliSink, HumanWriter, JsonWriter                      ← cli
      checkpoint.rs DiskCheckpointStore                                   ← cli
      disambig.rs   TtyDisambiguator                                      ← cli
      ctrl_c.rs     install_ctrl_c_handler()                              ← cli
    mcp/
      mod.rs        McpSink                                               ← mcp
      checkpoint.rs MemoryCheckpointStore                                 ← mcp
      disambig.rs   McpDisambiguator                                      ← mcp

  napi.rs           #[napi] exports                                       ← napi
```

### Module principles

- Pure vs async split within a module: sync computation always compiled, async
  extension behind `pipeline`. Pattern from `resilience/`: `next_retry_delay()`
  is always there; `with_resilience()` requires the feature.
- `pipeline::Pipeline` (pure data type + `render()`) is always compiled because
  tools may want to render a pipeline state without executing it.
- `telemetry` uses `NoopProvider` (zero-cost impl) so it compiles clean without
  a runtime dep.

---

## 4. Error type consolidation

The original spec had `AxiError` (rendering-focused, michi) as a separate concept
from `AgentError` (infrastructure, the proposed execution spec). In a unified
crate these merge into `michi::Error`.

```rust
// src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // ── Execution errors (pipeline feature) ───────────────────────────────
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String, retryable: bool, retry_after: Option<Duration> },

    #[error("Timeout after {elapsed:?}")]
    Timeout { elapsed: Duration },

    #[error("Step {id} failed")]
    StepFailed { id: String, #[source] source: Box<Error> },

    #[error("Circuit {name} is open, retry after {retry_after:?}")]
    CircuitOpen { name: String, retry_after: Duration },

    #[error("No match for query: {query}")]
    NoMatch { query: String },

    #[error("Ambiguous match for {query}: {count} candidates")]
    AmbiguousMatch { query: String, count: usize },

    #[error("Cyclic dependency: {cycle:?}")]
    CyclicDependency { cycle: Vec<String> },

    #[error("Cancelled")]
    Cancelled,

    #[error("Cache error: {0}")]
    Cache(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    // ── Domain errors (always) ─────────────────────────────────────────────
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl Error {
    /// Render to agent-readable stdout string (AxiError-style).
    pub fn render(&self) -> String { ... }
    pub fn exit_code(&self) -> i32 { 1 }
    pub fn class(&self) -> ErrorClass { ... }
    pub fn is_retryable(&self) -> bool { ... }
}

pub enum ErrorClass { User, Internal, Transient }

/// Wrapper that omits the inner value from Debug/Display output.
pub struct Sensitive<T>(pub T);
```

`Sensitive<T>` and `ErrorClass` are always compiled. The execution error variants
(`Http`, `Timeout`, `StepFailed`, `CircuitOpen`, `NoMatch`, `AmbiguousMatch`,
`CyclicDependency`, `Cancelled`, `Cache`) are gated behind `#[cfg(feature = "pipeline")]`.

---

## 5. Repo shape

```
michi/                          ← git root: git@github.com:orin-dx/michi.git
  Cargo.toml                    ← [workspace] members + [package] michi lib
  Cargo.lock
  rust-toolchain.toml           ← pinned stable channel
  build.rs                      ← napi-build (conditional on napi feature)
  justfile                      ← unified task runner (see §6)
  .rustfmt.toml                 ← edition=2021, max_width=120
  .clippy.toml                  ← msrv, too-many-arguments-threshold=6
  .typos.toml                   ← spell-check config
  deny.toml                     ← license + advisory + bans policy
  CLAUDE.md                     ← agent session instructions
  README.md
  src/                          ← michi lib source (as above)
  benches/
    toon_render.rs
    kv_render.rs
    pipeline_render.rs
  tests/
    toon_integration.rs
    kv_integration.rs
    resilience_integration.rs
    idempotency_integration.rs
    pipeline_integration.rs     ← pipeline feature, uses tokio::test
    snapshot_tests.rs           ← insta snapshots
  packages/
    michi-node/                 ← NAPI cdylib (separate Cargo crate, required by Rust)
      Cargo.toml                ← crate-type = ["cdylib"], deps on michi napi feature
      package.json              ← name: "@orin-dx/michi"
      index.js                  ← platform binary loader
      index.d.ts                ← generated by napi-derive v3
      src/lib.rs                ← thin #[napi] shim over michi::*
      __test__/
        index.test.mjs
  .github/
    workflows/
      ci.yml
  docs/
    00-overview.md
    01-spec.md
    projects/
      01-mvp.md
      agents/                   ← session briefs for S0–S4
    superpowers/
      specs/
        2026-07-03-michi-design.md   ← this file
```

**Workspace Cargo.toml:**

```toml
[workspace]
members = [
    ".",
    "packages/michi-node",
]
resolver = "2"

[workspace.package]
edition     = "2021"
rust-version = "1.96"
license     = "AGPL-3.0-or-later"
repository  = "https://github.com/orin-dx/michi"
authors     = ["orin-dx"]

[profile.release]
opt-level      = 3
lto            = "thin"
codegen-units  = 1

[profile.bench]
inherits = "release"
debug    = true
```

No pnpm workspace root for v0.1 (one JS package, no cross-package hoisting
needed). Add `pnpm-workspace.yaml` when a second JS package joins the repo.

---

## 6. Tooling stack

| Tool | Role | Version |
|---|---|---|
| `just` | Unified local task runner | latest |
| `cargo nextest` | Parallel test runner (replaces `cargo test`) | via taiki-e/install-action |
| `cargo-llvm-cov` | Coverage → lcov → Codecov | via taiki-e/install-action |
| `cargo-deny` | License + advisory + ban policy | latest |
| `divan` | Benchmarks (not criterion) | 0.1 |
| `insta` | Snapshot tests | 1, yaml feature |
| `typos` | Spell-check | latest |
| `pnpm` | JS package manager for michi-node | 9+ |
| `@napi-rs/cli` | NAPI build + publish tooling | 3 |
| `napi` / `napi-derive` | NAPI bindings | **3** (not 2) |

**Not used:**
- `criterion` — divan is the benchmark crate (consistent with oxc-react-codegen)
- `rayon` — pure sync rendering has no parallelism to exploit at the crate level
- `miette` — callers own error display; michi's error type focuses on agent rendering
- Moon — repo is too small; two crates don't need a polyglot task graph
- tokio in default features — execution layer is always opt-in via `pipeline` feature

### justfile (canonical tasks)

```justfile
default:
    @just --list

# ── Build ──────────────────────────────────────────────────────────────────
build:
    cargo build --workspace

build-release:
    cargo build --release --workspace

build-node:
    cd packages/michi-node && pnpm build --platform

build-node-release:
    cd packages/michi-node && pnpm build --platform --release

# ── Test ───────────────────────────────────────────────────────────────────
test: test-rust test-node

test-rust:
    cargo nextest run --workspace

test-rust-all:
    cargo nextest run --workspace --all-features

test-node: build-node
    cd packages/michi-node && pnpm test

# ── Lint ───────────────────────────────────────────────────────────────────
check: fmt-check clippy deny typos

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-features -- -D warnings

deny:
    cargo deny check

typos:
    typos

# ── Snapshots ──────────────────────────────────────────────────────────────
snapshots:
    cargo insta review

# ── Benchmarks ─────────────────────────────────────────────────────────────
bench:
    cargo bench --workspace

# ── Coverage ───────────────────────────────────────────────────────────────
coverage:
    cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

# ── Pre-push ───────────────────────────────────────────────────────────────
ci: check test
```

CI (GitHub Actions) uses raw cargo/pnpm commands directly — no `just` dep in CI.

---

## 7. Spec updates required in 01-spec.md

These items in the existing spec are now outdated and should be corrected during
implementation:

| Location | Current (wrong) | Correct |
|---|---|---|
| `Cargo.toml` `[dependencies.napi]` | `version = "2"` | `version = "3"` |
| `Cargo.toml` `[dependencies.napi-derive]` | `version = "2"` | `version = "3"` |
| `Cargo.toml` `[dev-dependencies]` | `criterion = { version = "0.5" }` | `divan = "0.1"` |
| `[features]` | `napi`, `cli = []` only | Full feature taxonomy (see §2) |
| Non-goals | "zero tokio, zero async-std" | True for default features; `pipeline` feature adds tokio |
| Consumer map | No pipeline mention | Add pipeline execution consumer shape |
| `error.rs` | `AxiError, ErrorCode` | `Error` (unified), `ErrorClass`, `Sensitive<T>` |
| `[[bench]]` | `harness = false` under criterion | divan bench setup |

---

## 8. Open questions (not blocking v0.1)

**Q1 — MSRV:** Set to 1.96 in workspace (matching oxc-react-codegen pattern).
Verify no dep requires higher before first publish.

**Q2 — disk cache path:** `DiskCache` needs a path at construction. CLI adapter
uses `~/.cache/michi/`. MCP adapter uses temp dir. Injected by the caller —
michi provides no default path discovery.

**Q3 — AGPL vs MIT/Apache:** Spec says AGPL-3.0-or-later. Confirm with project
owner before publish. AGPL requires source disclosure for network use, which may
affect suite tool consumers.

**Q4 — `napi4` feature level:** Spec historically chose `napi4` for
ThreadsafeFunction. With NAPI v3 and the sync-only default feature set, verify
the minimum level needed. For v0.1 (rendering only, no async NAPI), `napi4` may
be unnecessary overhead; `napi6` or `napi8` may be required for v3 features.
Defer to napi-rs v3 docs.

**Q5 — pipeline feature in npm package:** Does `packages/michi-node` expose
pipeline types to TypeScript? For v0.1: no. The npm package is rendering
primitives only. Pipeline execution in TypeScript consumers goes through the MCP
adapter layer, which is a Rust binary or sidecar, not a NAPI binding.

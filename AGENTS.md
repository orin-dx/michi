# michi — Agent Instructions

## Commands

- Always prefix shell commands with `rtk`.
- `just test` — run all tests (Rust + Node)
- `just test-rust` — Rust tests only (cargo nextest)
- `just check` — fmt + clippy (`--all-targets`, single workspace-wide invocation, matches CI) + deny + typos + markdown fmt
- `just pre-push` — fmt-check + `lint-affected` (moon-detected affected crates only, one multi `-p` clippy invocation — not the full workspace)
- `just build-node` — compile NAPI binary for local platform
- `just bench` — run divan benchmarks
- `just snapshots` — review insta snapshot diffs
- `just coverage` — generate lcov.info

## Non-negotiables

- No `unwrap()`/`expect()` in lib code (tests OK with message)
- No `unsafe` outside napi boundary
- `BTreeMap` for output maps, `HashMap` for internal state only
- Pre-allocate strings with `String::with_capacity`
- Truncation uses char boundaries (`floor_char_boundary`)
- Every `pub` item has a doc comment
- NAPI: `version = "3"`, `#[napi(catch_unwind)]` on every export
- Benchmarks: divan (not criterion)
- Tests: `cargo nextest run` (not `cargo test`)

## Writing

- Write docs in plain, concise language. Keep them friendly and scannable; remove filler and AI-sounding prose.
- Comments should explain non-obvious choices, stay useful, and wrap at 120 columns.

## Philosophy

- Before adding a new primitive or judging whether work is "done," read `PRINCIPLES.md` — the inclusion checklist and process conventions this project runs on.

## Architecture

- Virtual Cargo workspace: the root `Cargo.toml` has no `[package]` and no shared version; Callisto writes each crate's `version` and keeps `michi-core`, `michi-graph`, `michi-pipeline` and the npm package equal (fixed group `michi`, lockstep releases, checked by CI version-sync)
- `crates/michi-core/` — pure sync rendering library, every primitive (default features: only `thiserror` + `compact_str`). Integration tests, divan benches and examples live in `crates/michi-core/{tests,benches,examples}` (`cargo run -p michi-core --example basic_toon`)
- `crates/michi-graph/` — **MIT**, no dependencies, `no_std` + `alloc`: deterministic dependency graphs (`GraphBuilder` → `Graph` → `Dag`; validation reports every problem at once; topological order, layers, ready sets, closures; `Dag<K, E>` keeps edge kinds for `ready_by_edge`). Depends on nothing in michi, releases with the fixed group, and must never gain a non-permissive dependency (`just check-graph-license`). API: `docs/spec/09-graph.md`.
- `crates/michi-pipeline/` — async pipeline execution (shipped): validated `Plan` of `ExecStep`s, `execute`/`resume` returning a `RunRecord`, keyed `BreakerRegistry`, retry budget, per-key concurrency limits, deadline groups, `poll_step`, `step_blocking`, `CancellationToken` (wraps tokio-util), `StepObserver`, its own `ExecutionError` (not a `michi_core::Error` variant — see `docs/spec/06-decisions.md`). API: `docs/spec/08-pipeline-execution.md`. Depends on `michi-core` (exact `=` version pin), `michi-graph` (exact `=` pin; no michi-graph type in its public API) + `tokio` + `tokio-util`; not re-exported through `michi-core`.
- `packages/michi-node/` — thin NAPI cdylib shim (`publish = false`), npm package name `@orin-dx/michi`; the exports themselves live in `crates/michi-core/src/napi.rs` behind michi-core's `napi` feature and are re-exported via `pub use michi_core::napi::*;`
- Published crates: `michi-core`, `michi-pipeline` and `michi-graph`. There is no `michi` crate (the name on crates.io belongs to an unrelated project)
- Feature flags (on `michi-core`): `napi`, `serde` (opt-in Serialize/Deserialize + `toon::list()`), `schemars`, `miette`. Execution layers (`pipeline`, `fuzzy`, `cache`, `cli`) land as separate crates, never as features here — `michi-pipeline` shipped this way (see above); `fuzzy`/`cache`/`cli` are not built anywhere. See `docs/spec/01-overview-and-setup.md`'s Cargo.toml section for the current split.
- See `docs/spec/06-decisions.md` and `docs/adr/README.md` for decisions (`docs/superpowers/` is historical)
- See `docs/spec/` for full module API reference

## Module guide

All modules live in `crates/michi-core/src/`.

| Module | Purpose |
| --- | --- |
| `toon` | TOON list rendering — token-optimized agent list format |
| `kv` | Key-value single-item rendering |
| `hints` | `help[]` hint blocks |
| `truncate` | Token-safe content truncation |
| `truncation` | `TruncationOutcome`: what a cap on a list cut, however the caller chose the survivors |
| `empty` | Definitive empty state responses |
| `error` | Unified `michi_core::Error` type with agent rendering |
| `idempotency` | Partial-success reporting (`PartialSuccess`/`FailedOp`) — despite the name, idempotency keys and `render_already_done` live in `resilience`, not here |
| `resilience` | Retry config, delay calculation, retry-after parsing |
| `status` | Health/status response rendering |
| `recovery` | Recovery hint blocks |
| `response` | `AgentResponse` builder — composes all primitives |
| `mcp` | MCP `CallToolResult` assembly — always compiled, no feature gate |
| `audience` | `Audience` (assistant/user) — shared by `mcp` and `response::render_for()` |
| `pipeline` | Pipeline pure data type + render (execution lives in `crates/michi-pipeline`, not `crates/michi-core`) |
| `napi` | NAPI export surface for `@orin-dx/michi` (`napi` feature only) — the one module allowed `unsafe` |

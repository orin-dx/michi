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

- `src/` — pure sync rendering library (default features: zero runtime deps)
- `crates/michi-pipeline/` — async pipeline execution (Plan 2, shipped): `execute_pipeline`/`execute_pipeline_parallel`, `CircuitBreaker`, `CancellationToken`, its own `ExecutionError` (not a `michi_core::Error` variant — see `docs/spec/06-decisions.md`). Depends on `michi-core` + `michi-resilience` + `tokio`; not re-exported through the root `michi` facade, versioned independently.
- `packages/michi-node/` — thin NAPI cdylib shim, npm package name `@orin-dx/michi`
- Feature flags: `napi`, `serde` (opt-in Serialize/Deserialize + `toon::list()`), `schemars`, `miette`. Plan 2 execution lands as separate crates when built, never as features here — `pipeline` has shipped this way (see `crates/michi-pipeline/` above); `fuzzy`/`cache`/`cli` remain unbuilt. See `docs/spec/01-overview-and-setup.md`'s Cargo.toml section for the current split.
- See `docs/superpowers/specs/2026-07-03-michi-design.md` for decisions
- See `docs/spec/` for full module API reference

## Module guide

| Module | Purpose |
| --- | --- |
| `toon` | TOON list rendering — token-optimized agent list format |
| `kv` | Key-value single-item rendering |
| `hints` | `help[]` hint blocks |
| `truncate` | Token-safe content truncation |
| `empty` | Definitive empty state responses |
| `error` | Unified `michi::Error` type with agent rendering |
| `idempotency` | Partial-success reporting (`PartialSuccess`/`FailedOp`) — despite the name, idempotency keys and already-done detection live in `resilience` (`AlreadyDone`/`already_done()`), not here |
| `resilience` | Retry config, delay calculation, retry-after parsing |
| `status` | Health/status response rendering |
| `recovery` | Recovery hint blocks |
| `response` | `AgentResponse` builder — composes all primitives |
| `mcp` | MCP `CallToolResult` assembly — always compiled, no feature gate |
| `audience` | `Audience` (assistant/user) — shared by `mcp` and `response::render_for()` |
| `pipeline` | Pipeline pure data type + render (execution now in `crates/michi-pipeline`, not this table's `src/`) |
| `telemetry` | No-op telemetry provider (`NoopProvider`) — zero-cost default, always compiled |

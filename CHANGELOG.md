# michi

## [Unreleased]

- The workspace publishes two crates, `michi-core` and `michi-pipeline`, plus the `@orin-dx/michi` npm package, all on one version (0.3.0). The `michi` facade and the `michi-toon`, `michi-truncate` and `michi-resilience` crates are merged into `michi-core` as the `toon`, `truncate` and `resilience` modules; replace `michi::` paths with `michi_core::`. The `michi` name on crates.io belongs to an unrelated project. See [06-decisions.md](docs/spec/06-decisions.md).
- License changed to FSL-1.1-MIT (from AGPL-3.0-or-later), for suite consistency with Lumen and Wisp and to remove the FSL/AGPL linking conflict. See [06-decisions.md](docs/spec/06-decisions.md).

## 0.2.0

- `nextRetryDelay` now rejects `NaN` and `Infinity` for `jitterFactor` and `jitterSeed` with a clear error. Previously, these values silently zeroed jitter and caused synchronized retry storms. `RetryConfig::new` is hardened the same way for Rust callers.
- `render_toon()` now returns `Result<String, ToonError>` instead of `String`. Previously, invalid input (row/field arity mismatches, oversized rows, structural characters in names) degraded silently to a best-effort or malformed string; now every validation failure is surfaced as an explicit `Err`. The loose `render()` function (the pre-`ToonDocument` code path with debug/release-divergent behavior) has been removed — `render_toon()` and `list()` are the only entry points, both routed through `ToonDocument`'s validated construction path.

  Callers must add `?` or explicit error handling at `render_toon()` call sites.

  `ToonDocument` — the proof-carrying type this validated construction path routes through (`ToonDocument::validate(&opts)` fallibly constructs, `.render()` infallibly renders) — is now re-exported at the `michi-toon` crate root, so callers who want the validate/render split explicitly no longer need to go through `render_toon()`.

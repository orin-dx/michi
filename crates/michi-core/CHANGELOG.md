# michi-core

## Unreleased

- `michi-core` now contains the whole synchronous library. The `michi-toon`, `michi-truncate` and `michi-resilience` crates and the `michi` facade are merged into it as the `toon`, `truncate` and `resilience` modules, and the NAPI exports move here behind the `napi` feature. Replace `michi::` paths with `michi_core::`; module paths are otherwise unchanged (`michi::toon::list` becomes `michi_core::toon::list`). `michi-core` and `michi-pipeline` now share one version.
- Quoted TOON cells double their backslashes (`C:\src\a,b` → `"C:\\src\\a,b"`). Before, backslashes were lost on parsing and a value ending in `\` merged two cells.
- Line breaks in caller text are written as `\n` / `\r` in every renderer instead of passing through raw (hints, empty-state names, error messages, recovery tool/reason/keys, KV keys, already-done fields, partial-success ids) or being dropped (KV values, status fields, TOON cells). TOON cells containing a line break are quoted. TOON hints keep ordinary punctuation instead of replacing `,` `[` `]` `{` `}` with `_`.
- `toon::list()` headers are the union of every item's fields in first-seen order; rows missing a field get an empty cell. Before, the first item decided the columns.
- `AgentResponse`: rows that fail TOON validation set `isError: true` in MCP results and the JSON render, not just in the text body. New `AgentResponse::is_error()`.
- `Value::from(u64)` / `Value::from(usize)` above `i64::MAX`, and such numbers in `toon::list()`, keep their exact digits (as `Value::Str`) instead of clamping to `i64::MAX` or rounding through `f64`.
- `RecoveryHint` serializes as `{"tool", "params": {key: value}, "reason"?}`, matching the structured companions; `reason` is omitted when absent.
- The `schemars` schemas for `ContentBlock`, `CallToolResult` and `RecoveryHint` describe the wire form serde emits. The `schemars` feature now enables `serde`.

## 0.1.1

- Dependency updates
  - `michi-toon` → `0.2.0`

## Merged history: michi-toon 0.2.0

- `render_toon()` now returns `Result<String, ToonError>` instead of `String`. Previously, invalid input (row/field arity mismatches, oversized rows, structural characters in names) degraded silently to a best-effort or malformed string; now every validation failure is surfaced as an explicit `Err`. The loose `render()` function (the pre-`ToonDocument` code path with debug/release-divergent behavior) has been removed — `render_toon()` and `list()` are the only entry points, both routed through `ToonDocument`'s validated construction path.

  Callers must add `?` or explicit error handling at `render_toon()` call sites.

  `ToonDocument` — the proof-carrying type this validated construction path routes through (`ToonDocument::validate(&opts)` fallibly constructs, `.render()` infallibly renders) — is re-exported from the `toon` module, so callers who want the validate/render split explicitly don't need to go through `render_toon()`.

## Merged history: michi-resilience 0.1.1

- `nextRetryDelay` now rejects `NaN` and `Infinity` for `jitterFactor` and `jitterSeed` with a clear error. Previously, these values silently zeroed jitter and caused synchronized retry storms. `RetryConfig::new` is hardened the same way for Rust callers.

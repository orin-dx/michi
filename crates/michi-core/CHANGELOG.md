# michi-core

## 0.3.0

- **Breaking:** `pipeline::StepStatus` gains `Running`, `CompletedEarlier`, `OutcomeUnknown(UnknownCause)`, `NotRun(NotRunReason)` and `Panicked`, and `Skipped` now carries a `SkipReason` (`PrerequisiteFailed` or `Cancelled`). The reason enums are `#[non_exhaustive]`. New `StepStatus::reason()` and `is_success()`; labels are snake_case (`completed_earlier`, `outcome_unknown`, `not_run`). With `serde`, the status types serialize in snake_case.
- **Breaking:** `Pipeline::render()` emits `step[N]{id,name,status,reason}:`, with the reason label or `null` in the new column. Output stays TOON 4.4 conformant.
- New `resilience::RetryBudget`: a pure retry budget modelled on tower's `TpsBudget` (each success deposits a fraction of a retry, each retry withdraws one, capped). `michi-pipeline` enforces it per run.

- `michi-core` now contains the whole synchronous library. The `michi-toon`, `michi-truncate` and `michi-resilience` crates and the `michi` facade are merged into it as the `toon`, `truncate` and `resilience` modules, and the NAPI exports move here behind the `napi` feature. Replace `michi::` paths with `michi_core::`; module paths are otherwise unchanged (`michi::toon::list` becomes `michi_core::toon::list`). `michi-core` and `michi-pipeline` now share one version.
- Quoted TOON cells double their backslashes (`C:\src\a,b` → `"C:\\src\\a,b"`). Before, backslashes were lost on parsing and a value ending in `\` merged two cells.
- Line breaks in caller text are written as `\n` / `\r` in every renderer instead of passing through raw (hints, empty-state names, error messages, recovery tool/reason/keys, KV keys, already-done fields, partial-success ids) or being dropped (KV values, status fields, TOON cells). TOON cells containing a line break are quoted. TOON hints keep ordinary punctuation instead of replacing `,` `[` `]` `{` `}` with `_`.
- `toon::list()` headers are the union of every item's fields in first-seen order; rows missing a field get an empty cell. Before, the first item decided the columns.
- `AgentResponse`: rows that fail TOON validation set `isError: true` in MCP results and the JSON render, not just in the text body. New `AgentResponse::is_error()`.
- `Value::from(u64)` / `Value::from(usize)` above `i64::MAX`, and such numbers in `toon::list()`, keep their exact digits (as the new numeric `Value::UInt`, never truncated or quoted) instead of clamping to `i64::MAX` or rounding through `f64`.
- `RecoveryHint` serializes as `{"tool", "params": {key: value}, "reason"?}`, matching the structured companions; `reason` is omitted when absent.
- The `schemars` schemas for `ContentBlock`, `CallToolResult` and `RecoveryHint` describe the wire form serde emits. The `schemars` feature now enables `serde`.
- Deserializing a `RecoveryHint` rejects parameter values it cannot hold exactly (arrays, objects, integers outside `i64`) instead of rewriting them; floats keep their exact value.
- Deserializing a `ContentBlock` whose `type` is not `"text"` is an error instead of a silent conversion to text.
- `Error::InvalidInput` / `Error::NotFound` renders escape line breaks like every other renderer.
- The default build no longer depends on serde: `compact_str`'s serde support is enabled only by the `serde` feature.
- Truncation reports what was cut: the suffix is `" ({removed} more chars — use {hint})"`, where `removed` is the number of chars cut, instead of `" ({original} chars truncated — use {hint})"`, which reported the original length although some chars remained. An empty hint now drops the `— use …` clause instead of printing `use )`.
- TOON cells no longer tell agents to use `full=true`, a parameter the calling tool may not have: a truncated cell ends with `" ({removed} more chars)"` unless the caller sets the new `ToonOptions::truncation_hint` / `AgentResponse::truncation_hint` (NAPI: `JsToonOptions.truncationHint`, `AgentResponse.truncationHint()`). Pass `.truncation_hint("full=true")` to keep the old clause.
- A cap too small for the suffix gives the first `max_chars - 1` chars and `…` (`signal` is `"…"`) instead of a fragment such as `" (500 chars tr"`; a cap of 0 still gives the empty string.
- Documented that `max_cell_len` / `truncate_cells_at` count chars before TOON escaping and quoting, so a quoted cell can be longer, and that `Truncated::original_len` is a byte length.
- Every agent-facing block is now a TOON 4.4 document, not only lists, and no block ends with a newline. `render_hints`, `append_hints`, `render_kv`, `render_recovery`, `append_recovery`, `DomainError::render`, `Error::render`, `StatusResponse::render`, `PartialSuccess::render` and `render_already_done` return a finished document; `append_hints`/`append_recovery` add one LF before the block unless the buffer is empty or already ends in one. Code that concatenated blocks relying on the trailing newline must join them with `\n` or use the append helpers.
  - KV: one space after each colon instead of column alignment; keys and text values quoted per TOON §7.3/§7.2 (`note: "a: b"`); `Missing` is `null` instead of `—`; `Float(f, decimals)` rounds to `decimals` places and then drops trailing zeros (`Float(1.5, 3)` → `1.5`, not `1.500`; NaN/±Infinity → `null`). `Duration` stays `1.5s`.
  - **Breaking:** `render_kv` returns `Result<String, KvError>`. New `KvError` (`ReservedKey`, `DuplicateKey`) rejects a repeated key and the keys `totalCount`, `help` and `recovery`, which would duplicate a root key. `AgentResponse`'s KV path renders such items as an `error: kv_validation_failed` body and sets `is_error`; NAPI `renderKv` throws `kv_validation_failed: …`.
  - **Breaking:** a TOON type name `recovery` is rejected (`ToonError::InvalidTypeName`) like `totalCount` and `help`, since `AgentResponse` appends a `recovery` table.
  - Recovery hints: `recovery[N]:` with `  tool: suggestedParams: { k: v } — reason` lines → the table `recovery[N]{tool,params,reason}:` with `params` as the quoted compact JSON object (`"{\"user\":\"alice\"}"`, `"{}"` when empty) and `reason` `null` when absent.
  - Status: `tool:        x` / `key:   value  [DEGRADED: reason]` → `tool: x`, `description: …` and the table `items[N]{key,value,health,reason}:` (`items: []` when empty). `health` is `ok`, `degraded`, `error`, or `null` when the item has no health signal, so `Health::Ok` and `None` no longer render identically.
  - Errors: `message` is quoted when §7.2 requires it. `Error::InvalidInput(m)` / `Error::NotFound(m)` render `error: invalid_input` / `error: not_found`, `message: m`, `exit_code: 1` — the `DomainError` shape — instead of `error: Invalid input: m`.
  - Partial success: the summary is quoted (`partial_success: "2 completed, 1 failed, 1 skipped"`), `completed`/`skipped` are inline arrays (`completed[2]: a,b`), and the failed operations' recovery hints are a `recovery[N]{tool,params,reason}:` table instead of a `help[N]:` block.
  - `render_already_done`: `status:    already_done` → `status: already_done`; `operation`/`summary` quoted per §7.2.
- serde_json's `float_roundtrip` feature is enabled with the `serde` feature, so deserialized floats are correctly rounded.

## 0.1.1

- Dependency updates
  - `michi-toon` → `0.2.0`

## Merged history: michi-toon 0.2.0

- `render_toon()` now returns `Result<String, ToonError>` instead of `String`. Previously, invalid input (row/field arity mismatches, oversized rows, structural characters in names) degraded silently to a best-effort or malformed string; now every validation failure is surfaced as an explicit `Err`. The loose `render()` function (the pre-`ToonDocument` code path with debug/release-divergent behavior) has been removed — `render_toon()` and `list()` are the only entry points, both routed through `ToonDocument`'s validated construction path.

  Callers must add `?` or explicit error handling at `render_toon()` call sites.

  `ToonDocument` — the proof-carrying type this validated construction path routes through (`ToonDocument::validate(&opts)` fallibly constructs, `.render()` infallibly renders) — is re-exported from the `toon` module, so callers who want the validate/render split explicitly don't need to go through `render_toon()`.

## Merged history: michi-resilience 0.1.1

- `nextRetryDelay` now rejects `NaN` and `Infinity` for `jitterFactor` and `jitterSeed` with a clear error. Previously, these values silently zeroed jitter and caused synchronized retry storms. `RetryConfig::new` is hardened the same way for Rust callers.

# michi-core

## 0.7.1

- Docs and doc comments match the 0.7 code: the READMEs, crate and module docs, and the Node package's API tables list every export (`parseRetryAfter`, `nextRetryDelay`, `isRetryableStatus`, `AgentResponse.renderFor`, `.hasHumanContent`), and several comments that described removed behavior are corrected. No behavior changes.

## 0.7.0

- Released together with the `michi` fixed group.

## 0.6.0

- Breaking: michi-core now uses `compact_str` 0.10 (was 0.8). `toon::Value::Str` holds a 0.10 `CompactString` and `From<CompactString> for Value` takes one, so code that builds these from its own `CompactString` must upgrade `compact_str` to 0.10. Conversions from `&str` and `String` are unchanged.
- Breaking for users of the `schemars` feature: michi-core now implements schemars 1.x's `JsonSchema` (was 0.8). Upgrade your own `schemars` dependency to 1.x; `schemars::gen::SchemaGenerator` and `schemars::schema::Schema` are now `schemars::SchemaGenerator` and `schemars::Schema`.
  
  Generated schemas are JSON Schema 2020-12 instead of draft-07. They describe the same wire form, spelled differently: subschemas under `$defs`, `const` for unit variants, `prefixItems` for tuples, and a `$ref` beside `default` without an `allOf` wrapper.

## 0.5.0

- Fix review findings: a per-object byte budget for Node's `AgentResponse`, tiny floats that rendered `0`, schemas that disagreed with serde, and retry defaults that claimed a timeout was safe to retry.
  
  - @orin-dx/michi: one `AgentResponse` now holds at most 64 MiB of strings across all its calls, not 64 MiB per call (65 separate 1 MiB hints were accepted and rendered 68 MB). Replacing `items`, `kvItems`, `truncationHint` or `humanContent` gives the old value's bytes back. Over the total, a call throws `cumulative byte total N exceeds maximum of 67108864` and leaves the builder unchanged. The constructor's `typeName` and `emptyState`'s `typeName` are capped at 4 KiB, `humanContent` at 1 MiB.
  - michi-core, @orin-dx/michi: floats needing more than 255 decimal places no longer render `0`. Without `decimalsVal`, `1e-300` and `5e-324` render as themselves. New `KvValue::FULL_PRECISION` (255) never rounds, in TOON and the JSON companions; a smaller decimals count still rounds as asked.
  - michi-core: `RecoveryHint` deserialization accepts floats of any size (tiny ones such as `1e-300` were rejected) and integers past `i64::MAX` that have an exact float form, so a hint's own serialized floats (`Float(1e19, 0)`) read back. An integer past `i64::MAX` that would be rounded (`18446744073709551615`) is still rejected; send it as a string. The schema says so in its description.
  - michi-core: the JSON Schemas of `KvValue::Int`, `toon::Value::Int`, `toon::Value::UInt` and `KvValue::Float`'s decimals state the bounds serde enforces.
  - michi-core: `DomainError::new` defaults `retryable` to `false` for `Timeout` and `ExternalFailure` (it was `true`): the request may have taken effect. `retryable` means calling again is expected to help without repeating an effect; pass `.retryable(true)` when the operation is idempotent. `RateLimited` and `Unavailable` stay retryable. A `Timeout` not marked retryable is the new `ErrorClass::Ambiguous` (it was classed `Transient` through the old default), and carries the hint `MAY_HAVE_TAKEN_EFFECT_HINT` until `.retryable(true)` removes it. `is_retryable_status` documents that 502 and 504 are safe to retry only for idempotent requests.
  - michi-core, @orin-dx/michi: `parse_retry_after` treats a pre-1970 HTTP-date as a past date (zero delay), rejects unpadded or signed digit fields such as `+8:+9:+7`, and saturates delay-seconds past `u64::MAX` instead of returning `None`.
  - michi-core: rendering recovery params with repeated-key de-duplication is linear (40,000 params took 1.77 s).
  - michi-core: `MAX_CELLS` counts one cell per row when a list has no fields, so `toon::list` of millions of empty items is rejected with `TooManyCells`.
- michi-pipeline: retries and resume no longer repeat an effect that may already have landed.
  
  - **Breaking:** `ExecutionError::Failed` has a third field, `effect: Effect` (`NotApplied` or `Unknown`). Build it with the new constructors: `failed(msg)` (not retryable, effect unknown, the default), `refused(msg)` (not retryable, not applied), `rejected(msg)` (retryable, not applied) or `transient(msg)` (retryable, effect unknown). `ExecutionError::effect()` reports the effect of any error.
  - **Breaking:** a non-idempotent step that fails with an unknown effect (`failed(msg)`, an HTTP 500 or other 5xx) now ends `OutcomeUnknown(StepFailed)`, not `Failed`, and resume holds it until verified. Before, resume ran it again and could commit twice. Use `refused(msg)` for failures that certainly had no effect.
  - **Breaking:** `ReplayClass::Definitive` is replaced by `Refused` (not applied) and `Ambiguous` (effect unknown).
  - **Breaking:** `ExecutionError::refused` counts as healthy for the circuit breaker (like an HTTP 4xx), so validation failures no longer open it. `PlanFingerprint` parses only 16 lowercase hex digits (it used to accept a leading `+` and uppercase).
  - Precheck: a `Replay::Prechecked` step implements `Step::precheck`, which runs before every attempt and answers `Precheck::Applied` (the step ends `CompletedEarlier` without running), `NotApplied` (it runs) or `Unknown` (it doesn't; `NotRun(Unverified)` if nothing ran yet). Resume uses it instead of a manual verdict.
  - Durable starts: `ExecOptions::durable_starts(observer)` takes a `DurableObserver`, and a step runs only after its `record_start` saves the in-flight marker; if that fails or panics, the step ends `NotRun(StartNotRecorded)` without being invoked. This closes the gap where a crash rebuilt a step that ran as `Pending`.
  - Records saved by 0.4 still load: a `Failed` error without `effect` reads as `Unknown`, and the attempt class `definitive` reads as `Ambiguous`, so resume holds such steps instead of re-running them.
  
  michi-core: `UnknownCause::StepFailed` (rendered `step_failed`) and `NotRunReason::{Unverified, StartNotRecorded}` (rendered `unverified`, `start_not_recorded`), on enums that were already `#[non_exhaustive]`. `@orin-dx/michi`'s `renderPipeline` accepts the three new reasons.

## 0.4.0

- michi-core: public types can now grow in 0.4.x without breaking callers.
  
  - **Breaking:** `toon::Value`, `KvValue` and `RetryConfigError` are `#[non_exhaustive]`. A `match` on them outside michi-core needs a `_` arm.
  - **Breaking:** `FailedOp`, `PartialSuccess`, `KvItem`, `StatusItem`, `RecoveryHint`, `pipeline::PipelineStep` and `pipeline::Pipeline` are `#[non_exhaustive]`. Struct literals no longer compile outside michi-core; use the constructors below. Fields stay public, so reading and assigning them still works.
  - New constructors: `FailedOp::new(operation, reason)` with `.recovery(hint)`, `PartialSuccess::new(completed, failed, skipped)` and `PartialSuccess::default()`, `KvItem::new(key, value)`, `StatusItem::new(key, value)` with `.health(health)`, `PipelineStep::new(id, name, status)`, `Pipeline::new(id, steps)`. `RecoveryHint::new(tool)` is unchanged.
    - Before: `KvItem { key: "id".into(), value: KvValue::Int(1) }`
    - After: `KvItem::new("id", KvValue::Int(1))`
  - **Breaking (paths):** the crate root re-exports `resilience` and `truncate` items by explicit list instead of `pub use resilience::*` and `pub use truncate::*`. Every path that worked before still works (`michi_core::RetryConfig`, `michi_core::already_done`, `michi_core::truncate`, …); items added to those modules later appear only under their module.
  - `RetryConfig` and `toon::ToonOptions` implement `PartialEq`, `PipelineStep` and `Pipeline` implement `PartialEq` and `Eq`.
  - More functions are `#[must_use]` (for example `RecoveryHint::new`, `Hint::as_str`, `IdempotencyKey::as_str`), so discarding their result now warns.
- michi-core: values survive every wire, and invalid TOON is never emitted.
  
  - **Breaking (`schemars`):** `StatusResponse`, `StatusItem`, `Health`, `toon::ToonOptions`, `ErrorClass`, `KvItem`, `OutputFormat`, `pipeline::Pipeline` and `pipeline::PipelineStep` no longer implement `JsonSchema`. Nothing serializes them, so their schemas described no real wire form. Every remaining schema describes what serde emits.
  - `toon::Value::Float` NaN and ±Infinity now round-trip through serde as `"NaN"`, `"Infinity"` and `"-Infinity"` (they serialized as `null` and failed to read back), the same form `KvValue` uses. Rendered TOON still shows `null`, and the schema lists the three strings.
  - `Value::from(f32)` and f32 fields in `toon::list()` keep the f32's shortest digits: `0.1f32` renders `0.1`, not `0.10000000149011612`.
  - `AgentResponse` on the KV path with no items keeps a total count: `kv_items(vec![]).total_count(0)` renders `totalCount: 0` in the text and JSON body instead of an empty body.
  - `KvValue` NaN and ±Infinity round-trip through serde as named strings; the JSON companions write `null` for them, matching the TOON body, instead of the quoted strings `"NaN"`/`"inf"`.
  - `empty_state` with a reserved type name (`totalCount`, `help`, `recovery`) renders a `toon_validation_failed` error block instead of invalid TOON. New `try_empty_state` and `try_empty_state_with_hints` return the `ToonError`.
  - `RecoveryHint::param` replaces an existing key in place instead of adding a second one, and every renderer writes a repeated key once, so the JSON never holds duplicate keys.
  - MCP serde accepts what MCP allows: only `content` is required, `isError` defaults to `false`, `structuredContent` may be absent or `null`, and `annotations`/`audience` are optional. `structuredContent` is omitted when empty, and serializing a non-object `structuredContent` is an error instead of invalid MCP. New `CallToolResult::try_new` and `StructuredContentError`.
  - With `serde`, `DomainError` serializes to exactly the object `render_json()` writes, and its `schemars` schema describes that object.
  - GitHub annotations escape `%` (and `:`/`,` in the title), so a literal `%0A` in a message no longer becomes a line break.
  - `parse_retry_after` accepts the RFC 850 and asctime HTTP-date forms as well as IMF-fixdate.
  - `next_retry_delay` rounds `base_delay` and `retry_after` up to whole milliseconds, so a sub-millisecond delay no longer becomes zero.
  - Correction to the 0.3.0 notes: `toon::list()` rows missing a field get a `null` cell, not an empty one.
- Packaging and docs: the minimum supported Rust version is now 1.93 (was 1.96). The crates and npm packages ship their license text, michi-pipeline ships a README, docs.rs builds michi-core with the `serde`, `schemars` and `miette` features, the READMEs' links work on crates.io and npm, and michi-pipeline re-exports `RetryConfig`, `RetryConfigError`, `RetryBudget` and `IdempotencyKey`, so configuring retries needs no direct michi-core dependency.
- Report what a list cap cut, and keep an error's cause chain apart from its hints.
  
  - michi-core: new `TruncationOutcome<T>` (`kept`, `total_before`, `truncated()`, `omitted()`) reports what a cap on a list cut, however you chose the survivors. `truncate_by_count(items, max)` keeps the first `max`; `truncate_by_size(items, budget, size_of)` keeps items in order while your `size_of` (bytes, tokens, …) fits the budget. Wrap your own selection with `TruncationOutcome::new(kept, total_before)`. `hint(guidance)` returns `showing 2 of 5; {guidance}`, or `None` when nothing was cut.
  - michi-core: `AgentResponse::truncation(&outcome, guidance)` sets `totalCount` and adds that hint in one call, so the two always agree. Pass `outcome.kept` to `.items(...)` as before.
  - michi-core: `DomainError` has a `causes` field and a `.cause(text)` builder for why an error happened, separate from `hints` (what to do next). `render()` writes them as `causes[N]: …` after `message`; `render_json()`, serde and the JSON Schema add a `causes` array (`[]` when empty); with `miette`, `help()` lists them as `caused by: …` lines after the hints.
  - michi-core: `DomainError::from_error(code, &err)` takes the message from `err` and one cause per `source()` link, outermost first, skipping a link whose text the previous one already includes (common with `#[error("…: {0}")]`).
  - @orin-dx/michi: `renderDomainError` takes an optional fifth argument, `causes` (string array), rendered as `causes[N]: …`. Each cause is capped at 1 MiB and counts toward the call's 64 MiB limit.

## 0.3.1

- First published release. 0.3.0 was never published; its changes, listed below, ship in 0.3.1.
- Joined the `michi` group at this version.

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

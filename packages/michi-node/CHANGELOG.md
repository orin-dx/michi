# michi-node

## 0.7.0

- Released together with the `michi` fixed group.

## 0.6.0

- Released together with the `michi` fixed group.

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

## 0.4.0

- Oversize inputs are now rejected quickly instead of costing seconds and gigabytes.
  
  - michi-core: TOON lists are capped at 1,000 fields (`toon::MAX_FIELDS`), 1,000,000 cells (`toon::MAX_CELLS`) and a 256-char truncation hint (`toon::MAX_TRUNCATION_HINT_CHARS`). `ToonOptions::validate()` and `toon::list()` return the new `ToonError::TooManyFields`, `TooManyCells` and `TruncationHintTooLong` variants, and `AgentResponse` reports them as a `toon_validation_failed` error block with `isError` set. Valid lists render exactly as before, and `AgentResponse` no longer copies its rows on each render.
  - @orin-dx/michi: collection arguments are checked before they are converted, and each call is limited to 1,000,000 cells and 64 MiB of strings (1 MiB per cell, hint or message; 4 KiB per type name, field name, key or truncation hint). Rejecting a 100,001-row `renderToon` input now takes well under a millisecond, and a JS string reused across many cells is counted every time it is used. TypeScript types are unchanged.
  - michi-pipeline: `resume` and `Plan::new` are linear in the number of steps and dependencies; resuming 50,000 steps takes about 20 ms instead of seconds.
- @orin-dx/michi: four new exports cover the rest of the michi-core rendering surface.
  
  - `renderPartialSuccess(completed, failed, skipped)` renders a partial-success report: the summary, `completed[N]`, the `failed[N]{operation,reason}:` table, `skipped[N]`, then the failed operations' recovery hints as one `recovery[N]{tool,params,reason}:` table. A failed op is `{ operation, reason, recovery?: { tool, reason? } }`.
  - `renderPipeline(steps)` renders `step[N]{id,name,status,reason}:`. `status` and `reason` are typed `StepStatus` labels (`completed`, `outcome_unknown` with `timeout`, `skipped` with `prerequisite_failed`, …); an unknown status, a missing or unknown reason, or a reason on a status that takes none throws.
  - `emptyStateWithHints(typeName, hints)` is `emptyState` plus a `help[N]: …` block, and throws `toon_validation_failed: …` for a reserved type name like `emptyState`.
  - `alreadyDone(stored)` classifies an idempotency lookup: `{ done: true, result }` for any stored string, `{ done: false }` for `null` or `undefined`.
  - All four check their inputs before converting them: at most 10,000 entries per list, 4 KiB per id, operation, status or reason label, 1 MiB per name, reason or hint, and 64 MiB of strings per call.
- @orin-dx/michi now ships native binaries for linux-arm64 (glibc and musl) and Intel macOS.
- @orin-dx/michi: no silent rounding and no invalid TOON.
  
  - `emptyState` throws `toon_validation_failed: …` for a reserved `typeName` (`totalCount`, `help`, `recovery`), like `renderToon`. It returned invalid TOON before.
  - `renderKv`, `renderStatus` and `AgentResponse.kvItems` keep a float exact when `decimalsVal` is absent: `1e-7` renders `1e-7` and `1.23456789` stays `1.23456789`. They rounded to 6 places, so small values rendered `0`. An explicit `decimalsVal` still rounds.
  - `AgentResponse` on the KV path with no items keeps its total count (`totalCount: 0`) instead of rendering an empty body.
  - michi-core's fixes to recovery hints (no duplicate keys), empty states, Retry-After parsing and sub-millisecond delays apply here too; see the michi-core changelog.
- Packaging and docs: the minimum supported Rust version is now 1.93 (was 1.96). The crates and npm packages ship their license text, michi-pipeline ships a README, docs.rs builds michi-core with the `serde`, `schemars` and `miette` features, the READMEs' links work on crates.io and npm, and michi-pipeline re-exports `RetryConfig`, `RetryConfigError`, `RetryBudget` and `IdempotencyKey`, so configuring retries needs no direct michi-core dependency.
- Report what a list cap cut, and keep an error's cause chain apart from its hints.
  
  - michi-core: new `TruncationOutcome<T>` (`kept`, `total_before`, `truncated()`, `omitted()`) reports what a cap on a list cut, however you chose the survivors. `truncate_by_count(items, max)` keeps the first `max`; `truncate_by_size(items, budget, size_of)` keeps items in order while your `size_of` (bytes, tokens, …) fits the budget. Wrap your own selection with `TruncationOutcome::new(kept, total_before)`. `hint(guidance)` returns `showing 2 of 5; {guidance}`, or `None` when nothing was cut.
  - michi-core: `AgentResponse::truncation(&outcome, guidance)` sets `totalCount` and adds that hint in one call, so the two always agree. Pass `outcome.kept` to `.items(...)` as before.
  - michi-core: `DomainError` has a `causes` field and a `.cause(text)` builder for why an error happened, separate from `hints` (what to do next). `render()` writes them as `causes[N]: …` after `message`; `render_json()`, serde and the JSON Schema add a `causes` array (`[]` when empty); with `miette`, `help()` lists them as `caused by: …` lines after the hints.
  - michi-core: `DomainError::from_error(code, &err)` takes the message from `err` and one cause per `source()` link, outermost first, skipping a link whose text the previous one already includes (common with `#[error("…: {0}")]`).
  - @orin-dx/michi: `renderDomainError` takes an optional fifth argument, `causes` (string array), rendered as `causes[N]: …`. Each cause is capped at 1 MiB and counts toward the call's 64 MiB limit.

## 0.3.1

- First published release. `@orin-dx/michi` installs a release-built native binary through its per-platform packages (`@orin-dx/michi-darwin-arm64`, `@orin-dx/michi-linux-x64-gnu`, `@orin-dx/michi-linux-x64-musl`) and ships its TypeScript declarations.
- Joined the `michi` group at this version.

## 0.3.0

- Versions in lockstep with the `michi-core` and `michi-pipeline` crates (0.3.0); the exports now come from `michi-core`'s `napi` feature.
- Every rendered string is TOON 4.4: empty lists render `key: []`, null cells `null`, empty strings `""`, help blocks inline (`help[N]: a,b`), KV/error/status/recovery blocks as TOON, and no output ends with a newline. See `crates/michi-core/CHANGELOG.md` for the full list.
- `renderToon`/`AgentResponse` quote, escape and canonicalize values per the spec; integers above `Number.MAX_SAFE_INTEGER` are not affected (inputs are validated JS safe integers).
- An `AgentResponse` whose rows fail validation now reports `isError: true` in `toCallToolResult()` and `renderJson()`.
- Truncated cells say how many characters were cut and no longer suggest `full=true`; pass the new `JsToonOptions.truncationHint` / `AgentResponse.truncationHint()` to add your own instruction.
- `renderKv` throws `kv_validation_failed: …` for duplicate or reserved keys (`totalCount`, `help`, `recovery`).

## 0.2.0

- Every numeric argument crossing the NAPI boundary is now validated through a dedicated newtype (`JsCount`, `JsInt`, `JsRanged`/`JsDecimals`/`JsRetryCount`/`JsHttpStatus`, `JsFloat`, `JsUnitInterval`, `JsDelayMillis`) instead of ad hoc per-call-site coercion. Generated TypeScript signatures are unchanged (still plain `number`), but out-of-domain values that were previously silently coerced now throw:

  - `truncate`'s `maxChars` and `JsToonOptions`/`JsAgentResponse`'s `totalCount`: a negative or fractional value was previously clamped to `0` (or, for `JsToonOptions.totalCount`, silently dropped); now rejected with a thrown error.
  - `JsToonValue.decimalsVal`: a value outside `[0, 20]` was previously clamped into range; now rejected.
  - `nextRetryDelay`'s `maxRetries`/`attempt`, `baseDelayMs`/`maxDelayMs`, `jitterFactor`/`jitterSeed`, and `retryAfterMs`: all seven parameters now go through the same validating newtypes and reject non-finite, negative, fractional (where integral), or overflow-inducing values with a consistent error message, instead of the previous ad hoc validation that only covered a subset of these parameters.
  - `isRetryableStatus`'s `status`: a value outside `[100, 599]` was previously coerced to `0` via a fallible cast with a silent fallback; now rejected.
  - `JsToonValue.intVal` widened from a 32-bit range to the full JS-safe-integer range (`±9007199254740991`), so large integers (for example millisecond timestamps) round-trip losslessly through `renderToon`/`renderKv` instead of being truncated to 32 bits.

- NAPI `renderToon()` rejections are now prefixed `toon_validation_failed: ` (e.g. `toon_validation_failed: row 0 has 1 values but 2 fields declared`). Previously the rejection reason was the bare validation message. Any caller matching on exact error text needs to update for the new prefix.
- `renderToon`, `renderKv`, `renderStatus`, and `JsAgentResponse#items`/`#kvItems` now reject a `JsToonValue` whose declared `type` doesn't match its payload, instead of silently defaulting. Previously, `{ type: 'int' }` with `intVal` omitted rendered as `0`, and an unrecognized `type` (e.g. a typo like `'integer'`) silently rendered as `null`/dropped the value. Both cases now throw a descriptive error (e.g. `JsToonValue: type is "int" but intVal is missing`, or `JsToonValue: unknown type "integer": expected "str", "int", "float", "bool", or "null"`) before any rendering happens.

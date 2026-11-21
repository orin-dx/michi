# michi-core

## 0.11.0

_2026-11-21 · [compare](https://github.com/orin-dx/michi/compare/michi@0.10.0...michi@0.11.0)_

### Minor Changes

- **Breaking:** types that have a rule across their fields now keep those fields private, so the rule holds for the value's whole life.

  - A `RetryConfig` can no longer get a zero `max_delay` after `try_new` (that turned a server's `Retry-After: 60` into an immediate retry).
  - A `DomainError` can no longer have its code or retry flag changed behind the builder (that left a retryable timeout without its "verify first" hint).
  - `CallToolResult.structured_content` is an `Option<JsonObject>`, checked when it is built in every build, so every result serializes.
  - `TruncationOutcome`'s total can never fall below the items kept.

  Changes to your code:

  - `DomainError`: read fields with `code()`, `message()`, `causes()`, `hints()`, `get_recovery()`, `get_retry_after()` and `is_retryable()`; classify with `class()`. `render()` and `render_json()` become `render(OutputFormat::Text)` and `render(OutputFormat::Json)`. `retry_after` takes `impl Into<Option<Duration>>`. Use the new `context(prefix)` instead of writing to `message`.
  - `michi_core::Error` and `michi_core::Result` are removed; nothing in michi produced them. Replace `michi_core::Error::Domain(e)` with `e`.
  - `RetryConfig`: read with `max_retries()`, `base_delay()`, `max_delay()` and `jitter_factor()`; build with `new`, `try_new` or `default`.
  - `TruncationOutcome`: `outcome.kept` becomes `outcome.kept()` (or `into_kept()` to take the items), `outcome.total_before` becomes `total_before()`. Build one with `new`, `truncate_by_count` or `truncate_by_size` as before.
  - `Pipeline::new(id, steps)` becomes `Pipeline::new(steps)`; a `Pipeline` has no `id`.
  - `CallToolResult::new` takes `Option<JsonObject>`: wrap structured content as `Some(JsonObject::try_new(text)?)`. `CallToolResult::try_new` is removed.
  - `StatusResponse::with_hints` is renamed `hints`; `render_already_done` takes `&[Hint]`.
  - The `telemetry` module and its `NoopProvider` are removed (nothing used them).

  Also new: `ErrorCode` and `ErrorClass` implement `Hash`; `ErrorCode` implements `Display` (its label) and `FromStr` (error type `UnknownErrorCode`) and has `ALL`; `StepStatus` has `ALL` and `from_labels` (error type `StepLabelError`); `DomainError` implements `PartialEq`; `JsonObject` has `as_str()` and `into_string()`. `AgentResponse::new`, `StatusResponse::new`, `Hint::new` and `IdempotencyKey::new` are `#[must_use]`.
- One `AgentResponse` can now hold every kind of outcome, so a batch with results, per-operation outcomes and an error is one valid TOON document, one JSON object and one MCP result.

  - `error(DomainError)` adds the error block, which renders first.
  - `partial_success(name, ps)`, `status(s)`, `pipeline(name, p)` and `already_done(operation, summary)` add sections.
  - Their hints and recovery hints join the response's single `help` and `recovery` block.
  - If the response has an error and an invalid section, or two blocks that write the same root key, it reports both on every output (text, JSON, `render_for`, `to_call_tool_result`): your error leads and stays the JSON `error`, and each validation failure follows its causes as `toon_validation_failed: <message>` (or `kv_validation_failed`). `isError` stays true. With several failures and no caller error, the first failure is the `error` and the rest are its causes.
- **Breaking:** `AgentResponse::hints(vec)` now replaces only the hints you added with `hint()` and `hints()`. A `truncated_section()`'s `showing N of M` line is kept (before, `hints()` dropped it), and so are the hints of `status()`, `partial_success()` and `error()` blocks. If you called `hints()` to clear everything, that line and those hints now stay.
- **Breaking:** the JSON your tools return changes in three places.

  - Error JSON: `DomainError::render(OutputFormat::Json)`, its serde form and its `schemars` schema are now the response envelope `{"body", "error": {"code", "message", "causes", "retryable"}, "hints", "recovery", "isError"}`. Before it was `{isError, error, message, causes, retryable, hints, recovery}`, where `error` was the code and `recovery` an object or `null`. Read `error.code` where you read `error`, and `recovery[0]` where you read `recovery` (`recovery` is always an array). The text render is unchanged.
  - Validation failures: a section or key-value failure is now an `error` object, `{"code": "toon_validation_failed", "message": …, "causes": [], "retryable": false}` (or `kv_validation_failed`). Before it was `isError: true` with no `error` object. The text is unchanged.
  - `totalCount` in a section's JSON follows the text's rule: it appears when the total is larger than the rows returned, is `0` for an empty list, and is left out otherwise (before, it was the declared value whenever one was set). The budget roll-up's `total` column follows the same rule. `Section::total()` still returns the value as given.
- **Breaking:** list rendering is now `AgentResponse`'s job, and the older standalone paths are removed so `totalCount`, `help[]` and the truncation hint appear once for the whole response.

  - `ToonOptions` describes the list alone: `hints`, `total_count` and `truncation_hint` (and their builders) are removed. `render_toon` writes the list and the `truncated` block. `render_kv(items)` loses its `total_count` and `hints` parameters.
  - `empty_state`, `empty_state_with_hints`, `try_empty_state`, `try_empty_state_with_hints` and the `empty` module are removed. Use `AgentResponse::new(name)` (plus `.hint(..)`) where you called `empty_state`.
  - One empty list everywhere: an empty `Toon` section (so an untouched `AgentResponse`), an empty `Pipeline` and a `StatusResponse` with no items now render `name: []` and `totalCount: 0` (`step: []` and `items: []` for the last two). Before, an empty `Toon` section rendered `name: []` alone. An empty `Kv` section still renders nothing.

  Migration:

  - `render_toon(&ToonOptions::new(..).total_count(Some(n)).hints(h))` becomes `AgentResponse::new(name).section(Section::toon(name, fields, rows).total_count(Some(n))).hint(..)` rendered with `render(OutputFormat::Text)`.
  - For hints on a standalone list, use `append_hints(&mut out, &hints)`.
- **Breaking:** `PartialSuccess` now renders as one table with a row per operation, and can no longer list one operation as both completed and failed.

  - Before: a quoted summary line, then separate `completed`, `failed` and `skipped` tables. Now: `partial_success[N]{index,operation,status,reason}:`, rows sorted by index. The failed operations' recovery table is unchanged. An empty report renders `partial_success: []` and `totalCount: 0`. Anything that parses the old text must read the new table.
  - `PartialSuccess::new` becomes `PartialSuccess::try_new`, which returns `DuplicateOpIndex` when an index repeats. Its fields are private: read them with `completed()`, `failed()` and `skipped()`.
  - `OpRef` is split into `CompletedOp` (no reason) and `SkippedOp`. Replace `OpRef::new` with `CompletedOp::new` or `SkippedOp::new`, and `PartialSuccess::new(c, f, s)` with `PartialSuccess::try_new(c, f, s)?`.
- Error text that reaches an agent now has more credentials removed. `DomainError::from_error` and `from_error_with_causes` use the new `michi_core::redact::redact_common_shapes`, which, on top of URL userinfo, redacts:

  - the value of `token=`, `access_token=`, `api_key=`, `password=` and similar pairs, quoted (`'x'`), as JSON (`"password": "x"` or JSON-escaped `\"x\"`), and after the fullwidth `＝` and `：`
  - `Key: value` headers such as `X-Api-Key:`, `Cookie:` and `Set-Cookie:`
  - the credential after a scheme word (`Authorization: Basic ...`, `Bearer`, `Token`, `Digest`)
  - `ghp_`, `github_pat_`, `npm_` and `pypi-` tokens

  Details:

  - Prose such as `token: ...` is redacted too, which is the safe direction.
  - `redact_url_userinfo` also handles JSON-escaped (`https:\/\/`) and percent-encoded (`https%3A%2F%2F`) URLs, and a `"` inside a password.
  - `redact_secrets` is now idempotent: it copies an existing `[REDACTED]` through, so a secret such as `RED` no longer turns the marker into `[[REDACTED]ACTED]` on a second pass. A known secret that starts with or contains the marker (`[REDACTED]private-tail`) is still removed whole; redacting twice is a no-op unless a secret contains `[`.
  - This is best effort: the docs list what is and is not covered.
- **Breaking:** a response's text and JSON can no longer contradict each other, and more names are reserved so caller data cannot pose as michi's own keys.

  - A section's `truncated` is derived (its total is above its count). The budget's `truncated` is also true whenever a section was cut.
  - A successful response can no longer render an `error:` / `message:` document from caller data.
  - A provenance label `fingerprint` is always rejected (it read exactly like the real fingerprint).
  - `budget`, `sections`, `provenance`, `error`, `message`, `causes` and `exit_code` join `totalCount`, `help`, `recovery` and `truncated` as reserved. `truncated` was already a reserved type name and is now a reserved KV key too. As a type name (`render_toon`, `list`), a KV key (`render_kv`, `kv_items`) or a section name, a reserved name is a validation error, and the message lists the reserved keys. Rename the offender, for example a `message` field becomes `commit_message`.
  - `Section`'s fields are private: read them with `name()`, `body()`, `count()`, `total()` and `truncated()`. `Section::truncated(bool)` and `Section::returned_count(n)` are removed. A text section that stands for several items is `Section::text_with_count(name, body, n)`, and `SectionBody::Text` is now `Text { text, count }`.
  - `SectionCost` and `BudgetSummary::section()` are removed, and the budget's JSON no longer has a `sections` array (the response's `sections` carries those counts). Drop `.section(SectionCost::new(..))` on budgets.
  - `truncated_section` now takes `(name, fields, &outcome, row, guidance)` and builds the rows from `outcome.kept()`: `.truncated_section(Section::toon(name, f, rows), &outcome, g)` becomes `.truncated_section(name, f, &outcome, |item| row_of(item), g)`. Drop `.truncated(..)` on sections.

## 0.10.0

_2026-11-15 · [compare](https://github.com/orin-dx/michi/compare/michi@0.9.0...michi@0.10.0)_

### Minor Changes

- Added `BudgetSummary`, `SectionCost`, `ProvenanceBlock` and `ProvenanceEntry`: plain types for a response-level budget report and a provenance block, with builder methods, camelCase `serde::Serialize` behind the `serde` feature and `schemars::JsonSchema` behind `schemars`. They are not attached to `AgentResponse` yet.
- michi-core: `AgentResponse` keeps its body in one list of sections. `items()` and `kv_items()` are shorthand that append a section named for the response's type, so calling both renders both bodies in call order instead of silently dropping one, as long as their root keys differ. Every section is a root key of the document: a repeated key (two `items()` calls, two `kv_items()` with a shared key, a name like `budget`, a repeated provenance label) renders the `toon_validation_failed` error naming the key, with `isError` true, instead of invalid TOON. For several lists use `.section(Section::toon(distinct_name, ..))`. The same applies to `@orin-dx/michi`'s `items()` and `kvItems()`, which render one list per response. A `Text` section now renders as one escaped string value (`name: "..."`), not raw prose.

  Breaking: removed `AgentResponse::render_toon()`, `render_kv()`, `total_count()` and `truncation()`, and NAPI `renderToon()`, `renderKv()` and `totalCount()`. A response with only `items()` or `kv_items()` renders the same text, with these exceptions: a `totalCount` line renders only when the total is larger than the rows returned (so an empty list no longer prints `totalCount: 0`), and `render(Json)` / `structuredContent` gain a `sections` array next to `body`.

  Migration: `AgentResponse::new(..).items(..).hints(..).render_toon()` becomes `.render(OutputFormat::Text)`; `.total_count(n)` becomes `Section::toon(name, fields, rows).total_count(Some(n))` passed to `.section(..)`; `.items(rows, f).truncation(&outcome, g)` becomes `.truncated_section(Section::toon(name, f, rows), &outcome, g)`. In JavaScript, `renderToon()` and `renderKv()` become `renderText()`, and `totalCount(n)` becomes the optional third argument of `items(rows, fields, n)` (second of `kvItems(items, n)`).
- `PartialSuccess` reports each operation's position in your batch, so `results[i]` and `ops[i]` stay aligned in the rendered output. `completed` and `skipped` are `Vec<OpRef>`, a new type with `index`, `operation` and an optional `reason` (a machine token such as `budget_exhausted`), and `FailedOp` gains `index`.

  Breaking: the rendered report changes. `completed[2]: a,b` becomes a table, `completed[2]{index,operation}:` with one `0,a` row per op, and `failed` and `skipped` are `{index,operation,reason}` tables too. A skipped op with no reason renders `null` in that column. Anything that parses the old text must read the new tables. In Rust, build the vectors from `OpRef::new(index, operation)` (add `.reason(token)` for a skipped op) instead of strings, and pass the index as the first argument of `FailedOp::new(index, operation, reason)`. In Node, `renderPartialSuccess` takes `{ index, operation, reason? }` objects for `completed` and `skipped` instead of strings, and each `FailedOp` needs an `index`.
- michi-core: `AgentResponse::budget(BudgetSummary)` and `AgentResponse::provenance(ProvenanceBlock)` attach a response-level `budget:` block and `provenance:` block. A response now renders its sections, then `budget:`, `provenance:`, `help[]` and `recovery[]`; the per-section roll-up under `budget:` appears only when the response has two or more sections. `render(Json)` and `to_call_tool_result().structured_content` gain `sections`, `budget` and `provenance` keys, present only when set (a response with no section, budget or provenance keeps exactly `body`, `hints`, `recovery` and `isError`). When two or more sections cut cells or report a `totalCount`, each is now one merged TOON table with a leading `section` column (`truncated[N]{section,row,field,removed}:`, `totalCount[N]{section,total}:`) instead of repeating a root key, which TOON forbids.
- michi-core: `AgentResponse` can hold several blocks. Add them with `.section(Section)`; a `Section` is a TOON list, a key-value block or a single text value, escaped like any TOON string (`Section::toon`, `Section::kv`, `Section::text`), optionally with a pre-truncation `total_count`. Sections render in the order you add them, then `help[]` and `recovery[]`.

## 0.9.0

- Released together with the `michi` fixed group.

## 0.8.0

- michi-core: truncation is reported out of band, and `DomainError::from_error` no longer copies the cause chain.
  
  **Truncated TOON cells.** A cell longer than `max_cell_len` used to end in text such as `(233 more chars — use full=true)`, and a cell whose own data ended the same way was indistinguishable from a real cut. A cut cell now keeps `max_cell_len - 1` chars and ends in a plain `…`, and every cut is listed in a new `truncated[N]{row,field,removed}:` block after the list (`row` is 0-based, `removed` is the chars cut). `truncation_hint` (`truncationHint` in Node) still carries your "get it in full" instruction, now as one extra line in `help[]` (`Cut cells are listed in the truncated block; use full=true for the full value`) that appears only when a cell was cut. `AgentResponse` merges that line into its own hints, in the text, JSON and MCP outputs. `truncated` is now a reserved list type name, like `totalCount`, `help` and `recovery`. `truncate()`, `truncate_inline()` and the Node `truncate()` are unchanged: they still append the in-band suffix, so use them for a single free-text string, not for TOON cells.
  
  Migrate: anything that parsed `(N more chars` out of a cell should read the `truncated` block instead; snapshots of truncated output need regenerating; rename a list type called `truncated`.
  
  **`DomainError::from_error`** now keeps only the error's own `Display` as `message`. The `source()` chain, which often holds URLs with tokens or file paths, stays out of agent-visible output. To keep the chain as `causes`, call the new `DomainError::from_error_with_causes` (the old behaviour). The top-level message is still shown, so make sure it is safe too.
  
  **Docs.** `docs/spec/02-toon-format.md` has a new trust model section: michi guarantees one value cannot create extra lines, fields or cells, and does not stop prompt injection in data; hints, recovery text, error messages, type and field names come from the tool author, never from data.
- Docs: the README now scopes its TOON claims to what was measured (token savings on flat, uniform 100-row lists; retrieval accuracy within noise on single-hop questions), says TOON here is for models reading tool output, and states that `michi-pipeline` resume is at-least-once per step unless the step passes its idempotency key to a downstream that deduplicates. Adds a plain License section.
- New `michi_core::redact` module: `redact_secrets(text, &secrets)` replaces known secret values with `[REDACTED]`, and `redact_url_userinfo(text)` removes the `user:password@` part of URLs in a text (a password containing `@` or `/` is removed whole). Both are best-effort and do not find secrets in general. `DomainError::from_error` and `from_error_with_causes` now apply `redact_url_userinfo` to the error text they copy, so a URL with credentials in a library error no longer reaches the agent; an `@` later in a URL's path makes the redaction over-reach rather than leak.
- `truncate()` reports the cut outside the text. The old form appended `" (N more chars — use hint)"` inside the returned string, which data could imitate, the same flaw cut TOON cells had. Now `truncate(content, max_chars)` returns `Truncated { content, original_chars, removed }`: text that fits comes back unchanged, and a longer text keeps its first `max_chars - 1` chars and ends in a plain `…`. `Truncated::hint(guidance)` gives the `cut N of M chars; …` line for the response's `help[]`.
  
  Migrate: drop the `hint` argument; read `removed` (or call `hint(guidance)`) instead of parsing the suffix; replace `truncate_inline(c, n, h)` with `truncate(c, n).content` and report the cut yourself. In Node, `truncate(content, maxChars)` now returns `{ content, originalChars, removed, wasTruncated }` instead of a string.
- The Node package's TypeScript types lose their `Js` prefix: `ToonValue`, `ToonOptions`, `KvItem`, `RecoveryHint`, `StatusItem`, `FailedOp`, `PipelineStep`, `AlreadyDone`, `ContentBlock`, `Annotations`, `CallToolResult` and `Truncated` (the `AgentResponse` class already had none). Runtime behavior is unchanged except that error messages name the new type (`ToonValue must be an object`). `renderDomainError`'s `code` is now typed as the union of the nine error labels (`'invalid_input' | 'not_found' | ...`) instead of `string`; the runtime still rejects anything else. Migrate: rename the imported types.

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

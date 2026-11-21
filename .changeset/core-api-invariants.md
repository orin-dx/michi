---
michi-core: minor
---

**Breaking:** types that have a rule across their fields now keep those fields private, so the rule holds for the value's whole life.

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

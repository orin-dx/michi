---
michi-core: minor
michi-pipeline: minor
michi-node: minor
---

michi-core: a value's rules now hold for its whole life, because the types with a rule across their fields keep those fields private. A `RetryConfig` can no longer get a zero `max_delay` after `try_new`, which turned a server's `Retry-After: 60` into an immediate retry. A `DomainError` can no longer have its code or retry flag changed behind the builder, which left a retryable timeout without its "verify first" hint. A `PartialSuccess` can no longer list one operation as both completed and failed. `CallToolResult.structured_content` is now an `Option<JsonObject>`, checked when it is built in every build, so every result serializes. The wire output is unchanged.

Breaking: `DomainError`'s fields are private. Read them with `code()`, `message()`, `causes()`, `hints()`, `get_recovery()`, `get_retry_after()` and `is_retryable()`, and use `class()` to classify. `render()` and `render_json()` are now `render(OutputFormat::Text)` and `render(OutputFormat::Json)`. `retry_after` takes `impl Into<Option<Duration>>`, and the new `context(prefix)` replaces writing to `message`. `michi_core::Error` and `michi_core::Result` are removed; nothing in michi produced them. `RetryConfig`'s fields are private; read them with `max_retries()`, `base_delay()`, `max_delay()` and `jitter_factor()`, and build one with `new`, `try_new` or `default`. `PartialSuccess::new` is now `PartialSuccess::try_new`, which returns `DuplicateOpIndex` when an index repeats. Its fields are read with `completed()`, `failed()` and `skipped()`. `OpRef` is split into `CompletedOp` (no reason) and `SkippedOp`. `Pipeline` has no `id`, so `Pipeline::new(steps)` takes only the steps. `CallToolResult::new` takes `Option<JsonObject>`; `CallToolResult::try_new` is removed (use `JsonObject::try_new`). `StatusResponse::with_hints` is renamed `hints`, and `render_already_done` takes `&[Hint]`.

Also: `ErrorCode` and `ErrorClass` implement `Hash`, `ErrorCode` implements `Display` (its label), and `DomainError` implements `PartialEq`.

michi-pipeline: `StepId` and `RunId` implement `FromStr` and `Borrow<str>`, so `"fetch".parse::<StepId>()` works and a map keyed by id can be looked up with a `&str`. `AttemptRecord` implements `Eq`. `RunRecord::to_pipeline()` no longer carries the run id (read it with `run_id()`). An ambiguous timeout's `DomainError` now carries one verify-first hint, not none: the conversion used to write `retryable = false` past the builder and so dropped core's hint.

michi-node: Breaking: `renderPartialSuccess` takes `CompletedOp[]` and `SkippedOp[]` in place of `OpRef[]`. It throws when an `index` appears twice across the three lists, and when a completed operation has a `reason`. Before, a duplicate rendered as two outcomes and the reason was dropped without a word.

Migration: replace `e.code`, `e.message`, `e.retryable` and the other field reads with the getters. Replace `e.render()` with `e.render(OutputFormat::Text)`, and replace `cfg.max_delay` and the other config fields with `cfg.max_delay()`. Replace `PartialSuccess::new(c, f, s)` with `PartialSuccess::try_new(c, f, s)?`, `OpRef::new` with `CompletedOp::new` or `SkippedOp::new`, and `Pipeline::new(id, steps)` with `Pipeline::new(steps)`. Wrap structured content as `Some(JsonObject::try_new(text)?)`. Replace `michi_core::Error::Domain(e)` with `e`.

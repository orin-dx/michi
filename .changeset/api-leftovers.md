---
michi-core: minor
---

michi-core: Breaking: `TruncationOutcome`'s fields are private, so its total can never fall below the items kept. Read them with `kept()`, `into_kept()` and `total_before()`, and build one with `new`, `truncate_by_count` or `truncate_by_size` as before. The unused `telemetry` module and its `NoopProvider` are removed. `AgentResponse::new`, `StatusResponse::new`, `Hint::new` and `IdempotencyKey::new` are `#[must_use]`.

Migration: replace `outcome.kept` with `outcome.kept()` (or `outcome.into_kept()` to take the items) and `outcome.total_before` with `outcome.total_before()`. Drop any use of `michi_core::telemetry`.

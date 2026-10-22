---
michi-core: minor
michi-node: minor
---

Fix review findings: a per-object byte budget for Node's `AgentResponse`, tiny floats that rendered `0`, schemas that disagreed with serde, and retry defaults that claimed a timeout was safe to retry.

- @orin-dx/michi: one `AgentResponse` now holds at most 64 MiB of strings across all its calls, not 64 MiB per call (65 separate 1 MiB hints were accepted and rendered 68 MB). Replacing `items`, `kvItems`, `truncationHint` or `humanContent` gives the old value's bytes back. Over the total, a call throws `cumulative byte total N exceeds maximum of 67108864` and leaves the builder unchanged. The constructor's `typeName` and `emptyState`'s `typeName` are capped at 4 KiB, `humanContent` at 1 MiB.
- michi-core, @orin-dx/michi: floats needing more than 255 decimal places no longer render `0`. Without `decimalsVal`, `1e-300` and `5e-324` render as themselves. New `KvValue::FULL_PRECISION` (255) never rounds, in TOON and the JSON companions; a smaller decimals count still rounds as asked.
- michi-core: `RecoveryHint` deserialization accepts floats of any size (tiny ones such as `1e-300` were rejected) and integers past `i64::MAX` that have an exact float form, so a hint's own serialized floats (`Float(1e19, 0)`) read back. An integer past `i64::MAX` that would be rounded (`18446744073709551615`) is still rejected; send it as a string. The schema says so in its description.
- michi-core: the JSON Schemas of `KvValue::Int`, `toon::Value::Int`, `toon::Value::UInt` and `KvValue::Float`'s decimals state the bounds serde enforces.
- michi-core: `DomainError::new` defaults `retryable` to `false` for `Timeout` and `ExternalFailure` (it was `true`): the request may have taken effect. `retryable` means calling again is expected to help without repeating an effect; pass `.retryable(true)` when the operation is idempotent. `RateLimited` and `Unavailable` stay retryable. A `Timeout` not marked retryable is the new `ErrorClass::Ambiguous` (it was classed `Transient` through the old default), and carries the hint `MAY_HAVE_TAKEN_EFFECT_HINT` until `.retryable(true)` removes it. `is_retryable_status` documents that 502 and 504 are safe to retry only for idempotent requests.
- michi-core, @orin-dx/michi: `parse_retry_after` treats a pre-1970 HTTP-date as a past date (zero delay), rejects unpadded or signed digit fields such as `+8:+9:+7`, and saturates delay-seconds past `u64::MAX` instead of returning `None`.
- michi-core: rendering recovery params with repeated-key de-duplication is linear (40,000 params took 1.77 s).
- michi-core: `MAX_CELLS` counts one cell per row when a list has no fields, so `toon::list` of millions of empty items is rejected with `TooManyCells`.

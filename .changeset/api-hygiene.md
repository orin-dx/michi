---
michi-core: minor
---

michi-core: public types can now grow in 0.4.x without breaking callers.

- **Breaking:** `toon::Value`, `KvValue` and `RetryConfigError` are `#[non_exhaustive]`. A `match` on them outside michi-core needs a `_` arm.
- **Breaking:** `FailedOp`, `PartialSuccess`, `KvItem`, `StatusItem`, `RecoveryHint`, `pipeline::PipelineStep` and `pipeline::Pipeline` are `#[non_exhaustive]`. Struct literals no longer compile outside michi-core; use the constructors below. Fields stay public, so reading and assigning them still works.
- New constructors: `FailedOp::new(operation, reason)` with `.recovery(hint)`, `PartialSuccess::new(completed, failed, skipped)` and `PartialSuccess::default()`, `KvItem::new(key, value)`, `StatusItem::new(key, value)` with `.health(health)`, `PipelineStep::new(id, name, status)`, `Pipeline::new(id, steps)`. `RecoveryHint::new(tool)` is unchanged.
  - Before: `KvItem { key: "id".into(), value: KvValue::Int(1) }`
  - After: `KvItem::new("id", KvValue::Int(1))`
- **Breaking (paths):** the crate root re-exports `resilience` and `truncate` items by explicit list instead of `pub use resilience::*` and `pub use truncate::*`. Every path that worked before still works (`michi_core::RetryConfig`, `michi_core::already_done`, `michi_core::truncate`, …); items added to those modules later appear only under their module.
- `RetryConfig` and `toon::ToonOptions` implement `PartialEq`, `PipelineStep` and `Pipeline` implement `PartialEq` and `Eq`.
- More functions are `#[must_use]` (for example `RecoveryHint::new`, `Hint::as_str`, `IdempotencyKey::as_str`), so discarding their result now warns.

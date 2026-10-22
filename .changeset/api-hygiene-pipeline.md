---
michi-pipeline: minor
---

michi-pipeline: public types can now grow in 0.4.x without breaking callers.

- **Breaking:** the re-exported `RetryConfigError` is `#[non_exhaustive]`, so a `match` on it needs a `_` arm.
- **Breaking:** `RunRecord::to_pipeline()` returns michi-core's `Pipeline`, whose `Pipeline` and `PipelineStep` are now `#[non_exhaustive]`: read their fields as before, but build them with `Pipeline::new` and `PipelineStep::new`.
- `FnStep` implements `Debug` (showing its replay class) without requiring the closure to be `Debug`. The re-exported `RetryConfig` implements `PartialEq`.
- `RunRecord` accessors (`outcome()`, `steps()`, `step()`, `error()`, `render()`, …) are `#[must_use]`, so discarding their result now warns.

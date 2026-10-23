---
michi-pipeline: patch
---

michi-pipeline: `RunRecord::needs_verification()` and `StepRecord::awaits_verification()` no longer list idempotent steps, which resume re-runs anyway. Each `StepRecord` now records its step's `Replay` (`StepRecord::replay()`), set by the executor and by `RunRecord::from_parts` from the plan; a record without it (saved by hand or by an older version) still counts as not idempotent. Prechecked steps stay listed, since their precheck may not be able to tell and a caller's verdict takes precedence. With `serde`, the record serializes `replay` when known and reads records without it.

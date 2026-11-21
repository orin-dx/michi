---
michi-pipeline: minor
---

**Breaking:** `RunRecord::to_pipeline()` no longer carries the run id (read it with `run_id()`), and `RunRecord::render` of a run with no steps now renders `step: []` and `totalCount: 0`.

Also new: `StepId` and `RunId` implement `FromStr` and `Borrow<str>`, so `"fetch".parse::<StepId>()` works and a map keyed by id can be looked up with a `&str`. `AttemptRecord` implements `Eq`.

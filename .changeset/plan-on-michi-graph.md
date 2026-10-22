---
michi-pipeline: patch
---

michi-pipeline: `Plan` validation and ordering now come from `michi-graph`. Execution order, the fingerprint and every `PlanViolation` are unchanged, with two exceptions: a step that can never run because it sits between two cycles (depending on one, depended on by another) is no longer listed in `PlanViolation::Cycle`, which now lists exactly the steps on a cycle; and `PlanViolation` gains `TooManySteps` for plans over `u32::MAX - 1` steps (it is `#[non_exhaustive]`, so existing matches keep compiling).

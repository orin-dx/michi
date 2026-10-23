---
michi-pipeline: minor
---

michi-pipeline: poll steps. `poll_step(schedule, check)` calls an async check on a fixed `PollSchedule` (default: at once, then after 2 s, 4 s, 8 s, 16 s, then every 30 s) until it answers `Readiness::Ready`, returns a non-retryable error, or the step's deadline (its own or its deadline group's) or cancellation stops it. The whole poll is one attempt: it never draws from the retry budget, it runs until its deadline rather than the attempt timeout (which bounds each check instead), and when time runs out it fails with `ExecutionError::DeadlinePassed` naming the last observation. Supporting additions: `Step::attempt_bound()` returning `AttemptBound::{Timeout, Deadline}`, `StepContext::attempt_deadline()` and `StepContext::attempt_timeout()`.

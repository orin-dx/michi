---
michi-pipeline: minor
michi-core: minor
---

michi-pipeline: retries and resume no longer repeat an effect that may already have landed.

- **Breaking:** `ExecutionError::Failed` has a third field, `effect: Effect` (`NotApplied` or `Unknown`). Build it with the new constructors: `failed(msg)` (not retryable, effect unknown, the default), `refused(msg)` (not retryable, not applied), `rejected(msg)` (retryable, not applied) or `transient(msg)` (retryable, effect unknown). `ExecutionError::effect()` reports the effect of any error.
- **Breaking:** a non-idempotent step that fails with an unknown effect (`failed(msg)`, an HTTP 500 or other 5xx) now ends `OutcomeUnknown(StepFailed)`, not `Failed`, and resume holds it until verified. Before, resume ran it again and could commit twice. Use `refused(msg)` for failures that certainly had no effect.
- **Breaking:** `ReplayClass::Definitive` is replaced by `Refused` (not applied) and `Ambiguous` (effect unknown).
- **Breaking:** `ExecutionError::refused` counts as healthy for the circuit breaker (like an HTTP 4xx), so validation failures no longer open it. `PlanFingerprint` parses only 16 lowercase hex digits (it used to accept a leading `+` and uppercase).
- Precheck: a `Replay::Prechecked` step implements `Step::precheck`, which runs before every attempt and answers `Precheck::Applied` (the step ends `CompletedEarlier` without running), `NotApplied` (it runs) or `Unknown` (it doesn't; `NotRun(Unverified)` if nothing ran yet). Resume uses it instead of a manual verdict.
- Durable starts: `ExecOptions::durable_starts(observer)` takes a `DurableObserver`, and a step runs only after its `record_start` saves the in-flight marker; if that fails or panics, the step ends `NotRun(StartNotRecorded)` without being invoked. This closes the gap where a crash rebuilt a step that ran as `Pending`.
- Records saved by 0.4 still load: a `Failed` error without `effect` reads as `Unknown`, and the attempt class `definitive` reads as `Ambiguous`, so resume holds such steps instead of re-running them.

michi-core: `UnknownCause::StepFailed` (rendered `step_failed`) and `NotRunReason::{Unverified, StartNotRecorded}` (rendered `unverified`, `start_not_recorded`), on enums that were already `#[non_exhaustive]`. `@orin-dx/michi`'s `renderPipeline` accepts the three new reasons.

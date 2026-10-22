---
michi-pipeline: minor
---

michi-pipeline: run records no longer misreport a run, and retries follow the server and spread out.

- **Breaking:** `RunRecord::from_parts(run_id, saved_fingerprint, &plan, steps)` now takes the plan and returns `Result<RunRecord, ResumeError>`. It refuses a fingerprint that is not the plan's (`FingerprintMismatch`) and a saved step the plan lacks (`UnknownStep`), lists a plan step with no saved record as `Pending`, and keeps the **last** record saved for a step, so the documented observer pattern (`StepRecord::in_flight` on start, the finished record on finish) rebuilds a finished step as finished instead of in flight.
- **Breaking (serde):** a serialized `RunRecord` carries `plan_steps`, and deserializing fails if `steps` does not list that many distinct steps. A record missing steps used to read as `Succeeded`. Records saved by 0.3 need `"plan_steps": <number of plan steps>` added.
- `RunRecord::outcome()` is `Succeeded` only when every plan step is listed and succeeded. A step a rebuilt record shows as `Running` makes the outcome `OutcomeUnknown` and is a root cause.
- `RunRecord::needs_verification()` lists every step resume holds for verification: `OutcomeUnknown`, `Panicked` and `Running` (it listed only `OutcomeUnknown`). New `StepRecord::awaits_verification()`.
- `RunRecord::error()` is `None` only for a successful run: with no root cause it names the first step that did not succeed (a cancelled run, a pending step).
- A step that returns `ExecutionError::Cancelled` while the run was not cancelled now ends `Failed` (a root cause; dependents name it in `caused_by`) instead of `Skipped(Cancelled)`. Resume runs it again.
- A server's `retry_after` (HTTP 429/503 or `CircuitOpen`, also inside `StepFailed`) is waited in full even past `RetryConfig::max_delay`, which used to cut a 120 s hint to 30 s. If the wait would pass the step deadline, the step stops retrying at once (`step deadline reached`).
- Retries get per-retry jitter by default, derived from the run id, step id and attempt, so steps that fail together no longer retry in lockstep and a run's delays stay reproducible. `ExecOptions::jitter_seed(x)` still fixes one seed for everything.
- HTTP 4xx responses other than 408 and 429 no longer count as circuit-breaker failures (the downstream answered); they reset the failure count like a success. A caller's bad requests can no longer open the breaker for a whole key.
- `ExecOptions::step_deadline(Duration::MAX)` or a huge `ExecStep::deadline` means no deadline instead of panicking the step with "overflow when adding duration to instant".
- `RunId::unique` ids are `{prefix}-{millis}-{pid}-{counter}`: the new separator stops two processes from printing the same id (pid 1388 with counter 1152 and pid 50000 with counter 0 both printed `…-12kw0`).

# michi-pipeline

## Unreleased

- One replay rule decides both whether `CircuitBreaker::call` re-invokes a step and whether the returned error says replay is safe. A timeout (the breaker's own, or one the step returns, also when wrapped in `StepFailed`) and HTTP 502 or 504 are ambiguous: the attempt may have committed. The breaker re-invokes the step after one only if the step opts in with the new `Step::is_idempotent()` (default `false`), or `FnStep::idempotent()` for closures. HTTP 429 and 503 mean "not processed" and are still retried for every step, so a step that may have committed must not report them. Circuit-open and cancellation never ran the step. Other errors are unchanged.
- **Breaking:** for a non-idempotent step, an ambiguous outcome now comes back as the new `ExecutionError::AmbiguousOutcome { source }` instead of a bare `Timeout` or `Http { status: 502 | 504 }`. That includes the `source` of the `StepFailed` that `execute_pipeline` and `execute_pipeline_parallel` return. `is_retryable()` is `false` for it. Its `DomainError` keeps the inner code, is not retryable, has no `retry_after`, and carries a hint telling the agent to verify whether the operation completed before retrying. An idempotent step's ambiguous outcome is still returned bare and stays retryable.
- The `StepFailed` to `DomainError` conversion now keeps the inner error's hints and recovery hint. Before, it rebuilt the error and dropped them.
- **Breaking:** `execute_pipeline` rejects duplicate step ids with `DuplicateStepId` before running anything, the same as `execute_pipeline_parallel`. Before, it ran them.
- **Breaking:** `execute_pipeline` resets every step to `Pending` at the start of a run. When a step fails, every later step is now marked `Skipped` instead of being left alone. Before, statuses from an earlier run survived: re-running a pipeline that started `[Completed, Completed]` with a failing first step left `[Failed, Completed]`, and the second step never ran.
- `execute_pipeline_parallel` rejects duplicate step ids with `DuplicateStepId` before running anything, including ids no edge mentions.
- Cycle detection is iterative, so very large graphs (100,000 steps) no longer overflow the stack.
- `CircuitBreaker` keeps phase, probe and failure count in one state machine. An outcome from a call admitted before the breaker last changed state is ignored: it can no longer reopen a recovered breaker, extend the open window or reset the failure count.
- Depends on `michi-core` (exact version) instead of `michi-core` and `michi-resilience`; versions in lockstep with it.

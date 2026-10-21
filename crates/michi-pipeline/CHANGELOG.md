# michi-pipeline

## Unreleased

- A step is re-invoked after a timeout only if it opts in with the new `Step::is_idempotent()` (default `false`), or `FnStep::idempotent()` for closures. A timed-out attempt may already have committed, so retrying it could repeat a mutation. Errors a step returns itself still follow `is_retryable()`.
- `execute_pipeline_parallel` rejects duplicate step ids with `DuplicateStepId` before running anything, including ids no edge mentions.
- Cycle detection is iterative, so very large graphs (100,000 steps) no longer overflow the stack.
- `CircuitBreaker` keeps phase, probe and failure count in one state machine. An outcome from a call admitted before the breaker last changed state is ignored: it can no longer reopen a recovered breaker, extend the open window or reset the failure count.
- Depends on `michi-core` (exact version) instead of `michi-core` and `michi-resilience`; versions in lockstep with it.

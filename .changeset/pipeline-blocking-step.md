---
michi-pipeline: minor
---

michi-pipeline: blocking steps. `step_blocking(|ctx| -> Result<(), ExecutionError>)` (with `.idempotent()`, like `step_fn`) runs each attempt on Tokio's blocking thread pool, for steps that shell out to CLIs. A blocking closure keeps running after the executor stops waiting (a timeout, cancellation, a dropped run), so the closure's `ctx.cancellation()` fires at that point too, and the abandoned attempt's effect is unknown. A non-idempotent blocking step never starts a second closure while an earlier one with the same idempotency key is still running in the process: such an attempt fails with an unknown effect without calling the closure.

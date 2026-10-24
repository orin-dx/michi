# michi-pipeline

Async pipeline execution for [michi](https://github.com/orin-dx/michi) on Tokio. Define steps, validate them into a plan, run it sequentially or in parallel, and get back a run record saying what happened to each step.

It handles retries with a retry budget, per-attempt timeouts, shared deadlines for groups of steps, keyed circuit breakers, per-key concurrency limits, cancellation, a progress observer, and resume from a saved record. Resume is at-least-once per step unless the step hands its idempotency key to a downstream that deduplicates (see the [execution spec](https://github.com/orin-dx/michi/blob/main/docs/spec/08-pipeline-execution.md)). Step statuses and the record's TOON rendering come from [`michi-core`](https://crates.io/crates/michi-core).

## Install

```bash
cargo add michi-pipeline
cargo add tokio --features macros,rt
```

The michi crates release together on one version, and `michi-pipeline` pins `michi-core` and `michi-graph` to that exact version. Update all of them in the same step (`cargo update -p michi-core -p michi-pipeline -p michi-graph`); updating one alone fails to resolve.

The `serde` feature adds `Serialize`/`Deserialize` to the run record, step records, errors and ids, so you can persist them.

## Example

```rust
use std::time::Duration;

use michi_pipeline::{
    execute, step_fn, ExecOptions, ExecStep, Plan, RetryBudget, RetryConfig, RunId, RunOutcome, StepId,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fetch = StepId::try_new("fetch")?;
    let plan = Plan::new(vec![
        ExecStep::new(fetch.clone(), "Fetch data", step_fn(|_ctx| async { Ok(()) })),
        ExecStep::new(
            StepId::try_new("upload")?,
            "Upload",
            step_fn(|ctx| async move {
                // The same key on every attempt and on resume: pass it to a deduplicating downstream.
                let _key = ctx.idempotency_key();
                Ok(())
            }),
        )
        .depends_on(fetch),
    ])?;

    let options = ExecOptions::new()
        .retry(RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(5), 0.2))
        .retry_budget(RetryBudget::new(0.1, 10, 100))
        .attempt_timeout(Duration::from_secs(10));

    let record = execute(&plan, RunId::try_new("run-1")?, &options).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    print!("{}", record.render());
    Ok(())
}
```

`RetryConfig`, `RetryBudget` and `IdempotencyKey` are re-exported from `michi-core`, so you don't need it as a direct dependency to configure retries.

## Failures and resume

A failure says two things: whether retrying could help, and whether the effect may have landed. Pick the constructor that matches:

| Constructor | Retried | Effect | Use it when |
| --- | --- | --- | --- |
| `ExecutionError::failed(msg)` | no | unknown | you can't vouch for the effect (the default) |
| `ExecutionError::refused(msg)` | no | not applied | you rejected the work before doing anything |
| `ExecutionError::rejected(msg)` | yes | not applied | a dependency turned the request away; try later |
| `ExecutionError::transient(msg)` | idempotent or prechecked steps only | unknown | a transport hiccup mid-request |

A step that is not idempotent and fails with an unknown effect ends `outcome_unknown`, and `resume` holds it until you verify it. A step that can check for itself is `Replay::Prechecked`: its `precheck` runs before every attempt, and resume uses it instead of asking you.

```rust
use michi_pipeline::{ExecutionError, Precheck, PrecheckFuture, Replay, Step, StepContext, StepFuture};

struct Publish;

impl Step for Publish {
    fn run<'a>(&'a self, _ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin(async { Err(ExecutionError::failed("registry closed the connection")) })
    }

    fn replay(&self) -> Replay {
        Replay::Prechecked
    }

    fn precheck<'a>(&'a self, _ctx: &'a StepContext) -> PrecheckFuture<'a> {
        // Answer `NotApplied` only with proof; a stale read is `Unknown`.
        Box::pin(async { Ok(Precheck::Applied) })
    }
}
```

## Waiting for something

`poll_step` checks on a fixed schedule (by default at once, then after 2 s, 4 s, 8 s, 16 s, then every 30 s) until the check says it's ready, a non-retryable error, the step's deadline or cancellation. The whole poll is one attempt, so it never uses the retry budget, and it runs until its deadline rather than the attempt timeout. A deadline group gives several polls one shared window:

```rust
use std::time::Duration;

use michi_pipeline::{poll_step, ExecOptions, ExecStep, PollSchedule, Readiness, StepId};

let verify = ExecStep::new(
    StepId::try_new("verify-a").unwrap(),
    "Verify a@1.2.3 is visible",
    poll_step(PollSchedule::default(), |_ctx| async {
        Ok(Readiness::NotYet("registry serves 1.2.2".to_string()))
    }),
)
.deadline_group("verify");
let options = ExecOptions::new().group_deadline("verify", Duration::from_secs(360));
```

If the deadline comes first, the step fails with `ExecutionError::DeadlinePassed` and the last thing the check saw, and resume polls again.

## Blocking work

`step_blocking` runs a synchronous closure on Tokio's blocking thread pool, for steps that shell out to `cargo`, `npm` or `twine`. A blocking closure can't be stopped from outside: after a timeout or cancellation the executor stops waiting, but the closure keeps running, so it should watch `ctx.cancellation()`, which fires then, and kill any child process it started. A non-idempotent blocking step never starts a second closure while an earlier one with the same idempotency key (the same step in the same run) is still running.

```rust
use michi_pipeline::{step_blocking, ExecutionError};

let check = step_blocking(|ctx| {
    if ctx.cancellation().is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    // Run the command here, polling `ctx.cancellation()` while it runs.
    Ok(())
})
.idempotent();
```

To make sure a step only runs once its start is saved, implement `DurableObserver::record_start` and pass the observer to `ExecOptions::durable_starts(observer)`.

## Docs

- API reference: [docs.rs/michi-pipeline](https://docs.rs/michi-pipeline)
- Design and semantics: [08-pipeline-execution.md](https://github.com/orin-dx/michi/blob/main/docs/spec/08-pipeline-execution.md)
- Changes: [CHANGELOG.md](https://github.com/orin-dx/michi/blob/main/crates/michi-pipeline/CHANGELOG.md)

## License

FSL-1.1-MIT. See [LICENSE](https://github.com/orin-dx/michi/blob/main/LICENSE).

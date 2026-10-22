# michi-pipeline

Async pipeline execution for [michi](https://github.com/orin-dx/michi) on Tokio. Define steps, validate them into a plan, run it sequentially or in parallel, and get back a run record saying what happened to each step.

It handles replay-safe retries with a retry budget, per-attempt timeouts, keyed circuit breakers, cancellation, a progress observer, and resume from a saved record. Step statuses and the record's TOON rendering come from [`michi-core`](https://crates.io/crates/michi-core).

## Install

```bash
cargo add michi-pipeline
cargo add tokio --features macros,rt
```

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

To make sure a step only runs once its start is saved, implement `DurableObserver::record_start` and pass the observer to `ExecOptions::durable_starts(observer)`.

## Docs

- API reference: [docs.rs/michi-pipeline](https://docs.rs/michi-pipeline)
- Design and semantics: [08-pipeline-execution.md](https://github.com/orin-dx/michi/blob/main/docs/spec/08-pipeline-execution.md)
- Changes: [CHANGELOG.md](https://github.com/orin-dx/michi/blob/main/crates/michi-pipeline/CHANGELOG.md)

## License

FSL-1.1-MIT. See [LICENSE](https://github.com/orin-dx/michi/blob/main/LICENSE).

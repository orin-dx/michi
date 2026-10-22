# Pipeline execution (`michi-pipeline`)

`michi-pipeline` runs a static graph of async steps on Tokio and returns a record of what happened to each one. It is an embeddable library, not an orchestrator: there is no server, no persistence, no scheduling across runs and no dynamic dependencies. What it adds over a plain `JoinSet` is the part agents need: an honest status for every step, retries that never blindly replay work that may already have taken effect, and a resume that continues an interrupted run without repeating finished steps.

This document describes the API that replaced `execute_pipeline`, `execute_pipeline_parallel` and `StepDependencies` (see [06-decisions.md](06-decisions.md), "Pipeline execution redesign"). The design follows the research report in [`docs/research/pipeline-design.md`](../research/pipeline-design.md).

## Identity

- `StepId` names a step. It is chosen by the caller, unique within a plan, and meant to be read and copied by agents, so it is a short slug: 1 to 64 ASCII characters, letters, digits, `_`, `-` and `.`, starting with a letter, digit or `_`. `StepId::try_new` rejects anything else with an `IdError` (`Empty`, `TooLong`, `InvalidChar`). Ids never contain `:`, so the idempotency key below is unambiguous.
- A step's `name` is free-form display text and may repeat.
- `RunId` names one run. It has the same charset. The caller supplies it, because only the caller knows what a run is (one agent request, one job). `RunId::unique(prefix)` builds `{prefix}-{millis}-{pid}-{counter}` (numbers in base 36, each after its own `-`, so two processes cannot print the same digits) without a UUID dependency.
- Reusing a `RunId` for a different plan or different inputs is the caller's mistake and michi cannot detect it: downstream services that deduplicate on the idempotency key would treat the new work as a repeat.

## Plan

A step definition is an `ExecStep`: an id, a name, the ids it depends on, the runner (`impl Step`) and optional per-step overrides (retry policy, per-attempt timeout, total step deadline, breaker key).

`Plan::new(steps)` validates the whole definition before anything runs and returns either an immutable `Plan` or a `PlanError` listing every violation at once:

- `DuplicateStepId`: a later step reuses an earlier step's id.
- `UnknownDependency`: a step depends on an id no step has.
- `SelfDependency`: a step depends on itself.
- `Cycle`: the steps that lie on a dependency cycle, found without recursion, so graph depth is bounded by memory rather than the call stack.
- `TooManySteps`: more than `u32::MAX - 1` steps.

Validation and ordering come from [`michi-graph`](09-graph.md); no michi-graph type appears in this crate's API.

A repeated dependency in one step's list is collapsed. An invalid plan is an `Err` before execution, so it can never be confused with a run that happened and failed.

`Plan::execution_order()` is the dry run: the order sequential mode dispatches steps in, which is topological order with ties broken by declaration order. `Plan::fingerprint()` is a stable 64-bit FNV-1a hash over each step's id and dependency ids, independent of declaration order. Names, runners and options are not part of it, because closures cannot be hashed.

## Steps

```rust
pub trait Step: Send + Sync {
    fn run<'a>(&'a self, ctx: &'a StepContext) -> Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>>;
    fn replay(&self) -> Replay { Replay::NotIdempotent }
}
```

Steps return `()`: they act through side effects and share state through `Arc` captures. Outputs do not flow between steps.

`StepContext` carries the run id, step id, attempt number (0-based), a cancellation token that is a child of the run's token, and the step's deadline if one is set. `idempotency_key()` is `run_id:step_id`. It stays the same across attempts and across resume, so a step that passes it to a deduplicating downstream gets each effect applied once. The attempt number is for logging and backoff only and is never part of the key.

`step_fn(|ctx| async move { ... })` adapts a closure. `FnStep::idempotent()` opts it in to `Replay::Idempotent`.

## Replay rule

Every failed attempt is classified by whether it could have taken effect (RFC 9110 §9.2.2). The same class decides whether the executor retries, which terminal status the step ends in, and whether the returned error says replay is safe.

| Class | Errors | Retried | Terminal status if it is the last outcome |
| --- | --- | --- | --- |
| Not executed | `CircuitOpen` or `Cancelled` returned by the step | `CircuitOpen` after its `retry_after`; `Cancelled` no | `NotRun(CircuitOpen)`; for `Cancelled`, `Skipped(Cancelled)` if the run was cancelled, otherwise `Failed` |
| Rejected | HTTP 429 and 503, `Failed { retryable: true }` | yes, every step | `Failed` |
| Ambiguous | timeouts, HTTP 502 and 504, `Interrupted`, `AmbiguousOutcome` | only `Replay::Idempotent` steps | idempotent: `Failed`; otherwise `OutcomeUnknown` |
| Definitive | everything else | no | `Failed` |

Treating 429 and 503 as "not processed" is a convention, not an RFC guarantee, so a step that may have committed its effect must not report them. It reports 502, 504, a timeout or a non-retryable error instead. A step returns `Cancelled` only when it stopped without effect.

A step that returns `Cancelled` while the run's token has not fired cancelled itself (its own timer or token, say). Nothing upstream asked it to stop, so it is not a consequence of anything: it ends `Failed`, a root cause whose dependents are skipped with `caused_by` naming it. It never took effect, so resume runs it again like any failed step.

A server's `retry_after` (on `Http` or `CircuitOpen`, also inside `StepFailed`) is waited in full, even past `RetryConfig::max_delay`: the server said when to come back, and coming back sooner only earns another rejection. `max_delay` caps michi's own exponential backoff (michi-core's `next_retry_delay` keeps that cap; the pipeline applies the server rule on top). The step deadline still bounds the wait: if the wait would end at or past the deadline, the step stops retrying at once and its reason ends `step deadline reached`. Without a deadline, a long hint is waited out, and cancellation interrupts the wait.

A non-idempotent step whose last outcome is ambiguous ends `OutcomeUnknown` and its error is wrapped in `ExecutionError::AmbiguousOutcome`, which is not retryable and converts to a `DomainError` telling the agent to verify whether the operation completed. An idempotent step's ambiguous outcome stays bare and retryable.

## Status model

`michi_core::pipeline::StepStatus` is `#[non_exhaustive]`, as are its reason enums. Each terminal status maps to one recovery verb.

| Status | Meaning | Agent verb |
| --- | --- | --- |
| `Pending` | not started | none yet |
| `Running` | in flight (only seen in records persisted from `on_step_started`) | resume treats it as ambiguous |
| `Completed` | ran and succeeded in this run | none |
| `CompletedEarlier` | succeeded in an earlier attempt of this run, not re-run by resume | none |
| `Failed` | ran and failed, definitively or with a replay-safe error | fix the cause, then resume |
| `OutcomeUnknown(cause)` | may have taken effect: `Timeout`, `Gateway` (502/504), `Cancelled` (dropped mid-flight after cancellation), `Interrupted` (in flight when the process stopped) | verify, then resume |
| `Skipped(PrerequisiteFailed)` | a prerequisite did not complete | fix the root cause, then resume |
| `Skipped(Cancelled)` | cancelled before it took effect | resume |
| `NotRun(CircuitOpen)` | its circuit breaker was open, so it was never invoked | wait, then resume |
| `Panicked` | the step's code panicked: a harness failure, not a step error | fix the bug, verify, then resume |

`Pipeline::render()` emits `step[N]{id,name,status,reason}:` with snake_case labels (`completed_earlier`, `outcome_unknown`, `not_run`, ...) and a reason (`prerequisite_failed`, `cancelled`, `circuit_open`, `timeout`, `gateway`, `interrupted`) or `null`. It stays TOON 4.4 conformant.

## Run record

`execute(&plan, run_id, &opts).await` returns a `#[must_use]` `RunRecord` with private fields and accessors. It never returns an error: a run that happened is a record, however it went.

- `run_id()`, `plan_fingerprint()`, `elapsed()`, `was_cancelled()` (the caller's token fired), `observer_panics()` (observer calls that panicked; see Observer).
- `steps()`: one `StepRecord` per plan step, in declaration order. Each has the step id, name, terminal status, a human-readable `reason`, the final `error`, every attempt (number, replay class of its failure, error text, elapsed time), when it started relative to the run, how long it took, and for a consequential skip the root-cause step (`caused_by`).
- `outcome()`: `Succeeded` only if the record lists every plan step and each is `Completed` or `CompletedEarlier`. Otherwise `OutcomeUnknown` if any step's outcome is unknown (`OutcomeUnknown`, or `Running` in a rebuilt record), else `Cancelled` if the caller cancelled, else `Failed`. A skipped, not-run, pending, running or panicked step therefore never reads as success.
- `root_causes()`: steps that failed in their own right (`Failed`, `OutcomeUnknown`, `NotRun`, `Panicked`, and `Running` in a rebuilt record). `consequential()`: skipped steps. An agent sees one cause instead of N errors.
- `needs_verification()`: every step resume holds until the caller verifies it (unless it is idempotent): `OutcomeUnknown`, `Panicked` and `Running`. These are exactly the steps `ResumeOptions::verified` accepts (`StepRecord::awaits_verification()`).
- `completion_order()`: ids of steps that completed in this run, in the order they finished, so callers can compensate if they choose to.
- `error()`: the first root cause as `ExecutionError::StepFailed`, which converts to `DomainError`. When no step failed in its own right (a cancelled run, or a rebuilt record with pending steps) it names the first step that did not succeed, so `error()` is `None` only for a `Succeeded` run.
- `to_pipeline()` and `render()`: the michi-core `Pipeline` view and its TOON.

A record always holds exactly one `StepRecord` per plan step, in declaration order, and stores the plan's step count, so a record that lost steps cannot read as a success.

With the `serde` feature, `RunRecord`, `StepRecord`, `ExecutionError` and the ids serialize, so callers can persist records. The serialized record carries `plan_steps`; deserializing fails if `steps` does not list that many distinct steps (a missing or repeated step), so a truncated or tampered record is an error rather than a success. Records saved by 0.3 lack `plan_steps` and must be re-saved (or have `"plan_steps": <step count>` added).

`RunRecord::from_parts(run_id, saved_fingerprint, &plan, steps)` rebuilds a record from step records saved one at a time. It returns `ResumeError::FingerprintMismatch` if the saved fingerprint is not the plan's and `ResumeError::UnknownStep` for a saved step the plan lacks. When `steps` holds several records for one step, the last one wins, so an append-only log of `StepRecord::in_flight` on start and the finished record on finish rebuilds each step's latest status. A plan step with no saved record is `Pending`.

## Scheduling

One scheduler loop drives both modes. It keeps a remaining-prerequisite count per step and a ready set ordered by declaration index, and spawns ready steps onto a `tokio::task::JoinSet` owned by the call, so no step outlives `execute` or `resume`.

- `Mode::Sequential` (default): one step at a time, in `Plan::execution_order()`.
- `Mode::Parallel { max_concurrency: NonZeroUsize }`: the loop dispatches ready steps in declaration order while fewer than `max_concurrency` are running. The cap lives in the dispatch loop, not in a semaphore inside spawned tasks, so admission order is declaration order rather than wake order.

`FailurePolicy` decides what a failure does to the rest of the run:

- `SkipDependents` (default): the failed step's transitive dependents are `Skipped(PrerequisiteFailed)` and independent branches keep running. This applies in sequential mode too, which changes the old behaviour of stopping at the first failure.
- `FailFast`: the first failure stops dispatching and cancels the run's token. Running steps get the cancellation grace period to finish, then their attempts are dropped and they end `OutcomeUnknown(Cancelled)` (or `Failed` if idempotent). Steps never started end `Skipped(Cancelled)`.

A step that panics is caught through its `JoinError` and ends `Panicked`. Other steps keep running and the breaker probe it held, if any, is released.

## Cancellation and time

`CancellationToken` wraps tokio-util's token, so tokio-util's 0.x type is not part of michi's public API. `ExecOptions::cancellation(token)` installs the caller's token; the run uses a child of it, and each step gets a child of the run token in its context.

When the token fires, nothing new is dispatched and every step not yet started ends `Skipped(Cancelled)`. A running attempt can watch `ctx.cancellation()` and stop cooperatively by returning `ExecutionError::Cancelled`, which also ends `Skipped(Cancelled)`. If it is still running after the grace period (`ExecOptions::cancel_grace`, default 5 s), its future is dropped and it ends ambiguous. Backoff sleeps race the token, so a cancelled run never waits out a retry delay.

Each attempt runs under `tokio::time::timeout` (`attempt_timeout`, default 30 s). An optional total deadline (`step_deadline`) bounds a step across all its attempts and backoff: no attempt starts or sleeps past it, and the last attempt is cut to the time left. A deadline too far away for the clock to represent (such as `Duration::MAX`) means no deadline; it never panics. All time is `tokio::time`, so tests run on a paused clock.

## Breakers and budgets

Circuit breakers are keyed. A `BreakerRegistry` holds one breaker per key, created on first use with the registry's `BreakerConfig`. A step's key is `ExecStep::breaker_key` (for example, one key per downstream service) and defaults to its step id. Pass the same `Arc<BreakerRegistry>` to every run so samples accumulate across runs; without one, no breaker applies.

Each breaker keeps the existing state machine: phase, probe ownership and failure count behind one mutex, with a generation counter so an outcome from a call admitted under an earlier generation is ignored. A step admitted while the breaker is closed retries per its policy and reports one outcome. A half-open probe gets exactly one attempt. An open breaker never invokes the step, and the step ends `NotRun(CircuitOpen)`. Cancellation and interruption are not reported as breaker failures.

A client error (HTTP 4xx other than 408 and 429) is reported to the breaker as a success: the downstream answered and rejected this one request, which says it is up. Counting a caller's 404s or 400s as failures would let one bad input open the breaker for every caller of that key. 408 and 429 (the downstream is slow or shedding load), 5xx, timeouts and other errors count as failures. This follows Envoy's outlier detection, which ejects a host on consecutive 5xx and resets on any other response.

Retries are jittered. Without `ExecOptions::jitter_seed`, each retry's jitter seed is derived from the run id, step id and attempt number (FNV-1a with a splitmix64 finalizer, mapped to `[0, 1)`), so steps that fail together back off by different amounts instead of retrying in lockstep, while the same run and plan always wait the same delays. `jitter_seed(x)` fixes one seed for every retry of every step, as before.

Retries are budgeted per run with `michi_core::resilience::RetryBudget`, a pure helper modelled on tower's `TpsBudget`: each successful step deposits a fraction of a retry (10% by default), each retry withdraws one, the budget starts with a small reserve (10) and is capped (100). When the budget is empty the step stops retrying and ends with its last outcome. First attempts are never budgeted. `ExecOptions::no_retry_budget()` turns it off.

Retry only at one layer: a step whose own HTTP client already retries should use `RetryConfig` with zero retries here.

`michi-pipeline` re-exports `RetryConfig`, `RetryConfigError`, `RetryBudget` and `IdempotencyKey` from `michi_core::resilience`, so callers configure retries without a direct `michi-core` dependency:

```rust
use std::time::Duration;

use michi_pipeline::{ExecOptions, RetryBudget, RetryConfig};

let options = ExecOptions::new()
    .retry(RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(5), 0.2))
    .retry_budget(RetryBudget::new(0.1, 10, 100));
```

## Observer

`ExecOptions::observer(Arc<dyn StepObserver>)` is called synchronously from the scheduler loop. `on_step_started` fires before a step is spawned and `on_step_finished` fires with each step's terminal `StepRecord`, including skipped steps and, on resume, steps carried over. A closure `Fn(&RunId, &StepRecord)` implements it. Persisting each finished record, plus `StepRecord::in_flight(id, name)` for each started one, lets a caller rebuild the run with `RunRecord::from_parts` after a crash. An append-only log is enough: `from_parts` keeps the last record saved for each step, so a step that started and finished rebuilds as finished. Resume then treats a step still `Running` as ambiguous instead of re-running it blindly.

The observer is caller code, so a panic in it is caught (`std::panic::catch_unwind`) rather than unwinding the scheduler, which would lose the record and drop every running step. The run carries on, the observer is still called for later events, and `RunRecord::observer_panics()` counts the caught panics. A non-zero count means the observer missed events, so progress it persisted may be incomplete. The process's panic hook still runs.

## Resume

`resume(&prior, &plan, &ResumeOptions).await` continues an interrupted run. It returns `Err(ResumeError)` before running anything if the plan's fingerprint differs from the record's (`FingerprintMismatch`), the record names a step the plan lacks (`UnknownStep`), or a step marked verified is not awaiting verification (`NotAwaitingVerification`).

- It always runs under the prior record's `RunId`, so idempotency keys stay stable. There is no way to resume under a new id.
- `Completed` and `CompletedEarlier` steps are never re-run; they become `CompletedEarlier`.
- `Failed`, `Skipped`, `NotRun` and `Pending` steps (and steps missing from the record) run again.
- `OutcomeUnknown`, `Panicked` and `Running` steps run again only if the step is `Replay::Idempotent`. Otherwise they keep their prior record, and their dependents are skipped, until the caller marks them with `ResumeOptions::verified(id, Verification::Applied)` (treated as `CompletedEarlier`) or `Verification::NotApplied` (run again).

Resume indexes the prior record's steps once (every record lists each step once: `from_parts` keeps the last saved record per step and deserializing rejects a repeated step), so preparing a resume is O(N log N) in the step count: 50,000 carried-over steps took 1.6 s with a linear lookup per step and take about 20 ms now. `RunRecord::step(id)` is still a linear scan; index `steps()` yourself to look up many. `Plan::new` likewise drops repeated dependencies with an O(1) check per edge, so a wide fan-in is linear.

Resume is forward recovery and at least once per step: michi has no atomic commit between a step's effect and the record, so a step can run again if the process stopped after its effect but before its record was saved. Effects are exactly once only when the step hands `idempotency_key()` to a downstream that deduplicates. Resume is not incremental recomputation: it does not notice that a completed step's inputs changed.

## Non-goals

Dynamic dependencies, data passing between steps, cross-run concurrency limits, compensation or sagas, durable storage, input-hash memoization and critical-path scheduling. A concise rendering mode (non-completed steps plus a completed count) is not built yet.

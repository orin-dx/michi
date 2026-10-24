# Pipeline execution (`michi-pipeline`)

`michi-pipeline` runs a static graph of async steps on Tokio and returns a record of what happened to each one. It is an embeddable library, not an orchestrator: there is no server, no persistence, no scheduling across runs and no dynamic dependencies. What it adds over a plain `JoinSet` is the part agents need: an honest status for every step, retries that never blindly replay work that may already have taken effect, and a resume that continues an interrupted run without repeating finished steps.

This document describes the API that replaced `execute_pipeline`, `execute_pipeline_parallel` and `StepDependencies` (see [06-decisions.md](06-decisions.md), "Pipeline execution redesign"). The design follows the research report in [`docs/research/pipeline-design.md`](../research/pipeline-design.md).

## Identity

- `StepId` names a step. It is chosen by the caller, unique within a plan, and meant to be read and copied by agents, so it is a short slug: 1 to 64 (`MAX_ID_LEN`) ASCII characters, letters, digits, `_`, `-` and `.`, starting with a letter, digit or `_`. `StepId::try_new` rejects anything else with an `IdError` (`Empty`, `TooLong`, `InvalidChar`). Ids never contain `:`, so the idempotency key below is unambiguous.
- A step's `name` is free-form display text and may repeat.
- `RunId` names one run. It has the same charset. The caller supplies it, because only the caller knows what a run is (one agent request, one job). `RunId::unique(prefix)` builds `{prefix}-{millis}-{pid}-{counter}` (numbers in base 36, each after its own `-`, so two processes cannot print the same digits) without a UUID dependency.
- Reusing a `RunId` for a different plan or different inputs is the caller's mistake and michi cannot detect it: downstream services that deduplicate on the idempotency key would treat the new work as a repeat.

## Plan

A step definition is an `ExecStep`: an id, a name, the ids it depends on, the runner (`impl Step`) and optional per-step settings (retry policy, per-attempt timeout, total step deadline, breaker key, concurrency key, deadline group).

`Plan::new(steps)` validates the whole definition before anything runs and returns either an immutable `Plan` or a `PlanError` listing every violation at once:

- `DuplicateStepId`: a later step reuses an earlier step's id.
- `UnknownDependency`: a step depends on an id no step has.
- `SelfDependency`: a step depends on itself.
- `Cycle`: the steps that lie on a dependency cycle, found without recursion, so graph depth is bounded by memory rather than the call stack.
- `TooManySteps`: more than `u32::MAX - 1` steps.
- `Other { message }`: a michi-graph violation this version does not know by name, with michi-graph's message. `PlanViolation` is `#[non_exhaustive]`.

Validation and ordering come from [`michi-graph`](09-graph.md); no michi-graph type appears in this crate's API.

A repeated dependency in one step's list is collapsed. An invalid plan is an `Err` before execution, so it can never be confused with a run that happened and failed.

`Plan::execution_order()` is the dry run: the order sequential mode dispatches steps in, which is topological order with ties broken by declaration order. `Plan::fingerprint()` is a stable 64-bit FNV-1a hash over each step's id and dependency ids, independent of declaration order. Names, runners and options are not part of it, because closures cannot be hashed.

## Steps

```rust
pub trait Step: Send + Sync {
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a>;
    fn replay(&self) -> Replay { Replay::NotIdempotent }
    /// Looks at the outside world before an attempt. Only consulted for `Replay::Prechecked` steps.
    fn precheck<'a>(&'a self, ctx: &'a StepContext) -> PrecheckFuture<'a> { Box::pin(async { Ok(Precheck::Unknown) }) }
    /// What bounds an attempt: the attempt timeout (default) or the step's deadline (see Poll steps).
    fn attempt_bound(&self) -> AttemptBound { AttemptBound::Timeout }
}
```

Steps return `()`: they act through side effects and share state through `Arc` captures. Outputs do not flow between steps.

`StepContext` carries the run id, step id, attempt number (0-based), a cancellation token that is a child of the run's token, the step's deadline if one is set (the earlier of its own and its group's), when the executor will cut this attempt (`attempt_deadline()`) and the attempt timeout configured for the step (`attempt_timeout()`). `idempotency_key()` is `run_id:step_id`. It stays the same across attempts and across resume, so a step that passes it to a deduplicating downstream gets each effect applied once. The attempt number is for logging and backoff only and is never part of the key.

`step_fn(|ctx| async move { ... })` adapts a closure. `FnStep::idempotent()` opts it in to `Replay::Idempotent`. A prechecked step implements `Step` itself.

`Replay` has three values: `NotIdempotent` (the default), `Idempotent` (repeating the step is harmless) and `Prechecked` (repeating it is not harmless, but its precheck reliably tells whether the effect is already in place).

### Poll steps

`poll_step(schedule, check)` waits for something another step caused, such as a published version becoming visible in a registry. `check` is an async closure that reads the outside world and answers `Readiness::Ready` or `Readiness::NotYet(observed)`, where `observed` describes what it saw.

```rust
use std::time::Duration;

use michi_pipeline::{poll_step, ExecStep, PollSchedule, Readiness, StepId};

let verify = ExecStep::new(
    StepId::try_new("verify-npm-a").unwrap(),
    "Verify a@1.2.3 on npm",
    poll_step(PollSchedule::default(), |_ctx| async {
        // Read the registry; a 404 or an older version is "not yet", not an error.
        Ok(Readiness::NotYet("registry serves 1.2.2".to_string()))
    }),
)
.depends_on(StepId::try_new("publish-npm-a").unwrap())
.deadline_group("verify-npm");
```

- **Schedule.** The first check runs at once. After that, `PollSchedule` waits each of its delays in turn, then a fixed interval. The default is 2 s, 4 s, 8 s and 16 s, then every 30 s. `PollSchedule::new(delays, then_every)` and `PollSchedule::every(interval)` build others. Each delay counts from the end of the previous check. A zero delay is floored to 1 ms, so a poll never spins.
- **One attempt.** The whole poll is one attempt. It never draws from the retry budget, and `RetryConfig` never applies because the poll never returns a retryable error. It is recorded as one `AttemptRecord`.
- **Bounded by the deadline, not the attempt timeout.** The step's `Step::attempt_bound()` is `AttemptBound::Deadline`. The executor lets such an attempt run until the step's deadline, its own or its group's, whichever is earlier, instead of cutting it at the attempt timeout. The attempt timeout bounds each check instead (`StepContext::attempt_timeout()`). Without any deadline, the attempt timeout bounds the whole poll, as it would any attempt. Give a poll step a `deadline` or a `deadline_group`.
- **Stops early, not late.** The poll knows its bound (`StepContext::attempt_deadline()`). When the next check would start at or after it, the poll stops at once, so the executor never has to cut it and record an ambiguous `Timeout`. The error is `ExecutionError::DeadlinePassed`, saying how many checks ran over how long and what the last check observed (`not ready after 4 checks over 14000ms, and the next check would fall past the deadline; last observed: registry serves 1.2.2`). The step ends `Failed`, its dependents are skipped, and resume runs it again.
- **Check errors.** A retryable error, such as a 503 or a check that timed out, counts as not ready yet, and its text becomes the last observation. Any other error ends the step with that error.
- **Cancellation** stops the poll at once, during a check or a wait, with `ExecutionError::Cancelled`, so the step ends `Skipped(Cancelled)` without waiting out the grace period.
- **Replay.** Polling only reads, so the step is `Replay::Idempotent`.

Any `Step` can return `AttemptBound::Deadline` for the same treatment. It should watch `StepContext::attempt_deadline()` and finish before it, because an attempt the executor has to cut ends with `Timeout`, whose effect is unknown.

### Blocking steps

`step_blocking(|ctx| -> Result<(), ExecutionError>)` runs a synchronous closure on Tokio's blocking thread pool (`tokio::task::spawn_blocking`), for work that blocks a thread: shelling out to `cargo publish`, `npm publish` or `twine upload`, or a synchronous client. `.idempotent()` opts it in to `Replay::Idempotent`, like `step_fn`. The closure gets an owned `StepContext`, and its result is the attempt's result. A panic in it ends the step `Panicked`.

Rust cannot stop a blocking closure from outside. When the executor stops waiting for an attempt (the attempt timeout, the cancellation grace period running out, or the whole run being dropped), it drops the await, but the closure keeps running on its thread until it returns. A child process it started keeps running too. The rules follow from that:

- **The closure's token fires when the executor gives up.** `ctx.cancellation()` in the closure is a token for this attempt alone. It fires when the run is cancelled and also when the executor stops waiting for the attempt. A closure that starts a child process should poll `ctx.cancellation().is_cancelled()` (and can watch `ctx.attempt_deadline()`) and kill the child when it fires:

  ```rust
  use std::process::Command;
  use std::time::Duration;

  use michi_pipeline::{step_blocking, ExecutionError};

  let publish = step_blocking(|ctx| {
      let mut child = Command::new("cargo")
          .args(["publish", "-p", "michi-core"])
          .spawn()
          .map_err(|e| ExecutionError::refused(format!("could not start cargo: {e}")))?;
      loop {
          match child.try_wait() {
              Ok(Some(status)) if status.success() => return Ok(()),
              Ok(Some(status)) => return Err(ExecutionError::failed(format!("cargo publish exited with {status}"))),
              Ok(None) if ctx.cancellation().is_cancelled() => {
                  let _ = child.kill();
                  return Err(ExecutionError::failed("cargo publish was killed; it may have uploaded"));
              }
              Ok(None) => std::thread::sleep(Duration::from_millis(100)),
              Err(e) => return Err(ExecutionError::failed(format!("lost track of cargo: {e}"))),
          }
      }
  });
  ```

- **An abandoned attempt's effect is unknown.** It ends with `Timeout` or `Interrupted` like any other attempt the executor cut, so a non-idempotent step ends `OutcomeUnknown` and is never retried in the run.
- **No second closure while the first may still run.** For a step that is not idempotent, a new attempt does not start while an earlier closure with the same idempotency key (`run_id:step_id`) is still running in this process. The replay rules already rule this out within a run. It can happen when `resume` runs in the same process right after a timeout and the caller marks the step `Verification::NotApplied` too early. That attempt does not call the closure. It fails with an unknown effect (`ExecutionError::failed`, saying an earlier attempt is still running), so the step stays `OutcomeUnknown` until the caller verifies again once the first closure has finished.
- **Idempotent steps may overlap.** A retry after a timeout starts a new closure while the old one may still run.
- **Scope of the guard.** It lives in the `BlockingStep` value, so it covers one process. Steps that share one runner through `ExecStep::from_arc` have different step ids and do not block each other.
- **`JoinSet` drop.** Each attempt runs inside the scheduler's `JoinSet`. Dropping the `execute` or `resume` future aborts the async side, but Tokio never aborts a `spawn_blocking` task, so the closure runs to completion and its result is discarded. The closure's token fires, so a closure that watches it can stop early. The runtime waits for blocking threads when it shuts down.

## Replay rule

Every failed attempt carries two independent facts:

- **`retryable`** (`ExecutionError::is_retryable()`): would trying again help?
- **`effect`** (`ExecutionError::effect()`): did the attempt's effect land? `Effect::NotApplied` (it certainly did not) or `Effect::Unknown` (it may have). `Effect` is `#[non_exhaustive]` and has no `Applied` value: a step that knows its effect landed and still failed says so in its message and reports `Unknown`, because resume must not run it again either way.

The default is `Unknown`: an error counts as not applied only when it says so, so an unclassified failure is never treated as safe. `retryable: false` never implies "not applied" (a 400 is not retryable and not applied; a timeout is retryable and unknown).

The executor derives everything from those two facts and the step's `Replay`:

|  | `effect: NotApplied` | `effect: Unknown` |
| --- | --- | --- |
| **retried in this run** | if `retryable` | if `retryable` and the step is `Idempotent` or `Prechecked` |
| **terminal status (last attempt)** | `Failed` | idempotent: `Failed`; otherwise `OutcomeUnknown(cause)` |
| **resume** | runs it again | idempotent: runs it again; prechecked: precheck decides; otherwise held until verified |

What each error means:

| Error | `retryable` | `effect` |
| --- | --- | --- |
| `CircuitOpen` (step never invoked) | yes, after `retry_after` | `NotApplied` |
| `Cancelled` returned by the step (stopped before effect) | no | `NotApplied` |
| HTTP 429, 503 | yes | `NotApplied` |
| HTTP 4xx other than 429, including 408 (the server refused this request) | no | `NotApplied` |
| HTTP 502, 504 | yes | `Unknown` |
| HTTP 500 and other 5xx, and any non-4xx/5xx status reported as an error | no | `Unknown` |
| `Timeout`, `Interrupted` | yes | `Unknown` |
| `Panicked`, `AmbiguousOutcome` | no | `Unknown` |
| `DeadlinePassed` (a deadline group's window closed before the step started, or a poll step ran out of time) | no | `NotApplied` |
| `Failed { message, retryable, effect }` | as given | as given |
| `StepFailed { source, .. }` | as `source` | as `source` |

A step whose 4xx can follow a committed effect (a 409 after a partial write, say) reports `ExecutionError::failed` instead of the raw status. Treating 429 and 503 as "not processed" is a convention, not an RFC guarantee, so a step that may have committed its effect must not report them either.

Four constructors build `Failed`:

| Constructor | `retryable` | `effect` | Use it when |
| --- | --- | --- | --- |
| `ExecutionError::failed(msg)` | no | `Unknown` | the default: something went wrong and you can't vouch for the effect |
| `ExecutionError::refused(msg)` | no | `NotApplied` | you rejected the work before doing anything (validation, a failed precondition) |
| `ExecutionError::rejected(msg)` | yes | `NotApplied` | a dependency turned the request away and asking again later may work |
| `ExecutionError::transient(msg)` | yes | `Unknown` | a transport-level hiccup mid-request; only idempotent or prechecked steps retry it |

`ReplayClass` is the per-attempt summary kept in `AttemptRecord`: `NotExecuted` (circuit open, `Cancelled`), `Rejected` (not applied, retryable), `Refused` (not applied, not retryable) and `Ambiguous` (effect unknown). Records saved by 0.4 call some attempts `definitive`; that reads as `Ambiguous`.

A non-idempotent step whose last error's effect is unknown ends `OutcomeUnknown` with a cause: `Timeout`, `Gateway` (502/504), `Cancelled` (interrupted after cancellation), `Interrupted` (a panic inside a returned error) or `StepFailed` (anything else: `failed`, `transient`, HTTP 500). Its error is wrapped in `ExecutionError::AmbiguousOutcome`, which is not retryable and converts to a `DomainError` telling the agent to verify whether the operation completed. An idempotent step's error stays bare.

A step that returns `Cancelled` while the run's token has not fired cancelled itself (its own timer or token, say). Nothing upstream asked it to stop, so it is not a consequence of anything: it ends `Failed`, a root cause whose dependents are skipped with `caused_by` naming it. It never took effect, so resume runs it again like any failed step.

A server's `retry_after` (on `Http` or `CircuitOpen`, also inside `StepFailed`) is waited in full, even past `RetryConfig::max_delay`: the server said when to come back, and coming back sooner only earns another rejection. `max_delay` caps michi's own exponential backoff (michi-core's `next_retry_delay` keeps that cap; the pipeline applies the server rule on top). The step deadline still bounds the wait: if the wait would end at or past the deadline, the step stops retrying at once and its reason ends `step deadline reached`. Without a deadline, a long hint is waited out, and cancellation interrupts the wait.

## Precheck

Precheck is verification the step performs for itself, before it acts. It only runs for `Replay::Prechecked` steps; a step that is not prechecked never has `precheck` called. `Precheck` (`#[non_exhaustive]`) mirrors `Verification`:

- **When it runs:** before every attempt, after breaker admission, in a fresh run as well as on resume. It runs under the attempt timeout and races cancellation like an attempt. It is not an attempt: it is not recorded in `attempts`, not reported to the breaker and does not draw from the retry budget.
- **`Applied`:** the effect is already in place. The step ends `CompletedEarlier` with reason `already applied (precheck)`, and its dependents run. If an earlier attempt in this run failed (a timeout, say), that attempt stays recorded and is reported to the breaker as it would be on failure.
- **`NotApplied`:** there is proof the effect is absent, so the attempt runs (unless the run was cancelled meanwhile).
- **`Unknown`**, a timeout, or an `Err` from precheck: no attempt runs. If no attempt has run in this run, the step ends `NotRun(Unverified)` (or `Skipped(Cancelled)` if the run was cancelled): nothing was invoked, dependents are skipped, and resume tries again, precheck first. If an attempt already ran, the step ends with that attempt's error: `OutcomeUnknown(cause)` if its effect was unknown, `Failed` if it was not applied.
- **`Err(refused(..))`** (not retryable, not applied): a blocking precondition. The step ends `Failed` with that error, unless an earlier attempt in this run had an unknown effect, in which case it ends `OutcomeUnknown` with that attempt's cause, since the refusal cannot clear the doubt. An HTTP 4xx other than 429 returned from precheck counts as a refusal too.
- **Proof of absence:** precheck returns `NotApplied` only with proof. Reading a registry that serves stale documents (npm caches a package document for up to 300 s) is not proof that a version is absent, so such a step returns `Unknown` until the read is authoritative. This rule is the step author's to keep; michi cannot check it.

`CompletedEarlier` therefore means "its effect was already in place, so it was not run": carried over by resume, verified as applied by the caller, or found applied by precheck.

## Status model

`michi_core::pipeline::StepStatus` is `#[non_exhaustive]`, as are its reason enums. Each terminal status maps to one recovery verb.

| Status | Meaning | Agent verb |
| --- | --- | --- |
| `Pending` | not started | none yet |
| `Running` | in flight (only seen in records persisted from `DurableObserver::record_start` or `on_step_started`) | resume treats it as ambiguous |
| `Completed` | ran and succeeded in this run | none |
| `CompletedEarlier` | its effect was already in place, so it was not run: succeeded earlier in this run, verified as applied, or found applied by precheck | none |
| `Failed` | ran and failed without effect (`NotApplied`), an idempotent step failed, or its deadline group's window closed before it started (`DeadlinePassed`) | fix the cause, then resume |
| `OutcomeUnknown(cause)` | may have taken effect: `Timeout`, `Gateway` (502/504), `Cancelled` (dropped mid-flight after cancellation), `Interrupted` (in flight when the process stopped), `StepFailed` (an error that does not say whether the effect landed) | verify, then resume |
| `Skipped(PrerequisiteFailed)` | a prerequisite did not complete | fix the root cause, then resume |
| `Skipped(Cancelled)` | cancelled before it took effect | resume |
| `NotRun(CircuitOpen)` | its circuit breaker was open, so it was never invoked | wait, then resume |
| `NotRun(Unverified)` | its precheck could not tell whether the effect was in place, so it was never invoked | resume (precheck runs again) |
| `NotRun(StartNotRecorded)` | with durable starts, its in-flight marker could not be saved, so it was never invoked | fix the store, then resume |
| `Panicked` | the step's code panicked: a harness failure, not a step error | fix the bug, verify, then resume |

`Pipeline::render()` emits `step[N]{id,name,status,reason}:` with snake_case labels (`completed_earlier`, `outcome_unknown`, `not_run`, ...) and a reason (`prerequisite_failed`, `cancelled`, `circuit_open`, `unverified`, `start_not_recorded`, `timeout`, `gateway`, `interrupted`, `step_failed`) or `null`. It stays TOON 4.4 conformant.

## Run record

`execute(&plan, run_id, &opts).await` returns a `#[must_use]` `RunRecord` with private fields and accessors. It never returns an error: a run that happened is a record, however it went.

- `run_id()`, `plan_fingerprint()`, `elapsed()`, `was_cancelled()` (the caller's token fired), `observer_panics()` (observer calls that panicked; see Observer).
- `steps()`: one `StepRecord` per plan step, in declaration order. Each has the step id, name, terminal status, a human-readable `reason`, the final `error`, every attempt (number, replay class of its failure, error text, elapsed time), when it started relative to the run, how long it took, for a consequential skip the root-cause step (`caused_by`), and the step's `Replay` (`replay()`, set by the executor and by `from_parts`; `None` in a hand-saved record such as `StepRecord::in_flight`, which counts as not idempotent).
- `outcome()`: `Succeeded` only if the record lists every plan step and each is `Completed` or `CompletedEarlier`. Otherwise `OutcomeUnknown` if any step's outcome is unknown (`OutcomeUnknown`, or `Running` in a rebuilt record), else `Cancelled` if the caller cancelled, else `Failed`. A skipped, not-run, pending, running or panicked step therefore never reads as success.
- `root_causes()`: steps that failed in their own right (`Failed`, `OutcomeUnknown`, `NotRun`, `Panicked`, and `Running` in a rebuilt record). `consequential()`: skipped steps. An agent sees one cause instead of N errors.
- `needs_verification()`: every step resume holds until the caller verifies it (unless it is prechecked): `OutcomeUnknown`, `Panicked`, `Running`, and `Failed` with a final error whose effect is not `NotApplied`. A step the record knows is idempotent is never listed, since resume runs it anyway. The executor never ends a non-idempotent step `Failed` with an unknown effect, so that last case comes from records saved by 0.4 or by hand. Prechecked steps stay listed: their precheck may not be able to tell, and a verdict takes precedence over it. These are exactly the steps `ResumeOptions::verified` accepts (`StepRecord::awaits_verification()`).
- `completion_order()`: ids of steps dispatched in this run that ended in success (including ones whose precheck found the effect in place), in the order they finished, so callers can compensate if they choose to. Steps carried over by resume are not included.
- `error()`: the first root cause as `ExecutionError::StepFailed`, which converts to `DomainError`. When no step failed in its own right (a cancelled run, or a rebuilt record with pending steps) it names the first step that did not succeed, so `error()` is `None` only for a `Succeeded` run.
- `to_pipeline()` and `render()`: the michi-core `Pipeline` view and its TOON.

A record always holds exactly one `StepRecord` per plan step, in declaration order, and stores the plan's step count, so a record that lost steps cannot read as a success.

With the `serde` feature, `RunRecord`, `StepRecord`, `ExecutionError` and the ids serialize, so callers can persist records. The serialized record carries `plan_steps`; deserializing fails if `steps` does not list that many distinct steps (a missing or repeated step), so a truncated or tampered record is an error rather than a success. Records saved by 0.3 lack `plan_steps` and must be re-saved (or have `"plan_steps": <step count>` added). Records saved by 0.4 lack `effect` on `Failed` errors; it defaults to `Unknown`, so a 0.4 record never makes resume more willing to re-run a step than it should be. A step record serializes `replay` when it is known; records saved by 0.5 lack it and read as not idempotent. `PlanFingerprint` parses only what it prints: exactly 16 lowercase hex digits.

`RunRecord::from_parts(run_id, saved_fingerprint, &plan, steps)` rebuilds a record from step records saved one at a time. It returns `ResumeError::FingerprintMismatch` if the saved fingerprint is not the plan's and `ResumeError::UnknownStep` for a saved step the plan lacks. When `steps` holds several records for one step, the last one wins, so an append-only log of `StepRecord::in_flight` on start and the finished record on finish rebuilds each step's latest status. A plan step with no saved record is `Pending`. Each rebuilt record takes its step's `Replay` from the plan.

## Scheduling

One scheduler loop drives both modes. It keeps a remaining-prerequisite count per step and a ready set ordered by declaration index, and spawns ready steps onto a `tokio::task::JoinSet` owned by the call, so no step's async work outlives `execute` or `resume`. A `step_blocking` closure is the exception: its thread runs to completion (see Blocking steps).

- `Mode::Sequential` (default): one step at a time, in `Plan::execution_order()`.
- `Mode::Parallel { max_concurrency: NonZeroUsize }`: the loop dispatches ready steps in declaration order while fewer than `max_concurrency` are running. The cap lives in the dispatch loop, not in a semaphore inside spawned tasks, so admission order is declaration order rather than wake order.

### Concurrency keys

`ExecStep::concurrency_key(key)` puts a step in a key, and `ExecOptions::concurrency_limit(key, NonZeroUsize)` caps how many steps of that key run at once, within the mode's overall cap. The usual key is a downstream, so "at most two publishes to npm at a time" is:

```rust
use std::num::NonZeroUsize;

use michi_pipeline::{ExecOptions, Mode};

let options = ExecOptions::new()
    .mode(Mode::Parallel { max_concurrency: NonZeroUsize::new(8).unwrap() })
    .concurrency_limit("npm", NonZeroUsize::new(2).unwrap());
```

- **One key per step.** A later `concurrency_key` call replaces the earlier one. A step that must respect two limits (a registry and a shared runner, say) is better split, or keyed by the tighter one. Any string is a key, the empty string included.
- **Unlimited by default.** A key with no configured limit is unlimited, and a limit for a key no step uses is ignored, so one `ExecOptions` can serve several plans. A later `concurrency_limit` for the same key replaces the limit.
- **Waiting does not block.** A ready step whose key is at its limit waits in that key's queue. It takes no slot of the overall cap and holds back no other step: later-declared steps of other keys, or with no key, are dispatched past it.
- **Dispatch order.** Every time a slot frees, the scheduler dispatches the earliest-declared step that can run now: a ready step whose key has room, or a waiting one whose key now does. Steps of one key therefore start in declaration order, and the same plan and options always dispatch in the same order. No step waits forever: a plan is finite, and a waiting step goes as soon as its key has room and no earlier-declared step can run.
- **What holds a slot.** A step holds its key's slot from dispatch until it ends: precheck, every attempt and the backoff between retries. A step whose breaker is open or whose start could not be recorded frees it at once.
- **Lifecycle events.** `on_step_started` and `DurableObserver::record_start` fire when a step is dispatched, not when it starts waiting. A step still waiting when the run is cancelled (or fail-fast stops it) ends `Skipped(Cancelled)` without being invoked, and resume runs it again under the limits of the resumed run's options.
- **Per run.** Limits apply within one `execute` or `resume` call. Limits across concurrent runs remain a non-goal; share a semaphore inside the step for that.
- `Mode::Sequential` runs one step at a time anyway, so a limit never delays a sequential run.

`FailurePolicy` decides what a failure does to the rest of the run:

- `SkipDependents` (default): the failed step's transitive dependents are `Skipped(PrerequisiteFailed)` and independent branches keep running. This applies in sequential mode too, which changes the old behaviour of stopping at the first failure.
- `FailFast`: the first failure stops dispatching and cancels the run's token. Running steps get the cancellation grace period to finish, then their attempts are dropped and they end `OutcomeUnknown(Cancelled)` (or `Failed` if idempotent). Steps never started end `Skipped(Cancelled)`.

A step that panics is caught through its `JoinError` and ends `Panicked`. Other steps keep running and the breaker probe it held, if any, is released.

## Cancellation and time

`CancellationToken` wraps tokio-util's token, so tokio-util's 0.x type is not part of michi's public API. `ExecOptions::cancellation(token)` installs the caller's token; the run uses a child of it, and each step gets a child of the run token in its context.

When the token fires, nothing new is dispatched and every step not yet started ends `Skipped(Cancelled)`. A running attempt can watch `ctx.cancellation()` and stop cooperatively by returning `ExecutionError::Cancelled`, which also ends `Skipped(Cancelled)`. If it is still running after the grace period (`ExecOptions::cancel_grace`, default 5 s), its future is dropped and it ends ambiguous. Backoff sleeps race the token, so a cancelled run never waits out a retry delay.

Each attempt runs under `tokio::time::timeout` (`attempt_timeout`, default 30 s; zero is floored to 1 ms), except that an `AttemptBound::Deadline` step (a poll step) runs until its deadline (see Poll steps). `StepContext::attempt_deadline()` tells an attempt when it will be cut. An optional total deadline (`step_deadline`) bounds a step across all its attempts and backoff: no attempt starts or sleeps past it, and the last attempt is cut to the time left. A deadline too far away for the clock to represent (such as `Duration::MAX`) means no deadline; it never panics. All time is `tokio::time`, so tests run on a paused clock.

### Deadline groups

A step deadline starts when its step starts, so steps that start at different times end at different times. A deadline group gives several steps one window instead. Callisto's case: after a batch of npm publishes, each verify step polls the registry, and npm may serve a stale package document for up to 300 s, so every verify should give up at the same moment, about one cache window after the first verify began, not one window after each.

```rust
use std::time::Duration;

use michi_pipeline::ExecOptions;

// Every step with `.deadline_group("verify-npm")` shares one 6-minute window.
let options = ExecOptions::new().group_deadline("verify-npm", Duration::from_secs(360));
```

- **When the clock starts.** The window opens when the group's first step is dispatched in this run (after its concurrency key admits it and its start is recorded), and ends `window` later. Steps dispatched later get what is left of it.
- **Earliest wins.** A step's deadline is the earlier of its own (`deadline` or `step_deadline`, counted from its start) and its group's window end. Every existing deadline rule applies to it: no attempt or backoff runs past it, the last attempt is cut to the time left, and a retry whose wait would end at or past it is not made (the reason then ends `deadline group <key> reached`, or `step deadline reached` when the step's own deadline is the earlier). `StepContext::deadline()` reports the earlier one.
- **A closed window does not start steps.** A step of the group that would be dispatched once the window has ended (it became ready late, or waited for its concurrency key or a global slot) is not invoked. It ends `Failed` with `ExecutionError::DeadlinePassed`, which is not retryable and `NotApplied`: nothing ran, so nothing was applied. It is a root cause: dependents are skipped and fail-fast stops the run. It gets no start event and no attempt, and it is not reported to a breaker.
- **Resume opens a new window.** Windows are per run and are not saved in the record. A step that ended `DeadlinePassed` is `Failed` and `NotApplied`, so resume runs it again; its group's window opens anew when the first group step is dispatched in the resumed run.
- **Configuration.** A step has at most one group; a later `deadline_group` call replaces it. A group with no configured window has no deadline, and a window for a group no step uses is ignored. A window too long for the clock to represent (such as `Duration::MAX`) means no deadline. A zero window behaves like a zero step deadline: the first step gets a 1 ms attempt and the others do not run.

There is no absolute `deadline_at`. A tokio `Instant` means nothing after a restart, so a resumed run could not honour it, and the group window covers the shared-window case.

## Breakers and budgets

Circuit breakers are keyed. A `BreakerRegistry` holds one breaker per key, created on first use with the registry's `BreakerConfig` (default: opens after 5 consecutive failures, for 30 s; a zero threshold is floored to 1 and a zero open duration to 1 ms). A step's key is `ExecStep::breaker_key` (for example, one key per downstream service) and defaults to its step id. Pass the same `Arc<BreakerRegistry>` to every run so samples accumulate across runs; without one, no breaker applies.

Each breaker keeps the existing state machine: phase, probe ownership and failure count behind one mutex, with a generation counter so an outcome from a call admitted under an earlier generation is ignored. A step admitted while the breaker is closed retries per its policy and reports one outcome. A half-open probe gets exactly one attempt. An open breaker never invokes the step, and the step ends `NotRun(CircuitOpen)`. Cancellation, interruption and `DeadlinePassed` are not reported to the breaker; a probe they end is released.

A client error (HTTP 4xx other than 408 and 429) is reported to the breaker as a success: the downstream answered and rejected this one request, which says it is up. Counting a caller's 404s or 400s as failures would let one bad input open the breaker for every caller of that key. `ExecutionError::refused` counts as a success for the same reason: the step rejected the work itself before reaching the downstream (validation, a precondition), which says nothing about the downstream. 408 and 429 (the downstream is slow or shedding load), 5xx, timeouts, `rejected` (a dependency turned the request away), `failed` and `transient` (the step can't say what happened, so the downstream may be at fault) and other errors count as failures. Precheck outcomes are never reported. This follows Envoy's outlier detection, which ejects a host on consecutive 5xx and resets on any other response.

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

`ExecOptions::observer(Arc<dyn StepObserver>)` is called synchronously from the scheduler loop. `on_step_started` fires before a step is spawned and `on_step_finished` fires with each step's terminal `StepRecord`, including skipped steps and, on resume, steps carried over. A closure `Fn(&RunId, &StepRecord)` implements it (finish events only). Persisting each finished record, plus `StepRecord::in_flight(id, name)` for each started one, lets a caller rebuild the run with `RunRecord::from_parts` after a crash. An append-only log is enough: `from_parts` keeps the last record saved for each step, so a step that started and finished rebuilds as finished. Resume then treats a step still `Running` as ambiguous instead of re-running it blindly.

The observer is caller code, so a panic in it is caught (`std::panic::catch_unwind`) rather than unwinding the scheduler, which would lose the record and drop every running step. The run carries on, the observer is still called for later events, and `RunRecord::observer_panics()` counts the caught panics. A non-zero count means the observer missed events, so progress it persisted may be incomplete. The process's panic hook still runs.

### Durable starts

Without durable starts, a start hook that fails to save the in-flight marker (it panics, or swallows a storage error) does not stop the step. After a crash, the rebuilt record shows that step `Pending` and resume runs it again. `ExecOptions::durable_starts(observer)` closes that gap:

```rust
pub trait DurableObserver: StepObserver {
    /// Persist the in-flight marker; return `Ok` only once it is saved. `Err` or a panic stops the step.
    fn record_start(&self, run_id: &RunId, step_id: &StepId) -> Result<(), String>;
}
```

- Without durable starts nothing changes: `on_step_started` is called, a panic is caught and counted, and the step is dispatched.
- `durable_starts(observer)` takes the observer itself, so durable starts cannot be turned on without one that records starts: a closure observer (finish events only) is not a `DurableObserver` and does not compile there. A later `observer(..)` replaces it and turns durable starts off.
- With it, the scheduler calls `record_start` instead of `on_step_started`, before dispatching. `Ok` dispatches the step. `Err(reason)` or a panic does not: the step ends `NotRun(StartNotRecorded)` with the reason, its dependents are skipped, other branches carry on (fail-fast stops the run, as for any failure), and a panic is counted in `observer_panics()`. Nothing was invoked, so resume runs it again.

A step can then only run once its marker is saved, so a rebuilt record never shows a step that ran as `Pending`.

## Resume

`resume(&prior, &plan, &ResumeOptions).await` continues an interrupted run. It returns `Err(ResumeError)` before running anything if the plan's fingerprint differs from the record's (`FingerprintMismatch`), the record names a step the plan lacks (`UnknownStep`), or a step marked verified is not awaiting verification (`NotAwaitingVerification`). It always runs under the prior record's `RunId`, so idempotency keys stay stable. There is no way to resume under a new id.

Resume decides per step from the prior record:

| Prior status | Resume |
| --- | --- |
| `Completed`, `CompletedEarlier` | not run; `CompletedEarlier` |
| `Failed` whose final error is `NotApplied`, or whose step the record knows is idempotent | runs again |
| `Failed` whose final error's effect is unknown (or that has no error) | treated like `OutcomeUnknown` below; held, it becomes `OutcomeUnknown(StepFailed)` |
| `Skipped`, `NotRun`, `Pending`, missing | runs again |
| `OutcomeUnknown`, `Panicked`, `Running` | a verdict decides; without one, idempotent: runs again; `Prechecked`: runs, precheck decides; otherwise held |
| a status this version does not know | held as it is |

A verdict for a step that is not awaiting verification, including one the record knows is idempotent, is refused with `NotAwaitingVerification`. A held step keeps its prior record and its dependents are skipped until the caller marks it with `ResumeOptions::verified(id, Verification::Applied)` (treated as `CompletedEarlier`) or `Verification::NotApplied` (run again). A verdict takes precedence over idempotence and precheck, except that a prechecked step marked `NotApplied` still runs its precheck before the attempt. A held `Running` step becomes `OutcomeUnknown(Interrupted)`.

Resume indexes the prior record's steps once (every record lists each step once: `from_parts` keeps the last saved record per step and deserializing rejects a repeated step), so preparing a resume is O(N log N) in the step count: 50,000 carried-over steps took 1.6 s with a linear lookup per step and take about 20 ms now. `RunRecord::step(id)` is still a linear scan; index `steps()` yourself to look up many. `Plan::new` drops repeated dependencies by sorting the edges once (in michi-graph), so a wide fan-in costs O(E log E) rather than O(deg²).

Resume is forward recovery and at least once per step: michi has no atomic commit between a step's effect and the record, so without durable starts a step can run again if the process stopped after its effect but before its in-flight marker was saved. With durable starts, such a step rebuilds as `Running` and is held. Effects are exactly once only when the step hands `idempotency_key()` to a downstream that deduplicates. Resume is not incremental recomputation: it does not notice that a completed step's inputs changed.

## Non-goals

Dynamic dependencies, data passing between steps, cross-run concurrency limits, compensation or sagas, durable storage, input-hash memoization and critical-path scheduling. A concise rendering mode (non-completed steps plus a completed count) is not built yet.

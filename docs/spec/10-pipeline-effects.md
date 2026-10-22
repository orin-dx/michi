# Effects, precheck and durable starts (amendment to 08)

Status: **implemented in 0.5.** The rules are in [08-pipeline-execution.md](08-pipeline-execution.md) and the decision record, including the choices made while building it, is "Two facts per failure" in [06-decisions.md](06-decisions.md). This document stays as the rationale; where it and 08 differ, 08 is current. The differences are listed under "Deviations" at the end.

## The problem

michi's promise is that retries and resume never blindly repeat work that may already have taken effect. Three paths break it today:

1. **A definitive failure is re-run on resume.** A non-idempotent step can commit its effect and then return `Failed { retryable: false }`. It ends `Failed`, and resume runs every `Failed` step again, so the effect repeats. A consumer probe observed two commits. HTTP 500 has the same problem: it is classed as definitive, but a server error says nothing about whether the request was applied (RFC 9110 §15.6).
2. **`retryable: false` is read as "not applied".** It only says retrying would not help. Whether the effect landed is a separate fact, and the model has nowhere to record it.
3. **A start that was never recorded.** If `on_step_started` fails to save the in-flight marker (it panics, or its storage write fails), the scheduler dispatches the step anyway. After a crash, the rebuilt record shows it `Pending` and resume runs it again.

Mature orchestrators behave the same way (Step Functions redrive, Airflow "clear", Dagster "re-execute from failure" all re-run failed steps and leave idempotency to the step author), so michi is no worse than the industry. But the strictness is the product ([pipeline research](../research/pipeline-design.md): michi's refusal to auto-retry ambiguous non-idempotent work "is stricter than Temporal, Airflow or Step Functions"), and these paths quietly undo it.

## Two facts per failure, not one

Every failed attempt carries two independent facts:

- **`retryable`**: would trying again help? (unchanged meaning)
- **`effect`**: did the attempt's effect land? `Effect::NotApplied` (it certainly did not) or `Effect::Unknown` (it may have).

`Effect` is `#[non_exhaustive]`. There is no `Applied` value for failures: a step that knows its effect landed and still failed should say so in its message and report `Unknown`, because resume must not re-run it either way.

The executor derives everything from those two facts and the step's `Replay`:

|  | `effect: NotApplied` | `effect: Unknown` |
| --- | --- | --- |
| **retried in this run** | if `retryable` | if `retryable` and the step is `Idempotent` or `Prechecked` |
| **terminal status (last attempt)** | `Failed` | idempotent: `Failed`; otherwise `OutcomeUnknown(cause)` |
| **resume** | runs it again | idempotent: runs it again; prechecked: precheck decides; otherwise held until verified |

The default is `Unknown`. An error counts as not applied only when it says so, so an unclassified failure is never treated as safe.

### What each error means

| Error | `retryable` | `effect` |
| --- | --- | --- |
| `CircuitOpen` (step never invoked) | yes, after `retry_after` | `NotApplied` |
| `Cancelled` returned by the step (stopped before effect) | no | `NotApplied` |
| HTTP 429, 503 | yes | `NotApplied` |
| HTTP 4xx other than 429 (the server refused this request) | no (408: yes) | `NotApplied` |
| HTTP 502, 504 | yes | `Unknown` |
| HTTP 500 and other 5xx | no | `Unknown` (new: was definitive) |
| `Timeout`, `Interrupted` | yes | `Unknown` |
| `Panicked` | no | `Unknown` |
| `Failed { message, retryable, effect }` | as given | as given |

A step whose 4xx can follow a committed effect (a 409 after a partial write, say) reports `Failed` with `Effect::Unknown` instead of the raw status.

`Failed` gains the `effect` field. Four constructors cover the combinations so steps rarely spell out the fields:

| Constructor | `retryable` | `effect` | Use it when |
| --- | --- | --- | --- |
| `ExecutionError::failed(msg)` | no | `Unknown` | the default: something went wrong and you can't vouch for the effect |
| `ExecutionError::refused(msg)` | no | `NotApplied` | you rejected the work before doing anything (validation, a failed precondition) |
| `ExecutionError::rejected(msg)` | yes | `NotApplied` | a dependency turned the request away and asking again later may work |
| `ExecutionError::transient(msg)` | yes | `Unknown` | a transport-level hiccup mid-request; only idempotent or prechecked steps retry it |

`ReplayClass` stays as the per-attempt summary in `AttemptRecord`, with `Definitive` split by effect: `NotExecuted`, `Rejected` (retryable, not applied), `Refused` (not retryable, not applied) and `Ambiguous` (effect unknown). `ExecutionError::effect()` is new; `is_retryable()` keeps its meaning.

### New unknown cause

A non-idempotent step whose last error is `Unknown` without a more specific cause (a `Failed` with `Effect::Unknown`, an HTTP 500) ends `OutcomeUnknown(UnknownCause::StepFailed)`, rendered `outcome_unknown` with reason `step_failed`. Its error is wrapped in `AmbiguousOutcome` as today, and the agent verb is the existing one for `OutcomeUnknown`: verify, then resume. `UnknownCause` is `#[non_exhaustive]` in michi-core, so the variant is additive.

## Precheck: verification the step does itself

Callisto's release executor never applies an effect blindly: it observes the provider first and either adopts what already landed or proceeds with proof the effect is absent (Callisto ADR 3). Build tools do the same with outputs (Bazel, Nx and Turborepo skip work whose outputs exist). Precheck brings that into michi.

```rust
pub trait Step: Send + Sync {
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a>;
    fn replay(&self) -> Replay { Replay::NotIdempotent }
    /// Looks at the outside world before an attempt. Only consulted for `Replay::Prechecked` steps.
    fn precheck<'a>(&'a self, ctx: &'a StepContext) -> PrecheckFuture<'a> { Box::pin(async { Ok(Precheck::Unknown) }) }
}

#[non_exhaustive]
pub enum Precheck {
    /// The effect is already in place: don't run the step.
    Applied,
    /// There is proof the effect is absent: run the step.
    NotApplied,
    /// Can't tell (eventual consistency, a read that failed softly): don't run the step.
    Unknown,
}
```

`Precheck` mirrors the existing `Verification::Applied` / `NotApplied`: precheck is verification the step performs for itself.

- **`Replay::Prechecked`** is a new variant (`Replay` is `#[non_exhaustive]`). It says the step's precheck reliably tells whether its effect landed, so an attempt with an unknown effect may be retried through it. A step that is not `Prechecked` never has `precheck` called.
- **When it runs:** before every attempt of a `Prechecked` step, after breaker admission. It is not reported to the breaker and does not draw from the retry budget. It runs under the attempt timeout; a timeout or an `Err` from precheck counts as `Unknown`, except that an `Err` with `Effect::NotApplied` and `retryable: false` (`refused`) fails the step outright, which is how precheck reports a blocking precondition.
- **`Applied`:** the step ends `CompletedEarlier` with reason `already applied (precheck)`, in a fresh run as well as on resume. `CompletedEarlier`'s meaning widens from "succeeded in an earlier attempt of this run" to "its effect was already in place, so it was not run".
- **`NotApplied`:** the attempt runs.
- **`Unknown`:** no attempt runs. If an earlier attempt in this run had an unknown effect, the step ends `OutcomeUnknown` with that attempt's cause. If no attempt ran, it ends `NotRun(NotRunReason::Unverified)`, rendered `not_run` with reason `unverified`: nothing was invoked, dependents are skipped, and resume tries again (precheck first).
- **Proof of absence:** precheck returns `NotApplied` only with proof. Reading a registry that serves stale documents (npm caches a package document for up to 300 s) is not proof that a version is absent, so such a step returns `Unknown` until the read is authoritative. This rule is the step author's to keep; michi cannot check it.

Resume uses precheck instead of a manual verdict: an `OutcomeUnknown`, `Panicked` or `Running` step that is `Prechecked` runs again, and precheck decides. `ResumeOptions::verified` still works and takes precedence.

## Durable starts

`StepObserver` gains a fallible start hook used only when the caller asks for durable starts:

```rust
pub trait StepObserver: Send + Sync {
    fn on_step_started(&self, run_id: &RunId, step_id: &StepId) { let _ = (run_id, step_id); }
    /// With `ExecOptions::durable_starts()`: persist the in-flight marker. `Err` or a panic stops the step.
    fn record_start(&self, run_id: &RunId, step_id: &StepId) -> Result<(), String> { self.on_step_started(run_id, step_id); Ok(()) }
    fn on_step_finished(&self, run_id: &RunId, record: &StepRecord);
}
```

- Without `durable_starts()` nothing changes: `on_step_started` is called, a panic is caught and counted, and the step is dispatched.
- With it, the scheduler calls `record_start` before dispatching. `Ok` dispatches the step. `Err(reason)` or a panic does not: the step ends `NotRun(NotRunReason::StartNotRecorded)` with the reason, its dependents are skipped, and other branches carry on. Nothing was invoked, so resume runs it again.
- This closes the crash gap: a step can only run once its in-flight marker is saved, so a rebuilt record never shows a step that ran as `Pending`. It is Callisto's rule ("`Attempting` only after a proof") applied to michi's records.

## Resume, restated

With the amendment, resume decides per step from the prior record:

| Prior status | Resume |
| --- | --- |
| `Completed`, `CompletedEarlier` | not run; `CompletedEarlier` |
| `Failed` whose final error is `NotApplied`, or any `Failed` idempotent step | runs again |
| `Failed` whose final error is `Unknown` and the step is not idempotent (only in records saved before this change, or by a hand-built `StepRecord`) | held for verification, like `OutcomeUnknown`; `Prechecked` steps run and precheck decides |
| `Skipped`, `NotRun`, `Pending`, missing | runs again |
| `OutcomeUnknown`, `Panicked`, `Running` | idempotent: runs again; `Prechecked`: runs, precheck decides; otherwise held until `ResumeOptions::verified` |

Records deserialized from 0.4 lack `effect` on `Failed` errors; it defaults to `Unknown`, so a 0.4 record never makes resume more willing to re-run than it should be. `StepRecord::awaits_verification()` and `RunRecord::needs_verification()` include the held `Failed` case.

## What changes for callers

Breaking (fine before 1.0, listed in the changeset):

- `ExecutionError::Failed` has a third field, `effect`. Code that builds it with a struct literal must add it or switch to the constructors.
- A non-idempotent step that returns `Failed { retryable: false }` (now `failed(msg)`) or gets an HTTP 500 ends `OutcomeUnknown(StepFailed)`, not `Failed`. Use `refused(msg)` for failures that certainly had no effect.
- `ReplayClass::Definitive` becomes `Refused` (not applied) or `Ambiguous` (unknown).

Additive: `Effect`, `ExecutionError::effect()`, the four constructors, `Replay::Prechecked`, `Step::precheck`, `Precheck`, `UnknownCause::StepFailed`, `NotRunReason::{Unverified, StartNotRecorded}`, `StepObserver::record_start`, `ExecOptions::durable_starts()`.

michi-core gains three reason variants and their render labels, all on `#[non_exhaustive]` enums.

## Considered and rejected

- **Treat every `Failed` as unknown and drop the distinction.** Then a validation error in a non-idempotent step would need manual verification before every resume. `refused` keeps the common "nothing happened" case cheap.
- **Infer the effect from `retryable`.** That is today's bug: the two facts are independent (a 400 is not retryable and not applied; a timeout is retryable and unknown).
- **Run precheck for every step by default.** A default precheck can only answer `Unknown`, which would stop every step. Precheck is opt-in through `Replay::Prechecked`.
- **Precheck only on resume.** Callisto observes before every effect, including the first, because a fresh run of the same release must adopt what an earlier run published. Running it before each attempt also makes ambiguous retries of prechecked steps safe within one run.
- **A milestone inside one step ("issued, unverified").** Two steps express it, as Callisto's brief already notes: an effect step and a verify step that depends on it.

## Out of scope here

Callisto's other requests stay separate and land only as its spike needs them: a poll step bounded by a deadline rather than a retry count, a shared deadline for a group of steps, a per-key concurrency cap, a blocking-step adapter, and typed step outputs.

## Tests the implementation needs

- The Codex probe: a non-idempotent step commits and returns `failed(msg)`; resume holds it (and runs it once verified `NotApplied`). Same with an HTTP 500.
- `refused(msg)` in a non-idempotent step ends `Failed` and resume runs it again.
- A prechecked step: `Applied` ends `CompletedEarlier` without running, in a fresh run and on resume; `NotApplied` runs; `Unknown` with no prior attempt ends `NotRun(Unverified)` and resume retries; a timed-out attempt followed by `Applied` on the retry ends `CompletedEarlier` with one recorded attempt.
- Durable starts: `record_start` returning `Err` or panicking leaves the step `NotRun(StartNotRecorded)` and never invokes it; with an append-only log, a crash after a successful `record_start` rebuilds the step as `Running`, never `Pending`.
- A 0.4 record whose `Failed` error lacks `effect` deserializes as `Unknown` and resume holds the step.
- Every new status renders as TOON 4.4 and the overall outcome is never `Succeeded` with a `NotRun` or held step.

## Deviations in the implementation

Each is the safer reading where this proposal was silent or the code disagreed:

- **HTTP 408** stays non-retryable, as michi-core's `is_retryable_status` says. It is `NotApplied`. Changing retry policy was not needed for the defects.
- **A held `Failed` step** becomes `OutcomeUnknown(StepFailed)` when resume keeps it, so the record names the recovery resume will follow.
- **`awaits_verification()`** lists every `Failed` whose effect is not `NotApplied`, including idempotent steps, because a `StepRecord` does not know its step's `Replay`. Resume still runs idempotent ones.
- **Precheck after an attempt:** when precheck says `Unknown` after an attempt whose effect was not applied, the step ends `Failed` with that attempt's error (the proposal only covered an unknown earlier attempt). A `refused` precheck after an attempt with an unknown effect ends `OutcomeUnknown`, not `Failed`.
- **A `NotApplied` verdict on a prechecked step** still runs the precheck before the attempt, instead of the verdict skipping it.
- **Durable starts without a start-recording observer** (none, or a closure) refuse every step with `NotRun(StartNotRecorded)`.
- **Breaker:** found in the same sweep, `refused` now counts as a success for the breaker, like an HTTP 4xx.

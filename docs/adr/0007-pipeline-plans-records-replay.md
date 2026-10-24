# 7. Pipeline execution: plans, run records and replay safety

Status: Accepted. Amended in 0.5: the `definitive` replay class became `Refused` (not applied) and `Ambiguous` (effect unknown), and plan validation and ordering moved to `michi-graph`. See `crates/michi-pipeline/CHANGELOG.md`.

## Context

`michi-pipeline`'s positional executors (`execute_pipeline`, `execute_pipeline_parallel`, `StepDependencies`) mutated a shared `Pipeline`, bound runners to list positions, could not resume, and reported "not run" and "ran and failed" with the same status. A retry or replay could repeat work that had already taken effect.

## Decision

Rebuild execution around a validated plan, one scheduler and an immutable run record, following the research report's nine ranked changes.

- **Identity:** `StepId` and `RunId` are caller-chosen slugs, so agents can read and echo them and `run_id:step_id` is a stable idempotency key.
- **Plan:** an immutable `Plan` is validated once and reports every violation before anything runs. Its fingerprint binds a record to the plan for resume.
- **Record:** `execute` returns a `#[must_use]` `RunRecord` with exactly one terminal status and a reason per step. `RunOutcome` is `Succeeded` only if every plan step succeeded.
- **Replay rule:** every failed attempt is not executed, rejected, ambiguous or definitive. Ambiguous outcomes (timeout, 502, 504, cancellation in flight) of a non-idempotent step end `OutcomeUnknown` and are never replayed automatically.
- **Resume:** `resume` runs under the record's run id, never re-runs completed steps, and re-runs unknown outcomes only if the step is idempotent or the caller verified it. It is at least once per step; exactly once needs a deduplicating downstream.
- **Adopted vs built:** adopted Tokio's `JoinSet` and `tokio::time`, plus tokio-util's `CancellationToken` as the one new mandatory dependency, wrapped so its 0.x version never appears in the public API. Built the scheduler, Kahn validation, the retry loop, the retry budget and the keyed breaker, because each is small and must share michi's replay classes.

## Alternatives

- Keep the positional executors and add resume. Rejected: position-bound runners and a mutated pipeline cannot give a stable step identity or a trustworthy record.
- A workflow engine or DAG crate (dagrs, petgraph, tower `Service` steps, a retry crate). Rejected: each adds a model or a second source of backoff truth, and none has the replay classification.
- Durable storage, sagas, data passing between steps. Out of scope: michi is an embeddable step runner, not an orchestrator.

## Consequences

- The record format is strict. A serialized record carries `plan_steps`, deserializing rejects a record that does not list every plan step once, and `RunRecord::from_parts` refuses a changed plan or a foreign step. A truncated or tampered record is an error, never a success; records saved by 0.3 must be re-saved.
- Steps must classify their own errors honestly: a step that may have committed must not report 429, 503 or a retryable failure.
- A caller who reuses a run id for a different plan or inputs defeats downstream deduplication; michi cannot detect it.
- The old API was removed without deprecation, since it was never published.

## Sources

- `docs/spec/08-pipeline-execution.md`
- `docs/research/pipeline-design.md`
- `docs/spec/06-decisions.md`, "Resolved 2026-10-21 — Pipeline execution redesign", "`michi-pipeline` retry and breaker semantics" and "Pipeline records cannot read as a false success"
- [ADR 1](0001-async-pipeline-separate-crate.md), [ADR 5](0005-two-crates-one-version.md)

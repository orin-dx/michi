# 1. Keep async pipeline execution in its own crate

Status: Accepted

## Context

The core renderer is synchronous and has no async runtime in its default dependency graph. Pipeline execution needs Tokio and benefits from an independent release cadence.

## Decision

Keep execution in `michi-pipeline`, separate from the `michi` facade. It depends on `michi-core`, `michi-resilience`, and Tokio; it is not re-exported by the facade.

## Alternative

Put execution behind a feature in the facade. Rejected because it would couple async runtime dependencies and release cadence to the synchronous rendering API.

## Consequences

Consumers choose whether to add the async crate and its runtime. The pipeline API and execution errors can evolve independently from the pure rendering API.

## Sources

- `ARCHITECTURE.md`
- `docs/spec/01-overview-and-setup.md`
- `docs/spec/06-decisions.md`, “A separate crate is justified by dependency weight and coupling”

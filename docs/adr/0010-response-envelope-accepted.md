# 10. Build the multi-section response envelope now

Status: Accepted

Supersedes the "The Wisp and monokl envelope ... stays a proposal" bullet in [ADR 9](0009-v1-positioning-and-criteria.md). The rest of ADR 9 stands.

## Context

[`07-wisp-and-monokl-envelope.md`](../spec/07-wisp-and-monokl-envelope.md) proposed letting one `AgentResponse` carry several named sections, a response-level budget report and a provenance block. ADR 9 deferred it because [PRINCIPLES.md](../../PRINCIPLES.md) gate 3 asks for independent consumers, and the two that asked (Wisp and monokl) are not independent: Wisp consumes monokl. The case rested on a gap in prior art, which is weaker evidence than gate 3 wants.

## Decision

The owner approved the design for implementation on 2026-11-15, knowing gate 3 is not met in the strict sense. The envelope is built in `michi-core`: `Section`, `AgentResponse::budget`, `AgentResponse::provenance`, and the JSON `sections`, `budget` and `provenance` keys. It adds no dependency and no feature flag.

Because it ships ahead of independent use, it carries the ADR 9 rule for design with no caller: it is marked unstable in the docs (a "Stability" note in spec 07 and in the rustdoc of `Section`, `BudgetSummary` and `ProvenanceBlock`), and 1.0 promises only what consumers have used by then. The per-op index in `PartialSuccess` (spec 07 section 4) is a separate, breaking change and lands on its own.

## Alternatives

- Keep it a proposal until a third, independent consumer appears. Rejected by the owner: monokl and Wisp are integrating now, and a hand-rolled envelope in each would be the duplication the gate exists to prevent.
- Build only sections and leave budget and provenance to callers. Rejected: the budget and provenance shapes are what both consumers need identically.

## Consequences

- Gate 3 is waived for this envelope, on the owner's call, and the waiver is recorded here instead of being silent.
- Implementation deviations from spec 07 are listed in [`06-decisions.md`](../spec/06-decisions.md), "Envelope implementation deviations (gate 5)".
- If no second consumer adopts it before 1.0, it stays marked unstable or is removed ([ADR 9](0009-v1-positioning-and-criteria.md)).

## Sources

- [`07-wisp-and-monokl-envelope.md`](../spec/07-wisp-and-monokl-envelope.md), "What this is not"
- [PRINCIPLES.md](../../PRINCIPLES.md), gates 3 and 5

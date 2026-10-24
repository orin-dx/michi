# 9. Position michi as a response contract, and earn 1.0 by use

Status: Proposed

## Context

An adversarial review of the README and docs found claims stated more strongly than the evidence supports, and a gap between what michi is for and how it is described. The TOON measurements cover flat, uniform lists of up to 100 rows ([benchmarks](../toon-benchmarks.md)), yet TOON had become the headline. What a tool author gets from michi that is hard to rebuild is the shape around the body: totals, a report of what was cut, next-step hints, recovery steps, structured errors and MCP assembly. The 0.x crates have no external consumer on a published release yet, so a 1.0 would freeze surface nobody has tried.

## Decision

**Positioning.** michi is a response contract for agent-facing tools, with a pluggable body encoding. The contract covers:

- totals (`totalCount`) and the report of what a list cap cut (`truncated`)
- `help[]` next-step hints and `recovery` steps
- structured errors (`DomainError`), status and partial-success reporting
- MCP `CallToolResult` assembly and audience routing

TOON 4.4 is one encoder for the body, and today the only one. It is the right choice for flat, uniform lists that a model reads; it is not claimed to be better for nested data, and it is for models reading tool output, not writing it. Docs describe the contract first and TOON as the default body.

**1.0 criteria.** A crate goes to 1.0 when both hold:

1. At least two independent consumers have run it on a published release for several weeks.
2. The frozen surface equals what those consumers use. Unused public surface is marked experimental or dropped before 1.0.

"Independent" means different tools by different authors, as in [PRINCIPLES.md](../../PRINCIPLES.md) gate 3. Passing tests, completeness and a long feature list are not criteria.

**Version split (proposed, open).** `michi-core` heads toward 1.0 on its own. `michi-pipeline` and `michi-graph` stay 0.x until a consumer runs them. This needs a change to the `michi` fixed group in `callisto.toml`, which today releases all four packages on one version (ADR 5). This record does not change `callisto.toml`. Whether to split, and how the npm package versions against `michi-core`, needs the maintainer's decision before this ADR is accepted.

**Deferred past 1.0.** Each needs evidence that does not exist yet:

- **Automatic format selection by data shape.** It needs measurements of format by shape (nested, sparse, long, mixed) and at least two consumers hand-rolling the same selection logic ([PRINCIPLES.md](../../PRINCIPLES.md) gate 3). Callers choose the body today.
- **`Untrusted<T>`-style taint types.** Rejected for now in the trust-model decision in [`06-decisions.md`](../spec/06-decisions.md); no second tool has asked for one.
- **Typed pipeline step outputs** and **scoped or borrowing pipeline steps.** No consumer has run pipelines yet, so the cost of the current shape is unknown.
- **The Wisp and monokl envelope** ([`07-wisp-and-monokl-envelope.md`](../spec/07-wisp-and-monokl-envelope.md)). It stays a proposal.

## Alternatives

- Call 0.7 feature-complete and tag 1.0. Rejected: it freezes surface no consumer has used.
- Keep TOON as the headline. Rejected: the evidence supports a narrower claim, and the contract is what still matters if a better body encoding appears.
- Keep one version for all four packages through 1.0. Possible and simpler to release, but it ties `michi-pipeline` and `michi-graph` to a freeze their usage has not earned. Left open above.

## Consequences

- README and benchmark docs state the measured scope and what is untested.
- Pre-1.0 breakage stays acceptable. Surface with no consumer can still change or go.
- 1.0 may arrive later than the feature set suggests, and for `michi-core` only.

## Sources

- [`docs/toon-benchmarks.md`](../toon-benchmarks.md), Limits
- [`PRINCIPLES.md`](../../PRINCIPLES.md), gates 3 and 5
- [ADR 5](0005-two-crates-one-version.md), `callisto.toml`
- `docs/spec/06-decisions.md`, "Resolved 2026-10-24 — Automatic format selection is deferred past 1.0"

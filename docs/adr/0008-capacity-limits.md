# 8. Capacity limits

Status: Accepted

## Context

A capacity investigation found inputs that cost far more than their size suggested. Measured on an Apple Silicon laptop, release builds: `ToonOptions::validate` with 50,000 fields took 1.12–1.35 s; a serde `list()` of 10,000 items with one distinct key each allocated 2.84 GiB; the NAPI exports took 3.0–3.4 s and 2.4–2.6 GiB to reject a 100,001 × 100 `rows` array, and 10k × 10 cells sharing one 100 KB JS string grew RSS by 11 GiB. One such call blocks a server thread or Node's event loop.

## Decision

Bound the work every entry point can be asked to do, and reject oversize input before doing it.

- **Core shape caps (michi-core):** `toon::MAX_FIELDS` = 1,000 header fields, `toon::MAX_CELLS` = 1,000,000 cells and `toon::MAX_TRUNCATION_HINT_CHARS` = 256 chars, reported as `ToonError` variants. `list()` checks them right after its key-union pass.
- **NAPI budgets:** exports that take collections or strings take untyped napi values and convert them incrementally (`napi/input.rs`). Lengths are checked first (100,000 rows; 1,000 fields, values per row or items; 10,000 hints or pipeline and partial-success entries). Each call then gets 1,000,000 cells and 64 MiB of string bytes, with 1 MiB per value, hint, reason, message or name, and 4 KiB per identifier. A string's UTF-8 length is charged before it is copied, so a shared JS string cannot amplify.
- **Numbers:** every JS number passes a validating newtype (`napi/num.rs`); none is clamped.
- **Linear algorithms:** duplicate detection uses a set, `resume` indexes the prior record once, and `Plan::new` dedups dependencies in O(1) per edge.

## Alternatives

- No caps, with the cost documented. Rejected: one accidental oversize call (a sparse `list()` input is easy to produce) blocks the host.
- Length checks in the body of typed napi exports. Rejected: napi-derive converts typed parameters in full before the body runs.
- Caps configurable per call. Not built: no consumer needs a larger document for an agent to read, and a fixed cap keeps the contract simple.

## Consequences

| Case                                        | Before                 | After                  |
| ------------------------------------------- | ---------------------- | ---------------------- |
| `validate`, 50,000 fields                   | 1.12–1.35 s            | rejected in ~1 µs      |
| serde `list()`, 10k × 10k table             | 2.84 GiB               | rejected in ~1.3 ms    |
| NAPI reject 100,001 × 100 rows              | 3.0–3.4 s, 2.4–2.6 GiB | ~0.1 ms, no allocation |
| NAPI 10k × 10 cells sharing a 100 KB string | 1.6 s, +11 GiB RSS     | rejected in ~11 ms     |
| `resume`, 50,000 carried-over steps         | 1.6–2.4 s              | ~21 ms                 |

- Previously accepted oversize input is now an error, so the change shipped in a minor release.
- New exports must follow the same pattern: untyped arguments, lengths first, then the per-call budget.

## Sources

- `docs/spec/06-decisions.md`, "Resolved 2026-10-21 — Capacity limits" (measurements and decisions)
- `docs/spec/02-toon-format.md`, Size limits
- `docs/spec/04-mcp-and-napi.md`, Input limits and Numeric boundary
- `crates/michi-core/src/napi/input.rs`, `crates/michi-core/src/napi/num.rs`

# 6. TOON output conforms to TOON 4.4

Status: Accepted

## Context

michi called its list format TOON, but its output diverged from the [TOON specification](https://github.com/toon-format/spec/blob/main/SPEC.md): `issues[0]{}:` for an empty list, empty cells for null, quoting only for `,` `"` LF CR, `"NaN"` for non-finite numbers, and its own line formats for the KV, status, error, recovery and partial-success blocks. A conforming decoder rejected or misread that output, and an agent had to parse several formats.

## Decision

Every agent-facing block michi emits is valid TOON encoder output for `toon-spec: 4.4` (Working Draft, 2026-10-08) with the default options: comma delimiter, 2-space indent, no trailing newline.

- The form follows from the value's shape (§1.4), so each block is a data-model choice: KV items are a root object, status items and recovery hints are tabular arrays, hints are an inline string array.
- Composed keys (`totalCount`, `help`, `recovery`) are reserved, so a composed document never repeats a root key (§14.3).
- Input whose shape can be wrong returns an error instead of non-conformant output (`ToonError`, `KvError`).
- A strict test-only decoder rejects non-canonical output, and the official encode fixtures run in CI.

## Alternatives

- Keep a michi dialect, documented as "TOON-like". Rejected: agents and tools that know TOON would misread values (`-x`, `42`, `true` and `a:b` decoded as other types or structure), and michi would own a second grammar.
- Conform only for lists and keep the readable KV and recovery line formats. Rejected: an agent would still parse two formats, and the recovery and partial-success blocks were inconsistent with each other.

## Consequences

- Output changed for most blocks: KV loses column alignment, `Missing` renders `null`, floats use canonical numbers, and the `help` block became `help[N]: h1,h2`.
- `render_kv` became fallible (`DuplicateKey`, `ReservedKey`), and `AgentResponse` reports validation failures as an error body on every surface.
- Conformance is to the encoder rules for the subset michi emits. Interoperability with independent decoders is tracked separately.
- A new TOON version is adopted by a new decision, not by drift.

## Sources

- `docs/spec/02-toon-format.md` (Conformance, Agent-facing blocks, Size limits)
- `docs/spec/06-decisions.md`, "Resolved 2026-10-21 — TOON output conforms to TOON 4.4" and "Every agent-facing block is TOON 4.4"
- `crates/michi-core/tests/toon_spec.rs`, `tests/toon_blocks.rs`, `tests/support/toon_parser.rs`
- `crates/michi-core/tests/toon_spec_fixtures.rs`: official encode fixtures vendored from `toon-format/spec` at `d09c3d3` (MIT)
- `docs/research/toon-interoperability.md`

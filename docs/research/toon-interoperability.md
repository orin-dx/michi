# TOON interoperability

Status: Open

## Question

Which shapes in Michi's documented TOON dialect can be exchanged with independent TOON implementations?

## Evidence so far

Michi's tests parse Michi's own output. That checks internal consistency, not interoperability. Michi now targets TOON 4.4 and runs its official encode fixtures (`crates/michi-core/tests/toon_spec_fixtures.rs`), which covers encoder conformance; whether independent decoders accept the output is still untested. See [`toon/escape.rs`](../../crates/michi-core/src/toon/escape.rs).

## Method

Michi owns the compatibility corpus. Prism runs it against pinned external implementations such as `toon-format`. Cover empty rows, delimiters and quotes, backslashes, newlines/carriage returns/tabs, Unicode, null versus empty, booleans, numeric-looking strings, nested cell values, truncation markers, and multi-section envelopes.

For each fixture:

1. Render with Michi and decode with an external implementation.
2. Encode the equivalent value externally and record whether Michi's grammar accepts it.
3. Compare semantic values; compare bytes only when canonical output is the contract.
4. Save mismatches with producer, version, and expected disposition.

## Decision this blocks

Do not claim broad external compatibility until every fixture is marked `compatible` or `intentionally_unsupported` with a reason. Scope any claim to the tested implementations and fixtures. Keep external parsers out of Michi's production dependency graph.

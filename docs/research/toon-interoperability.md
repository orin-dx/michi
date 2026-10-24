# TOON interoperability

Status: Partly answered. The reference decoder reads every shape michi emits; other implementations are untested.

## Question

Which shapes in Michi's documented TOON dialect can be exchanged with independent TOON implementations?

## Evidence so far

Michi targets TOON 4.4 and runs the official encode fixtures (`crates/michi-core/tests/toon_spec_fixtures.rs`), which covers encoder conformance.

`packages/michi-node/__test__/interop.test.mjs` goes the other way: it renders with Michi through the Node package and decodes the text with the reference implementation, [`@toon-format/toon`](https://www.npmjs.com/package/@toon-format/toon) 4.4.0 (pinned, a dev dependency of that package only). Expected values are written by hand in the test, so a disagreement between the two implementations fails it. CI runs it on every NAPI target.

| Shape | Result |
| --- | --- |
| Tables with several columns and rows, empty tables | Compatible |
| Delimiters, quotes, backslashes and structural characters (`: [ ] { } - #`) inside cells | Compatible |
| Newlines, carriage returns, tabs, leading and trailing spaces, Unicode whitespace | Compatible |
| Unicode text, astral characters, combining marks, right-to-left text | Compatible |
| Strings that look like numbers, booleans or `null`, and the empty string | Compatible: they stay strings |
| Real `null`, booleans, integers and floats (including `5e-324` and `1.7976931348623157e308`) | Compatible: types and values survive |
| Field names that need quoting | Compatible |
| `totalCount` and the `help[N]` block next to a table, and a `help` block alone | Compatible |
| A truncated cell | Compatible: the cell decodes to the kept text plus an ellipsis, and the cut decodes as a separate `truncated` list of `{row, field, removed}`; text that imitates the old in-cell marker stays plain data |
| 2,000 seeded random cells and hints, 1,000 random field names | Compatible, or rejected by Michi before rendering with `toon_validation_failed` |

## Limits of the claim

- **One implementation, one direction.** Only the reference TypeScript decoder at 4.4.0 is tested, and only Michi's output going into it. Michi has no public parser, so there is no external-encoder-into-Michi direction to test.
- **Michi emits a subset.** Flat tables, `totalCount`, inline string arrays and key-value lines. Nested objects, keyed tabular blocks and arrays of arrays are not shapes Michi writes, so they are not covered.
- **Only the table path is covered.** The key-value, status, recovery and partial-success blocks are not run through the decoder yet.
- **Integers beyond 2^53** are not covered: a JavaScript number cannot carry them into the test.
- **Other decoders** (the Rust and Python ports, for example) are untested. Run the same corpus against any of them before claiming support.

## What is still open

Run the key-value, status, recovery and partial-success blocks through the decoder, run the corpus against a second, independent decoder, and add the multi-section envelope shapes once spec 07 lands. Keep external parsers out of Michi's production dependency graph.

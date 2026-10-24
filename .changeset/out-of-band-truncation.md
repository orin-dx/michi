---
michi-core: minor
michi-node: minor
---

michi-core: truncation is reported out of band, and `DomainError::from_error` no longer copies the cause chain.

**Truncated TOON cells.** A cell longer than `max_cell_len` used to end in text such as `(233 more chars — use full=true)`, and a cell whose own data ended the same way was indistinguishable from a real cut. A cut cell now keeps `max_cell_len - 1` chars and ends in a plain `…`, and every cut is listed in a new `truncated[N]{row,field,removed}:` block after the list (`row` is 0-based, `removed` is the chars cut). `truncation_hint` (`truncationHint` in Node) still carries your "get it in full" instruction, now as one extra line in `help[]` (`Cut cells are listed in the truncated block; use full=true for the full value`) that appears only when a cell was cut. `AgentResponse` merges that line into its own hints, in the text, JSON and MCP outputs. `truncated` is now a reserved list type name, like `totalCount`, `help` and `recovery`. `truncate()`, `truncate_inline()` and the Node `truncate()` are unchanged: they still append the in-band suffix, so use them for a single free-text string, not for TOON cells.

Migrate: anything that parsed `(N more chars` out of a cell should read the `truncated` block instead; snapshots of truncated output need regenerating; rename a list type called `truncated`.

**`DomainError::from_error`** now keeps only the error's own `Display` as `message`. The `source()` chain, which often holds URLs with tokens or file paths, stays out of agent-visible output. To keep the chain as `causes`, call the new `DomainError::from_error_with_causes` (the old behaviour). The top-level message is still shown, so make sure it is safe too.

**Docs.** `docs/spec/02-toon-format.md` has a new trust model section: michi guarantees one value cannot create extra lines, fields or cells, and does not stop prompt injection in data; hints, recovery text, error messages, type and field names come from the tool author, never from data.

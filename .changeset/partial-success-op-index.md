---
michi-core: minor
---

`PartialSuccess` reports each operation's position in your batch, so `results[i]` and `ops[i]` stay aligned in the rendered output. `completed` and `skipped` are `Vec<OpRef>`, a new type with `index`, `operation` and an optional `reason` (a machine token such as `budget_exhausted`), and `FailedOp` gains `index`.

Breaking: the rendered report changes. `completed[2]: a,b` becomes a table, `completed[2]{index,operation}:` with one `0,a` row per op, and `failed` and `skipped` are `{index,operation,reason}` tables too. A skipped op with no reason renders `null` in that column. Anything that parses the old text must read the new tables. In Rust, build the vectors from `OpRef::new(index, operation)` (add `.reason(token)` for a skipped op) instead of strings, and pass the index as the first argument of `FailedOp::new(index, operation, reason)`. In Node, `renderPartialSuccess` takes `{ index, operation, reason? }` objects for `completed` and `skipped` instead of strings, and each `FailedOp` needs an `index`.

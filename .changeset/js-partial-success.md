---
michi-node: minor
---

**Breaking:** a batch's outcomes render as one table, and their input is checked harder. Build them with `AgentResponse.partialSuccess(name, completed, failed, skipped)` (`renderPartialSuccess` is removed; see the entry on one render path).

- Before: a quoted summary line, then separate `completed`, `failed` and `skipped` tables. Now: `name[N]{index,operation,status,reason}:`, one row per operation sorted by index. An empty report renders `name: []` and `totalCount: 0`. Anything that parses the old text must read the new table.
- It takes `CompletedOp[]` and `SkippedOp[]` in place of `OpRef[]`.
- It throws when an `index` appears twice across the three lists, and when a completed operation has a `reason`. Before, a duplicate rendered as two outcomes and the reason was dropped without a word.

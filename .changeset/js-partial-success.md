---
michi-node: minor
---

**Breaking:** `renderPartialSuccess` renders one table and checks its input harder.

- Before: a quoted summary line, then separate `completed`, `failed` and `skipped` tables. Now: `partial_success[N]{index,operation,status,reason}:`, one row per operation sorted by index. An empty report renders `partial_success: []` and `totalCount: 0`. Anything that parses the old text must read the new table.
- It takes `CompletedOp[]` and `SkippedOp[]` in place of `OpRef[]`.
- It throws when an `index` appears twice across the three lists, and when a completed operation has a `reason`. Before, a duplicate rendered as two outcomes and the reason was dropped without a word.

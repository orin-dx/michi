---
michi-core: minor
---

**Breaking:** `PartialSuccess` now renders as one table with a row per operation, and can no longer list one operation as both completed and failed.

- Before: a quoted summary line, then separate `completed`, `failed` and `skipped` tables. Now: `partial_success[N]{index,operation,status,reason}:`, rows sorted by index. The failed operations' recovery table is unchanged. An empty report renders `partial_success: []` and `totalCount: 0`. Anything that parses the old text must read the new table.
- `PartialSuccess::new` becomes `PartialSuccess::try_new`, which returns `DuplicateOpIndex` when an index repeats. Its fields are private: read them with `completed()`, `failed()` and `skipped()`.
- `OpRef` is split into `CompletedOp` (no reason) and `SkippedOp`. Replace `OpRef::new` with `CompletedOp::new` or `SkippedOp::new`, and `PartialSuccess::new(c, f, s)` with `PartialSuccess::try_new(c, f, s)?`.

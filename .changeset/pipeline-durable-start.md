---
michi-pipeline: minor
---

**Breaking:** `DurableObserver::record_start` for parallel steps now runs concurrently, so their saves overlap instead of queuing.

- Before: start markers were saved one after another in the scheduler loop. Now each step saves its own, so `record_start` must be safe to call concurrently and in any order across steps. Make your implementation thread-safe.
- A step still runs only after its own `record_start` returns `Ok`. A cancel or fail-fast stop during the save ends the step `Skipped(Cancelled)`.
- A deadline group's window now opens before the first step's save, so the save counts against it. A step whose window ends during its save ends `Failed` with `DeadlinePassed` (the same message as a window that ends before dispatch) without being invoked.
- A step that ends during its save was never invoked, so its record has no `started_at` or `elapsed`.

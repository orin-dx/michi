---
michi-pipeline: minor
---

Breaking: `DurableObserver::record_start` now runs on a blocking thread owned by its step, not inline in the scheduler loop. Parallel steps save their start markers concurrently (8 steps with a 100 ms save: 830 ms before, 114 ms after), so `record_start` must be safe to call concurrently and in any order across steps. A step still runs only after its own `record_start` returns `Ok`, and looks at the run again after the save: a cancel or fail-fast stop during it ends the step `Skipped(Cancelled)`. A deadline group's window now opens before the first step's save, so the save counts against it; a step whose window ends during its save ends `Failed` with `DeadlinePassed` without being invoked.

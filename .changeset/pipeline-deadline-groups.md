---
michi-pipeline: minor
---

michi-pipeline: deadline groups. `ExecStep::deadline_group(key)` with `ExecOptions::group_deadline(key, window)` gives several steps one shared window that opens when the group's first step is dispatched, so steps started at different times expire together. A step's deadline is the earlier of its own and its group's. A step that would start after its group's window closed is not run: it ends `Failed` with the new `ExecutionError::DeadlinePassed` (not retryable, not applied), and resume runs it again in a new window.

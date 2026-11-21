---
michi-pipeline: patch
---

A step that never ran no longer reports a run time: a step refused by an open breaker, or cancelled before it started, now has no `started_at` or `elapsed` in its record. Before, both showed how long the step waited.

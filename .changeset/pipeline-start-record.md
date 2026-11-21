---
michi-pipeline: patch
---

A step that is never invoked no longer gets a `started_at` or `elapsed` in its record. Before, a step cancelled, failed by its deadline group's window or stopped by an open breaker while its durable start was being saved reported the save's duration as its run time. A deadline group window that ends during that save now gives the same `DeadlinePassed` message as one that ends before dispatch.

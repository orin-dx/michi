---
michi-pipeline: patch
---

A step whose code was never invoked no longer gets a `started_at` or `elapsed` in its record. Before, a step cancelled or failed by its deadline group's window while its durable start was being saved reported the save's duration as its run time, and a step stopped by an open breaker or cancelled before it started reported the time it waited. A deadline group window that ends during the durable save now gives the same `DeadlinePassed` message as one that ends before dispatch.

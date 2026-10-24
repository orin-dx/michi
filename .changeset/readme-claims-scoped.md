---
michi-core: patch
michi-pipeline: patch
michi-graph: patch
michi-node: patch
---

Docs: the README now scopes its TOON claims to what was measured (token savings on flat, uniform 100-row lists; retrieval accuracy within noise on single-hop questions), says TOON here is for models reading tool output, and states that `michi-pipeline` resume is at-least-once per step unless the step passes its idempotency key to a downstream that deduplicates. Adds a plain License section.

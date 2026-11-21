---
michi-core: minor
michi-pipeline: minor
michi-node: minor
---

michi-core: every outcome composes into one `AgentResponse`. `error(DomainError)` adds the error block, which renders first, and `partial_success(name, ps)`, `status(s)`, `pipeline(name, p)` and `already_done(operation, summary)` add sections. Their hints and recovery hints join the response's one `help` and `recovery` block. A batch with results, per-op outcomes and an error is now one valid TOON document, one JSON object and one MCP result. NAPI: `AgentResponse` gains `error`, `partialSuccess`, `pipeline`, `status` and `alreadyDone`, so `renderJson()` and `toCallToolResult()` now cover errors too.

michi-core: Breaking: one error JSON shape. `DomainError::render(OutputFormat::Json)`, its serde form and its `schemars` schema are the response envelope: `{"body", "error": {"code", "message", "causes", "retryable"}, "hints", "recovery", "isError"}`. `recovery` is always an array, as in every response. Before, the JSON was `{isError, error, message, causes, retryable, hints, recovery}`, with `error` the code and `recovery` an object or `null`. Read `error.code` where you read `error`, and `recovery[0]` where you read `recovery`. The text render is unchanged.

michi-core: Breaking: `PartialSuccess::render` is one table, `partial_success[N]{index,operation,status,reason}:`, with a row per operation sorted by index. Before, it wrote a quoted summary line and separate `completed`, `failed` and `skipped` tables. The failed operations' recovery table is unchanged. An empty report renders `partial_success: []` and `totalCount: 0`. JS `renderPartialSuccess` renders the same.

michi-core: Breaking: an empty `Pipeline` renders `step: []` and `totalCount: 0`, and a `StatusResponse` with no items renders `items: []` and `totalCount: 0`. This is the same empty list as any other section. `michi-pipeline`'s `RunRecord::render` of a run with no steps changes with it.

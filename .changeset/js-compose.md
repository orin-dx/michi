---
michi-node: minor
---

`AgentResponse` can now hold every kind of outcome, so a batch with results, per-operation outcomes and an error is one valid TOON document, one JSON object and one MCP result.

- New methods: `error`, `partialSuccess`, `pipeline`, `status` and `alreadyDone`. `renderJson()` and `toCallToolResult()` now cover errors too.
- Their hints and recovery hints join the response's single `help` and `recovery` block.
- A response with an error and an invalid section reports both on every output: your error leads and stays the JSON `error`, and each validation failure follows its causes as `toon_validation_failed: <message>` (or `kv_validation_failed`). `isError` stays true.

---
michi-core: minor
---

One `AgentResponse` can now hold every kind of outcome, so a batch with results, per-operation outcomes and an error is one valid TOON document, one JSON object and one MCP result.

- `error(DomainError)` adds the error block, which renders first.
- `partial_success(name, ps)`, `status(s)`, `pipeline(name, p)` and `already_done(operation, summary)` add sections.
- Their hints and recovery hints join the response's single `help` and `recovery` block.
- If the response has an error and an invalid section, or two blocks that write the same root key, it reports both on every output (text, JSON, `render_for`, `to_call_tool_result`): your error leads and stays the JSON `error`, and each validation failure follows its causes as `toon_validation_failed: <message>` (or `kv_validation_failed`). `isError` stays true. With several failures and no caller error, the first failure is the `error` and the rest are its causes.

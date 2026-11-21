---
michi-core: minor
michi-node: minor
---

michi-core: a response with an error and an invalid section (or two blocks writing one root key) reports both, and the same on every surface (text, JSON, `render_for`, `to_call_tool_result`, NAPI): the caller's error leads and stays the JSON `error`, and each validation failure is added after its causes as `toon_validation_failed: <message>` (or `kv_validation_failed`). `isError` stays true. Several failures (two failed sections, or a failed section beside a collision) are one `error` object too: the caller's error, or without one the first failure, with every other failure as a cause, never a collision on `error`.

michi-core: Breaking: a validation failure is an `error` object in JSON, `{"code": "toon_validation_failed", "message": …, "causes": [], "retryable": false}` (or `kv_validation_failed`), the same keys as every other error. Before, it was `isError: true` with no `error` object. The text is unchanged.

michi-core: Breaking: a section's JSON `totalCount` follows the text's rule. It appears when the total exceeds the rows and is `0` for an empty list, and is left out for a total at or below the rows (it was the declared value whenever one was set). The budget roll-up's `total` column (new in this release) follows the same rule. `Section::total()` returns the value as given.

michi-core: Breaking: `AgentResponse::hints(vec)` replaces only the hints added with `hint()` and `hints()`. A `truncated_section()`'s `showing N of M` line is kept (before, `hints()` dropped it), and so are the hints of the new `status()`, `partial_success()` and `error()` blocks.

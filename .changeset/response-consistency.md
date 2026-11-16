---
michi-core: minor
michi-node: minor
---

michi-core: Breaking: a validation failure no longer hides the caller's error. A response with `error(e)` and an invalid section (or two blocks writing one root key) used to render `error: toon_validation_failed` with "duplicate root key 'error'" in the text while its JSON still said `e`'s code. Now `e` leads on every surface (text, JSON, `render_for`, `to_call_tool_result`, NAPI) and stays the JSON `error`, and the failure is added as its last cause, `toon_validation_failed: <message>`. `isError` stays true. Several failures (two failed sections, or a failed section beside a collision) are one `error` object too: the caller's error, or without one the first failure, with every other failure as a cause, never a collision on `error`.

michi-core: Breaking: a validation failure is an `error` object in JSON, `{"code": "toon_validation_failed", "message": …, "causes": [], "retryable": false}` (or `kv_validation_failed`), the same keys as every other error. Before, it was `isError: true` with no `error` object. The text is unchanged.

michi-core: Breaking: a section's JSON `totalCount` follows the text's rule. It appears when the total exceeds the rows and is `0` for an empty list, and is left out for a total at or below the rows (it was the declared value whenever one was set). The budget roll-up's `total` column follows the same rule, so it shows `null` where it showed a total equal to the rows, and `0` for an empty list. `Section::total()` still returns the value as given.

michi-core: Breaking: `AgentResponse::hints(vec)` replaces only the hints added with `hint()` and `hints()`. A `status()` report's hints and a `truncated_section()`'s `showing N of M` line are kept; before, `hints()` dropped them.

michi-core: Breaking: `JsonObject::new` is renamed `JsonObject::try_new`, since it can fail. `"…".parse::<JsonObject>()` is unchanged.

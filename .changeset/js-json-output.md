---
michi-node: minor
---

**Breaking:** the JSON from `renderJson()` and `toCallToolResult().structuredContent` changes in two places.

- A validation failure is now an `error` object, `{"code": "toon_validation_failed", "message": …, "causes": [], "retryable": false}` (or `kv_validation_failed`). Before it was `isError: true` with no `error` object. The text is unchanged.
- A section's `totalCount` appears when the total is larger than the rows returned, is `0` for an empty list, and is left out otherwise (before, it was whatever total you passed).

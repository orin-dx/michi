---
michi-core: minor
---

**Breaking:** the JSON your tools return changes in three places.

- Error JSON: `DomainError::render(OutputFormat::Json)`, its serde form and its `schemars` schema are now the response envelope `{"body", "error": {"code", "message", "causes", "retryable"}, "hints", "recovery", "isError"}`. Before it was `{isError, error, message, causes, retryable, hints, recovery}`, where `error` was the code and `recovery` an object or `null`. Read `error.code` where you read `error`, and `recovery[0]` where you read `recovery` (`recovery` is always an array). The text render is unchanged.
- Validation failures: a section or key-value failure is now an `error` object, `{"code": "toon_validation_failed", "message": …, "causes": [], "retryable": false}` (or `kv_validation_failed`). Before it was `isError: true` with no `error` object. The text is unchanged.
- `totalCount` in a section's JSON follows the text's rule: it appears when the total is larger than the rows returned, is `0` for an empty list, and is left out otherwise (before, it was the declared value whenever one was set). The budget roll-up's `total` column follows the same rule. `Section::total()` still returns the value as given.

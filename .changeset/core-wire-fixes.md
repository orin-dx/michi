---
michi-core: minor
---

michi-core: values survive every wire, and invalid TOON is never emitted.

- **Breaking (`schemars`):** `StatusResponse`, `StatusItem`, `Health`, `toon::ToonOptions`, `ErrorClass`, `KvItem`, `OutputFormat`, `pipeline::Pipeline` and `pipeline::PipelineStep` no longer implement `JsonSchema`. Nothing serializes them, so their schemas described no real wire form. Every remaining schema describes what serde emits.
- `toon::Value::Float` NaN and ±Infinity now round-trip through serde as `"NaN"`, `"Infinity"` and `"-Infinity"` (they serialized as `null` and failed to read back), the same form `KvValue` uses. Rendered TOON still shows `null`, and the schema lists the three strings.
- `Value::from(f32)` and f32 fields in `toon::list()` keep the f32's shortest digits: `0.1f32` renders `0.1`, not `0.10000000149011612`.
- `AgentResponse` on the KV path with no items keeps a total count: `kv_items(vec![]).total_count(0)` renders `totalCount: 0` in the text and JSON body instead of an empty body.
- `KvValue` NaN and ±Infinity round-trip through serde as named strings; the JSON companions write `null` for them, matching the TOON body, instead of the quoted strings `"NaN"`/`"inf"`.
- `empty_state` with a reserved type name (`totalCount`, `help`, `recovery`) renders a `toon_validation_failed` error block instead of invalid TOON. New `try_empty_state` and `try_empty_state_with_hints` return the `ToonError`.
- `RecoveryHint::param` replaces an existing key in place instead of adding a second one, and every renderer writes a repeated key once, so the JSON never holds duplicate keys.
- MCP serde accepts what MCP allows: only `content` is required, `isError` defaults to `false`, `structuredContent` may be absent or `null`, and `annotations`/`audience` are optional. `structuredContent` is omitted when empty, and serializing a non-object `structuredContent` is an error instead of invalid MCP. New `CallToolResult::try_new` and `StructuredContentError`.
- With `serde`, `DomainError` serializes to exactly the object `render_json()` writes, and its `schemars` schema describes that object.
- GitHub annotations escape `%` (and `:`/`,` in the title), so a literal `%0A` in a message no longer becomes a line break.
- `parse_retry_after` accepts the RFC 850 and asctime HTTP-date forms as well as IMF-fixdate.
- `next_retry_delay` rounds `base_delay` and `retry_after` up to whole milliseconds, so a sub-millisecond delay no longer becomes zero.
- Correction to the 0.3.0 notes: `toon::list()` rows missing a field get a `null` cell, not an empty one.

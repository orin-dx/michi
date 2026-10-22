---
michi-node: minor
---

@orin-dx/michi: no silent rounding and no invalid TOON.

- `emptyState` throws `toon_validation_failed: …` for a reserved `typeName` (`totalCount`, `help`, `recovery`), like `renderToon`. It returned invalid TOON before.
- `renderKv`, `renderStatus` and `AgentResponse.kvItems` keep a float exact when `decimalsVal` is absent: `1e-7` renders `1e-7` and `1.23456789` stays `1.23456789`. They rounded to 6 places, so small values rendered `0`. An explicit `decimalsVal` still rounds.
- `AgentResponse` on the KV path with no items keeps its total count (`totalCount: 0`) instead of rendering an empty body.
- michi-core's fixes to recovery hints (no duplicate keys), empty states, Retry-After parsing and sub-millisecond delays apply here too; see the michi-core changelog.

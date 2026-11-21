---
michi-node: minor
---

**Breaking:** `totalCount`, `help[]` and the truncation hint now belong to `AgentResponse`, which places each once for the whole response. The older standalone parameters and functions are removed.

- `renderToon` no longer takes `totalCount`, `hints` or `truncationHint`, and `renderKv(items)` no longer takes `totalCount` or `hints`. `AgentResponse.items`, `kvItems` and `truncationHint` keep them: use those to attach them.
- `emptyState` and `emptyStateWithHints` are removed. Use `new AgentResponse(name)` (plus `.hint(..)`).
- An empty list now renders `name: []` and `totalCount: 0` (it rendered `name: []` alone), also from `renderPipeline` (`step: []`) and `renderStatus` (`items: []`).

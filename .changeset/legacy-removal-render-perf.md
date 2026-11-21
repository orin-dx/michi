---
michi-core: minor
---

michi-core: Breaking: `ToonOptions` is the list alone. `hints`, `total_count` and `truncation_hint` (and their builders) are removed; `totalCount`, `help[]` and the truncation hint belong to `AgentResponse`, which places each once for the whole response. `render_toon` writes the list entry and the `truncated` block. `render_kv(items)` lost its `total_count` and `hints` parameters. NAPI: `renderToon` no longer takes `totalCount`, `hints` or `truncationHint`, and `renderKv(items)` no longer takes `totalCount` or `hints`; `AgentResponse.items`, `kvItems` and `truncationHint` keep them.

Migration: `render_toon(&ToonOptions::new(..).total_count(Some(n)).hints(h))` becomes `AgentResponse::new(name).section(Section::toon(name, fields, rows).total_count(Some(n))).hint(..)` rendered with `render(OutputFormat::Text)`. For hints on a standalone list, `append_hints(&mut out, &hints)`.

michi-core: Breaking: one definitive empty state. `empty_state`, `empty_state_with_hints`, `try_empty_state`, `try_empty_state_with_hints`, the `empty` module and NAPI `emptyState` and `emptyStateWithHints` are removed. An empty `Toon` section, and so an untouched `AgentResponse`, now renders `name: []` and `totalCount: 0` (it rendered `name: []` alone). Use `AgentResponse::new(name)` (plus `.hint(..)`) where you called `empty_state`. An empty `Kv` section still renders nothing.

michi-core: `AgentResponse::render`, `is_error()` and `to_call_tool_result()` no longer copy every row on each call. They validate and render a borrowed view of the section's rows: 1,000 rows took 2,015 allocations to render and 2,011 for `is_error()`, and now take about what `render_toon` takes. `toon::list` no longer keeps every parsed item until the rows are built. Output is unchanged.

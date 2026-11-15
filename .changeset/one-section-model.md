---
michi-core: minor
---

michi-core: `AgentResponse` keeps its body in one list of sections. `items()` and `kv_items()` are shorthand that append a section named for the response's type, so calling both (or either twice) renders every body in call order instead of silently dropping one. The same applies to `@orin-dx/michi`'s `items()` and `kvItems()`.

Breaking: removed `AgentResponse::render_toon()`, `render_kv()`, `total_count()` and `truncation()`, and NAPI `renderToon()`, `renderKv()` and `totalCount()`. A response with only `items()` or `kv_items()` renders the same text, with these exceptions: a `totalCount` line renders only when the total is larger than the rows returned (so an empty list no longer prints `totalCount: 0`), and `render(Json)` / `structuredContent` gain a `sections` array next to `body`.

Migration: `AgentResponse::new(..).items(..).hints(..).render_toon()` becomes `.render(OutputFormat::Text)`; `.total_count(n)` becomes `Section::toon(name, fields, rows).total_count(Some(n))` passed to `.section(..)`; `.items(rows, f).truncation(&outcome, g)` becomes `.truncated_section(Section::toon(name, f, rows), &outcome, g)`. In JavaScript, `renderToon()` and `renderKv()` become `renderText()`, and `totalCount(n)` becomes the optional third argument of `items(rows, fields, n)` (second of `kvItems(items, n)`).

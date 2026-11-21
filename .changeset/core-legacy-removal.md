---
michi-core: minor
---

**Breaking:** list rendering is now `AgentResponse`'s job, and the older standalone paths are removed so `totalCount`, `help[]` and the truncation hint appear once for the whole response.

- `ToonOptions` describes the list alone: `hints`, `total_count` and `truncation_hint` (and their builders) are removed. `render_toon` writes the list and the `truncated` block. `render_kv(items)` loses its `total_count` and `hints` parameters.
- `empty_state`, `empty_state_with_hints`, `try_empty_state`, `try_empty_state_with_hints` and the `empty` module are removed. Use `AgentResponse::new(name)` (plus `.hint(..)`) where you called `empty_state`.
- One empty list everywhere: an empty `Toon` section (so an untouched `AgentResponse`), an empty `Pipeline` and a `StatusResponse` with no items now render `name: []` and `totalCount: 0` (`step: []` and `items: []` for the last two). Before, an empty `Toon` section rendered `name: []` alone. An empty `Kv` section still renders nothing.

Migration:

- `render_toon(&ToonOptions::new(..).total_count(Some(n)).hints(h))` becomes `AgentResponse::new(name).section(Section::toon(name, fields, rows).total_count(Some(n))).hint(..)` rendered with `render(OutputFormat::Text)`.
- For hints on a standalone list, use `append_hints(&mut out, &hints)`.

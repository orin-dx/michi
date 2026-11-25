---
michi-core: minor
---

**Breaking:** `AgentResponse` is now the one way to build an already-done report and the one place to add hints to a status report. The duplicate function and hint channel are removed.

- `render_already_done(op, summary, &hints)` → `AgentResponse::new("already_done").already_done(op, summary).hints(hints).render(OutputFormat::Text)`.
- `already_done(stored)` and the `AlreadyDone` enum are removed, at `michi_core::resilience` and at the crate root: match on your stored `Option` yourself.
- `StatusResponse::hints(vec)` and its `hints` field are removed: `StatusResponse::new(..).hints(h).render()` → `AgentResponse::new("status").status(s).hints(h).render(OutputFormat::Text)`. `StatusResponse::render()` stays, without a `help` line.

A response cuts table cells over 200 characters by default; add `.truncate_cells_at(usize::MAX)` for the old uncut text.

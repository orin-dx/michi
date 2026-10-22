---
michi-core: minor
michi-node: minor
---

Report what a list cap cut, and keep an error's cause chain apart from its hints.

- michi-core: new `TruncationOutcome<T>` (`kept`, `total_before`, `truncated()`, `omitted()`) reports what a cap on a list cut, however you chose the survivors. `truncate_by_count(items, max)` keeps the first `max`; `truncate_by_size(items, budget, size_of)` keeps items in order while your `size_of` (bytes, tokens, …) fits the budget. Wrap your own selection with `TruncationOutcome::new(kept, total_before)`. `hint(guidance)` returns `showing 2 of 5; {guidance}`, or `None` when nothing was cut.
- michi-core: `AgentResponse::truncation(&outcome, guidance)` sets `totalCount` and adds that hint in one call, so the two always agree. Pass `outcome.kept` to `.items(...)` as before.
- michi-core: `DomainError` has a `causes` field and a `.cause(text)` builder for why an error happened, separate from `hints` (what to do next). `render()` writes them as `causes[N]: …` after `message`; `render_json()`, serde and the JSON Schema add a `causes` array (`[]` when empty); with `miette`, `help()` lists them as `caused by: …` lines after the hints.
- michi-core: `DomainError::from_error(code, &err)` takes the message from `err` and one cause per `source()` link, outermost first, skipping a link whose text the previous one already includes (common with `#[error("…: {0}")]`).
- @orin-dx/michi: `renderDomainError` takes an optional fifth argument, `causes` (string array), rendered as `causes[N]: …`. Each cause is capped at 1 MiB and counts toward the call's 64 MiB limit.

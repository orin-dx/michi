---
michi-node: minor
---

**Breaking:** `AgentResponse` is now the one way to render an error, a batch's outcomes, pipeline state, a status report or an already-done report. The standalone functions that duplicated it are removed. Below, `r` is `new AgentResponse(name)`.

- `renderDomainError(code, message, hints, false, causes)` → `r.error(code, message, causes)`, a `r.hint(h)` per hint, then `r.renderText()`.
- `renderDomainError(code, message, [], true)` → `renderGithubAnnotation(code, message)`.
- `renderPartialSuccess(completed, failed, skipped)` → `r.partialSuccess('partial_success', completed, failed, skipped)`, then `r.renderText()`.
- `renderPipeline(steps)` → `r.pipeline('step', steps)`, then `r.renderText()`.
- `renderStatus(toolName, description, items, hints)` → `r.status(toolName, description, items)`, a `r.hint(h)` per hint, then `r.renderText()`.
- `renderAlreadyDone(operation, summary, hints)` → `r.alreadyDone(operation, summary)`, a `r.hint(h)` per hint, then `r.renderText()`.
- `alreadyDone(stored)` and the `AlreadyDone` type are removed: check your stored result yourself (`stored != null`).

The replacements also give you `renderJson()` and `toCallToolResult()`. Like any response, they cut a table cell over 200 characters and list it in the `truncated` block; the removed functions never cut.

New: `renderGithubAnnotation(code, message)` returns a GitHub Actions annotation, `::error title={code}::{message}`, on one line.

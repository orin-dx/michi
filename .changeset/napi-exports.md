---
michi-node: minor
---

@orin-dx/michi: four new exports cover the rest of the michi-core rendering surface.

- `renderPartialSuccess(completed, failed, skipped)` renders a partial-success report: the summary, `completed[N]`, the `failed[N]{operation,reason}:` table, `skipped[N]`, then the failed operations' recovery hints as one `recovery[N]{tool,params,reason}:` table. A failed op is `{ operation, reason, recovery?: { tool, reason? } }`.
- `renderPipeline(steps)` renders `step[N]{id,name,status,reason}:`. `status` and `reason` are typed `StepStatus` labels (`completed`, `outcome_unknown` with `timeout`, `skipped` with `prerequisite_failed`, …); an unknown status, a missing or unknown reason, or a reason on a status that takes none throws.
- `emptyStateWithHints(typeName, hints)` is `emptyState` plus a `help[N]: …` block, and throws `toon_validation_failed: …` for a reserved type name like `emptyState`.
- `alreadyDone(stored)` classifies an idempotency lookup: `{ done: true, result }` for any stored string, `{ done: false }` for `null` or `undefined`.
- All four check their inputs before converting them: at most 10,000 entries per list, 4 KiB per id, operation, status or reason label, 1 MiB per name, reason or hint, and 64 MiB of strings per call.

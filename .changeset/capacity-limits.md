---
michi-node: minor
---

Oversize inputs are now rejected quickly instead of costing seconds and gigabytes.

- michi-core: TOON lists are capped at 1,000 fields (`toon::MAX_FIELDS`), 1,000,000 cells (`toon::MAX_CELLS`) and a 256-char truncation hint (`toon::MAX_TRUNCATION_HINT_CHARS`). `ToonOptions::validate()` and `toon::list()` return the new `ToonError::TooManyFields`, `TooManyCells` and `TruncationHintTooLong` variants, and `AgentResponse` reports them as a `toon_validation_failed` error block with `isError` set. Valid lists render exactly as before, and `AgentResponse` no longer copies its rows on each render.
- @orin-dx/michi: collection arguments are checked before they are converted, and each call is limited to 1,000,000 cells and 64 MiB of strings (1 MiB per cell, hint or message; 4 KiB per type name, field name, key or truncation hint). Rejecting a 100,001-row `renderToon` input now takes well under a millisecond, and a JS string reused across many cells is counted every time it is used. TypeScript types are unchanged.
- michi-pipeline: `resume` and `Plan::new` are linear in the number of steps and dependencies; resuming 50,000 steps takes about 20 ms instead of seconds.

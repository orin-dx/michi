---
michi-core: minor
---

**Breaking:** `AgentResponse::hints(vec)` now replaces only the hints you added with `hint()` and `hints()`. A `truncated_section()`'s `showing N of M` line is kept (before, `hints()` dropped it), and so are the hints of `status()`, `partial_success()` and `error()` blocks. If you called `hints()` to clear everything, that line and those hints now stay.

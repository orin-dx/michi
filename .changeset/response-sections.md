---
michi-core: minor
---

michi-core: `AgentResponse` can hold several blocks. Add them with `.section(Section)`; a `Section` is a TOON list, a key-value block or a single text value, escaped like any TOON string (`Section::toon`, `Section::kv`, `Section::text`), optionally with a pre-truncation `total_count`. Sections render in the order you add them, then `help[]` and `recovery[]`.

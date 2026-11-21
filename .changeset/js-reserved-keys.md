---
michi-node: minor
---

**Breaking:** `renderToon`, `renderKv` and `kvItems` now reject more names, so caller data cannot pose as michi's own keys.

- `budget`, `sections`, `provenance`, `error`, `message`, `causes` and `exit_code` join `totalCount`, `help`, `recovery` and `truncated` as reserved, as a list type name or a key-value key. The error message lists the reserved keys.
- Rename any key or list type that matches one, for example a `message` field becomes `commit_message`.

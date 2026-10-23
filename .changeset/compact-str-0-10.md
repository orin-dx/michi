---
michi-core: minor
---

Breaking: michi-core now uses `compact_str` 0.10 (was 0.8). `toon::Value::Str` holds a 0.10 `CompactString` and `From<CompactString> for Value` takes one, so code that builds these from its own `CompactString` must upgrade `compact_str` to 0.10. Conversions from `&str` and `String` are unchanged.

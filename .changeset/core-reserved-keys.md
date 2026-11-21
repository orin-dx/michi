---
michi-core: minor
---

**Breaking:** a response's text and JSON can no longer contradict each other, and more names are reserved so caller data cannot pose as michi's own keys.

- A section's `truncated` is derived (its total is above its count). The budget's `truncated` is also true whenever a section was cut.
- A successful response can no longer render an `error:` / `message:` document from caller data.
- A provenance label `fingerprint` is always rejected (it read exactly like the real fingerprint).
- `budget`, `sections`, `provenance`, `error`, `message`, `causes` and `exit_code` join `totalCount`, `help`, `recovery` and `truncated` as reserved. `truncated` was already a reserved type name and is now a reserved KV key too. As a type name (`render_toon`, `list`), a KV key (`render_kv`, `kv_items`) or a section name, a reserved name is a validation error, and the message lists the reserved keys. Rename the offender, for example a `message` field becomes `commit_message`.
- `Section`'s fields are private: read them with `name()`, `body()`, `count()`, `total()` and `truncated()`. `Section::truncated(bool)` and `Section::returned_count(n)` are removed. A text section that stands for several items is `Section::text_with_count(name, body, n)`, and `SectionBody::Text` is now `Text { text, count }`.
- `SectionCost` and `BudgetSummary::section()` are removed, and the budget's JSON no longer has a `sections` array (the response's `sections` carries those counts). Drop `.section(SectionCost::new(..))` on budgets.
- `truncated_section` now takes `(name, fields, &outcome, row, guidance)` and builds the rows from `outcome.kept()`: `.truncated_section(Section::toon(name, f, rows), &outcome, g)` becomes `.truncated_section(name, f, &outcome, |item| row_of(item), g)`. Drop `.truncated(..)` on sections.

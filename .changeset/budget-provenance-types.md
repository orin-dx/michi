---
michi-core: minor
---

Added `BudgetSummary`, `SectionCost`, `ProvenanceBlock` and `ProvenanceEntry`: plain types for a response-level budget report and a provenance block, with builder methods, camelCase `serde::Serialize` behind the `serde` feature and `schemars::JsonSchema` behind `schemars`. They are not attached to `AgentResponse` yet.

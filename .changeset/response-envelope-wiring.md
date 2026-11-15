---
michi-core: minor
---

michi-core: `AgentResponse::budget(BudgetSummary)` and `AgentResponse::provenance(ProvenanceBlock)` attach a response-level `budget:` block and `provenance:` block. A response now renders its sections, then `budget:`, `provenance:`, `help[]` and `recovery[]`; the per-section roll-up under `budget:` appears only when the response has two or more sections. `render(Json)` and `to_call_tool_result().structured_content` gain `sections`, `budget` and `provenance` keys, present only when set (a response with no section, budget or provenance keeps exactly `body`, `hints`, `recovery` and `isError`). When two or more sections cut cells or report a `totalCount`, each is now one merged TOON table with a leading `section` column (`truncated[N]{section,row,field,removed}:`, `totalCount[N]{section,total}:`) instead of repeating a root key, which TOON forbids.

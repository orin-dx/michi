---
michi-core: minor
---

michi-core: Breaking: the NAPI byte budget is now 16 MiB, was 64 MiB, per call and per `AgentResponse`. It limited input only, and a render writes up to 19 times the bytes it holds (a control character is 6 bytes in the text, 13 in the JSON, 19 in `toCallToolResult()`). At 64 MiB a maximal response took 3.2 GiB and 25 s in `toCallToolResult()`, and a `renderJson()` over V8's string limit failed only after it was rendered. At 16 MiB every render is under that limit, so a response renders or is rejected before anything is rendered. In JavaScript, a call with more than 16 MiB of strings, or an `AgentResponse` that would hold more, now throws `input byte total N exceeds maximum of 16777216` or `cumulative byte total N exceeds maximum of 16777216` where up to 64 MiB was accepted. Every other limit, error message and TypeScript type is unchanged.

michi-core: Breaking: `michi_core::napi` (behind the `napi` feature) now exposes only `num`. The typed functions `render_toon(JsToonOptions)`, `render_hints`, `append_hints`, `render_recovery`, `render_kv`, `render_status`, `render_domain_error`, `render_already_done`, `render_partial_success`, `render_pipeline`, `already_done`, `empty_state_with_hints` and the `JsAgentResponse` Rust methods are private; the JS exports are the only entry points. `napi.rs` is split by concern into `napi/*.rs`.

michi-core: `ErrorCode` gains `ALL` and `FromStr`, and `StepStatus` gains `ALL` and `from_labels` (with `StepLabelError`): the inverse of `label()` and `reason()`, built from the same tables.

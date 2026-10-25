---
michi-core: minor
michi-node: minor
---

The Node package's TypeScript types lose their `Js` prefix: `ToonValue`, `ToonOptions`, `KvItem`, `RecoveryHint`, `StatusItem`, `FailedOp`, `PipelineStep`, `AlreadyDone`, `ContentBlock`, `Annotations`, `CallToolResult` and `Truncated` (the `AgentResponse` class already had none). Runtime behavior is unchanged except that error messages name the new type (`ToonValue must be an object`). `renderDomainError`'s `code` is now typed as the union of the nine error labels (`'invalid_input' | 'not_found' | ...`) instead of `string`; the runtime still rejects anything else. Migrate: rename the imported types.

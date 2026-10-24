---
michi-core: minor
michi-node: minor
---

Remove public items that nothing used. Migrate by deleting the call:

- `AlreadyDone`, `already_done()` and the Node `alreadyDone` are gone; keep the `Option<String>` you looked up. `render_already_done()` is unchanged.
- `Sensitive<T>` is gone. It only changed `Debug` and `Display` and no renderer accepted it; keep secrets out of hints, messages and causes.
- `telemetry` and `NoopProvider` are gone.
- `exit_code()` is gone from `DomainError`, `Error` and `PartialSuccess`. The rendered `exit_code: 1` line in error blocks is unchanged.

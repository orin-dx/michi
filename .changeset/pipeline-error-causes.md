---
michi-pipeline: minor
---

**Breaking:** converting an `ExecutionError` to a `DomainError` no longer puts step text in the trusted `message`.

- Before: `message` carried what the step or server said. Now: `message` is fixed text (`http 502`, `the step reported an error`, `step panicked`, `deadline passed`) after the step context `step <id> (<name>) failed: `.
- What the step or server said (an HTTP body, a `failed()` message, a panic payload) is one entry in `causes`, redacted and cut to 512 bytes including the `…` marker. Read it from `causes`, not `message`.
- An ambiguous timeout's `DomainError` now carries its "verify first" hint (it carried none before).

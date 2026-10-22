---
michi-core: patch
michi-pipeline: patch
michi-node: patch
---

Packaging and docs: the minimum supported Rust version is now 1.93 (was 1.96). The crates and npm packages ship their license text, michi-pipeline ships a README, docs.rs builds michi-core with the `serde`, `schemars` and `miette` features, the READMEs' links work on crates.io and npm, and michi-pipeline re-exports `RetryConfig`, `RetryConfigError`, `RetryBudget` and `IdempotencyKey`, so configuring retries needs no direct michi-core dependency.

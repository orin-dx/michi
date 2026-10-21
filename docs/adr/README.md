# Architecture decision records

ADRs record decisions that shape the workspace and would be costly or confusing to reverse. Current behavior belongs in [`docs/spec/`](../spec/README.md); workspace structure belongs in [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

Write an ADR when a decision changes crate boundaries, dependencies, public contracts, licensing, security, or another cross-cutting constraint. Do not record local implementation choices or every resolved API detail.

Use `Proposed` while a decision is under review and `Accepted` once it is implemented. To change an accepted decision, add a new ADR that names the one it supersedes; preserve the old record. Cite the source and alternatives considered.

## Decisions

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-async-pipeline-separate-crate.md) | Keep async pipeline execution in its own crate. | Accepted |
| [0002](0002-consumer-integration-before-crates-io.md) | Require an external consumer integration before the first crates.io release. | Superseded by ADR 4 |
| [0003](0003-fsl-1-1-mit-license.md) | License the workspace under FSL-1.1-MIT. | Accepted |
| [0004](0004-publish-before-consumer-integration.md) | Publish an initial version before requiring downstream integration. | Accepted |
| [0005](0005-two-crates-one-version.md) | Publish `michi-core` and `michi-pipeline` on one version; no facade. Amends ADR 1's dependency detail. | Accepted |

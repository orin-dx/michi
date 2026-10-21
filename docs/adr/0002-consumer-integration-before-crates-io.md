# 2. Require a consumer integration before the first crates.io release

Status: Superseded by [ADR 4](0004-publish-before-consumer-integration.md)

## Context

Internal tests show that the workspace works for itself. They do not show that an external Rust consumer can depend on the published API.

## Decision

Before the first crates.io release, require at least one integration test from a consumer outside this repository against a tagged Michi revision. Use a tagged Git dependency during early integration; publish to crates.io once the integration is proven.

## Alternative

Publish once Michi's internal tests pass. Rejected because internal tests do not verify downstream integration or crate usability.

## Consequences

The release gate is based on evidence, not a preselected consumer roster. Michi releases first; a consumer can move from the tagged Git dependency to the published crate afterward.

## Sources

- `PRINCIPLES.md`, “Publish/release gates are earned”
- `docs/spec/05-scope-and-quality.md`, “Versioning and release”
- `docs/spec/06-decisions.md`, “Open until a real integration exists”

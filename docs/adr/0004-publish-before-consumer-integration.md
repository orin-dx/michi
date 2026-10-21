# 4. Publish the initial version before requiring consumer integration

Status: Accepted

## Context

Consumers need a versioned Michi package to test the published API. Requiring a consumer integration before the first release makes that test depend on a package that does not yet exist.

## Decision

Publish an initial version after reviewing the package contents, selected versions, and release scope. Use it to validate integration in an external consumer. Treat downstream findings as input to follow-up releases; consumer integration is not a prerequisite for the first publication.

## Alternative

Require a consumer to test a tagged Git dependency before publication. Rejected as the release gate: that tests Git dependency integration, not the versioned package consumers are expected to use.

## Consequences

The first release establishes the versions downstream projects can depend on. Keep release scope and package contents explicit, then record consumer integration results before claiming downstream compatibility or stability.

## Supersedes

[ADR 2](0002-consumer-integration-before-crates-io.md), which required consumer integration before the first crates.io release.

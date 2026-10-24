# Callisto publishing migration

Status: Callisto 0.10 adopted; release sequence set by ADR 4. The release workflow has since moved to Callisto 0.12 (`.github/workflows/callisto-release.yml`); the evidence below is from the 0.10 migration.

## Evidence

The Callisto 0.10 CLI recognizes all three `napi.targets`. Package rules require ecosystem-qualified IDs; the previous path-like selectors matched no packages. The corrected selectors produce no status warnings.

The new workflow pins Callisto's setup and release actions to `callisto@0.10.0`. It creates version PRs with `mode: version-pr`, accepts a release only from a verified merge of the managed PR, and checks every NAPI artifact before publishing. NAPI no longer creates a separate GitHub release; Callisto owns tags and releases.

A dry run on this workspace plans all current Cargo and npm package versions because Callisto has no release history for these packages. Review that package set and the selected versions before the first release. External consumer integration follows publication; see [ADR 4](../adr/0004-publish-before-consumer-integration.md).

## Verification limit

`actionlint` and `git diff --check` pass. Callisto's 0.10 matrix and status commands pass. The local pnpm shim could not update its permissions, so the NAPI CLI help check could not run. No publication was attempted.

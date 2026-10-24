# 5. Publish two crates on one version

Status: Accepted. Amended 2026-10-22: a third crate, `michi-graph` (MIT, for license containment), joined the fixed group and is not re-exported from `michi-core`; `michi-pipeline` pins it exactly like `michi-core`. Versions are now written per crate by Callisto, not in `[workspace.package]`. See `docs/spec/06-decisions.md`, "Resolved 2026-10-22 — michi-graph: an MIT leaf crate".

## Context

Before the first release the workspace had five crates (`michi`, `michi-core`, `michi-toon`, `michi-truncate`, `michi-resilience`) plus `michi-pipeline`, each versioned on its own. No consumer used a leaf crate alone, every change cascaded version bumps across the chain, and the `michi` name on crates.io belongs to an unrelated project.

## Decision

Publish two crates, `michi-core` (every synchronous primitive, with `toon`, `truncate` and `resilience` as modules) and `michi-pipeline` (async execution on Tokio), plus the `@orin-dx/michi` npm package. All three share one version through a Callisto `[[fixed-group]]`, and `michi-pipeline` pins `michi-core` with an exact `=` requirement.

- `michi-pipeline` stays separate for the reason ADR 1 gives: Tokio must not enter the default build.
- A part is split out again only for a heavy dependency or once it becomes a deliberately stable interface, and it is then re-exported from `michi-core` so consumers see no change.

## Alternatives

- Keep the split with independent versions. Rejected: separate crates only contain breaking changes when versioned independently, and every part changes at the same pre-1.0 pace, so the split bought release overhead without protecting consumers.
- Keep the split with lockstep versions. Rejected: lockstep removes the one benefit of the split, and cross-crate fixes need extra public API.
- Merge `michi-pipeline` too. Rejected: it would put Tokio in every consumer's build.

## Consequences

Rust paths change from `michi::` to `michi_core::`. One release covers every package, and a change anywhere bumps all three. A clean release build measured michi's own crates at about 1.9 s of 3.96 s, so build time was not a reason to keep the split.

## Supersedes

The dependency detail of [ADR 1](0001-async-pipeline-separate-crate.md): `michi-pipeline` now depends on `michi-core` alone, and there is no `michi` facade. ADR 1's decision to keep execution in its own crate stands.

## Sources

- `docs/spec/06-decisions.md`, "Resolved 2026-10-21 — Two crates, one version"
- `callisto.toml`, `Cargo.toml` (`[workspace.package] version`)

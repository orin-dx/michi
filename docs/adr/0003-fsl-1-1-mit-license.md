# 3. License the workspace under FSL-1.1-MIT

Status: Accepted. Amended 2026-10-22: `michi-graph` is MIT (see `docs/spec/06-decisions.md`, "Resolved 2026-10-22 — michi-graph: an MIT leaf crate"). Every other crate and the Node package stay FSL-1.1-MIT.

## Context

The former AGPL license conflicted with FSL-licensed suite consumers that link Michi. Splitting a permissive core was considered but is not part of the current workspace.

## Decision

License every workspace crate and the Node package under FSL-1.1-MIT. Each release converts to MIT after two years under the license terms.

## Alternatives

- Keep AGPL, which preserves the redistribution conflict for FSL suite consumers.
- Split out a permissive core, which adds a license boundary the workspace does not currently need.

## Consequences

Suite consumers can link Michi without an AGPL redistribution conflict. No crate is designated as a permissively licensed core.

## Sources

- Commit `48481a7`, which changed the workspace license
- `docs/spec/06-decisions.md`, “Resolved 2026-09-04 — License changed from AGPL-3.0-or-later to FSL-1.1-MIT”

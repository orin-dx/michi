# Contributing to michi

By participating in this project, you agree to abide by the [Code of Conduct](./CODE_OF_CONDUCT.md). Found a security issue? See [SECURITY.md](./SECURITY.md) instead of opening a public issue.

Thanks for building with us. `michi` is a narrow workspace of response formatting primitives for developer and agent tools.

If you are proposing a new primitive, read [`PRINCIPLES.md`](PRINCIPLES.md) first to understand the inclusion gates.

## Local Setup

```bash
git clone git@github.com:orin-dx/michi.git
cd michi
just build
just test
```

Tool versions are managed by `proto` (`.prototools`). Running `just --list` shows all available local tasks.

## Before Opening a Pull Request

```bash
just check-all   # Run formatting checks, clippy, rustdoc, and unit tests
```

Ensure all lints and tests pass cleanly.

## Development Rules

- **Zero panics in library code**: Avoid `unwrap()`, `expect()`, or `panic!` in non-test library code.
- **No `unsafe` outside NAPI glue**: `unsafe` is restricted to michi-core's `napi` module (`crates/michi-core/src/napi.rs` and `napi/`) for FFI bindings (`unsafe_code = "deny"` workspace-wide everywhere else).
- **Document public items**: Every `pub` item requires a clear Rustdoc comment.
- **Pre-allocate string buffers**: Use `String::with_capacity` when building strings.
- **UTF-8 char safety**: Truncate strings on Unicode scalar boundaries using `floor_char_boundary`.
- **Markdown formatting**: Markdown prose is not hard-wrapped — one line per paragraph (`just fmt-md`).

## Changing public API

Spec first: the spec describes the API, so a change to the API changes the spec in the same pull request. Write (or update) the acceptance criteria and the `docs/spec/` entry before the code, then make the code match.

CI runs `scripts/check-api-docs.py` on every pull request. If the diff adds or removes a `pub` item (`fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`, `use`, `union`, or a `pub` field) under `crates/*/src/`, it must also touch `docs/spec/`, `docs/adr/`, a crate `README.md`, `README.md` or `ARCHITECTURE.md`. Test modules, doc-comment edits and `pub(crate)` items don't count. Run it locally with `python3 scripts/check-api-docs.py` (it compares against `origin/main`).

It only checks that some doc moved, so read the spec you touched and make sure it covers the change. For a change that truly needs no doc (a rename inside a private module that re-exports the same names, say), put a line in the pull request body that starts `No spec change needed:` and gives the reason, then re-run the job.

## Codebase Map

| Target Task              | Path / Location                               |
| ------------------------ | --------------------------------------------- |
| Documentation map        | `docs/README.md`                              |
| Inclusion rules          | `PRINCIPLES.md`                               |
| Crate architecture       | `ARCHITECTURE.md`                             |
| Truncation primitives    | `crates/michi-core/src/truncate.rs`           |
| Resilience & retry math  | `crates/michi-core/src/resilience.rs`         |
| TOON formatting          | `crates/michi-core/src/toon/`                 |
| Core AXI types           | `crates/michi-core/src/`                      |
| Async pipeline execution | `crates/michi-pipeline/`                      |
| Dependency graphs (MIT)  | `crates/michi-graph/`                         |
| NAPI exports             | `crates/michi-core/src/napi.rs`               |
| Node.js package shim     | `packages/michi-node/`                        |
| Tests, benches, examples | `crates/michi-core/{tests,benches,examples}/` |
| TOON spec & grammar      | `docs/spec/02-toon-format.md`                 |

## Fuzzing

`fuzz/` holds [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) targets that attack michi's claims: no panics, truncation on char boundaries, escaping that keeps one value on one line, and tolerant MCP wire input. It is a standalone project outside the workspace (nightly only, its own lockfile), so `just check` and CI never build it.

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
just fuzz toon_render        # 60 seconds; `just fuzz toon_render 600` for ten minutes
```

| Target | Property it asserts |
| --- | --- |
| `toon_render` | `Ok` output is canonical TOON 4.4 (strict test decoder), its line count matches the header, no raw CR/LF in a cell, cells decode back |
| `truncate` | Results stay within the bound, keep a whole-char prefix and report the cut exactly; the count and size caps keep a prefix |
| `resilience` | `parse_retry_after` and the retry config never panic; delays stay in `[base, max_delay]` and never shorten a `Retry-After` |
| `mcp_wire` | Arbitrary JSON into every `Deserialize` type is rejected cleanly or round-trips; the tolerant MCP input rules hold |
| `graph` | Validation matches an independent oracle; order, layers, closures and ready sets of a valid graph are consistent |
| `plan` | `Plan::new` matches the same oracle; a valid plan's `execution_order` is topological and instant steps run to success |

The weekly `Fuzz` workflow runs every target for five minutes and can be started by hand with a chosen duration. A crash fails the job and uploads the input as an artifact.

A crash is a bug, so it needs a regression test, not just a stored artifact:

1. Reproduce it with `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>` and minimize it with `cargo +nightly fuzz tmin <target> <file>`.
2. Write the smallest input that still fails as a unit or integration test in the affected crate (`crates/<crate>/tests/`), and watch it fail for the right reason.
3. Fix the bug, then add a patch changeset for the crate if the fix changes behavior.
4. If the fuzz target's own property was wrong (an overreaching assertion, not a library bug), fix the target and say why in the commit.

## Releasing

[Callisto](https://github.com/orin-dx/callisto) versions and publishes `michi-core`, `michi-pipeline`, `michi-graph` and `@orin-dx/michi`. They're a fixed group (`michi`), so a change to any of them releases all four on one version.

1. With your change, add a changeset: a Markdown file in `.changeset/` (any name, e.g. `.changeset/toon-quote-tabs.md`). The front matter maps each changed package to a bump (`major`, `minor`, `patch` or `none`); the body is the changelog entry, written for users of the package.

   ```markdown
   ---
   michi-core: patch
   ---

   `render_toon` quotes cells that contain a tab.
   ```

   Package names are `michi-core`, `michi-pipeline`, `michi-graph` and `michi-node` (the npm package). Changes with no user-facing effect don't need one.

2. Merging to `main` opens or updates Callisto's version PR (`callisto/version-packages`), which bumps versions and writes the changelogs.
3. Merging the version PR runs the Release workflow (`.github/workflows/callisto-release.yml`): it builds the native binaries, publishes to crates.io and npm, and creates one GitHub release tagged `michi@{version}`.

**Post-release check.** When the run published a package to a registry, or a re-run adopted one from a failed earlier attempt, the `smoke-published` job installs that version from the public registries into clean projects on Linux x64, Linux arm64 and macOS arm64 and uses it: `npm install @orin-dx/michi`, render TOON, then build a program against `michi-core`, `michi-pipeline` and `michi-graph` from crates.io. It retries for up to 10 minutes while the registries catch up. It also fails if any of the seven npm packages lacks a provenance attestation (`SMOKE_REQUIRE_ATTESTATIONS=1`): `@orin-dx/michi` is published by pnpm and the platform packages by npm, and pnpm only reads `PNPM_CONFIG_PROVENANCE`, so releases through 0.7.1 shipped the main package unattested. A red job means the published packages are broken, so fix forward with a new release. Run the same check by hand with `just smoke-published <version>`.

The first publish of a brand-new npm package can fail with Callisto error E157: npm accepted the upload but doesn't show the version yet. That's registry propagation lag. Wait a few minutes, then use "Re-run failed jobs" on the Release run. It adopts what already landed and publishes the rest. Don't regenerate the release intent.

Each FSL-licensed published package carries a copy of the root `LICENSE` (`crates/michi-core/LICENSE`, `crates/michi-pipeline/LICENSE`, `packages/michi-node/LICENSE`, `packages/michi-node/npm/*/LICENSE`); `michi-graph` has its own MIT `LICENSE`. If the root license changes, copy it to each; `crates/michi-core/tests/license_copies.rs` fails until they match.

## Commit Style

Use imperative conventional commit prefixes: `feat:`, `fix:`, `chore:`, `test:`, `docs:`, `ci:`.

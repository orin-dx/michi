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

**Post-release check.** When the run published a package, the `smoke-published` job installs that version from the public registries into clean projects on Linux x64, Linux arm64 and macOS arm64 and uses it: `npm install @orin-dx/michi`, render TOON, then build a program against `michi-core`, `michi-pipeline` and `michi-graph` from crates.io. It retries for up to 10 minutes while the registries catch up. A red job means the published packages are broken, so fix forward with a new release. Run the same check by hand with `just smoke-published <version>`.

The first publish of a brand-new npm package can fail with Callisto error E157: npm accepted the upload but doesn't show the version yet. That's registry propagation lag. Wait a few minutes, then use "Re-run failed jobs" on the Release run. It adopts what already landed and publishes the rest. Don't regenerate the release intent.

Each FSL-licensed published package carries a copy of the root `LICENSE` (`crates/michi-core/LICENSE`, `crates/michi-pipeline/LICENSE`, `packages/michi-node/LICENSE`, `packages/michi-node/npm/*/LICENSE`); `michi-graph` has its own MIT `LICENSE`. If the root license changes, copy it to each; `crates/michi-core/tests/license_copies.rs` fails until they match.

## Commit Style

Use imperative conventional commit prefixes: `feat:`, `fix:`, `chore:`, `test:`, `docs:`, `ci:`.

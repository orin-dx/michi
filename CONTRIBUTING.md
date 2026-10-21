# Contributing to michi

By participating in this project, you agree to abide by the [Code of Conduct](./CODE_OF_CONDUCT.md). Found a security issue? See [SECURITY.md](./SECURITY.md) instead of opening a public issue.

Thanks for building with us. `michi` is a narrow workspace of response formatting primitives for developer and agent tools.

If you are proposing a new primitive, read [`PRINCIPLES.md`](PRINCIPLES.md) first to understand the inclusion gates.

## Local Setup

```bash
git clone git@github.com:orin-axi/michi.git
cd michi
just build
just test
```

Tool versions are managed by `proto` (`.prototools`). Running `just --list` shows all available local tasks.

## Before Opening a Pull Request

```bash
just check-all   # Run formatting checks, clippy, WASM target check, rustdoc, and unit tests
```

Ensure all lints and tests pass cleanly.

## Development Rules

- **Zero panics in library code**: Avoid `unwrap()`, `expect()`, or `panic!` in non-test library code.
- **No `unsafe` outside NAPI glue**: `unsafe` is restricted to `src/napi.rs` for FFI bindings (`#![deny(unsafe_code)]` everywhere else).
- **Document public items**: Every `pub` item requires a clear Rustdoc comment.
- **Pre-allocate string buffers**: Use `String::with_capacity` when building strings.
- **UTF-8 char safety**: Truncate strings on Unicode scalar boundaries using `floor_char_boundary`.
- **Markdown formatting**: Markdown prose is not hard-wrapped — one line per paragraph (`just fmt-md`).

## Codebase Map

| Target Task              | Path / Location               |
| ------------------------ | ----------------------------- |
| Documentation map        | `docs/README.md`               |
| Inclusion rules          | `PRINCIPLES.md`               |
| Crate architecture       | `ARCHITECTURE.md`             |
| Truncation primitives    | `crates/michi-truncate/`      |
| Resilience & retry math  | `crates/michi-resilience/`    |
| TOON formatting & parser | `crates/michi-toon/`          |
| Core AXI types           | `crates/michi-core/`          |
| NAPI / Node.js bindings  | `packages/michi-node/`        |
| TOON spec & grammar      | `docs/spec/02-toon-format.md` |

## Commit Style

Use imperative conventional commit prefixes: `feat:`, `fix:`, `chore:`, `test:`, `docs:`, `ci:`.

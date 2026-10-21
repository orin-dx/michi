# michi Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `michi` v0.1.0 — a complete Rust crate of pure AXI rendering primitives with NAPI npm wrapper, full test suite, benchmarks, and CI.

**Architecture:** One Cargo workspace (root lib crate + `packages/michi-node` NAPI cdylib). All 11 core modules are sync/pure — zero async, zero runtime deps. Feature flags gate optional extensions; this plan only implements default features + `napi`. Pipeline execution (`pipeline` feature) is Plan 2.

**Tech Stack:** Rust 2021 / 1.96 MSRV, thiserror 2, serde 1, napi/napi-derive 3, divan 0.1, insta 1 (yaml), proptest 1, cargo-nextest, cargo-llvm-cov, cargo-deny, just, pnpm 9, @napi-rs/cli 3.

**Scope note:** `resilience/policy.rs`, `resilience/circuit.rs`, `pipeline/executor.rs`, `sink/`, `fuzzy/`, `cache/`, `adapters/` are created as empty `#[cfg(feature = "pipeline")] // Plan 2` stubs only. Do not implement them here.

---

## Non-negotiables

1. `rust-version = "1.96"` in workspace Cargo.toml
2. No `unwrap()` / `expect()` in library code — only in tests (with message) or on infallible `writeln!` into String (comment `// infallible`)
3. No `unsafe` except inside `#[napi]` boundary (managed by napi-rs)
4. `BTreeMap` for any map appearing in rendered output; `HashMap` only for internal intermediate state
5. Pre-allocate strings: `String::with_capacity(estimate)` before any render loop
6. Unicode safety: truncation uses char boundaries — `str::floor_char_boundary` or `char_indices`
7. Every `pub` item has a doc comment
8. `#[derive(Debug)]` on all public types
9. `#[must_use]` on `AgentResponse` builder methods
10. napi: `version = "3"`, `#[napi(catch_unwind)]` on every export, no `u64` across boundary
11. Benchmarks use `divan` (not criterion)
12. Snapshots committed with `cargo insta review`, CI sets `INSTA_UPDATE=no`
13. All tests run with `cargo nextest run`

---

## File map

```
michi/
  Cargo.toml                      Task 1 — workspace + lib package
  rust-toolchain.toml             Task 1
  build.rs                        Task 1
  justfile                        Task 1
  .rustfmt.toml                   Task 1
  .clippy.toml                    Task 1
  .typos.toml                     Task 1
  deny.toml                       Task 19
  CLAUDE.md                       Task 1
  src/
    lib.rs                        Task 15
    toon/
      mod.rs                      Task 3
      escape.rs                   Task 2
      render.rs                   Task 2
    kv/
      mod.rs                      Task 4
    hints.rs                      Task 5
    truncate.rs                   Task 6
    empty.rs                      Task 7
    error.rs                      Task 8
    idempotency.rs                Task 9
    resilience/
      mod.rs                      Task 10
      policy.rs                   Task 10 (Plan 2 stub)
      circuit.rs                  Task 10 (Plan 2 stub)
    status.rs                     Task 11
    recovery.rs                   Task 12
    response.rs                   Task 13
    pipeline/
      mod.rs                      Task 14
      executor.rs                 Task 14 (Plan 2 stub)
    telemetry/
      mod.rs                      Task 14 (always, zero-cost NoopProvider)
    sink/
      mod.rs                      Task 14 (Plan 2 stub)
    napi.rs                       Task 17
  benches/
    toon_render.rs                Task 16
    kv_render.rs                  Task 16
  tests/
    toon_integration.rs           Task 3
    kv_integration.rs             Task 4
    snapshot_tests.rs             Task 16
  packages/
    michi-node/
      Cargo.toml                  Task 17
      package.json                Task 17
      index.js                    Task 17
      src/lib.rs                  Task 17
      __test__/
        index.test.mjs            Task 17
  .github/workflows/ci.yml        Task 18
```

---

## Task 1: Repo scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `build.rs`
- Create: `justfile`
- Create: `.rustfmt.toml`
- Create: `.clippy.toml`
- Create: `.typos.toml`
- Create: `CLAUDE.md`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[workspace]
members  = [".", "packages/michi-node"]
resolver = "2"

[workspace.package]
edition      = "2021"
rust-version = "1.96"
license      = "AGPL-3.0-or-later"
repository   = "https://github.com/orin-dx/michi"
authors      = ["orin-dx"]

[workspace.dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"

[profile.release]
opt-level     = 3
lto           = "thin"
codegen-units = 1

[profile.bench]
inherits = "release"
debug    = true

[package]
name        = "michi"
version     = "0.1.0"
description = "AXI response primitives for agent-ergonomic tools"
keywords    = ["axi", "agent", "mcp", "cli", "llm"]
categories  = ["text-processing", "encoding", "development-tools"]
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true

[features]
default  = []
pipeline = ["dep:tokio", "dep:tokio-util", "dep:async-trait", "dep:uuid"]
fuzzy    = ["dep:nucleo-matcher", "pipeline"]
cache    = ["dep:moka", "dep:sha2", "pipeline"]
cli      = ["dep:indicatif", "dep:inquire", "dep:crossterm", "dep:ctrlc", "dep:atty", "pipeline"]
mcp      = ["pipeline"]
napi     = ["dep:napi", "dep:napi-derive"]
full     = ["pipeline", "fuzzy", "cache", "cli", "mcp"]

[dependencies]
serde.workspace      = true
serde_json.workspace = true
thiserror.workspace  = true

# pipeline feature
tokio        = { version = "1", features = ["full"], optional = true }
tokio-util   = { version = "0.7", features = ["rt"], optional = true }
async-trait  = { version = "0.1", optional = true }
uuid         = { version = "1", features = ["v4"], optional = true }

# fuzzy feature
nucleo-matcher = { version = "0.3", optional = true }

# cache feature
moka = { version = "0.12", features = ["future"], optional = true }
sha2 = { version = "0.10", optional = true }

# cli feature
indicatif = { version = "0.17", optional = true }
inquire   = { version = "0.7", optional = true }
crossterm = { version = "0.27", optional = true }
ctrlc     = { version = "3", optional = true }
atty      = { version = "0.2", optional = true }

# napi feature
napi        = { version = "3", features = ["napi6"], optional = true }
napi-derive = { version = "3", optional = true }

[build-dependencies]
napi-build = { version = "2", optional = true }

[dev-dependencies]
divan   = "0.1"
proptest = "1"
insta   = { version = "1", features = ["yaml"] }

[[bench]]
name    = "toon_render"
harness = false

[[bench]]
name    = "kv_render"
harness = false
```

- [ ] **Step 2: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 3: Write `build.rs`**

```rust
fn main() {
    #[cfg(feature = "napi")]
    {
        extern crate napi_build;
        napi_build::setup();
    }
}
```

Wait — `napi-build` is optional, so use a different pattern:

```rust
fn main() {
    if std::env::var("CARGO_FEATURE_NAPI").is_ok() {
        // napi-build is only available when the napi feature is active
        // This is invoked by packages/michi-node, not the root crate directly
    }
}
```

Actually for the root crate `build.rs`, the correct approach is:

```rust
fn main() {}
```

The `napi-build` setup belongs in `packages/michi-node/build.rs`. Write an empty `build.rs` at the root (or omit it). Write `packages/michi-node/build.rs` in Task 17.

- [ ] **Step 4: Write `.rustfmt.toml`**

```toml
edition              = "2021"
max_width            = 120
use_small_heuristics = "Max"
```

- [ ] **Step 5: Write `.clippy.toml`**

```toml
msrv                        = "1.96"
too-many-arguments-threshold = 6
too-many-lines-threshold     = 150
```

- [ ] **Step 6: Write `.typos.toml`**

```toml
[default]
extend-ignore-identifiers-re = [
    # Add known false-positives here, e.g. domain-specific spellings
]

[files]
extend-exclude = ["*.snap", "Cargo.lock"]
```

- [ ] **Step 7: Write `justfile`**

```justfile
default:
    @just --list

# ── Build ──────────────────────────────────────────────────────────────────
build:
    cargo build --workspace

build-release:
    cargo build --release --workspace

build-node:
    cd packages/michi-node && pnpm build --platform

build-node-release:
    cd packages/michi-node && pnpm build --platform --release

# ── Test ───────────────────────────────────────────────────────────────────
test: test-rust test-node

test-rust:
    cargo nextest run --workspace

test-rust-all:
    cargo nextest run --workspace --all-features

test-node: build-node
    cd packages/michi-node && pnpm test

# ── Lint ───────────────────────────────────────────────────────────────────
check: fmt-check clippy deny typos

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --workspace --all-features -- -D warnings

deny:
    cargo deny check

typos:
    typos

# ── Snapshots ──────────────────────────────────────────────────────────────
snapshots:
    cargo insta review

# ── Benchmarks ─────────────────────────────────────────────────────────────
bench:
    cargo bench --workspace

# ── Coverage ───────────────────────────────────────────────────────────────
coverage:
    cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

# ── Pre-push ───────────────────────────────────────────────────────────────
ci: check test
```

- [ ] **Step 8: Write `CLAUDE.md`**

```markdown
# michi — Agent Instructions

## Commands
- `just test` — run all tests (Rust + Node)
- `just test-rust` — Rust tests only (cargo nextest)
- `just check` — fmt + clippy + deny + typos
- `just build-node` — compile NAPI binary for local platform
- `just bench` — run divan benchmarks
- `just snapshots` — review insta snapshot diffs
- `just coverage` — generate lcov.info

## Non-negotiables
- No `unwrap()`/`expect()` in lib code (tests OK with message)
- No `unsafe` outside napi boundary
- `BTreeMap` for output maps, `HashMap` for internal state only
- Pre-allocate strings with `String::with_capacity`
- Truncation uses char boundaries (`floor_char_boundary`)
- Every `pub` item has a doc comment
- NAPI: `version = "3"`, `#[napi(catch_unwind)]` on every export
- Benchmarks: divan (not criterion)
- Tests: `cargo nextest run` (not `cargo test`)

## Architecture
- `src/` — pure sync rendering library (default features: zero runtime deps)
- `packages/michi-node/` — thin NAPI cdylib shim, published as npm `michi`
- Feature flags: `pipeline` (tokio), `fuzzy`, `cache`, `cli`, `mcp`, `napi`
- See `docs/superpowers/specs/2026-07-03-michi-design.md` for decisions
- See `docs/01-spec.md` for full module API reference

## Module guide
| Module | Purpose |
|---|---|
| `toon` | TOON list rendering — token-optimized agent list format |
| `kv` | Key-value single-item rendering |
| `hints` | `help[]` hint blocks |
| `truncate` | Token-safe content truncation |
| `empty` | Definitive empty state responses |
| `error` | Unified `michi::Error` type with agent rendering |
| `idempotency` | Idempotency keys, already-done detection |
| `resilience` | Retry config, delay calculation, retry-after parsing |
| `status` | Health/status response rendering |
| `recovery` | Recovery hint blocks |
| `response` | `AgentResponse` builder — composes all primitives |
| `pipeline` | Pipeline pure data type + render (execution in Plan 2) |
```

- [ ] **Step 9: Verify the workspace compiles**

```bash
cargo check --workspace
```

Expected: error about missing `src/lib.rs` and `packages/michi-node` — that's fine. Proceed to Task 2.

- [ ] **Step 10: Initialize git and commit scaffold**

```bash
git init
git remote add origin git@github.com:orin-dx/michi.git
git add Cargo.toml rust-toolchain.toml build.rs justfile .rustfmt.toml .clippy.toml .typos.toml CLAUDE.md
git commit -m "chore: repo scaffold — workspace, justfile, tooling config"
```

---

## Task 2: toon escape + render internals

**Files:**
- Create: `src/toon/escape.rs`
- Create: `src/toon/render.rs`

These are internal modules — no public API yet. Tests come in Task 3.

- [ ] **Step 1: Write `src/toon/escape.rs`**

```rust
/// Escape a scalar value for TOON row output.
///
/// Values containing commas or double-quotes are wrapped in double-quotes with
/// internal double-quotes escaped as `\"`. Null/empty values render as empty
/// string (the comma delimiter is still emitted by the caller).
pub(crate) fn escape_value(v: &str) -> std::borrow::Cow<'_, str> {
    if v.is_empty() {
        return std::borrow::Cow::Borrowed(v);
    }
    if v.contains(',') || v.contains('"') || v.contains('\n') {
        let mut out = String::with_capacity(v.len() + 2);
        out.push('"');
        for ch in v.chars() {
            if ch == '"' {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push('"');
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_value_is_not_escaped() {
        assert_eq!(escape_value("hello"), "hello");
    }

    #[test]
    fn value_with_comma_is_quoted() {
        assert_eq!(escape_value("a,b"), r#""a,b""#);
    }

    #[test]
    fn value_with_quote_escapes_it() {
        assert_eq!(escape_value(r#"say "hi""#), r#""say \"hi\"""#);
    }

    #[test]
    fn empty_value_is_unchanged() {
        assert_eq!(escape_value(""), "");
    }

    #[test]
    fn value_with_newline_is_quoted() {
        assert_eq!(escape_value("line\nbreak"), "\"line\nbreak\"");
    }
}
```

- [ ] **Step 2: Write `src/toon/render.rs`**

```rust
use super::{ToonOptions, Value};
use crate::toon::escape::escape_value;

/// Render a TOON document from options.
///
/// Pre-allocates output capacity based on row count × estimated row width.
pub(crate) fn render(opts: &ToonOptions) -> String {
    let row_count = opts.rows.len();
    let field_count = opts.fields.len();
    // Estimate: header ~60 chars + rows ~40 chars each + hints ~50 chars each
    let capacity = 60 + row_count * (field_count * 12 + 10) + opts.hints.len() * 60;
    let mut out = String::with_capacity(capacity);

    // type_name[count]{field,field,...}:
    out.push_str(&opts.type_name);
    out.push('[');
    out.push_str(&row_count.to_string());
    out.push_str("]{");
    for (i, field) in opts.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(field);
    }
    out.push_str("}:\n");

    // rows
    for row in &opts.rows {
        out.push_str("  ");
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let s = match val {
                Value::Str(s) => escape_value(s),
                Value::Int(n) => std::borrow::Cow::Owned(n.to_string()),
                Value::Float(f) => std::borrow::Cow::Owned(f.to_string()),
                Value::Bool(b) => std::borrow::Cow::Borrowed(if *b { "true" } else { "false" }),
                Value::Null => std::borrow::Cow::Borrowed(""),
            };
            out.push_str(&s);
        }
        out.push('\n');
    }

    // totalCount (if set and differs from row count, or always if Some)
    if let Some(total) = opts.total_count {
        out.push_str("totalCount: ");
        out.push_str(&total.to_string());
        out.push('\n');
    }

    // help[N]: hints
    if !opts.hints.is_empty() {
        out.push_str("help[");
        out.push_str(&opts.hints.len().to_string());
        out.push_str("]:\n");
        for hint in &opts.hints {
            out.push_str("  ");
            out.push_str(hint);
            out.push('\n');
        }
    }

    out
}
```

- [ ] **Step 3: Commit**

```bash
git add src/toon/escape.rs src/toon/render.rs
git commit -m "feat(toon): escape and render internals"
```

---

## Task 3: toon public API + tests

**Files:**
- Create: `src/toon/mod.rs`
- Create: `tests/toon_integration.rs`

- [ ] **Step 1: Write failing test for basic TOON render**

Create `tests/toon_integration.rs`:

```rust
use michi::toon::{render_toon, ToonOptions, Value};

#[test]
fn renders_basic_list() {
    let opts = ToonOptions {
        type_name: "issue".into(),
        fields: vec!["number".into(), "title".into(), "state".into()],
        rows: vec![
            vec![Value::Int(42), Value::Str("Fix login".into()), Value::Str("open".into())],
            vec![Value::Int(43), Value::Str("Add dark mode".into()), Value::Str("open".into())],
        ],
        total_count: Some(47),
        hints: vec!["Call get_issue with number=<number> for full detail".into()],
    };
    let out = render_toon(&opts);
    assert_eq!(
        out,
        "issue[2]{number,title,state}:\n  42,Fix login,open\n  43,Add dark mode,open\ntotalCount: 47\nhelp[1]:\n  Call get_issue with number=<number> for full detail\n"
    );
}

#[test]
fn renders_empty_state() {
    let opts = ToonOptions {
        type_name: "issue".into(),
        fields: vec![],
        rows: vec![],
        total_count: Some(0),
        hints: vec!["Try list_issues with a broader filter".into()],
    };
    let out = render_toon(&opts);
    assert_eq!(
        out,
        "issue[0]{}:\ntotalCount: 0\nhelp[1]:\n  Try list_issues with a broader filter\n"
    );
}

#[test]
fn escapes_comma_in_value() {
    let opts = ToonOptions {
        type_name: "item".into(),
        fields: vec!["name".into()],
        rows: vec![vec![Value::Str("Update deps, bump major".into())]],
        total_count: None,
        hints: vec![],
    };
    let out = render_toon(&opts);
    assert!(out.contains(r#""Update deps, bump major""#));
}

#[test]
fn null_value_renders_as_empty() {
    let opts = ToonOptions {
        type_name: "item".into(),
        fields: vec!["a".into(), "b".into()],
        rows: vec![vec![Value::Str("x".into()), Value::Null]],
        total_count: None,
        hints: vec![],
    };
    let out = render_toon(&opts);
    assert!(out.contains("  x,\n"));
}
```

- [ ] **Step 2: Run test — verify it fails**

```bash
cargo nextest run --test toon_integration 2>&1 | head -20
```

Expected: error — `michi::toon` not found, `render_toon` not defined.

- [ ] **Step 3: Write `src/toon/mod.rs`**

```rust
mod escape;
mod render;

/// A single cell value in a TOON row.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// UTF-8 string. Escaped if it contains commas, quotes, or newlines.
    Str(String),
    /// Signed integer.
    Int(i64),
    /// Floating-point number.
    Float(f64),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Null renders as empty (delimiter still emitted).
    Null,
}

/// Options for rendering a TOON document.
///
/// TOON (Token-Optimized Object Notation) is the canonical agent-facing list
/// format. Field names appear once in the header; rows are compact
/// comma-separated values. See `docs/01-spec.md` for the grammar.
#[derive(Debug, Clone)]
pub struct ToonOptions {
    /// Snake_case type name, e.g. `"issue"`, `"component"`.
    pub type_name: String,
    /// Ordered field names for the header, e.g. `["number", "title", "state"]`.
    pub fields: Vec<String>,
    /// Rows, each a Vec of values parallel to `fields`.
    pub rows: Vec<Vec<Value>>,
    /// Total available count (may exceed `rows.len()` when paginated). Emitted
    /// as `totalCount: N` when `Some`.
    pub total_count: Option<usize>,
    /// Agent-facing usage hints. Emitted as `help[N]:` block when non-empty.
    pub hints: Vec<String>,
}

/// Render a TOON document to a string.
///
/// # Panics
///
/// Does not panic. Row lengths differing from `fields.len()` produce
/// misaligned output (caller's responsibility to match lengths).
pub fn render_toon(opts: &ToonOptions) -> String {
    render::render(opts)
}
```

- [ ] **Step 4: Add `src/lib.rs` stub** (will be finalized in Task 15)

```rust
pub mod toon;
```

- [ ] **Step 5: Run tests — verify they pass**

```bash
cargo nextest run --test toon_integration
```

Expected: all 4 tests pass.

- [ ] **Step 6: Run clippy**

```bash
cargo clippy -- -D warnings
```

Fix any warnings before continuing.

- [ ] **Step 7: Commit**

```bash
git add src/toon/ src/lib.rs tests/toon_integration.rs
git commit -m "feat(toon): TOON render — types, escape, render, tests"
```

---

## Task 4: kv module + tests

**Files:**
- Create: `src/kv/mod.rs`
- Create: `tests/kv_integration.rs`

- [ ] **Step 1: Write failing test**

Create `tests/kv_integration.rs`:

```rust
use michi::kv::{render_kv, KvItem, KvValue};

#[test]
fn renders_basic_kv() {
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Str("abc-123".into()) },
        KvItem { key: "status".into(), value: KvValue::Str("open".into()) },
        KvItem { key: "count".into(), value: KvValue::Int(42) },
    ];
    let out = render_kv(&items);
    assert_eq!(out, "id: abc-123\nstatus: open\ncount: 42\n");
}

#[test]
fn renders_null_as_empty() {
    let items = vec![KvItem { key: "value".into(), value: KvValue::Null }];
    assert_eq!(render_kv(&items), "value: \n");
}

#[test]
fn renders_bool() {
    let items = vec![
        KvItem { key: "active".into(), value: KvValue::Bool(true) },
        KvItem { key: "deleted".into(), value: KvValue::Bool(false) },
    ];
    let out = render_kv(&items);
    assert_eq!(out, "active: true\ndeleted: false\n");
}

#[test]
fn empty_items_returns_empty_string() {
    assert_eq!(render_kv(&[]), "");
}
```

- [ ] **Step 2: Run test — verify it fails**

```bash
cargo nextest run --test kv_integration 2>&1 | head -10
```

- [ ] **Step 3: Write `src/kv/mod.rs`**

```rust
/// A single key-value pair for `render_kv`.
#[derive(Debug, Clone, PartialEq)]
pub struct KvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: KvValue,
}

/// A value in a key-value item.
#[derive(Debug, Clone, PartialEq)]
pub enum KvValue {
    /// UTF-8 string value.
    Str(String),
    /// Signed integer.
    Int(i64),
    /// Floating-point number.
    Float(f64),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Null renders as empty string after the colon.
    Null,
}

/// Render a list of key-value pairs as a multi-line `key: value` block.
///
/// Preferred for single items and small metadata blocks (up to ~5 fields).
/// For lists of 5+ items, prefer [`crate::toon::render_toon`].
pub fn render_kv(items: &[KvItem]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let capacity = items.len() * 20;
    let mut out = String::with_capacity(capacity);
    for item in items {
        out.push_str(&item.key);
        out.push_str(": ");
        match &item.value {
            KvValue::Str(s) => out.push_str(s),
            KvValue::Int(n) => out.push_str(&n.to_string()),
            KvValue::Float(f) => out.push_str(&f.to_string()),
            KvValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            KvValue::Null => {}
        }
        out.push('\n');
    }
    out
}
```

- [ ] **Step 4: Add kv to `src/lib.rs`**

```rust
pub mod kv;
pub mod toon;
```

- [ ] **Step 5: Run tests — verify they pass**

```bash
cargo nextest run --test kv_integration
```

Expected: all 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/kv/ src/lib.rs tests/kv_integration.rs
git commit -m "feat(kv): key-value render — types and tests"
```

---

## Task 5: hints module + tests

**Files:**
- Create: `src/hints.rs`

- [ ] **Step 1: Write `src/hints.rs`** (includes inline unit tests)

```rust
/// A contextual usage hint for an agent.
///
/// Hints are surfaced in `help[N]:` blocks at the end of a TOON or kv
/// response. They teach the agent what to call next.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint(pub String);

impl Hint {
    /// Create a hint from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The raw hint string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Hint {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl From<String> for Hint {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Render a `help[N]:` block from a slice of hints.
///
/// Returns an empty string when `hints` is empty.
pub fn render_hints(hints: &[Hint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let capacity = 12 + hints.len() * 50;
    let mut out = String::with_capacity(capacity);
    out.push_str("help[");
    out.push_str(&hints.len().to_string());
    out.push_str("]:\n");
    for hint in hints {
        out.push_str("  ");
        out.push_str(hint.as_str());
        out.push('\n');
    }
    out
}

/// Append a `help[N]:` block to an existing string in-place.
///
/// No-op when `hints` is empty.
pub fn append_hints(out: &mut String, hints: &[Hint]) {
    if hints.is_empty() {
        return;
    }
    out.push_str(&render_hints(hints));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_hint_renders_correctly() {
        let hints = [Hint::new("Call get_item with id=<id>")];
        assert_eq!(render_hints(&hints), "help[1]:\n  Call get_item with id=<id>\n");
    }

    #[test]
    fn multiple_hints() {
        let hints = [Hint::new("hint one"), Hint::new("hint two")];
        assert_eq!(render_hints(&hints), "help[2]:\n  hint one\n  hint two\n");
    }

    #[test]
    fn empty_hints_returns_empty() {
        assert_eq!(render_hints(&[]), "");
    }

    #[test]
    fn append_hints_modifies_string() {
        let mut s = "issue[0]{}:\n".to_string();
        append_hints(&mut s, &[Hint::new("try again")]);
        assert!(s.ends_with("help[1]:\n  try again\n"));
    }

    #[test]
    fn append_hints_noop_when_empty() {
        let mut s = "base".to_string();
        append_hints(&mut s, &[]);
        assert_eq!(s, "base");
    }

    #[test]
    fn hint_from_str_ref() {
        let h: Hint = "test".into();
        assert_eq!(h.as_str(), "test");
    }
}
```

- [ ] **Step 2: Add to `src/lib.rs`**

```rust
pub mod hints;
pub mod kv;
pub mod toon;
```

- [ ] **Step 3: Run tests**

```bash
cargo nextest run -p michi
```

Expected: all tests pass including the 6 in `hints`.

- [ ] **Step 4: Commit**

```bash
git add src/hints.rs src/lib.rs
git commit -m "feat(hints): Hint type, render_hints, append_hints"
```

---

## Task 6: truncate module + tests

**Files:**
- Create: `src/truncate.rs`

- [ ] **Step 1: Write `src/truncate.rs`**

```rust
/// Result of a truncation operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Truncated {
    /// The truncated content.
    pub content: String,
    /// Original length in bytes (for agent-readable messages).
    pub original_len: usize,
    /// Whether truncation actually occurred.
    pub was_truncated: bool,
}

/// Truncate content to `max_chars` characters, appending an agent-readable
/// suffix when truncated.
///
/// The suffix pattern is `" ({n} chars truncated — use {hint})"`. Uses char
/// boundaries — never splits a Unicode scalar.
///
/// # Arguments
/// * `content` — the string to truncate
/// * `max_chars` — maximum number of Unicode scalar values in the output
///   (including the suffix when truncated)
/// * `hint` — what the agent should call to get the full content,
///   e.g. `"full=true"`
pub fn truncate(content: &str, max_chars: usize, hint: &str) -> Truncated {
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return Truncated {
            content: content.to_string(),
            original_len: content.len(),
            was_truncated: false,
        };
    }

    let suffix = format!(" ({} chars truncated — use {})", char_count, hint);
    let suffix_chars = suffix.chars().count();
    let keep_chars = max_chars.saturating_sub(suffix_chars);

    // Find the byte boundary for `keep_chars` chars
    let byte_end = content
        .char_indices()
        .nth(keep_chars)
        .map(|(i, _)| i)
        .unwrap_or(content.len());

    let mut result = String::with_capacity(byte_end + suffix.len());
    result.push_str(&content[..byte_end]);
    result.push_str(&suffix);

    Truncated {
        content: result,
        original_len: content.len(),
        was_truncated: true,
    }
}

/// Truncate a value for inline use in a TOON field.
///
/// Produces a compact suffix: `"(N chars truncated — use {hint})"`. Suitable
/// for embedding inside a quoted TOON field value.
pub fn truncate_inline(content: &str, max_chars: usize, hint: &str) -> String {
    let t = truncate(content, max_chars, hint);
    t.content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_not_truncated() {
        let t = truncate("hello", 100, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.content, "hello");
    }

    #[test]
    fn long_content_is_truncated() {
        let content = "a".repeat(200);
        let t = truncate(&content, 50, "full=true");
        assert!(t.was_truncated);
        assert!(t.content.chars().count() <= 50);
        assert!(t.content.contains("chars truncated"));
        assert!(t.content.contains("full=true"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        // Multi-byte Unicode: each char is 3 bytes in UTF-8
        let content = "こんにちは世界！これはテストです。";
        let t = truncate(content, 10, "full=true");
        // Result must be valid UTF-8 (no panic = success)
        assert!(std::str::from_utf8(t.content.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_inline_returns_string() {
        let content = "x".repeat(200);
        let result = truncate_inline(&content, 30, "full=true");
        assert!(result.chars().count() <= 30);
    }

    #[test]
    fn exact_length_not_truncated() {
        let content = "hello";
        let t = truncate(content, 5, "full=true");
        assert!(!t.was_truncated);
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```rust
// src/lib.rs additions:
pub mod truncate;
```

```bash
cargo nextest run -p michi
git add src/truncate.rs src/lib.rs
git commit -m "feat(truncate): char-safe truncation with agent-readable suffix"
```

---

## Task 7: empty module + tests

**Files:**
- Create: `src/empty.rs`

- [ ] **Step 1: Write `src/empty.rs`**

```rust
use crate::hints::Hint;

/// Render a definitive empty state response.
///
/// Produces a TOON-compatible empty block: `type_name[0]{}:\ntotalCount: 0\n`.
/// Agents interpret this as "the collection exists but is genuinely empty" —
/// distinct from an error or a missing resource.
pub fn empty_state(type_name: &str) -> String {
    let mut out = String::with_capacity(type_name.len() + 20);
    out.push_str(type_name);
    out.push_str("[0]{}:\ntotalCount: 0\n");
    out
}

/// Render a definitive empty state with contextual usage hints.
pub fn empty_state_with_hints(type_name: &str, hints: &[Hint]) -> String {
    let mut out = empty_state(type_name);
    crate::hints::append_hints(&mut out, hints);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hints::Hint;

    #[test]
    fn basic_empty_state() {
        assert_eq!(empty_state("issue"), "issue[0]{}:\ntotalCount: 0\n");
    }

    #[test]
    fn empty_state_with_hints_appends_help_block() {
        let hints = [Hint::new("Try a broader filter")];
        let out = empty_state_with_hints("issue", &hints);
        assert_eq!(
            out,
            "issue[0]{}:\ntotalCount: 0\nhelp[1]:\n  Try a broader filter\n"
        );
    }

    #[test]
    fn empty_state_with_no_hints_matches_plain() {
        assert_eq!(empty_state_with_hints("task", &[]), empty_state("task"));
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/empty.rs src/lib.rs
git commit -m "feat(empty): definitive empty state rendering"
```

---

## Task 8: error module + tests

**Files:**
- Create: `src/error.rs`

This is the unified error type. For Plan 1, only the always-compiled variants (domain errors) are implemented. Execution error variants (`Http`, `Timeout`, `StepFailed`, etc.) are `#[cfg(feature = "pipeline")]` stubs.

- [ ] **Step 1: Write `src/error.rs`**

```rust
use std::time::Duration;

/// Classification of a `michi::Error` for routing and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// The caller provided invalid input. Do not retry.
    User,
    /// An internal or infrastructure failure. May be retryable.
    Internal,
    /// A transient failure expected to resolve without intervention.
    Transient,
}

/// Wraps a value so its inner content is omitted from `Debug` and `Display`.
///
/// Use this to prevent secrets (tokens, keys, passwords) from appearing in
/// logs or error messages.
#[derive(Clone)]
pub struct Sensitive<T>(pub T);

impl<T> std::fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl<T> std::fmt::Display for Sensitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The unified error type for the michi crate.
///
/// Carries both agent-renderable information (via [`Error::render`]) and
/// machine-readable classification (via [`Error::class`]).
///
/// Execution-layer variants (`Http`, `Timeout`, `StepFailed`, etc.) are only
/// present when the `pipeline` feature is enabled. See Plan 2.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // ── Domain errors (always compiled) ───────────────────────────────────

    /// The caller provided invalid or malformed input.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// A required resource was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    // ── Execution errors (pipeline feature) ───────────────────────────────

    #[cfg(feature = "pipeline")]
    #[error("HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        retryable: bool,
        retry_after: Option<Duration>,
    },

    #[cfg(feature = "pipeline")]
    #[error("Timeout after {elapsed:?}")]
    Timeout { elapsed: Duration },

    #[cfg(feature = "pipeline")]
    #[error("Step {id} failed")]
    StepFailed {
        id: String,
        #[source]
        source: Box<Error>,
    },

    #[cfg(feature = "pipeline")]
    #[error("Circuit {name} is open, retry after {retry_after:?}")]
    CircuitOpen { name: String, retry_after: Duration },

    #[cfg(feature = "pipeline")]
    #[error("No match for query: {query}")]
    NoMatch { query: String },

    #[cfg(feature = "pipeline")]
    #[error("Ambiguous match for '{query}': {count} candidates")]
    AmbiguousMatch { query: String, count: usize },

    #[cfg(feature = "pipeline")]
    #[error("Cyclic dependency detected: {cycle:?}")]
    CyclicDependency { cycle: Vec<String> },

    #[cfg(feature = "pipeline")]
    #[error("Operation was cancelled")]
    Cancelled,

    #[cfg(feature = "pipeline")]
    #[error("Cache error: {0}")]
    Cache(String),

    #[cfg(feature = "pipeline")]
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Render this error as an agent-readable plain-text string.
    ///
    /// The output is suitable for writing to stdout before exiting with
    /// [`Error::exit_code`].
    pub fn render(&self) -> String {
        format!("error: {self}")
    }

    /// The process exit code to use when this error is fatal.
    pub fn exit_code(&self) -> i32 {
        1
    }

    /// Classify this error for routing decisions (retry, alert, display).
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::InvalidInput(_) | Self::NotFound(_) => ErrorClass::User,
            #[cfg(feature = "pipeline")]
            Self::Http { retryable: true, .. }
            | Self::Timeout { .. }
            | Self::Cancelled => ErrorClass::Transient,
            #[cfg(feature = "pipeline")]
            _ => ErrorClass::Internal,
            #[cfg(not(feature = "pipeline"))]
            _ => ErrorClass::Internal,
        }
    }

    /// Whether this error is safe to retry automatically.
    pub fn is_retryable(&self) -> bool {
        matches!(self.class(), ErrorClass::Transient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_renders_correctly() {
        let e = Error::InvalidInput("field 'name' is required".into());
        assert_eq!(e.render(), "error: Invalid input: field 'name' is required");
    }

    #[test]
    fn not_found_renders_correctly() {
        let e = Error::NotFound("issue #99".into());
        assert_eq!(e.render(), "error: Not found: issue #99");
    }

    #[test]
    fn invalid_input_is_user_class() {
        let e = Error::InvalidInput("bad".into());
        assert_eq!(e.class(), ErrorClass::User);
        assert!(!e.is_retryable());
    }

    #[test]
    fn exit_code_is_one() {
        let e = Error::NotFound("x".into());
        assert_eq!(e.exit_code(), 1);
    }

    #[test]
    fn sensitive_redacts_debug() {
        let s = Sensitive("secret-token");
        assert_eq!(format!("{s:?}"), "<redacted>");
        assert_eq!(format!("{s}"), "<redacted>");
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/error.rs src/lib.rs
git commit -m "feat(error): unified Error type, ErrorClass, Sensitive<T>"
```

---

## Task 9: idempotency module + tests

**Files:**
- Create: `src/idempotency.rs`

- [ ] **Step 1: Write `src/idempotency.rs`**

```rust
/// An opaque idempotency key derived from operation inputs.
///
/// Used to detect and signal duplicate or already-completed operations without
/// re-executing them. Callers derive the key from stable operation parameters
/// (e.g. `sha256(user_id + operation + payload)`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    /// Create an idempotency key from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The raw key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for IdempotencyKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for IdempotencyKey {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

/// Result of an operation that may have already completed.
#[derive(Debug, Clone, PartialEq)]
pub enum AlreadyDone {
    /// The operation completed in a previous call. Contains the cached result.
    Yes { result: String },
    /// The operation has not been seen before. Proceed with execution.
    No,
}

/// Check whether an operation identified by `key` has already completed.
///
/// The `stored` parameter is the previously persisted result (if any). Callers
/// retrieve this from their own store (database, file, etc.) — michi does not
/// own any persistence.
///
/// Returns [`AlreadyDone::Yes`] when `stored` is `Some`, [`AlreadyDone::No`]
/// otherwise.
pub fn already_done(stored: Option<String>) -> AlreadyDone {
    match stored {
        Some(result) => AlreadyDone::Yes { result },
        None => AlreadyDone::No,
    }
}

/// Signals that an operation partially completed.
///
/// Use this when some steps of a multi-step operation succeeded before a
/// failure — the agent can resume from the checkpoint rather than retrying
/// from scratch.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSuccess {
    /// Identifiers of steps that completed successfully.
    pub completed: Vec<String>,
    /// Identifiers of steps that were not attempted.
    pub remaining: Vec<String>,
    /// Human-readable reason for the partial completion.
    pub reason: String,
}

impl PartialSuccess {
    /// Render this partial success as an agent-readable string.
    pub fn render(&self) -> String {
        format!(
            "partial: {} completed, {} remaining — {}",
            self.completed.len(),
            self.remaining.len(),
            self.reason
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_done_with_stored_result() {
        let result = already_done(Some("cached output".into()));
        assert_eq!(result, AlreadyDone::Yes { result: "cached output".into() });
    }

    #[test]
    fn already_done_with_no_stored_result() {
        assert_eq!(already_done(None), AlreadyDone::No);
    }

    #[test]
    fn idempotency_key_equality() {
        let a = IdempotencyKey::new("key-1");
        let b: IdempotencyKey = "key-1".into();
        assert_eq!(a, b);
    }

    #[test]
    fn partial_success_renders() {
        let ps = PartialSuccess {
            completed: vec!["step-a".into(), "step-b".into()],
            remaining: vec!["step-c".into()],
            reason: "rate limit hit".into(),
        };
        let out = ps.render();
        assert!(out.contains("2 completed"));
        assert!(out.contains("1 remaining"));
        assert!(out.contains("rate limit hit"));
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/idempotency.rs src/lib.rs
git commit -m "feat(idempotency): IdempotencyKey, already_done, PartialSuccess"
```

---

## Task 10: resilience module + tests

**Files:**
- Create: `src/resilience/mod.rs`
- Create: `src/resilience/policy.rs` (Plan 2 stub)
- Create: `src/resilience/circuit.rs` (Plan 2 stub)

- [ ] **Step 1: Write `src/resilience/mod.rs`**

```rust
#[cfg(feature = "pipeline")]
pub mod circuit;
#[cfg(feature = "pipeline")]
pub mod policy;

use std::time::Duration;

/// Configuration for automatic retry behaviour.
///
/// Callers implement the retry loop; michi provides the delay computation via
/// [`next_retry_delay`]. This keeps the library sync and runtime-agnostic —
/// the caller owns `sleep`.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Base delay for exponential back-off.
    pub base_delay: Duration,
    /// Maximum delay cap (back-off never exceeds this).
    pub max_delay: Duration,
    /// Jitter factor in `[0.0, 1.0]`. `0.0` = no jitter, `1.0` = full random jitter.
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.2,
        }
    }
}

/// Compute the delay before the next retry attempt.
///
/// Uses exponential back-off: `base_delay * 2^attempt`, capped at `max_delay`,
/// with optional jitter derived from `jitter_seed` (a value in `[0.0, 1.0]`
/// that the caller supplies — use a PRNG, not `rand` inside michi).
///
/// Returns `None` when `attempt >= config.max_retries`.
pub fn next_retry_delay(config: &RetryConfig, attempt: u32, jitter_seed: f64) -> Option<Duration> {
    if attempt >= config.max_retries {
        return None;
    }
    let exp = 2u64.saturating_pow(attempt);
    let base_ms = config.base_delay.as_millis() as u64;
    let raw_ms = base_ms.saturating_mul(exp);
    let capped_ms = raw_ms.min(config.max_delay.as_millis() as u64);
    let jitter_ms = (capped_ms as f64 * config.jitter_factor * jitter_seed) as u64;
    Some(Duration::from_millis(capped_ms + jitter_ms))
}

/// Parse the value of an HTTP `Retry-After` header.
///
/// Handles both delay-seconds (integer) and HTTP-date formats. Returns `None`
/// if the header value cannot be parsed.
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    // Try integer seconds first
    if let Ok(secs) = header_value.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // Try HTTP-date (RFC 7231): "Wed, 21 Oct 2015 07:28:00 GMT"
    // We do not parse dates (would require chrono dep). Return None for dates.
    // Callers that need date parsing add chrono themselves and convert to seconds.
    None
}

/// Return `true` if the HTTP status code is conventionally retryable.
///
/// Retryable: 429 (rate limit), 500, 502, 503, 504.
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_retry_uses_base_delay() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        let delay = next_retry_delay(&config, 0, 0.0).unwrap();
        assert_eq!(delay, config.base_delay);
    }

    #[test]
    fn second_retry_doubles() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        let delay = next_retry_delay(&config, 1, 0.0).unwrap();
        assert_eq!(delay, config.base_delay * 2);
    }

    #[test]
    fn delay_is_capped_at_max() {
        let config = RetryConfig {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.0,
            max_retries: 10,
        };
        // 2^5 * 1s = 32s, capped at 5s
        let delay = next_retry_delay(&config, 5, 0.0).unwrap();
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn beyond_max_retries_returns_none() {
        let config = RetryConfig::default();
        assert!(next_retry_delay(&config, 3, 0.0).is_none());
    }

    #[test]
    fn parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("60"), Some(Duration::from_secs(60)));
        assert_eq!(parse_retry_after("  120  "), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parse_retry_after_date_returns_none() {
        assert!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT").is_none());
    }

    #[test]
    fn retryable_status_codes() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(503));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(400));
    }
}
```

- [ ] **Step 2: Write Plan 2 stubs**

`src/resilience/policy.rs`:
```rust
// Plan 2: with_resilience() async wrapper — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
```

`src/resilience/circuit.rs`:
```rust
// Plan 2: CircuitBreaker, CircuitState — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
```

- [ ] **Step 3: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/resilience/ src/lib.rs
git commit -m "feat(resilience): RetryConfig, next_retry_delay, parse_retry_after"
```

---

## Task 11: status module + tests

**Files:**
- Create: `src/status.rs`

- [ ] **Step 1: Write `src/status.rs`**

```rust
/// Overall health of a system or component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// All components healthy.
    Ok,
    /// Some components degraded but system is operational.
    Degraded,
    /// System is not operational.
    Down,
}

impl Health {
    /// The string label used in rendered output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Down => "down",
        }
    }
}

impl std::fmt::Display for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A single item in a status response (component name + health).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItem {
    /// Component name, e.g. `"database"`, `"cache"`.
    pub name: String,
    /// Health status.
    pub health: Health,
    /// Optional detail message (e.g. latency, version, error reason).
    pub detail: Option<String>,
}

/// A structured status response for agent consumption.
///
/// Renders as a `kv`-style block with overall health first, then per-component
/// rows.
#[derive(Debug, Clone)]
pub struct StatusResponse {
    /// Overall system health (derived from components or set explicitly).
    pub health: Health,
    /// Per-component status items.
    pub items: Vec<StatusItem>,
    /// Optional version or build information.
    pub version: Option<String>,
}

impl StatusResponse {
    /// Render the status response as an agent-readable string.
    ///
    /// Format:
    /// ```text
    /// status: ok
    /// database: ok
    /// cache: degraded — high latency (320ms)
    /// version: 1.2.3
    /// ```
    pub fn render(&self) -> String {
        let capacity = 20 + self.items.len() * 40 + self.version.as_deref().map_or(0, str::len);
        let mut out = String::with_capacity(capacity);

        out.push_str("status: ");
        out.push_str(self.health.label());
        out.push('\n');

        for item in &self.items {
            out.push_str(&item.name);
            out.push_str(": ");
            out.push_str(item.health.label());
            if let Some(detail) = &item.detail {
                out.push_str(" — ");
                out.push_str(detail);
            }
            out.push('\n');
        }

        if let Some(v) = &self.version {
            out.push_str("version: ");
            out.push_str(v);
            out.push('\n');
        }

        out
    }

    /// Compute the overall health from component items.
    ///
    /// Returns `Down` if any item is `Down`, `Degraded` if any is `Degraded`,
    /// otherwise `Ok`. Does not modify `self.health` — callers use the return
    /// value to set it.
    pub fn compute_health(items: &[StatusItem]) -> Health {
        if items.iter().any(|i| i.health == Health::Down) {
            Health::Down
        } else if items.iter().any(|i| i.health == Health::Degraded) {
            Health::Degraded
        } else {
            Health::Ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_ok() {
        let resp = StatusResponse {
            health: Health::Ok,
            items: vec![
                StatusItem { name: "db".into(), health: Health::Ok, detail: None },
            ],
            version: Some("1.0.0".into()),
        };
        let out = resp.render();
        assert_eq!(out, "status: ok\ndb: ok\nversion: 1.0.0\n");
    }

    #[test]
    fn renders_degraded_with_detail() {
        let resp = StatusResponse {
            health: Health::Degraded,
            items: vec![
                StatusItem {
                    name: "cache".into(),
                    health: Health::Degraded,
                    detail: Some("high latency (320ms)".into()),
                },
            ],
            version: None,
        };
        let out = resp.render();
        assert!(out.contains("cache: degraded — high latency (320ms)"));
    }

    #[test]
    fn compute_health_down_wins() {
        let items = vec![
            StatusItem { name: "a".into(), health: Health::Ok, detail: None },
            StatusItem { name: "b".into(), health: Health::Down, detail: None },
            StatusItem { name: "c".into(), health: Health::Degraded, detail: None },
        ];
        assert_eq!(StatusResponse::compute_health(&items), Health::Down);
    }

    #[test]
    fn compute_health_degraded_when_no_down() {
        let items = vec![
            StatusItem { name: "a".into(), health: Health::Ok, detail: None },
            StatusItem { name: "b".into(), health: Health::Degraded, detail: None },
        ];
        assert_eq!(StatusResponse::compute_health(&items), Health::Degraded);
    }

    #[test]
    fn health_label_display() {
        assert_eq!(format!("{}", Health::Ok), "ok");
        assert_eq!(format!("{}", Health::Down), "down");
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/status.rs src/lib.rs
git commit -m "feat(status): StatusResponse, StatusItem, Health"
```

---

## Task 12: recovery module + tests

**Files:**
- Create: `src/recovery.rs`

- [ ] **Step 1: Write `src/recovery.rs`**

```rust
/// A recovery hint: a specific actionable step the agent should take to
/// resolve a failure.
///
/// Recovery hints are more prescriptive than `help[]` hints — they describe
/// what the agent *must* do to fix a broken state, not what it *can* do to
/// explore further.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryHint {
    /// Short label for the action, e.g. `"retry"`, `"authenticate"`.
    pub action: String,
    /// Full description: what to call and with what arguments.
    pub description: String,
}

impl RecoveryHint {
    /// Create a recovery hint.
    pub fn new(action: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            description: description.into(),
        }
    }
}

/// Render a set of recovery hints as an agent-readable block.
///
/// Format:
/// ```text
/// recovery[2]:
///   retry: Call list_issues again — the rate limit resets in 60s
///   authenticate: Call auth_login to refresh your token
/// ```
pub fn render_recovery(hints: &[RecoveryHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let capacity = 15 + hints.len() * 60;
    let mut out = String::with_capacity(capacity);
    out.push_str("recovery[");
    out.push_str(&hints.len().to_string());
    out.push_str("]:\n");
    for hint in hints {
        out.push_str("  ");
        out.push_str(&hint.action);
        out.push_str(": ");
        out.push_str(&hint.description);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_hint_renders() {
        let hints = [RecoveryHint::new("retry", "Call list_issues again")];
        assert_eq!(render_recovery(&hints), "recovery[1]:\n  retry: Call list_issues again\n");
    }

    #[test]
    fn multiple_hints() {
        let hints = [
            RecoveryHint::new("retry", "wait 60s then retry"),
            RecoveryHint::new("authenticate", "call auth_login first"),
        ];
        let out = render_recovery(&hints);
        assert!(out.starts_with("recovery[2]:"));
        assert!(out.contains("  retry: wait 60s then retry\n"));
        assert!(out.contains("  authenticate: call auth_login first\n"));
    }

    #[test]
    fn empty_hints_returns_empty() {
        assert_eq!(render_recovery(&[]), "");
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/recovery.rs src/lib.rs
git commit -m "feat(recovery): RecoveryHint, render_recovery"
```

---

## Task 13: response builder + tests

**Files:**
- Create: `src/response.rs`

The `AgentResponse` builder composes all primitives into a single rendered output string. It is the primary ergonomic entry point for callers.

- [ ] **Step 1: Write `src/response.rs`**

```rust
use crate::{
    hints::Hint,
    recovery::RecoveryHint,
    toon::{ToonOptions, Value},
    kv::{KvItem, KvValue},
};

/// Output format selector for `AgentResponse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// TOON list format (default for lists ≥ 5 items).
    #[default]
    Toon,
    /// Key-value block format (preferred for single items, ≤ 4 fields).
    Kv,
    /// Raw string — caller provides the fully-formed output.
    Raw,
}

/// A builder for composing agent-facing response strings.
///
/// Chain methods to build up the response, then call [`AgentResponse::build`]
/// to produce the final string. All methods return `&mut Self` for chaining;
/// annotated `#[must_use]` at the type level to catch dropped builders.
///
/// # Example
/// ```rust
/// use michi::response::AgentResponse;
/// use michi::hints::Hint;
///
/// let output = AgentResponse::new()
///     .content("issue[2]{id,title}:\n  1,Fix bug\n  2,Add feature\n")
///     .hint(Hint::new("Call get_issue with id=<id> for details"))
///     .build();
/// ```
#[must_use]
#[derive(Debug, Default)]
pub struct AgentResponse {
    content: String,
    hints: Vec<Hint>,
    recovery: Vec<RecoveryHint>,
}

impl AgentResponse {
    /// Create a new empty response builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the main content body (TOON, kv block, or raw string).
    pub fn content(&mut self, content: impl Into<String>) -> &mut Self {
        self.content = content.into();
        self
    }

    /// Render a TOON list as the content body.
    pub fn toon(&mut self, opts: ToonOptions) -> &mut Self {
        self.content = crate::toon::render_toon(&opts);
        self
    }

    /// Render a kv block as the content body.
    pub fn kv(&mut self, items: Vec<KvItem>) -> &mut Self {
        self.content = crate::kv::render_kv(&items);
        self
    }

    /// Append a single usage hint.
    pub fn hint(&mut self, hint: impl Into<Hint>) -> &mut Self {
        self.hints.push(hint.into());
        self
    }

    /// Append multiple usage hints.
    pub fn hints(&mut self, hints: impl IntoIterator<Item = impl Into<Hint>>) -> &mut Self {
        self.hints.extend(hints.into_iter().map(Into::into));
        self
    }

    /// Append a recovery hint.
    pub fn recovery(&mut self, hint: RecoveryHint) -> &mut Self {
        self.recovery.push(hint);
        self
    }

    /// Consume the builder and produce the final response string.
    pub fn build(&self) -> String {
        let mut out = self.content.clone();
        crate::hints::append_hints(&mut out, &self.hints);
        if !self.recovery.is_empty() {
            out.push_str(&crate::recovery::render_recovery(&self.recovery));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_content_with_hint() {
        let out = AgentResponse::new()
            .content("issue[0]{}:\ntotalCount: 0\n")
            .hint(Hint::new("Try a broader filter"))
            .build();
        assert_eq!(out, "issue[0]{}:\ntotalCount: 0\nhelp[1]:\n  Try a broader filter\n");
    }

    #[test]
    fn builds_kv_content() {
        let out = AgentResponse::new()
            .kv(vec![
                KvItem { key: "id".into(), value: KvValue::Str("abc".into()) },
                KvItem { key: "status".into(), value: KvValue::Str("open".into()) },
            ])
            .build();
        assert_eq!(out, "id: abc\nstatus: open\n");
    }

    #[test]
    fn empty_builder_produces_empty_string() {
        assert_eq!(AgentResponse::new().build(), "");
    }

    #[test]
    fn recovery_hints_appended() {
        let out = AgentResponse::new()
            .content("error: rate limited\n")
            .recovery(RecoveryHint::new("retry", "wait 60s"))
            .build();
        assert!(out.contains("recovery[1]:\n  retry: wait 60s\n"));
    }

    #[test]
    fn multiple_hints_render_correctly() {
        let out = AgentResponse::new()
            .content("x")
            .hints(["hint a", "hint b"])
            .build();
        assert!(out.contains("help[2]:\n  hint a\n  hint b\n"));
    }
}
```

- [ ] **Step 2: Add to lib.rs, run tests, commit**

```bash
cargo nextest run -p michi
git add src/response.rs src/lib.rs
git commit -m "feat(response): AgentResponse builder"
```

---

## Task 14: pipeline pure type + telemetry stub

**Files:**
- Create: `src/pipeline/mod.rs`
- Create: `src/pipeline/executor.rs` (Plan 2 stub)
- Create: `src/telemetry/mod.rs`
- Create: `src/sink/mod.rs` (Plan 2 stub)

- [ ] **Step 1: Write `src/pipeline/mod.rs`** (pure data only — no executor)

```rust
#[cfg(feature = "pipeline")]
pub mod executor;

/// Status of an individual pipeline step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Step completed successfully.
    Completed,
    /// Step was skipped (dependency failed or best-effort).
    Skipped,
    /// Step failed.
    Failed,
    /// Step has not been attempted.
    Pending,
}

impl StepStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Pending => "pending",
        }
    }
}

/// A pipeline step definition (pure data — no execution logic).
#[derive(Debug, Clone)]
pub struct PipelineStep {
    /// Unique step identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Current status.
    pub status: StepStatus,
}

/// A pipeline run state (pure data).
///
/// Renderable without the `pipeline` feature. The executor ([Plan 2]) produces
/// and updates this struct during a run.
#[derive(Debug, Clone, Default)]
pub struct Pipeline {
    /// Pipeline identifier (e.g. workflow name).
    pub id: String,
    /// All steps in declaration order.
    pub steps: Vec<PipelineStep>,
}

impl Pipeline {
    /// Render the pipeline state as a TOON list for agent consumption.
    ///
    /// Format:
    /// ```text
    /// step[3]{id,name,status}:
    ///   fetch-data,Fetch Data,completed
    ///   transform,Transform,pending
    ///   upload,Upload,pending
    /// ```
    pub fn render(&self) -> String {
        let n = self.steps.len();
        let capacity = 30 + n * 40;
        let mut out = String::with_capacity(capacity);

        out.push_str("step[");
        out.push_str(&n.to_string());
        out.push_str("]{id,name,status}:\n");

        for step in &self.steps {
            out.push_str("  ");
            out.push_str(&step.id);
            out.push(',');
            // Escape name if it contains a comma
            let name = if step.name.contains(',') {
                format!("\"{}\"", step.name)
            } else {
                step.name.clone()
            };
            out.push_str(&name);
            out.push(',');
            out.push_str(step.status.label());
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pipeline_steps() {
        let p = Pipeline {
            id: "my-pipeline".into(),
            steps: vec![
                PipelineStep { id: "fetch".into(), name: "Fetch Data".into(), status: StepStatus::Completed },
                PipelineStep { id: "upload".into(), name: "Upload".into(), status: StepStatus::Pending },
            ],
        };
        let out = p.render();
        assert_eq!(
            out,
            "step[2]{id,name,status}:\n  fetch,Fetch Data,completed\n  upload,Upload,pending\n"
        );
    }

    #[test]
    fn empty_pipeline_renders_header_only() {
        let p = Pipeline::default();
        assert_eq!(p.render(), "step[0]{id,name,status}:\n");
    }

    #[test]
    fn step_name_with_comma_is_quoted() {
        let p = Pipeline {
            id: "p".into(),
            steps: vec![PipelineStep {
                id: "s".into(),
                name: "Parse, validate".into(),
                status: StepStatus::Completed,
            }],
        };
        let out = p.render();
        assert!(out.contains(r#""Parse, validate""#));
    }
}
```

- [ ] **Step 2: Write Plan 2 stubs**

`src/pipeline/executor.rs`:
```rust
// Plan 2: PipelineExecutor, PipelineContext — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
```

`src/telemetry/mod.rs`:
```rust
/// No-op telemetry provider (zero-cost, always compiled).
///
/// Replace with a real provider by injecting your own implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopProvider;

impl NoopProvider {
    /// Record a span (no-op).
    #[inline]
    pub fn span(&self, _name: &str) {}
    /// Record a counter (no-op).
    #[inline]
    pub fn count(&self, _name: &str, _value: u64) {}
}
```

`src/sink/mod.rs`:
```rust
// Plan 2: OutputSink, AgentEvent, NoopSink — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
```

- [ ] **Step 3: Add to lib.rs, run tests, commit**

```rust
// Add to src/lib.rs:
pub mod pipeline;
pub mod telemetry;
```

```bash
cargo nextest run -p michi
git add src/pipeline/ src/telemetry/ src/sink/ src/lib.rs
git commit -m "feat(pipeline): pure data types + render; telemetry noop stub"
```

---

## Task 15: finalize lib.rs public API

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the final `src/lib.rs`**

```rust
//! # michi
//!
//! AXI response primitives for agent-ergonomic tools.
//!
//! `michi` (道) encodes seven of the ten AXI principles as typed, tested Rust:
//! TOON list rendering, key-value single-item rendering, contextual disclosure
//! (`help[]`), content truncation, definitive empty states, structured errors,
//! idempotency signals, retry delay primitives, and content-first status
//! responses.
//!
//! ## Feature flags
//!
//! | Feature | Adds |
//! |---|---|
//! | `pipeline` | `PipelineExecutor`, `CheckpointStore`, `OutputSink`, `CircuitBreaker` |
//! | `fuzzy` | `FuzzyMatcher`, `FuzzyResolver` |
//! | `cache` | Two-tier `Cache` (moka + disk) |
//! | `cli` | CLI surface adapters (indicatif, inquire) |
//! | `mcp` | MCP surface adapters |
//! | `napi` | NAPI exports (used by `packages/michi-node`) |
//! | `full` | All of the above except `napi` |
//!
//! Default features: none. A consumer with default features pulls in zero
//! async runtime dependencies.

pub mod empty;
pub mod error;
pub mod hints;
pub mod idempotency;
pub mod kv;
pub mod pipeline;
pub mod recovery;
pub mod resilience;
pub mod response;
pub mod sink;
pub mod status;
pub mod telemetry;
pub mod toon;
pub mod truncate;

// Re-export the most common types at the crate root for convenience.
pub use error::{Error, ErrorClass, Sensitive};
pub use hints::{append_hints, render_hints, Hint};
pub use response::AgentResponse;
pub use toon::{render_toon, ToonOptions, Value};

/// crate-level `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(feature = "napi")]
mod napi;
```

- [ ] **Step 2: Run full test suite**

```bash
cargo nextest run --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

Fix any issues before committing.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "feat: finalize public API surface in lib.rs"
```

---

## Task 16: snapshot tests + benchmarks

**Files:**
- Create: `tests/snapshot_tests.rs`
- Create: `benches/toon_render.rs`
- Create: `benches/kv_render.rs`

- [ ] **Step 1: Write `tests/snapshot_tests.rs`**

```rust
use michi::toon::{render_toon, ToonOptions, Value};
use michi::kv::{render_kv, KvItem, KvValue};
use michi::empty::empty_state_with_hints;
use michi::hints::Hint;
use michi::response::AgentResponse;

#[test]
fn snapshot_toon_basic_list() {
    let opts = ToonOptions {
        type_name: "issue".into(),
        fields: vec!["number".into(), "title".into(), "state".into()],
        rows: vec![
            vec![Value::Int(42), Value::Str("Fix login redirect".into()), Value::Str("open".into())],
            vec![Value::Int(43), Value::Str("Add dark mode".into()), Value::Str("open".into())],
            vec![Value::Int(44), Value::Str("Update deps, bump major".into()), Value::Str("closed".into())],
        ],
        total_count: Some(47),
        hints: vec![
            "Call get_issue with number=<number> for full detail".into(),
            "Call list_issues with state=open to filter".into(),
        ],
    };
    insta::assert_snapshot!(render_toon(&opts));
}

#[test]
fn snapshot_toon_empty_state() {
    let out = empty_state_with_hints("issue", &[Hint::new("Try list_issues with a broader filter")]);
    insta::assert_snapshot!(out);
}

#[test]
fn snapshot_kv_single_item() {
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Str("abc-123".into()) },
        KvItem { key: "title".into(), value: KvValue::Str("Fix login".into()) },
        KvItem { key: "state".into(), value: KvValue::Str("open".into()) },
        KvItem { key: "count".into(), value: KvValue::Int(3) },
    ];
    insta::assert_snapshot!(render_kv(&items));
}

#[test]
fn snapshot_agent_response_full() {
    let out = AgentResponse::new()
        .content("issue[0]{}:\ntotalCount: 0\n")
        .hint(Hint::new("Try list_issues with state=open"))
        .hint(Hint::new("Try list_issues with a different label"))
        .build();
    insta::assert_snapshot!(out);
}
```

- [ ] **Step 2: Generate snapshots**

```bash
INSTA_UPDATE=new cargo nextest run --test snapshot_tests
```

Expected: snapshots created in `tests/snapshots/`.

- [ ] **Step 3: Review and accept snapshots**

```bash
just snapshots
```

Accept all snapshots in the review interface.

- [ ] **Step 4: Write `benches/toon_render.rs`**

```rust
use divan::Bencher;
use michi::toon::{render_toon, ToonOptions, Value};

fn main() {
    divan::main();
}

fn make_opts(n: usize) -> ToonOptions {
    ToonOptions {
        type_name: "issue".into(),
        fields: vec!["number".into(), "title".into(), "state".into()],
        rows: (0..n)
            .map(|i| vec![
                Value::Int(i as i64),
                Value::Str(format!("Issue title number {i}")),
                Value::Str("open".into()),
            ])
            .collect(),
        total_count: Some(1000),
        hints: vec!["Call get_issue with number=<number>".into()],
    }
}

#[divan::bench(args = [1, 10, 100, 1000])]
fn render_n_rows(b: Bencher, n: usize) {
    let opts = make_opts(n);
    b.bench(|| render_toon(&opts));
}

#[divan::bench]
fn render_with_comma_escaping(b: Bencher) {
    let opts = ToonOptions {
        type_name: "item".into(),
        fields: vec!["name".into()],
        rows: (0..100)
            .map(|i| vec![Value::Str(format!("Item {i}, with comma"))])
            .collect(),
        total_count: None,
        hints: vec![],
    };
    b.bench(|| render_toon(&opts));
}
```

- [ ] **Step 5: Write `benches/kv_render.rs`**

```rust
use divan::Bencher;
use michi::kv::{render_kv, KvItem, KvValue};

fn main() {
    divan::main();
}

#[divan::bench(args = [1, 5, 20])]
fn render_n_items(b: Bencher, n: usize) {
    let items: Vec<KvItem> = (0..n)
        .map(|i| KvItem {
            key: format!("field_{i}"),
            value: KvValue::Str(format!("value_{i}")),
        })
        .collect();
    b.bench(|| render_kv(&items));
}
```

- [ ] **Step 6: Verify benchmarks compile and run**

```bash
cargo bench --no-run
just bench
```

Expected: divan output with timings for each group.

- [ ] **Step 7: Commit**

```bash
git add tests/snapshot_tests.rs tests/snapshots/ benches/
git commit -m "test: insta snapshots + divan benchmarks"
```

---

## Task 17: NAPI wrapper (packages/michi-node)

**Files:**
- Create: `packages/michi-node/Cargo.toml`
- Create: `packages/michi-node/build.rs`
- Create: `packages/michi-node/src/lib.rs`
- Create: `packages/michi-node/package.json`
- Create: `packages/michi-node/index.js`
- Create: `packages/michi-node/__test__/index.test.mjs`

- [ ] **Step 1: Write `packages/michi-node/Cargo.toml`**

```toml
[package]
name        = "michi-node"
version     = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true

[lib]
crate-type = ["cdylib"]

[dependencies]
michi      = { path = "../..", features = ["napi"] }
napi       = { version = "3", features = ["napi6"] }
napi-derive = { version = "3" }

[build-dependencies]
napi-build = "2"
```

- [ ] **Step 2: Write `packages/michi-node/build.rs`**

```rust
extern crate napi_build;

fn main() {
    napi_build::setup();
}
```

- [ ] **Step 3: Write `src/napi.rs` in the root crate** (behind `napi` feature)

```rust
use napi_derive::napi;
use crate::toon::{ToonOptions as RustToonOptions, Value as RustValue};
use crate::kv::{KvItem as RustKvItem, KvValue as RustKvValue};

/// Value type for a TOON row cell (JavaScript-friendly).
#[napi(object)]
pub struct JsToonValue {
    /// "str" | "int" | "float" | "bool" | "null"
    pub r#type: String,
    pub str_val: Option<String>,
    pub int_val: Option<i32>,      // note: no u64 across boundary
    pub float_val: Option<f64>,
    pub bool_val: Option<bool>,
}

/// Options for rendering a TOON document (JavaScript-friendly).
#[napi(object)]
pub struct JsToonOptions {
    pub type_name: String,
    pub fields: Vec<String>,
    pub rows: Vec<Vec<JsToonValue>>,
    pub total_count: Option<i32>,
    pub hints: Vec<String>,
}

fn js_value_to_rust(v: JsToonValue) -> RustValue {
    match v.r#type.as_str() {
        "str"   => RustValue::Str(v.str_val.unwrap_or_default()),
        "int"   => RustValue::Int(v.int_val.unwrap_or(0) as i64),
        "float" => RustValue::Float(v.float_val.unwrap_or(0.0)),
        "bool"  => RustValue::Bool(v.bool_val.unwrap_or(false)),
        _       => RustValue::Null,
    }
}

/// Render a TOON list document.
#[napi(catch_unwind)]
pub fn render_toon(opts: JsToonOptions) -> String {
    let rust_opts = RustToonOptions {
        type_name:   opts.type_name,
        fields:      opts.fields,
        rows:        opts.rows.into_iter()
                         .map(|row| row.into_iter().map(js_value_to_rust).collect())
                         .collect(),
        total_count: opts.total_count.map(|n| n as usize),
        hints:       opts.hints,
    };
    crate::toon::render_toon(&rust_opts)
}

/// Render a definitive empty state block.
#[napi(catch_unwind)]
pub fn empty_state(type_name: String) -> String {
    crate::empty::empty_state(&type_name)
}

/// Render a `help[N]:` hint block.
#[napi(catch_unwind)]
pub fn render_hints(hints: Vec<String>) -> String {
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    crate::hints::render_hints(&h)
}

/// Truncate content with an agent-readable suffix.
#[napi(catch_unwind)]
pub fn truncate(content: String, max_chars: i32, hint: String) -> String {
    crate::truncate::truncate_inline(&content, max_chars.max(0) as usize, &hint)
}
```

- [ ] **Step 4: Write `packages/michi-node/src/lib.rs`**

```rust
// Re-export all napi exports from the root crate.
// napi-derive generates index.d.ts from the annotations in michi::napi.
```

Actually for NAPI v3, the exports are defined in the root crate's `napi.rs` (compiled with napi feature). The `michi-node` crate's `src/lib.rs` needs to be essentially empty (napi-rs discovers exports from the compiled dependency):

```rust
// michi-node re-exports everything from michi's napi feature.
// The #[napi] exports in michi/src/napi.rs are automatically surfaced.
use michi as _;
```

> **Note:** Verify the exact napi-rs v3 pattern for re-exporting from a dependency before finalizing. The napi-rs CLI's `napi build` command handles type generation from `#[napi]` macros. Consult `https://napi.rs` v3 docs if the above doesn't compile.

- [ ] **Step 5: Write `packages/michi-node/package.json`**

```json
{
  "name": "michi",
  "version": "0.1.0",
  "description": "AXI response primitives for agent-ergonomic tools",
  "main": "index.js",
  "types": "index.d.ts",
  "license": "AGPL-3.0-or-later",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/orin-dx/michi.git",
    "directory": "packages/michi-node"
  },
  "scripts": {
    "build": "napi build --platform --js index.js --dts index.d.ts",
    "test": "node --experimental-vm-modules node_modules/.bin/jest",
    "prepublishOnly": "napi prepublish -t npm"
  },
  "devDependencies": {
    "@napi-rs/cli": "^3.0.0",
    "jest": "^29.0.0"
  },
  "engines": {
    "node": ">= 18"
  }
}
```

- [ ] **Step 6: Write `packages/michi-node/index.js`**

```js
// Platform-specific binary loader generated by @napi-rs/cli.
// Do not edit manually — regenerate with `napi build`.
const { existsSync, readFileSync } = require('fs')
const join = require('path').join

const { platform, arch } = process

let nativeBinding = null
let localFileExisted = false
let loadError = null

function isMusl() {
  if (!process.report || typeof process.report.getReport !== 'function') {
    try {
      const lddPath = require('child_process').execSync('which ldd').toString().trim()
      return readFileSync(lddPath, 'utf8').includes('musl')
    } catch {
      return true
    }
  }
  const report = process.report.getReport()
  const glibcVersionRuntime = report?.header?.glibcVersionRuntime
  return !glibcVersionRuntime
}

switch (platform) {
  case 'darwin':
    switch (arch) {
      case 'arm64': nativeBinding = require('./michi.darwin-arm64.node'); break
      case 'x64':   nativeBinding = require('./michi.darwin-x64.node'); break
      default: throw new Error(`Unsupported architecture on macOS: ${arch}`)
    }
    break
  case 'linux':
    switch (arch) {
      case 'x64':
        if (isMusl()) nativeBinding = require('./michi.linux-x64-musl.node')
        else          nativeBinding = require('./michi.linux-x64-gnu.node')
        break
      default: throw new Error(`Unsupported architecture on Linux: ${arch}`)
    }
    break
  default:
    throw new Error(`Unsupported OS: ${platform}`)
}

module.exports = nativeBinding
```

- [ ] **Step 7: Write `packages/michi-node/__test__/index.test.mjs`**

```js
import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { renderToon, emptyState, renderHints, truncate } from '../index.js'

describe('renderToon', () => {
  it('renders a basic list', () => {
    const out = renderToon({
      typeName: 'issue',
      fields: ['number', 'title', 'state'],
      rows: [
        [
          { type: 'int', intVal: 42 },
          { type: 'str', strVal: 'Fix login' },
          { type: 'str', strVal: 'open' },
        ],
      ],
      totalCount: 100,
      hints: ['Call get_issue with number=<number>'],
    })
    assert.ok(out.startsWith('issue[1]{number,title,state}:'))
    assert.ok(out.includes('42,Fix login,open'))
    assert.ok(out.includes('totalCount: 100'))
    assert.ok(out.includes('help[1]:'))
  })
})

describe('emptyState', () => {
  it('returns empty block', () => {
    const out = emptyState('issue')
    assert.equal(out, 'issue[0]{}:\ntotalCount: 0\n')
  })
})

describe('renderHints', () => {
  it('renders hint block', () => {
    const out = renderHints(['hint one', 'hint two'])
    assert.ok(out.startsWith('help[2]:'))
    assert.ok(out.includes('  hint one\n'))
  })

  it('returns empty for no hints', () => {
    assert.equal(renderHints([]), '')
  })
})

describe('truncate', () => {
  it('returns short content unchanged', () => {
    assert.equal(truncate('hello', 100, 'full=true'), 'hello')
  })

  it('truncates long content', () => {
    const out = truncate('a'.repeat(200), 50, 'full=true')
    assert.ok(out.length <= 60) // byte length; char count is the constraint
    assert.ok(out.includes('chars truncated'))
  })
})
```

> **Note:** Switch to Jest or `node:test` depending on what's available. The above uses `node:test` (Node 18+) to avoid a Jest dep for basic tests. Adjust `package.json` scripts accordingly.

- [ ] **Step 8: Install pnpm deps and build**

```bash
cd packages/michi-node && pnpm install
just build-node
```

Expected: `.node` binary generated for the local platform, `index.d.ts` auto-generated.

- [ ] **Step 9: Run Node tests**

```bash
cd packages/michi-node && node --test __test__/index.test.mjs
```

Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add packages/ src/napi.rs
git commit -m "feat(napi): NAPI v3 wrapper — renderToon, emptyState, renderHints, truncate"
```

---

## Task 18: GitHub Actions CI

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  INSTA_UPDATE: "no"
  RUST_BACKTRACE: 1

jobs:
  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-features -- -D warnings
      - name: typos
        uses: crate-ci/typos@master

  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@nextest
      - run: cargo nextest run --workspace
      - run: cargo nextest run --workspace --all-features

  deny:
    name: Deny
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2

  coverage:
    name: Coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@cargo-llvm-cov
      - uses: taiki-e/install-action@nextest
      - run: cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: false

  bench-build:
    name: Bench (build only)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo bench --no-run --workspace

  napi:
    name: NAPI (${{ matrix.target }})
    runs-on: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-apple-darwin
            runner: macos-13
          - target: aarch64-apple-darwin
            runner: macos-latest
          - target: x86_64-unknown-linux-gnu
            runner: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
          cache-dependency-path: packages/michi-node/pnpm-lock.yaml
      - run: pnpm install
        working-directory: packages/michi-node
      - run: pnpm build --target ${{ matrix.target }}
        working-directory: packages/michi-node
      - run: node --test __test__/index.test.mjs
        working-directory: packages/michi-node
        if: matrix.target != 'aarch64-apple-darwin' || runner.arch == 'ARM64'
```

- [ ] **Step 2: Commit**

```bash
git add .github/
git commit -m "ci: GitHub Actions — lint, test, deny, coverage, bench-build, NAPI matrix"
```

---

## Task 19: deny.toml + .gitignore

**Files:**
- Create: `deny.toml`
- Create: `.gitignore`

- [ ] **Step 1: Write `deny.toml`**

```toml
[advisories]
version = 2
ignore = []

[licenses]
version = 2
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "ISC",
    "Unicode-DFS-2016",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC0-1.0",
    "Zlib",
]
# AGPL is our own license — deny it in dependencies (we don't want AGPL deps)
deny = ["AGPL-3.0"]
exceptions = [
    # Add any needed exceptions here
]

[bans]
multiple-versions = "warn"
wildcards = "deny"
deny = []

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [ ] **Step 2: Write `.gitignore`**

```gitignore
/target
/packages/michi-node/target
*.node
node_modules
lcov.info
/packages/michi-node/index.d.ts
```

> **Note:** `index.d.ts` is generated by `napi build` — do not commit it (it regenerates on build). Add it to `.gitignore`.

- [ ] **Step 3: Run deny check**

```bash
just deny
```

Fix any license violations before proceeding.

- [ ] **Step 4: Commit**

```bash
git add deny.toml .gitignore
git commit -m "chore: deny.toml license policy, .gitignore"
```

---

## Task 20: final verification

- [ ] **Step 1: Full clean build**

```bash
cargo clean
just build
```

- [ ] **Step 2: All tests pass**

```bash
just test-rust
```

Expected: all tests pass, no warnings.

- [ ] **Step 3: All features compile**

```bash
cargo check --workspace --all-features
```

- [ ] **Step 4: Lint clean**

```bash
just check
```

Expected: no fmt errors, no clippy warnings, deny passes, no typos.

- [ ] **Step 5: Benchmark sanity**

```bash
cargo bench --no-run
```

- [ ] **Step 6: Snapshot acceptance**

```bash
cargo nextest run --test snapshot_tests
```

Expected: all pass (snapshots already committed).

- [ ] **Step 7: Tag v0.1.0**

```bash
git tag -a v0.1.0 -m "michi v0.1.0 — core rendering primitives"
```

Do not push to crates.io until the NAPI build matrix passes in CI.

---

## Self-review

**Spec coverage check:**

| Spec section | Covered by task |
|---|---|
| TOON grammar + rendering | Task 2, 3 |
| kv rendering | Task 4 |
| hints / help[] block | Task 5 |
| truncation, char-safe | Task 6 |
| empty states | Task 7 |
| Error type (always variants) | Task 8 |
| idempotency | Task 9 |
| resilience (pure sync) | Task 10 |
| status / health | Task 11 |
| recovery hints | Task 12 |
| AgentResponse builder | Task 13 |
| pipeline pure data type | Task 14 |
| telemetry NoopProvider | Task 14 |
| Plan 2 stubs (policy, circuit, executor, sink) | Tasks 10, 14 |
| Feature taxonomy in Cargo.toml | Task 1 |
| justfile | Task 1 |
| CLAUDE.md | Task 1 |
| Snapshot tests | Task 16 |
| divan benchmarks | Task 16 |
| NAPI v3 wrapper | Task 17 |
| CI (lint, test, deny, coverage, NAPI matrix) | Task 18 |
| deny.toml | Task 19 |

**Not in this plan (Plan 2):**
- `pipeline` feature: executor, checkpoint, step graph, cycle detection
- `resilience` feature: `with_resilience()`, `CircuitBreaker`
- `fuzzy` feature: `FuzzyMatcher`, `FuzzyResolver`
- `cache` feature: `Cache`, `DiskCache`
- `cli` feature: `CliSink`, `DiskCheckpointStore`, `TtyDisambiguator`
- `mcp` feature: `McpSink`, `MemoryCheckpointStore`

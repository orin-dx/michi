# michi — MVP Implementation Plan

> orin-dx · Draft · June 2026

---

> **Superseded.** This is an earlier, session-parallelized draft of the same v0.1.0 goal.
> `docs/superpowers/plans/2026-07-03-michi-core.md` is the plan that was actually executed —
> it has an explicit, correct scope note carving Plan 2 (pipeline execution, `fuzzy`,
> `cache`, `cli`, `mcp` adapters) out from the start. This document's "no features are
> deferred" claim below is **incorrect** and kept only for historical reference; see
> `michi-core.md`'s own "Not in this plan (Plan 2)" list for the accurate scope.

## Overview

The MVP delivers the pure-primitives `michi` crate as specified in `docs/01-spec.md`:
all 11 default-feature modules, the `AgentResponse` builder, the NAPI npm wrapper, a full
test suite, and benchmarks. Plan 2 (pipeline execution, `fuzzy`, `cache`, `cli`, `mcp`
adapters) is explicitly deferred — see the superseded notice above.

Implementation is split into 7 agent sessions across 5 phases. Sessions within
the same phase may run in parallel; the dependency graph enforces the ordering
between phases.

---

## Dependency graph

```text
Phase 0: Repository Setup           (1 session — must finish first)
    └─► Phase 1: Core Rendering     (2 sessions — parallel)
            └─► Phase 2: Primitives (2 sessions — parallel)
                    └─► Phase 3: Tests + NAPI  (2 sessions — parallel)
                            └─► Phase 4: Benchmarks + Polish (1 session)
```

---

## Phase summary

| Phase | Session | Depends on | Deliverable |
|---|---|---|---|
| 0 | S0 — Setup | — | Workspace, CI, stubs |
| 1 | S1A — Core Rendering | S0 | `toon`, `kv`, `hints` |
| 1 | S1B — Utilities | S0 | `truncate`, `empty`, `recovery` |
| 2 | S2A — Error + Resilience | S1A, S1B | `error`, `resilience` |
| 2 | S2B — Composition | S1A, S1B | `idempotency`, `status`, `response` |
| 3 | S3A — Tests | S2A, S2B | proptest + insta snapshots |
| 3 | S3B — NAPI | S2A, S2B | npm wrapper, Jest tests |
| 4 | S4 — Benchmarks + Polish | S3A, S3B | criterion, README, publish prep |

---

## Non-negotiable constraints (all sessions)

These rules apply to every file written in every session. No exceptions.

#### Rust

1. **Edition 2021, `rust-version = "1.93"`** — set in root `Cargo.toml`.
2. **No `unwrap()` or `expect()` in library code** — propagate all errors with
   `?`. Use `thiserror` for error types. `expect()` is only permitted in tests
   with a message explaining the invariant, and on infallible `writeln!` into a
   `String` (mark each such call with `// infallible`).
3. **No trait objects for dispatch** — no `Box<dyn Trait>` in hot paths. Use
   match arms or enum dispatch.
4. **No `unsafe` in this crate** — the only exception is inside the `#[napi]`
   boundary, which napi-rs manages.
5. **`BTreeMap` for any map appearing in rendered output** — deterministic key
   order. `HashMap` (or `FxHashMap`) is acceptable for internal intermediate
   state that never touches output.
6. **Pre-allocate strings in renderers** — use `String::with_capacity` with a
   reasonable estimate before any rendering loop. Never push to a string in a
   hot loop without pre-allocation.
7. **Unicode safety** — all truncation must use char boundaries, never byte
   offsets. Use `str::char_indices` or `str::floor_char_boundary`.
8. **All public items documented** — every `pub struct`, `pub fn`, `pub enum`
   has a doc comment. Doc comments include at least: what it does, any panics
   (including debug-only panics), and any important invariants.
9. **`#[derive(Debug)]` on all public types** unless there is a specific reason
   not to (document it if skipped).
10. **`#[must_use]` on builder methods** — the `AgentResponse` builder methods
    return `Self`; annotate with `#[must_use]` so callers can't accidentally
    discard the chain.

#### Testing

11. **Every public function has at least one unit test** — no exceptions, even
    for trivial wrappers.
12. **Snapshot tests are committed, not generated** — run `cargo insta review`
    to accept, then commit the `.snap` files. CI fails on unapproved snapshots.
13. **proptest strategies cover the full value space** — don't just use
    `proptest::arbitrary::any::<String>()` for strings that have constraints;
    build a custom strategy that generates valid and invalid inputs.

#### NAPI

14. **NAPI surface exports the builder and helpers only** — `AgentResponse`,
    `renderHints()`, `appendHints()`, `renderToon()`, `renderHintsOnly()`,
    `emptyState()`, `truncate()`, `truncateInline()`, `renderRecovery()`.
    Low-level module functions are Rust-only.
15. **All NAPI errors surface as typed JavaScript `Error` objects** — not raw
    panic strings. Domain errors convert via `From<AxiError> for napi::Error`.
16. **Builder methods at the NAPI boundary take `&mut self`, never owned
    `self`** — JavaScript owns the instance through GC, so Rust cannot move out
    of it. Hold the consuming Rust builder in an `Option<AgentResponse>` field
    and swap it with `Option::take()`. See Session 3B.
17. **Every `#[napi]` export is annotated `#[napi(catch_unwind)]`** — a Rust
    panic must become a thrown JS `Error`, never a Node.js process abort. This
    also applies to async exports (the `Promise` rejects instead of aborting).
18. **No `u64` crosses the boundary** — JS numbers are 64-bit floats (max safe
    integer 2^53). Use `i64`, `u32`, or `BigInt`. `Duration` is exposed as a
    `number` of milliseconds.
19. **`index.d.ts` is generated by `@napi-rs/cli`, never hand-edited** — when an
    inferred type is wrong, correct it with `#[napi(ts_return_type = ...)]` /
    `#[napi(ts_arg_type = ...)]` on the Rust item, not by patching the `.d.ts`.

#### CI

20. **CI runs on every PR**: `cargo fmt --check`, `cargo clippy -- -D warnings`,
    `cargo test`, `cargo test --features napi`, `cargo bench --no-run`.
21. **`cargo fmt --check` in CI** — format is not optional.
22. **Snapshot tests run with `INSTA_UPDATE=no`** — set in the job `env` so CI
    fails (never silently rewrites) on an unapproved snapshot.
23. **The build matrix uses `fail-fast: false`**, and the `linux-x64-musl` NAPI
    target is cross-compiled with `cargo-zigbuild`; `darwin-arm64` is built
    natively on a macOS runner (required for code-signing).

---

## CI matrix

Two layers run on every PR: a **Rust layer** (lint, test, bench-build) and a
**NAPI build layer** (one `.node` artifact per published target). The matrix
uses `fail-fast: false` so one target's failure does not cancel the rest.

| Job | Host runner | Target triple | Cross tooling | What runs |
|---|---|---|---|---|
| `lint` | ubuntu-latest | host | — | `cargo fmt --check`, `cargo clippy -- -D warnings` |
| `test` | ubuntu-latest | `x86_64-unknown-linux-gnu` | native | `cargo test`, `cargo test --features napi` (env `INSTA_UPDATE=no`) |
| `bench-build` | ubuntu-latest | host | — | `cargo bench --no-run` |
| `napi-linux-musl` | ubuntu-latest | `x86_64-unknown-linux-musl` | cargo-zigbuild | `napi build --release --cross-compile`, Jest |
| `napi-darwin-arm64` | macos-latest | `aarch64-apple-darwin` | native | `napi build --release`, Jest |

The `napi-*` jobs are added in Session 3B (they require the NAPI surface to
exist). The publish job in Session 4 downloads each `.node` artifact and runs
`napi prepublish`. Additional targets (Windows MSVC, linux-gnu arm64, darwin-x64)
follow the same matrix shape and can be appended post-MVP without restructuring.

---

## Phase 0 — Repository Setup

### Session 0 — Repository Setup

**Goal:** A buildable workspace with stubs for every module. No logic, just
structure. Every subsequent session adds flesh to a bone that already exists.

#### Deliverables

**`Cargo.toml` (workspace root)**

```toml
[workspace]
members = [".", "packages/michi-node"]
resolver = "2"

[workspace.package]
edition      = "2021"
rust-version = "1.93"
license      = "AGPL-3.0-or-later"
repository   = "https://github.com/orin-dx/michi"
```

**`Cargo.toml` (crate)**

```toml
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
default = []
napi    = ["dep:napi", "dep:napi-derive"]
cli     = []

[dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"

[dependencies.napi]
version  = "2"
features = ["napi4"]
optional = true

[dependencies.napi-derive]
version  = "2"
optional = true

[build-dependencies.napi-build]
version  = "1"
optional = true

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
proptest  = "1"
insta     = { version = "1", features = ["yaml"] }

[[bench]]
name    = "toon_render"
harness = false

[[bench]]
name    = "kv_render"
harness = false
```

**`src/lib.rs`** — stub with all module declarations and re-exports as specified
in `docs/01-spec.md`. All re-exported items are `pub use` stubs that compile. The
file should be complete enough that downstream session agents don't need to touch
it.

**Module stubs** — one file per module, each containing:

- The module's doc comment (copied from spec)
- Stub type definitions and function signatures with `todo!()` bodies
- `#[allow(dead_code)]` where needed to silence warnings during dev

**`build.rs`**

```rust
fn main() {
    #[cfg(feature = "napi")]
    napi_build::setup();
}
```

**`.github/workflows/ci.yml`**

```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    env:
      INSTA_UPDATE: "no"   # fail on unapproved snapshots; never auto-rewrite
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
      - run: cargo test --features napi
      - run: cargo bench --no-run
```

> The cross-compiled NAPI build jobs (`napi-linux-musl`, `napi-darwin-arm64`)
> are added in Session 3B once the NAPI surface exists. See [CI matrix](#ci-matrix).

**`rustfmt.toml`**

```toml
edition = "2021"
max_width = 100
use_field_init_shorthand = true
use_try_shorthand = true
```

**`clippy.toml`**

```toml
msrv = "1.93"
```

**`README.md`** — skeleton with crate name, one-line description, installation,
and a link to the spec. No prose yet.

**`.gitignore`** — standard Rust `.gitignore` plus `target/`, `*.snap.new`.

#### Session does NOT touch

Implementation logic. No renders, no escaping, no string building.

#### Done means

- `cargo build` and `cargo build --features napi` both succeed with only
  `todo!()` bodies (warnings allowed, errors not).
- `cargo fmt --check` and `cargo clippy -- -D warnings` pass on the stub tree.
- Every module from `docs/01-spec.md` exists as a file with its doc comment and
  signatures; `src/lib.rs` re-exports compile.
- CI workflow runs green on the stub commit.

---

## Phase 1 — Core Rendering

Two parallel sessions. Both depend only on S0.

### Session 1A — Core Rendering: `toon`, `kv`, `hints`

**Goal:** The three primary rendering modules — the most-called code in the
crate. These must be fast, correct, and well-tested in this session.

#### `toon/escape.rs`

The escaping logic for TOON cell values. Called from `render.rs`.

```rust
/// Escape a cell value for TOON output.
/// - Values with commas are wrapped in double quotes.
/// - Internal double quotes are backslash-escaped.
/// - Newlines are replaced with a truncation signal (TOON has no multi-line cells).
/// - Null renders as empty (produces ",," gaps).
/// - Numbers and bools render without quotes.
pub fn escape_cell(value: &Value, max_len: usize) -> String
```

Implementation notes:

- Detect special chars in a single pass over the string bytes before allocating.
  If no special chars and length is within `max_len`, return the original string
  slice (no allocation).
- For the quoting case, pre-allocate `value.len() + 2` (quotes) before iterating.
- Benchmark escaping in `benches/toon_render.rs`.

#### `toon/render.rs`

```rust
/// Render the TOON type header line.
/// Format: `type_name[count]{field1,field2,...}:`
fn render_header(type_name: &str, count: usize, fields: &[&str]) -> String

/// Render a single TOON data row.
/// Format: `  val1,val2,val3\n` (2-space indent)
fn render_row(cells: &[String]) -> String  // cells are pre-escaped

/// Render `totalCount: N\n` line.
fn render_total_count(n: u64) -> String
```

`render_toon()` in `toon/mod.rs` calls these in order:

1. `render_header`
2. For each row: escape all cells → `render_row`
3. `render_total_count` (if `opts.total_count.is_some()`)
4. `render_hints` (if `hints` non-empty)

Pre-allocate the output `String` before step 1:

```rust
// Rough estimate: header (~40) + N rows × (avg_cells × avg_cell_len + 2 indent + newline)
let cap = 40 + items.len() * (fields.len() * 20 + 3);
let mut out = String::with_capacity(cap);
```

#### `kv/mod.rs`

Column width = `items.iter().map(|i| i.key.len()).max().unwrap_or(0)`. Pad each
key to that width with trailing spaces.

```text
name:         Button          <- key padded to col_width, then ": ", then value
variant:      primary
tokens:       12
```

Format for `KvValue::Duration`: humanized, not raw seconds. "4 minutes ago" not
"240s". Implement a simple humanizer (no external dep needed: compare against
thresholds for seconds/minutes/hours/days).

`KvValue::Missing` renders as `—` (em-dash, U+2014).

#### `hints.rs`

```rust
// help[N]:          ← header line
//   hint one        ← 2-space indent
//   hint two

pub fn render_hints(hints: &[Hint]) -> String {
    if hints.is_empty() { return String::new(); }
    let mut out = String::with_capacity(hints.len() * 60);
    writeln!(out, "help[{}]:", hints.len()).unwrap(); // infallible
    for h in hints {
        writeln!(out, "  {}", h.0).unwrap(); // infallible
    }
    out
}
```

Note: `writeln!` on `String` never fails (infallible write); `.unwrap()` is
acceptable here and must be marked with `// infallible`.

#### Unit tests

Every function in all three modules must have tests covering:

- Happy path with representative inputs
- Edge cases: empty inputs, single item, maximum items
- Unicode: multi-byte characters in keys and values
- Escaping: commas in values, quotes in values, nulls, booleans
- Column alignment correctness in KV (different key lengths)
- Empty hints → empty string (no trailing newlines)

#### Done means

- `render_toon`, `render_kv`, and `render_hints` are fully implemented with no
  `todo!()` remaining in `toon/`, `kv/`, or `hints.rs`.
- Every public function in the three modules has a unit test; all pass.
- Escaping is verified against commas, embedded quotes, nulls, booleans, and
  multi-byte Unicode cells.
- `cargo clippy -- -D warnings` is clean for the three modules.

---

### Session 1B — Utilities: `truncate`, `empty`, `recovery`

**Goal:** The three supporting utility modules. Simpler than 1A but must be
correct, especially truncation (Unicode safety).

#### `truncate.rs`

**Critical:** never truncate on a byte boundary inside a multi-byte UTF-8
sequence. Use `str::floor_char_boundary` (stable since 1.65) or iterate with
`char_indices`:

```rust
pub fn truncate(content: &str, limit: usize) -> Truncated {
    if content.len() <= limit {
        return Truncated {
            content:      content.to_string(),
            truncated:    false,
            original_len: content.len(),
            signal:       None,
        };
    }
    // Find safe truncation point
    let end = content.floor_char_boundary(limit);
    let n_removed = content.len() - end;
    let signal = format!("({} chars truncated — use full=true)", n_removed);
    Truncated {
        content:      content[..end].to_string(),
        truncated:    true,
        original_len: content.len(),
        signal:       Some(signal),
    }
}
```

`truncate_inline` appends the signal in the same string:

```text
content_up_to_limit (N chars truncated — use full=true)
```

#### `empty.rs`

```rust
pub fn empty_state(type_name: &str) -> String {
    format!("{}[0]{{}}:\ntotalCount: 0\n", type_name)
}

pub fn empty_state_with_hints(type_name: &str, hints: &[Hint]) -> String {
    let base = empty_state(type_name);
    append_hints(&base, hints)
}
```

#### `recovery.rs`

`RecoveryHint::render()` produces:

```text
help[1]:
  Retry create_item with suggestedParams: { project: "PROJ", type: "Task" }
```

Params are rendered as compact JSON inline. Use `serde_json::to_string` on the
value (not `to_string_pretty`).

#### Unit tests

- `truncate`: exact boundary (len == limit), under, over, empty string,
  pure-ASCII, multi-byte Unicode boundary, signal message format
- `truncate_inline`: verify signal is appended, not on its own line
- `empty_state`: exact byte match against grammar
- `empty_state_with_hints`: hint block appended correctly
- `RecoveryHint`: builder chain, single param, multiple params, no params, with
  reason, without reason

#### Done means

- `truncate`, `truncate_inline`, `empty_state`, `empty_state_with_hints`, and
  `RecoveryHint::render` are fully implemented (no `todo!()`).
- A test proves truncation never splits a multi-byte UTF-8 sequence.
- `empty_state` output matches the grammar byte-for-byte.
- `cargo clippy -- -D warnings` is clean for the three modules.

---

## Phase 2 — Primitives

Two parallel sessions. Both depend on S1A and S1B.

### Session 2A — Error and Resilience: `error`, `resilience`

**Goal:** The two protocol-primitive modules. `error` must implement
`std::error::Error` correctly via `thiserror`. `resilience` must be purely
computational and well-tested via proptest.

#### `error.rs`

```rust
#[derive(Debug, thiserror::Error)]
#[error("{}", self.render())]
pub struct AxiError {
    pub code:        ErrorCode,
    pub message:     String,
    pub hints:       Vec<Hint>,
    pub recovery:    Option<RecoveryHint>,
    pub retryable:   bool,
    pub retry_after: Option<std::time::Duration>,
}
```

`render()` produces stdout-ready text:

```text
error[InvalidInput]: message text
help[1]:
  hint one
```

The error code is rendered as `error[VariantName]` using the Debug variant name.
`exit_code()` always returns `1` — no other values are defined.

`AxiError` implements `std::error::Error` via `thiserror`. It does NOT implement
`From<std::io::Error>` or any other error conversion — callers construct it
explicitly with `AxiError::new()`.

#### `resilience.rs`

`parse_retry_after(value: &str)` — two branches:

1. Try `value.parse::<u64>()` → `Duration::from_secs(n)`
2. Try HTTP-date parse: `Wed, 21 Oct 2026 07:28:00 GMT`
   - Parse manually (no external date dep needed for this simple fixed format)
   - Compute `target_time - now` using `std::time::SystemTime`
   - Return `None` if negative (past date)
3. Anything else → `None`

`next_retry_delay(attempt, config, retry_after)`:

```rust
// Base backoff
let base = config.initial_delay.mul_f64(config.backoff_factor.powi(attempt as i32));
// Jitter: ±25% of base
let jittered = if config.jitter {
    // Use a deterministic but unpredictable jitter from attempt number
    // No rand dep: use bit manipulation on attempt to approximate jitter
    let jitter_pct = 0.75 + (attempt.wrapping_mul(2654435761) % 100) as f64 / 200.0;
    base.mul_f64(jitter_pct)
} else {
    base
};
// Respect Retry-After if larger than backoff
let with_retry_after = match retry_after {
    Some(ra) if ra > jittered => ra,
    _ => jittered,
};
// Clamp to [initial_delay, max_delay]
with_retry_after.max(config.initial_delay).min(config.max_delay)
```

Note: no `rand` dependency. The deterministic jitter approximation above is
acceptable — the goal is "not perfectly periodic", not cryptographic quality.

#### Unit tests + proptest

Error module:

- Each `ErrorCode` variant: verify render format, `exit_code() == 1`
- Builder chaining: `.hint()`, `.recovery()`, `.retry_after()`
- `Display` impl produces same output as `render()`

Resilience module:

- `parse_retry_after`: integer seconds (0, 1, 120), valid HTTP-date (future,
  past), empty string, garbage string, partial date, non-UTF8-safe strings
- `next_retry_delay`: attempt 0/1/2/3 with default config, jitter bounds (always
  within ±30% of base), retry_after respected when larger
- `is_retryable_status`: 200, 429, 499, 500, 502, 503, 504

proptest for resilience:

```rust
proptest! {
    #[test]
    fn parse_retry_after_never_panics(s in ".*") {
        let _ = parse_retry_after(&s);
    }

    #[test]
    fn next_retry_delay_within_bounds(attempt in 0u32..10) {
        let config = RetryConfig::default();
        let delay = next_retry_delay(attempt, &config, None);
        prop_assert!(delay >= config.initial_delay);
        prop_assert!(delay <= config.max_delay);
    }
}
```

#### Done means

- `AxiError` implements `std::error::Error` and `Display`; `render()` and
  `Display` produce identical output, verified by test.
- `parse_retry_after` and `next_retry_delay` are implemented with no `todo!()`.
- proptest proves `parse_retry_after` never panics and `next_retry_delay` always
  lands in `[initial_delay, max_delay]`.
- `cargo clippy -- -D warnings` is clean for both modules.

---

### Session 2B — Composition: `idempotency`, `status`, `response`

**Goal:** The three composition modules. `response.rs` is the primary
integration point — it must correctly compose all prior modules.

#### `idempotency.rs`

`IdempotencyKey::from_hash` uses SHA-256. No external dep is strictly required —
`std::collections::hash_map::DefaultHasher` gives a non-cryptographic hash — but
if adding a dep, prefer `sha2` from the RustCrypto project (widely used, no
transitive deps).

```rust
use sha2::{Sha256, Digest};

impl IdempotencyKey {
    pub fn from_hash(operation: &str, data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(operation.as_bytes());
        hasher.update(b":");
        hasher.update(data);
        let result = hasher.finalize();
        Self(format!("{operation}:{:x}", result))
    }
}
```

If `sha2` is too heavy for this crate's goals, implement FNV-1a (no deps, ~32
lines) and document the tradeoff.

`already_done()` output format:

```text
status:     already done
operation:  create_item
summary:    Item 42 already exists with matching parameters
help[1]:
  hint
```

`PartialSuccess::render()` output format:

```text
status:     partial
completed:  3
failed:     1
skipped:    0

failed_ops[1]{operation,reason}:
  create_item,"Project PROJ not found"
help[1]:
  Retry failed operations with corrected parameters
```

#### `status.rs`

`StatusResponse::render()` delegates to `kv::render_kv()` but pre-processes
`StatusItem` into `KvItem`, appending a health signal to the value:

```rust
for item in &self.items {
    let value = match &item.health {
        None | Some(Health::Ok) => item.value.clone(),
        Some(Health::Degraded(reason)) => {
            // Append "[DEGRADED: reason]" to the rendered value string
            KvValue::Text(format!("{}  [DEGRADED: {}]",
                render_kv_value(&item.value), reason))
        }
        Some(Health::Error(reason)) => {
            KvValue::Text(format!("{}  [ERROR: {}]",
                render_kv_value(&item.value), reason))
        }
    };
    kv_items.push(KvItem { key: item.key.clone(), value });
}
```

#### `response.rs`

`AgentResponse` is the builder. The `render()` dispatch is straightforward:

```rust
pub fn render(&self, format: OutputFormat) -> String {
    match format {
        OutputFormat::Toon => self.render_toon(),
        OutputFormat::Kv   => self.render_kv(),
        OutputFormat::Json => self.render_json().to_string(),
    }
}

pub fn render_toon(&self) -> String {
    let fields: Vec<&str> = self.fields.iter().map(|s| s.as_str()).collect();
    render_toon(
        &self.type_name,
        &self.items,
        &fields,
        &self.hints,
        ToonOptions {
            max_cell_len: self.truncate_cells,
            total_count:  self.total_count,
        },
    )
}

pub fn render_hints_only(&self) -> String {
    render_hints(&self.hints)
}
```

`render_json()` produces a `serde_json::Value`. Use the `serde_json::json!`
macro for construction; don't hand-roll JSON serialisation.

#### Unit tests

Test the complete builder chain end-to-end:

- `.items()` → `render_toon()` produces valid TOON
- `.kv_items()` → `render_kv()` produces valid KV
- `.hint()` + `.hints()` both append to the same vec
- `.total_count()` appears in rendered output
- `.truncate_cells_at()` limits cell lengths
- `render_hints_only()` produces only the `help[]` block
- `already_done()` exact output format
- `PartialSuccess` with completed only, failed only, mixed
- `StatusResponse` with all health states

#### Done means

- `AgentResponse` builder, `StatusResponse::render`, `already_done`, and
  `PartialSuccess::render` are implemented with no `todo!()`.
- The full builder chain (`items`/`kv_items`/`hint`/`total_count`/
  `truncate_cells_at`) is covered by passing end-to-end tests.
- All three output formats (`Toon`, `Kv`, `Json`) dispatch correctly from
  `render()`.
- `cargo clippy -- -D warnings` is clean for the three modules.

---

## Phase 3 — Tests + NAPI

Two parallel sessions. Both depend on S2A and S2B (the full module set).

### Session 3A — Tests: proptest + insta

**Goal:** The property test and snapshot test suites. These run against the
completed module implementations from Sessions 1A, 1B, 2A, 2B.

#### proptest (`tests/*.rs`)

**TOON grammar conformance** — write a minimal test-only TOON parser and verify
round-trip:

```rust
// tests/toon_integration.rs
mod parser {
    // Simple line-by-line TOON parser for test verification only.
    // Not part of the public API.
    pub struct ToonDoc {
        pub type_name: String,
        pub count: usize,
        pub fields: Vec<String>,
        pub rows: Vec<Vec<String>>,
        pub total_count: Option<u64>,
        pub hints: Vec<String>,
    }
    pub fn parse(input: &str) -> Result<ToonDoc, String> { /* ... */ }
}

proptest! {
    #[test]
    fn toon_roundtrip(
        type_name in "[a-z][a-z0-9_]{0,15}",
        items in proptest::collection::vec(
            proptest::collection::vec(any::<String>(), 1..5usize),
            0..20usize
        ),
    ) {
        let fields = (0..items.first().map(|r| r.len()).unwrap_or(1))
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>();
        let field_refs: Vec<&str> = fields.iter().map(|s| s.as_str()).collect();
        let value_items: Vec<Vec<Value>> = items.iter()
            .map(|row| row.iter().map(|s| Value::Str(s.clone())).collect())
            .collect();
        let rendered = render_toon(&type_name, &value_items, &field_refs, &[], ToonOptions::default());
        let parsed = parser::parse(&rendered).expect("rendered TOON must be parseable");
        prop_assert_eq!(parsed.type_name, type_name);
        prop_assert_eq!(parsed.count, value_items.len());
    }
}
```

**Additional proptest cases** (as specified in `docs/01-spec.md`):

- `truncate_inline()` output never exceeds `limit + signal_len`
- `parse_retry_after()` never panics
- `next_retry_delay()` always within bounds

#### insta snapshots (`tests/snapshot_tests.rs`)

One snapshot per canonical example from the spec. Use `insta::assert_snapshot!`
with named snapshots:

```rust
#[test]
fn toon_list_response() {
    let result = AgentResponse::new("issues")
        .items(
            vec![
                vec![Value::Int(42), Value::Str("Fix login redirect".into()), Value::Str("open".into())],
                vec![Value::Int(43), Value::Str("Add dark mode".into()),      Value::Str("open".into())],
                vec![Value::Int(44), Value::Str("Update deps, bump major".into()), Value::Str("closed".into())],
            ],
            &["number", "title", "state"],
        )
        .total_count(47)
        .hint("Call get_issue with number=<number> for full detail")
        .hint("Call list_issues with state=open to filter")
        .render_toon();

    insta::assert_snapshot!("toon_list_response", result);
}
```

Expected snapshot (`tests/snapshots/toon_list_response.snap`):

```text
---
source: tests/snapshot_tests.rs
---
issues[3]{number,title,state}:
  42,Fix login redirect,open
  43,Add dark mode,open
  44,"Update deps, bump major",closed
totalCount: 47
help[2]:
  Call get_issue with number=<number> for full detail
  Call list_issues with state=open to filter
```

Snapshots to write:

- `toon_list_response` — 3-item list with `totalCount` and hints
- `toon_truncated_cells` — cells exceeding `max_cell_len`
- `toon_empty_state` — empty items list
- `kv_single_item` — single item with alignment
- `kv_with_missing` — `KvValue::Missing` renders as em-dash
- `status_degraded` — `StatusResponse` with `Degraded` health signal
- `status_error` — `StatusResponse` with `Error` health signal
- `already_done` — `idempotency::already_done()` output
- `partial_success_mixed` — `PartialSuccess` with completed and failed
- `error_render` — `AxiError` render output
- `recovery_hint` — `RecoveryHint` render

**Review workflow.** insta writes pending snapshots to `*.snap.new` on first run.
Accept them interactively with `cargo insta review` (then commit the resulting
`.snap` files), or accept all at once with `cargo insta accept`. Two env vars
control behavior:

- `INSTA_UPDATE=no` — never write `.snap.new`; an unmatched snapshot is a hard
  test failure. This is the **CI setting** (constraint 22).
- `INSTA_UPDATE=auto` (the local default) — write `.snap.new` for review but do
  not modify accepted snapshots.

Only committed `.snap` files count; `*.snap.new` is git-ignored (set in S0). CI
runs with `INSTA_UPDATE=no`, so a missing or stale snapshot fails the build
instead of being silently rewritten.

#### Done means

- The TOON round-trip property test and all three additional proptest cases pass
  (default proptest case count, no `#[ignore]`).
- All 11 named snapshots exist, are reviewed/accepted, and their `.snap` files
  are committed; no `*.snap.new` remains.
- `INSTA_UPDATE=no cargo test` passes locally (mirrors CI).

---

### Session 3B — NAPI Wrapper

**Goal:** The `packages/michi-node/` package. TypeScript consumers get the same
primitives as Rust consumers, with zero Rust knowledge required.

#### `packages/michi-node/Cargo.toml`

```toml
[package]
name    = "michi-node"
version = "0.1.0"
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true

[lib]
crate-type = ["cdylib"]

[dependencies]
michi       = { path = "../..", features = ["napi"] }
napi        = { version = "2", features = ["napi4"] }
napi-derive = "2"

[build-dependencies]
napi-build = "1"
```

> **Why `napi4`?** It is the lowest N-API level that provides
> `ThreadsafeFunction` and the `tokio_rt` feature — the prerequisite for any
> `async fn` at the boundary. Node 12+ supports it, making it the standard
> baseline. (napi-rs v3 is available and adds first-class ESM and Docker-free
> cross builds; the MVP stays on the v2 line for stability but the patterns
> below are identical on v3.)

#### `src/napi.rs` (in main crate, `napi` feature)

Export only the items listed in Q4 resolution: the builder and helpers. Every
export is annotated `#[napi(catch_unwind)]` (constraint 17) so a Rust panic
becomes a thrown JS `Error` rather than a process abort.

```rust
#[cfg(feature = "napi")]
mod napi_exports {
    use napi_derive::napi;
    use crate::*;

    #[napi(catch_unwind)]
    pub fn render_hints(hints: Vec<String>) -> String {
        let hint_objects: Vec<Hint> = hints.into_iter().map(Hint::new).collect();
        crate::hints::render_hints(&hint_objects)
    }

    #[napi(catch_unwind)]
    pub fn append_hints(body: String, hints: Vec<String>) -> String {
        let hint_objects: Vec<Hint> = hints.into_iter().map(Hint::new).collect();
        crate::hints::append_hints(&body, &hint_objects)
    }

    // ... rest of exports
}
```

#### The builder boundary problem (and the fix)

The hardest part of this session. The Rust `AgentResponse` builder **consumes
`self`** (each setter is `fn items(mut self, ...) -> Self`). But napi-rs class
methods can only take `&self` or `&mut self` — JavaScript owns the instance
through GC, so Rust may never move out of it. You therefore **cannot** expose the
consuming setters directly.

**Recommended solution — hold the consuming builder in `Option<AgentResponse>`
and swap it with `take()`** (constraint 16). Each setter takes `&mut self`,
`take()`s the inner value out (leaving `None`), applies the consuming Rust
setter, and stores the result back. Returning `&Self` lets JavaScript chain the
calls:

```rust
#[cfg(feature = "napi")]
#[napi(js_name = "AgentResponse")]
pub struct JsAgentResponse {
    // Nullable ownership slot: take() moves the value out, leaving None.
    inner: Option<crate::response::AgentResponse>,
}

#[cfg(feature = "napi")]
#[napi]
impl JsAgentResponse {
    #[napi(constructor)]
    pub fn new(type_name: String) -> Self {
        Self { inner: Some(crate::response::AgentResponse::new(&type_name)) }
    }

    // Setter: &mut self, swap-via-take, return &Self for JS chaining.
    #[napi(catch_unwind)]
    pub fn total_count(&mut self, n: i64) -> napi::Result<&Self> {
        let b = self.inner.take()
            .ok_or_else(|| napi::Error::from_reason("builder already consumed"))?;
        self.inner = Some(b.total_count(n as u64)); // consuming call, reassigned
        Ok(self)
    }

    #[napi(catch_unwind)]
    pub fn hint(&mut self, hint: String) -> napi::Result<&Self> {
        let b = self.inner.take()
            .ok_or_else(|| napi::Error::from_reason("builder already consumed"))?;
        self.inner = Some(b.hint(&hint));
        Ok(self)
    }

    // Terminal render: &self, read-only — never consumes.
    #[napi(catch_unwind)]
    pub fn render_toon(&self) -> napi::Result<String> {
        self.inner.as_ref()
            .map(|b| b.render_toon())
            .ok_or_else(|| napi::Error::from_reason("builder already consumed"))
    }
}
```

Why this shape:

- `Option::take()` is the canonical way to move a value out behind `&mut`,
  leaving a valid `None` so the borrow checker stays happy.
- Render methods take `&self` and only read `inner.as_ref()`, so they never
  consume the builder and can be called repeatedly.
- **Do not** make these methods `async`. Async + `&mut self` is unsound at the
  napi boundary (the borrow checker cannot enforce exclusivity across the event
  loop) and would have to be marked `unsafe`. The builder is synchronous, so this
  does not arise here.

#### NAPI type-mapping decisions

- **`Value` enum → `CellValue = string | number | boolean | null`.** napi-rs
  does not auto-generate TypeScript discriminated unions from Rust tagged-union
  enums. Accept cells as a flattened union and override the generated type:

  ```rust
  #[napi(
      catch_unwind,
      ts_args_type = "items: Array<Array<string | number | boolean | null>>, fields: Array<string>"
  )]
  pub fn items(&mut self, items: Vec<Vec<JsCellValue>>, fields: Vec<String>) -> napi::Result<&Self> {
      // map each JsCellValue -> crate Value, then call the consuming setter via take()
  }
  ```

- **`AxiError` → JS `Error` with a `code` string property.** Implement
  `From<AxiError> for napi::Error`, using `Error::new(Status::GenericFailure, msg)`
  and carrying the variant name in the `code` field. Errors crossing the boundary
  surface as typed `Error` objects, never panic strings (constraint 15).
- **`Duration` → `number` (milliseconds).** Never expose `u64`/`Duration`
  directly (constraint 18).
- **`index.d.ts` is generated**, never hand-written (constraint 19). When the
  inferred type is wrong, fix it with `ts_args_type` / `ts_return_type` on the
  Rust item.

#### `packages/michi-node/package.json`

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
    "url": "https://github.com/orin-dx/michi"
  },
  "keywords": ["axi", "agent", "mcp", "cli", "llm"],
  "napi": {
    "name": "michi",
    "triples": {
      "defaults": false,
      "additional": [
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-musl"
      ]
    }
  },
  "scripts": {
    "build": "napi build --platform --release",
    "test": "node --test __test__/index.test.mjs"
  },
  "devDependencies": {
    "@napi-rs/cli": "^2.18.0"
  }
}
```

`@napi-rs/cli` generates `index.js` and `index.d.ts` at build time. `index.js`
tries a locally compiled `michi.<platform>.node` first (dev iteration), then
falls back to the per-platform optional-dependency npm package. Each platform
binary ships as its own package gated by `cpu`/`os`; the main package lists them
in `optionalDependencies`, so npm installs only the matching one.

> **npm lockfile gotcha:** npm sometimes omits optional platform packages from
> `package-lock.json` when the lockfile was generated on a different
> architecture (npm/cli#4828). Prefer `pnpm`, or regenerate the lockfile on the
> target arch, when this bites in CI.

#### Cross-compilation (`linux-x64-musl`)

`darwin-arm64` builds natively on a macOS runner. The `linux-x64-musl` target is
cross-compiled from the Linux runner with **cargo-zigbuild**, which `@napi-rs/cli`
invokes automatically behind `--cross-compile` (alias `-x`):

```bash
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl
napi build --release --cross-compile --target x86_64-unknown-linux-musl
```

CI step (added to the matrix from S0's workflow):

```yaml
- uses: goto-bus-stop/setup-zig@v2
- run: cargo install cargo-zigbuild
- run: rustup target add x86_64-unknown-linux-musl
- run: npm run build -- --cross-compile --target x86_64-unknown-linux-musl
- run: npm test
```

napi-rs v3 removed the heavy `nodejs-rust:lts-debian` Docker images: cargo-zigbuild
is a lightweight Cargo binary, so no Docker layer is needed.

#### Tests (`__test__/index.test.mjs`)

Test every NAPI export with the built-in Node test runner:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToon, renderHints, AgentResponse } from '../index.js';

test('renderToon produces valid output', () => {
  const result = renderToon({
    typeName: 'issues',
    fields: ['number', 'title', 'state'],
    items: [[42, 'Fix login', 'open'], [43, 'Dark mode', 'open']],
    totalCount: 47,
    hints: ['Call get_issue with number=<number>'],
  });
  assert.ok(result.startsWith('issues[2]{number,title,state}:'));
  assert.ok(result.includes('totalCount: 47'));
  assert.ok(result.includes('help[1]:'));
});

test('AgentResponse builder', () => {
  const result = new AgentResponse('issues')
    .items([[42, 'Fix login', 'open']], ['number', 'title', 'state'])
    .totalCount(47)
    .hint('Call get_issue with number=<number>')
    .renderToon();
  assert.ok(result.startsWith('issues[1]'));
});

test('renderHints returns empty string for empty hints', () => {
  assert.equal(renderHints([]), '');
});
```

#### Done means

- `napi build --release` produces a working `.node` plus generated `index.js`
  and `index.d.ts`; the Node test suite passes on the host.
- The `AgentResponse` JS class chains setters (`.items().totalCount().hint()
  .renderToon()`) using the `Option`-take pattern, with no consuming `self`
  methods exposed.
- A forced Rust panic inside an export throws a JS `Error` (process survives),
  confirming `#[napi(catch_unwind)]` coverage.
- `linux-x64-musl` cross-builds via cargo-zigbuild and `darwin-arm64` builds
  natively; both run the Jest/Node tests green in CI.

---

## Phase 4 — Benchmarks + Polish

### Session 4 — Benchmarks + Polish

**Goal:** criterion benchmarks, README, and publish prep.

#### `benches/toon_render.rs`

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use michi::{render_toon, toon::{ToonOptions, Value}, hints::Hint};

fn bench_toon_100_rows(c: &mut Criterion) {
    let items: Vec<Vec<Value>> = (0..100)
        .map(|i| vec![
            Value::Int(i),
            Value::Str(format!("Issue title number {i}")),
            Value::Str("open".to_string()),
        ])
        .collect();
    let hints = vec![Hint::new("Call get_issue with number=<number>")];

    c.bench_function("toon_100_rows_4_fields", |b| {
        b.iter(|| {
            black_box(render_toon(
                black_box("issues"),
                black_box(&items),
                black_box(&["number", "title", "state"]),
                black_box(&hints),
                black_box(ToonOptions::default()),
            ))
        })
    });
}

fn bench_toon_1000_rows(c: &mut Criterion) {
    // Same structure, 1000 rows
    // ...
}

criterion_group!(benches, bench_toon_100_rows, bench_toon_1000_rows);
criterion_main!(benches);
```

Benchmarks must pass the targets from `docs/01-spec.md`:

- 100 items, 4 fields: < 500µs
- 1000 items, 4 fields: < 3ms

#### `benches/kv_render.rs`

Benchmark `render_kv()` with 10 items of varying key lengths.

#### README.md

Complete the skeleton from S0 with:

- Crate purpose (one paragraph)
- The TOON format (one example)
- Installation (`Cargo.toml` snippet + npm snippet)
- Quick start (5-line Rust example, 5-line TypeScript example)
- Link to `docs/01-spec.md`
- License

#### Publish checklist

Before tagging `v0.1.0`:

- [ ] `cargo publish --dry-run` succeeds
- [ ] All snapshots committed and reviewed (`INSTA_UPDATE=no cargo test` green)
- [ ] Benchmarks meet spec targets on the CI runner
- [ ] `cargo doc --no-deps --open` — all public items documented
- [ ] NAPI binaries built and tested on `darwin-arm64` and `linux-x64-musl`
- [ ] npm `pack --dry-run` succeeds

#### Done means

- Both benchmark targets compile and meet the spec thresholds on the CI runner.
- README is complete (purpose, TOON example, install, Rust + TS quick starts,
  spec link, license).
- Every item in the publish checklist is checked off; `cargo publish --dry-run`
  and npm `pack --dry-run` both succeed.

---

## Known constraints and gotchas

#### No `std::time::Instant` in `resilience.rs`

`parse_retry_after` for the HTTP-date format needs wall-clock time
(`SystemTime::now()`), not monotonic time. This is correct: `Retry-After` dates
are absolute, not relative.

#### `thiserror` v2

`Cargo.toml` uses `thiserror = "2"`. The v2 API differs from v1 in a few places
(notably `#[error]` format-string handling). Don't use v1 examples as reference.

#### insta snapshot review workflow

insta writes pending snapshots to `*.snap.new`. Accept interactively with
`cargo insta review` (then commit the `.snap` files) or in bulk with
`cargo insta accept`. The `INSTA_UPDATE` env var controls behavior:

- `INSTA_UPDATE=no` — **CI setting**: an unmatched snapshot is a hard failure;
  nothing is rewritten.
- `INSTA_UPDATE=auto` — local default: write `.snap.new` for review, leave
  accepted snapshots untouched.

```yaml
env:
  INSTA_UPDATE: "no"
```

Only committed `.snap` files count; `*.snap.new` is git-ignored.

#### NAPI builder pattern: `mut self` vs `&self`

`napi-derive` requires `&self` or `&mut self` for class methods — never owned
`self` — because JavaScript owns the instance through GC and Rust cannot move out
of it. The Rust `AgentResponse` builder consumes `self`, so it cannot be exposed
directly.

**Recommended fix:** wrap the builder in an `Option<AgentResponse>` field. Each
setter takes `&mut self`, `take()`s the inner value out (leaving `None`), applies
the consuming Rust setter, stores the result back, and returns `&Self` for JS
chaining; render methods take `&self` and read `inner.as_ref()`. See Session 3B
for the full implementation. Do not make these methods `async` — async +
`&mut self` is unsound at the boundary and would require `unsafe`.

#### `#[napi(catch_unwind)]` on every export

Without it, a Rust panic crosses straight into Node and aborts the **entire
process** — there is no isolation boundary. With it, the panic is converted to a
thrown JS `Error` (or a rejected `Promise` for async fns). Annotate every
exported function and method.

#### `u64` does not cross the boundary cleanly

JS numbers are 64-bit floats (max safe integer 2^53). Use `i64`, `u32`, or
`BigInt`. Expose `Duration` as a `number` of milliseconds, and pass `totalCount`
as `i64` (cast from `u64` internally).

#### `#[napi(object)]` clones; classes reference

A struct exported as `#[napi(object)]` is copied field-by-field across the
boundary — Rust mutations do not propagate back to the JS object. Use a class
(`#[napi(constructor)]`) when you need shared mutable state (as `AgentResponse`
does). Conversely, `Buffer`/typed arrays pass as zero-copy references; do not
retain them past the call without cloning.

#### TypeScript `const enum` interop

napi-rs emits TypeScript `const enum`s, which require `isolatedModules: false`
and do not work under `transpileModule` (esbuild, swc, Babel). If a consumer's
toolchain needs it, emit string-union types instead via a `dtsHeaderFile`
override rather than relying on `const enum`.

#### Cross-compilation with cargo-zigbuild

Use `cargo-zigbuild` for `linux-x64-musl` (and any non-native Linux target);
`@napi-rs/cli` selects it automatically under `--cross-compile`. `darwin-arm64`
is built natively on a macOS runner (also required for code-signing). Install is
a single Cargo binary — no Docker needed on napi-rs v3.

```yaml
- uses: goto-bus-stop/setup-zig@v2
- run: cargo install cargo-zigbuild
- run: rustup target add x86_64-unknown-linux-musl
- run: npm run build -- --cross-compile --target x86_64-unknown-linux-musl
```

#### `sha2` crate

If used for `IdempotencyKey::from_hash`, add it to `[dependencies]`. It has no
transitive deps and is maintained by the RustCrypto project. Version `^0.10`.

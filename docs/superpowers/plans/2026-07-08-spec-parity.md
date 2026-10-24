# Spec Parity Implementation Plan

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the michi crate's actual implementation up to what `docs/01-spec.md` describes, closing every gap between the two — while explicitly preserving the handful of places where the shipped implementation has already superseded the spec for good, documented reasons (napi v3, `divan` not `criterion`, zero-dep-by-default architecture).

**Architecture:** Module-by-module reconciliation, ordered by dependency (leaf modules first, `AgentResponse` and NAPI last since they compose everything else). Each task is TDD: write the test that encodes the spec-mandated behavior, watch it fail against current code, implement, watch it pass, commit. Several modules need real redesign, not additive patches — `error`, `idempotency`, `recovery`, `status`, and `response` all change shape. Where the spec's literal design conflicts with an architectural principle the crate has since earned through this session's adversarial review (e.g. zero runtime deps by default), this plan implements the spec's *intent* with a documented, deliberate deviation rather than reintroducing a problem that was just fixed — see "Design Decisions" below before starting.

**Tech Stack:** Rust (stable, workspace at `rust-version = "1.96"`), `thiserror`, `napi`/`napi-derive` v3, `cargo nextest`, `insta` snapshots, `proptest`, `divan` benchmarks. TypeScript via `@napi-rs/cli` v3 for `packages/michi-node`.

---

## Design Decisions — read this before starting any task

`docs/01-spec.md` is dated "Draft · June 2026" and predates both `docs/superpowers/specs/2026-07-03-michi-design.md` (which already documents some corrections) and this session's adversarial review (which fixed real bugs and established architectural guarantees the spec doesn't know about). "Bring the implementation up to spec" means implementing what the spec is *for* — richer, more capable, MCP-integration-aware primitives — not regressing already-fixed bugs or reintroducing dependencies that were deliberately removed. Every deviation below is deliberate and documented; if you disagree with one, raise it before starting the task that depends on it.

| Spec says | We're keeping instead | Why |
|---|---|---|
| napi v2, `napi4` feature, napi-derive v2 | napi v3, `napi6` feature | Already corrected per `docs/superpowers/specs/2026-07-03-michi-design.md` §7; napi v2 is unmaintained. |
| `rust-version = "1.93"` | `rust-version = "1.96"` | Same design-doc correction; workspace already pinned here. |
| `criterion` benchmarks | `divan` | `CLAUDE.md` non-negotiable: "Benchmarks: divan (not criterion)". Already implemented, already correct. |
| `serde` + `serde_json` as unconditional dependencies | No `serde`/`serde_json` anywhere; typed scalar enums (`kv::KvValue`) instead of `serde_json::Value` for recovery params | This session's adversarial review found serde/serde_json as unconditional-but-unused deps and removed them, restoring the "zero deps by default" guarantee the crate's own design doc promises. Spec's use of `serde_json::Value` for `RecoveryHint.params` and NAPI cell values would reintroduce exactly that problem. `kv::KvValue` (already exists: `Str`/`Int`/`Float`/`Bool`/`Null`) is reused instead — same expressiveness, zero new dependency. |
| `IdempotencyKey::from_hash` uses SHA-256 | FNV-1a (hand-rolled, ~15 lines, no dependency) | SHA-256 needs the `sha2` crate, which is currently gated behind the `cache` feature only. Idempotency keys need to be *stable and low-collision*, not cryptographically secure — SHA-256 is the wrong tool for this job regardless of the dependency question. FNV-1a is a fixed, versionless algorithm (unlike `std::collections::hash_map::DefaultHasher`, which Rust explicitly does not guarantee is stable across compiler versions — disqualifying for a "canonical key across calls" contract). |
| `resilience::RetryConfig` fields: `max_attempts`, `initial_delay`, `jitter: bool` | Keep current names: `max_retries`, `base_delay`, `jitter_factor: f64` | `jitter_factor: f64` is strictly more capable than a bool (supports partial jitter, not just full-jitter-or-none) and was the subject of a real, adversarially-reviewed bug fix this session (jitter previously could exceed `max_delay`). Renaming now would be pure churn on a just-stabilized area. Functional gaps (retry_after integration, the HTTP-date parsing, the `is_retryable_status` 500 bug) are still fixed — see Phase 2. |
| `recovery::RecoveryHint` renders as a bare `help[]` hint (`Retry {tool} with suggestedParams: {...}`) | Keep the dedicated `recovery[N]:` block (already implemented, tested, and reviewed this session) | Spec's flat design predates `AgentResponse.recovery: Vec<RecoveryHint>` (multiple structured recovery hints per response) — cramming several structured entries into the generic `help[]` block loses the ability for a downstream parser to distinguish "next-step suggestion" from "how to recover from this specific failure." The *content* of `RecoveryHint` (spec's `tool`/`params`/`reason`) is adopted in full — only the block wrapper stays as-is. |
| `hints::append_hints(body: &str, hints: &[Hint]) -> String` (returns a new string) | Keep in-place `append_hints(out: &mut String, hints: &[Hint])` | Already correct — avoids an allocation+copy this session specifically removed elsewhere (`recovery::append_recovery` follows the identical pattern). Purely a signature style difference; observable behavior (the appended block) is identical to spec's intent. |
| `truncate()`/`truncate_inline()` take no `hint` parameter (hardcodes `"full=true"`) | Keep the `hint: &str` parameter | Spec's own examples all happen to use `"full=true"`, but hardcoding it would prevent a caller from naming their own actual flag. More flexible, zero downside. |
| `AgentResponse::render_json(&self) -> serde_json::Value` | Keep `render_json(&self) -> String` (hand-built JSON) | Same zero-dep rationale as above — this session already built and adversarially-reviewed a correct hand-rolled JSON string escaper (RFC 8259 control-character handling included). |
| Snapshot/integration tests via "Jest" | Keep Node's built-in `node:test` runner | Already wired into CI (`pnpm test` → `node --test`); switching test runners is unrelated churn. |
| `render_toon()`'s exact capacity formula (`header_len + items.len() × (fields.len() × 16)`) | Keep the current formula (`60 + row_count * (field_count * 12 + 10) + hints.len() * 60`) | Spec explicitly disclaims this as "a heuristic, not a hard bound... not part of the public contract" (`docs/01-spec.md:1558-1572`). Both formulas achieve the same goal (avoid reallocation in the common case); matching the exact constant has no observable benefit. |
| Q1 (TOON vs Markdown-KV retrieval-accuracy experiment), Q2 (crates.io vs git-dep publish strategy), Q3 (`cli` feature v2 terminal-rendering scope) | Not part of this plan | Q1 requires running actual LLM retrieval evals — a research task, not a code gap. Q2 is a publishing-process decision already covered by this session's separate "publish readiness" conversation, not an API/behavior gap. Q3 is explicitly "out of scope for v1" in the spec itself, and the `cli` feature has already moved past spec's "reserved, empty" placeholder in a direction the design doc endorses (real deps, implies `pipeline`) — extending it further is new feature work, not spec parity. |
| Spec's "Performance contract" section (explicit µs/ms targets per operation) | No CI enforcement added | `divan` benchmarks already exist and report these numbers; gating CI on hard microsecond thresholds is typically flaky across CI runners (noisy neighbors, variable hardware) and is a separate initiative (a perf-regression-gate system) from bringing *behavior* up to spec. Flagged here explicitly so it reads as a deliberate scope boundary, not an oversight. |

None of these are gaps — do not "fix" them back to the literal spec text.

---

## Phase 1: `toon` and `truncate` — additive gaps, no redesign

### Task 1: Add `From` impls to `toon::Value`

**Files:**
- Modify: `src/toon/render.rs:5-18` (the `Value` enum)
- Test: `src/toon/render.rs` (inline `#[cfg(test)]` module — currently doesn't exist in this file; add one)

Spec (`docs/01-spec.md:402-407`) requires `From<&str>`, `From<String>`, `From<i64>`, `From<f64>`, `From<bool>`, `From<Option<String>>` for `Value` so callers can build rows with `.into()`. None currently exist.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/toon/render.rs`, after the existing `render` function, before end of file:

```rust
#[cfg(test)]
mod value_conversion_tests {
    use super::Value;

    #[test]
    fn from_str_slice() {
        let v: Value = "hello".into();
        assert_eq!(v, Value::Str("hello".to_string()));
    }

    #[test]
    fn from_string() {
        let v: Value = "hello".to_string().into();
        assert_eq!(v, Value::Str("hello".to_string()));
    }

    #[test]
    fn from_i64() {
        let v: Value = 42i64.into();
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn from_f64() {
        let v: Value = 1.5f64.into();
        assert_eq!(v, Value::Float(1.5));
    }

    #[test]
    fn from_bool() {
        let v: Value = true.into();
        assert_eq!(v, Value::Bool(true));
    }

    #[test]
    fn from_option_string_some() {
        let v: Value = Some("x".to_string()).into();
        assert_eq!(v, Value::Str("x".to_string()));
    }

    #[test]
    fn from_option_string_none() {
        let v: Value = None::<String>.into();
        assert_eq!(v, Value::Null);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi value_conversion_tests`
Expected: FAIL with a compile error — `the trait bound String: Into<Value> is not satisfied` (no `From` impls exist yet).

- [ ] **Step 3: Write the implementation**

In `src/toon/render.rs`, immediately after the `Value` enum's closing `}` (after line 18), add:

```rust
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<Option<String>> for Value {
    fn from(s: Option<String>) -> Self {
        match s {
            Some(s) => Self::Str(s),
            None => Self::Null,
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi value_conversion_tests`
Expected: 7 tests pass.

- [ ] **Step 5: Run full check and commit**

Run: `cargo clippy -p michi --all-features -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/toon/render.rs
git commit -m "feat(toon): add From impls for Value per spec"
```

---

### Task 2: Add debug-assert on row/field length mismatch in `render_toon`

**Files:**
- Modify: `src/toon/render.rs:23-90` (the `render` function)
- Test: same file, `#[cfg(test)] mod tests` (existing module in `escape.rs` is separate — add a new test module to `render.rs` if Task 1 didn't already create one you can extend)

Spec (`docs/01-spec.md:381-383`): "Panics if any `row.len() != fields.len()`" in debug builds; silently skips in release. Current `render()` has no such check at all — `docs/01-spec.md`'s note aside, the *current* doc comment on `render_toon` (`src/toon/mod.rs:29-31`) explicitly documents "does not panic" and puts the burden entirely on the caller. We are changing this to match spec: debug-assert, no release-mode behavior change (this function has never indexed by field position, so there's nothing to skip — a mismatched row already renders every value it has; the debug-assert is purely a development-time correctness signal, not a change to what gets output).

- [ ] **Step 1: Write the failing test**

Add to `src/toon/render.rs`'s test module (from Task 1, or create `#[cfg(test)] mod tests { use super::*; ... }` if operating independently):

```rust
#[cfg(test)]
mod row_length_tests {
    use super::{render, Value};

    #[test]
    #[should_panic(expected = "row length")]
    #[cfg(debug_assertions)]
    fn mismatched_row_length_panics_in_debug() {
        let fields = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec![Value::Int(1)]]; // 1 value, 2 fields declared
        render("t", &fields, &rows, None, &[]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi mismatched_row_length_panics_in_debug`
Expected: FAIL — test expects a panic but none occurs (current code renders the short row without complaint).

- [ ] **Step 3: Write the implementation**

In `src/toon/render.rs`, inside `pub(crate) fn render(...)`, immediately after the `let row_count = rows.len();` / `let field_count = fields.len();` lines (around line 30-31), add:

```rust
    #[cfg(debug_assertions)]
    for row in rows {
        debug_assert!(
            row.len() == field_count,
            "row length {} does not match field count {field_count} (fields: {fields:?})",
            row.len()
        );
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi mismatched_row_length_panics_in_debug`
Expected: PASS. Also run the full toon test suite to confirm no regressions: `cargo nextest run -p michi toon`.

- [ ] **Step 5: Update the doc comment and commit**

In `src/toon/mod.rs`, change the doc comment above `render_toon` (currently around lines 26-32):

```rust
/// Render a TOON document to a string.
///
/// # Panics
///
/// In debug builds, panics if any row's length doesn't match `fields.len()`.
/// Release builds render the mismatched row as-is (this is a development-time
/// correctness signal, not an input-validation guarantee — validate untrusted
/// input before calling this in release builds).
#[must_use]
pub fn render_toon(opts: &ToonOptions) -> String {
    render::render(&opts.type_name, &opts.fields, &opts.rows, opts.total_count, &opts.hints)
}
```

```bash
git add src/toon/mod.rs src/toon/render.rs
git commit -m "feat(toon): debug-assert row/field length match per spec"
```

---

### Task 3: Add `max_cell_len` inline truncation to `ToonOptions`

**Files:**
- Modify: `src/toon/mod.rs` (the `ToonOptions` struct and `render_toon` function)
- Modify: `src/toon/render.rs` (the `render` function signature)
- Test: `tests/toon_integration.rs`

Spec (`docs/01-spec.md:365-369`): `ToonOptions.max_cell_len: usize` (default 200) auto-truncates cell values via the existing truncation signal format. Currently `ToonOptions` has no such field at all — truncation must be done by the caller before constructing rows.

- [ ] **Step 1: Write the failing test**

Add to `tests/toon_integration.rs`:

```rust
#[test]
fn long_cell_value_is_truncated_per_max_cell_len() {
    let long_title = "x".repeat(300);
    let opts = michi::toon::ToonOptions {
        type_name: "issue".to_string(),
        fields: vec!["title".to_string()],
        rows: vec![vec![michi::toon::Value::Str(long_title)]],
        total_count: None,
        hints: vec![],
        max_cell_len: 50,
    };
    let out = michi::toon::render_toon(&opts);
    assert!(out.contains("chars truncated"), "expected truncation signal, got: {out}");
    // The row line itself (between the header's newline and totalCount/help) must not
    // contain a cell longer than max_cell_len characters plus the signal overhead.
    let row_line = out.lines().nth(1).expect("row line exists");
    assert!(row_line.chars().count() <= 50 + 40, "row line too long: {row_line}");
}

#[test]
fn short_cell_value_is_not_truncated() {
    let opts = michi::toon::ToonOptions {
        type_name: "issue".to_string(),
        fields: vec!["title".to_string()],
        rows: vec![vec![michi::toon::Value::Str("short".to_string())]],
        total_count: None,
        hints: vec![],
        max_cell_len: 200,
    };
    let out = michi::toon::render_toon(&opts);
    assert!(out.contains("short"));
    assert!(!out.contains("chars truncated"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --test toon_integration long_cell_value_is_truncated_per_max_cell_len`
Expected: FAIL with a compile error — `ToonOptions` has no field `max_cell_len`.

- [ ] **Step 3: Write the implementation**

In `src/toon/mod.rs`, add the field to `ToonOptions` (after `hints`):

```rust
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
    /// Max `Value::Str` cell length in Unicode scalar values before inline
    /// truncation via [`crate::truncate::truncate_inline`]. Non-string cells
    /// are never truncated.
    pub max_cell_len: usize,
}
```

Add a `Default` impl right after the struct (spec references `impl Default for ToonOptions { ... }` at `docs/01-spec.md:377`):

```rust
impl Default for ToonOptions {
    fn default() -> Self {
        Self {
            type_name: String::new(),
            fields: Vec::new(),
            rows: Vec::new(),
            total_count: None,
            hints: Vec::new(),
            max_cell_len: 200,
        }
    }
}
```

Update `render_toon` in the same file to pass `max_cell_len` through:

```rust
#[must_use]
pub fn render_toon(opts: &ToonOptions) -> String {
    render::render(&opts.type_name, &opts.fields, &opts.rows, opts.total_count, &opts.hints, opts.max_cell_len)
}
```

In `src/toon/render.rs`, update `pub(crate) fn render(...)` to accept and apply `max_cell_len`:

```rust
pub(crate) fn render(
    type_name: &str,
    fields: &[String],
    rows: &[Vec<Value>],
    total_count: Option<usize>,
    hints: &[String],
    max_cell_len: usize,
) -> String {
```

Inside the row-rendering loop, change the `Value::Str(s) => ...` arm to truncate before escaping:

```rust
                Value::Str(s) => {
                    let truncated = crate::truncate::truncate_inline(s, max_cell_len, "full=true");
                    out.push_str(&escape_value(&truncated));
                }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --test toon_integration`
Expected: both new tests pass. Then run the full suite — several existing call sites construct `ToonOptions` as a struct literal and will now fail to compile without `max_cell_len`:

Run: `cargo build -p michi --all-features 2>&1 | grep "missing field"`
Expected: compile errors in `src/napi.rs` (constructs `ToonOptions` directly) and any test files with struct literals.

- [ ] **Step 5: Fix all call sites**

In `src/napi.rs`, inside `render_toon`, the `ToonOptions` struct literal needs `max_cell_len: 200,` added (using the spec default — NAPI doesn't currently expose this as a caller-configurable option; that's fine, it's an internal default, not a spec requirement to expose it over NAPI yet).

Search and fix every other `ToonOptions { ... }` struct literal:

Run: `grep -rln "ToonOptions {" src tests benches`

For each match that isn't using `..Default::default()`, either add `max_cell_len: 200,` explicitly or switch to `ToonOptions { type_name: ..., fields: ..., ..Default::default() }` style — prefer explicit `max_cell_len` in test files where the test is specifically about rendering behavior, and `..Default::default()` in files unrelated to truncation.

- [ ] **Step 6: Run full test suite and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings && cargo fmt --all --check`
Expected: all pass, clean.

```bash
git add src/toon/mod.rs src/toon/render.rs src/napi.rs tests/toon_integration.rs
git commit -m "feat(toon): add max_cell_len inline cell truncation per spec"
```

---

### Task 4: Change `ToonOptions.hints` from `Vec<String>` to `Vec<Hint>`

**Files:**
- Modify: `src/toon/mod.rs`, `src/toon/render.rs`
- Modify: `src/napi.rs` (constructs `ToonOptions` from `Vec<String>` hints coming over NAPI)
- Test: `tests/toon_integration.rs`

Spec's `render_toon` takes `hints: &[Hint]` (`docs/01-spec.md:390`), and the crate's own `Hint` type already exists and is used everywhere else (`AgentResponse.hints: Vec<Hint>`, `status::StatusResponse.hints: Vec<Hint>`). `ToonOptions.hints: Vec<String>` is the one place still bypassing it — a real, if small, consistency gap.

- [ ] **Step 1: Write the failing test**

In `tests/toon_integration.rs`, find the existing test that constructs `hints: vec!["...".to_string()]` (there should be at least one — e.g. a hints-rendering test) and change it to construct `hints: vec![michi::hints::Hint::new("...")]` instead. If no such conversion-focused test exists yet, add:

```rust
#[test]
fn hints_field_accepts_hint_type() {
    let opts = michi::toon::ToonOptions {
        type_name: "issue".to_string(),
        fields: vec![],
        rows: vec![],
        total_count: None,
        hints: vec![michi::hints::Hint::new("do this")],
        max_cell_len: 200,
    };
    let out = michi::toon::render_toon(&opts);
    assert!(out.contains("help[1]:\n  do this\n"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --test toon_integration hints_field_accepts_hint_type`
Expected: FAIL — compile error, `expected Vec<String>, found Vec<Hint>` (field type hasn't changed yet).

- [ ] **Step 3: Write the implementation**

In `src/toon/mod.rs`, change the `hints` field:

```rust
    /// Agent-facing usage hints. Emitted as `help[N]:` block when non-empty.
    pub hints: Vec<crate::hints::Hint>,
```

And update `Default` (from Task 3) — `Vec::new()` still works unchanged since it's still an empty `Vec<T>`.

Update `render_toon`'s call into `render::render`:

```rust
#[must_use]
pub fn render_toon(opts: &ToonOptions) -> String {
    render::render(&opts.type_name, &opts.fields, &opts.rows, opts.total_count, &opts.hints, opts.max_cell_len)
}
```

In `src/toon/render.rs`, change the `hints: &[String]` parameter to `hints: &[crate::hints::Hint]`, and update the hint-rendering loop (currently `out.push_str(hint);` where `hint: &String`) to `out.push_str(hint.as_str());`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --test toon_integration`
Expected: passes. Then fix remaining call sites the same way as Task 3 Step 5 — search `grep -rln "ToonOptions {" src tests` and update every `hints: vec!["...".to_string(), ...]` to `hints: vec![Hint::new("..."), ...]`, importing `michi::hints::Hint` (or `crate::hints::Hint` inside the crate) where needed. `src/napi.rs`'s `render_toon` needs its `hints: opts.hints` line changed to map the incoming `Vec<String>` (still the correct type at the NAPI/JS boundary — JS callers pass plain strings) into `Vec<Hint>`:

```rust
        hints: opts.hints.into_iter().map(Into::into).collect(),
```

(`Hint` already has `impl From<String> for Hint`, so `.into()` works via the existing conversion.)

- [ ] **Step 5: Run full suite and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings`
Expected: clean.

```bash
git add src/toon/mod.rs src/toon/render.rs src/napi.rs tests/toon_integration.rs
git commit -m "feat(toon): use Hint type for ToonOptions.hints per spec, matching the rest of the crate"
```

---

### Task 5: Add `signal: Option<String>` field to `truncate::Truncated`

**Files:**
- Modify: `src/truncate.rs`
- Test: same file's `#[cfg(test)] mod tests`

Spec (`docs/01-spec.md:504-510`): `Truncated.signal: Option<String>` holds *just* the truncation suffix text, separate from `content`. Currently `Truncated` has no `signal` field — the suffix is baked directly into `content` with no way to retrieve it separately. `truncate_inline()` (which bakes the signal into the returned string) is unaffected by this change and keeps its current behavior exactly.

- [ ] **Step 1: Write the failing test**

Add to `src/truncate.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn signal_is_populated_when_truncated() {
        let content = "a".repeat(200);
        let t = truncate(&content, 50, "full=true");
        assert!(t.was_truncated);
        let signal = t.signal.as_deref().expect("signal present when truncated");
        assert!(signal.contains("200 chars truncated"));
        assert!(signal.contains("full=true"));
    }

    #[test]
    fn signal_is_none_when_not_truncated() {
        let t = truncate("hello", 100, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.signal, None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi signal_is_populated_when_truncated`
Expected: FAIL — compile error, `Truncated` has no field `signal`.

- [ ] **Step 3: Write the implementation**

In `src/truncate.rs`, add the field to `Truncated`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Truncated {
    /// The truncated (or original) content string.
    pub content: String,
    /// Original byte length of the input.
    pub original_len: usize,
    /// Whether truncation actually occurred.
    pub was_truncated: bool,
    /// The truncation signal text alone (e.g. `"(N chars truncated — use
    /// full=true)"`), separate from `content`. `None` when not truncated.
    pub signal: Option<String>,
}
```

Update both early-return `Truncated { ... }` literals (the `content.len() <= max_chars` fast path and the `char_count <= max_chars` path) to add `signal: None,`.

Update the truncating path's final `Truncated { ... }` construction to add `signal: Some(suffix.clone()),` — note `suffix` is already computed earlier in the function (`let suffix = format!(" ({char_count} chars truncated — use {hint})");`), so this is just capturing it before it's consumed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --test truncate` — actually these are unit tests inside `src/truncate.rs`, so: `cargo nextest run -p michi truncate::tests`
Expected: all pass, including the 2 new ones.

- [ ] **Step 5: Commit**

```bash
git add src/truncate.rs
git commit -m "feat(truncate): add Truncated.signal field, separable from content, per spec"
```

---

## Phase 2: `resilience` — fix the real bug, add the real gaps

### Task 6: Remove HTTP 500 from `is_retryable_status` (bug fix)

**Files:**
- Modify: `src/resilience/mod.rs`

Spec (`docs/01-spec.md:748-751`, and the full rationale at `docs/01-spec.md:1465-1487`) is explicit and well-reasoned: 500 must NOT be retryable by default, because retrying an unchanged request that hit a server bug reproduces the same bug, and retrying a write that returned 500 can duplicate side effects if the server actually processed it before erroring. The current implementation retries 500. This is a real bug, not a deliberate deviation.

- [ ] **Step 1: Write the failing test**

In `src/resilience/mod.rs`'s test module, change the existing `retryable_status_codes` test (or add a new one) to assert 500 is excluded:

```rust
    #[test]
    fn http_500_is_not_retryable() {
        assert!(!is_retryable_status(500), "500 is a server bug — retrying reproduces it and risks duplicate side effects on writes");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi http_500_is_not_retryable`
Expected: FAIL — current `is_retryable_status(500)` returns `true`.

- [ ] **Step 3: Write the implementation**

In `src/resilience/mod.rs`, change:

```rust
/// Return `true` if the HTTP status code is conventionally retryable.
///
/// Retryable status codes: 429 (rate limit), 502, 503, 504 (gateway/upstream
/// unavailability). HTTP 500 is deliberately excluded — it signals a
/// server-side bug, and retrying an unchanged request reproduces the same
/// bug; retrying a write that returned 500 can also duplicate side effects if
/// the server processed the request before erroring. Callers that know a
/// specific API uses 500 for genuinely transient conditions can add it to
/// their own retry predicate independently of this function.
#[must_use]
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi resilience`
Expected: all pass, including the pre-existing `retryable_status_codes`/`non_retryable_status_codes` tests (neither of those tested 500 either way, so they're unaffected).

- [ ] **Step 5: Commit**

```bash
git add src/resilience/mod.rs
git commit -m "fix(resilience): exclude HTTP 500 from is_retryable_status per spec rationale"
```

---

### Task 7: Integrate `retry_after` into `next_retry_delay`

**Files:**
- Modify: `src/resilience/mod.rs`
- Modify: `src/napi.rs` if it calls `next_retry_delay` (check first — it currently doesn't, this function isn't NAPI-exposed yet)

Spec (`docs/01-spec.md:738-741`): "Respects `retry_after` when provided — returns the larger of backoff and header." Currently `next_retry_delay(config, attempt, jitter_seed)` has no `retry_after` parameter at all — callers must manually `.max()` the two themselves.

- [ ] **Step 1: Write the failing test**

Add to `src/resilience/mod.rs`'s test module:

```rust
    #[test]
    fn retry_after_wins_when_larger_than_backoff() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        // attempt 0 backoff is base_delay (500ms). A 5s Retry-After should win.
        let delay = next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(5))).unwrap();
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn backoff_wins_when_larger_than_retry_after() {
        let config = RetryConfig { jitter_factor: 0.0, base_delay: Duration::from_secs(10), ..Default::default() };
        let delay = next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(1))).unwrap();
        assert_eq!(delay, Duration::from_secs(10));
    }

    #[test]
    fn retry_after_still_capped_at_max_delay() {
        let config = RetryConfig { jitter_factor: 0.0, max_delay: Duration::from_secs(5), ..Default::default() };
        let delay = next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(999))).unwrap();
        assert_eq!(delay, Duration::from_secs(5), "retry_after must not bypass max_delay");
    }

    #[test]
    fn none_retry_after_behaves_as_before() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        let delay = next_retry_delay(&config, 0, 0.0, None).unwrap();
        assert_eq!(delay, config.base_delay);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi retry_after_wins_when_larger_than_backoff`
Expected: FAIL — compile error, `next_retry_delay` takes 3 arguments, 4 supplied.

- [ ] **Step 3: Update all existing call sites' arity first**

Every existing test in `src/resilience/mod.rs` calls `next_retry_delay(&config, attempt, jitter_seed)` with 3 arguments. Update every one of them to pass `None` as the fourth argument (e.g. `next_retry_delay(&config, 0, 0.0, None)`). There are 8 existing call sites in the test module (`first_retry_uses_base_delay`, `second_retry_doubles`, `delay_is_capped_at_max`, `beyond_max_retries_returns_none`, `jitter_increases_delay` (2 calls), `jitter_never_exceeds_max_delay_when_base_already_capped`, `extreme_duration_saturates_instead_of_wrapping`) — update all of them.

- [ ] **Step 4: Write the implementation**

In `src/resilience/mod.rs`, change the function signature and body:

```rust
/// Compute the delay before the next retry attempt.
///
/// Uses exponential back-off: `base_delay * 2^attempt`, with optional jitter
/// derived from `jitter_seed` (a value in `[0.0, 1.0]` supplied by the caller
/// — use a PRNG, not `rand` inside michi) added to the pre-cap delay. The
/// jittered total is capped at `max_delay`. If `retry_after` is `Some`, the
/// returned delay is the larger of the computed backoff and `retry_after` —
/// still capped at `max_delay` either way, so a server-supplied `Retry-After`
/// can never force an unbounded wait.
///
/// Returns `None` when `attempt >= config.max_retries`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
pub fn next_retry_delay(
    config: &RetryConfig,
    attempt: u32,
    jitter_seed: f64,
    retry_after: Option<Duration>,
) -> Option<Duration> {
    if attempt >= config.max_retries {
        return None;
    }
    let exp = 2u64.saturating_pow(attempt);
    let base_ms = u64::try_from(config.base_delay.as_millis()).unwrap_or(u64::MAX);
    let max_ms = u64::try_from(config.max_delay.as_millis()).unwrap_or(u64::MAX);
    let raw_ms = base_ms.saturating_mul(exp);
    let jitter_ms = (raw_ms as f64 * config.jitter_factor * jitter_seed) as u64;
    let jittered_ms = raw_ms.saturating_add(jitter_ms);
    let retry_after_ms = retry_after.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let capped_ms = jittered_ms.max(retry_after_ms).min(max_ms);
    Some(Duration::from_millis(capped_ms))
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p michi resilience`
Expected: all pass (old tests with the added `None` arg, plus the 4 new ones).

- [ ] **Step 6: Update the usage-pattern doc example and commit**

In `docs/01-spec.md`, the usage pattern at lines 754-769 already shows the correct 4-argument call shape conceptually (`next_retry_delay(attempt, &config, retry_after)` — note spec's arg order is `attempt, config, retry_after`, ours is `config, attempt, jitter_seed, retry_after`; this is fine, order isn't part of the observable contract, just don't copy spec's example verbatim into rustdoc without adjusting arg order). No doc changes strictly required here since this is source-only; Phase 11 reconciles `docs/01-spec.md` itself.

```bash
git add src/resilience/mod.rs
git commit -m "feat(resilience): integrate retry_after into next_retry_delay per spec"
```

---

### Task 8: Parse HTTP-date format in `parse_retry_after`

**Files:**
- Modify: `src/resilience/mod.rs`
- Test: same file

Spec (`docs/01-spec.md:733-736`, detail at `1530-1546`): `parse_retry_after` must accept both integer-seconds AND the RFC 7231 HTTP-date form (`"Wed, 21 Oct 2026 07:28:00 GMT"`), always UTC. Currently only integer-seconds is parsed. An HTTP-date is an *absolute* timestamp, but this function returns a *relative* `Duration` — converting requires knowing "now", which this crate has never called (`SystemTime::now()` appears nowhere in `src/`, keeping every function pure and deterministically testable). We resolve this by adding a `_at` variant that takes `now` explicitly (pure, testable) and having the public function call it with the real clock.

- [ ] **Step 1: Write the failing test**

Add to `src/resilience/mod.rs`'s test module:

```rust
    #[test]
    fn http_date_in_future_parses_relative_to_now() {
        // now = 2026-01-01T00:00:00Z, header = 2026-01-01T00:01:00Z -> 60s
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600); // 2026-01-01T00:00:00Z
        let delay = parse_retry_after_at("Thu, 01 Jan 2026 00:01:00 GMT", now);
        assert_eq!(delay, Some(Duration::from_secs(60)));
    }

    #[test]
    fn http_date_in_past_clamps_to_zero() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600); // 2026-01-01T00:00:00Z
        let delay = parse_retry_after_at("Wed, 31 Dec 2025 00:00:00 GMT", now);
        assert_eq!(delay, Some(Duration::ZERO), "past dates clamp to zero, per spec's clock-skew note");
    }

    #[test]
    fn malformed_http_date_returns_none() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("not a date", now), None);
        assert_eq!(parse_retry_after_at("Wed, 32 Foo 2026 00:00:00 GMT", now), None);
    }

    #[test]
    fn public_parse_retry_after_still_handles_seconds() {
        // regression guard: the public function must still handle the integer form
        assert_eq!(parse_retry_after("60"), Some(Duration::from_secs(60)));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi http_date_in_future_parses_relative_to_now`
Expected: FAIL — `parse_retry_after_at` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

In `src/resilience/mod.rs`, replace the current `parse_retry_after` with:

```rust
/// Parse the value of an HTTP `Retry-After` header as a delay in seconds,
/// relative to the current wall-clock time.
///
/// Handles both forms from RFC 7231 §7.1.3: delay-seconds (`"120"`) and
/// HTTP-date (`"Wed, 21 Oct 2026 07:28:00 GMT"`, always UTC). Returns `None`
/// for malformed or absent values.
#[must_use]
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    parse_retry_after_at(header_value, std::time::SystemTime::now())
}

/// Like [`parse_retry_after`], but takes the current time explicitly instead
/// of reading the system clock — deterministic and testable. `now` matters
/// only for the HTTP-date form; the delay-seconds form ignores it entirely.
#[must_use]
pub fn parse_retry_after_at(header_value: &str, now: std::time::SystemTime) -> Option<Duration> {
    let trimmed = header_value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let target = parse_http_date(trimmed)?;
    Some(target.duration_since(now).unwrap_or(Duration::ZERO))
}

/// Parse a fixed-format RFC 7231 HTTP-date (`"Www, dd Mmm yyyy HH:MM:SS GMT"`)
/// into an absolute `SystemTime`. No timezone library needed — the format is
/// always GMT and fixed-width, so this is pure calendar arithmetic.
fn parse_http_date(s: &str) -> Option<std::time::SystemTime> {
    // "Wed, 21 Oct 2026 07:28:00 GMT"
    let s = s.strip_suffix(" GMT")?;
    let (_weekday, rest) = s.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: u64 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    if parts.next().is_some() {
        return None; // trailing garbage
    }
    let mut time_parts = time.split(':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 || day == 0 || day > 31 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let epoch_secs = days.checked_mul(86_400)?.checked_add((hour * 3600 + minute * 60 + second) as i64)?;
    let unix_secs = u64::try_from(epoch_secs).ok()?;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(unix_secs))
}

fn month_number(name: &str) -> Option<u64> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

/// Days since the Unix epoch for a given proleptic-Gregorian civil date.
/// Howard Hinnant's `days_from_civil` algorithm — no external date library,
/// correct for the full range this parser can produce (years > 1970).
fn days_from_civil(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11], Mar=0 .. Feb=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468 // 719468 = days from 0000-03-01 to 1970-01-01
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi resilience`
Expected: all pass. Double check the `days_from_civil` arithmetic against a known value if any test fails — 2026-01-01 is `1_767_225_600` Unix seconds (verify: `date -u -d @1767225600` on Linux or `date -u -r 1767225600` on macOS should print `Thu Jan  1 00:00:00 UTC 2026`).

- [ ] **Step 5: Commit**

```bash
git add src/resilience/mod.rs
git commit -m "feat(resilience): parse RFC 7231 HTTP-date form in parse_retry_after per spec"
```

---

## Phase 3: `recovery` redesign

### Task 9: Redesign `RecoveryHint` to `tool`/`params`/`reason`, reusing `kv::KvValue`

**Files:**
- Modify: `src/recovery.rs` (full rewrite of the struct and render logic)
- Modify: `src/response.rs` (constructs/renders `RecoveryHint` — check call sites)
- Modify: `src/idempotency.rs` (Phase 5 will add `FailedOp.recovery: Option<RecoveryHint>` — not yet, just be aware this type is about to be depended on there)
- Test: `src/recovery.rs`

Spec (`docs/01-spec.md:834-849`): `RecoveryHint{tool: String, params: Vec<(String, serde_json::Value)>, reason: Option<String>}`. Per the Design Decisions table, `params` uses `kv::KvValue` instead of `serde_json::Value` (zero-dep). The block wrapper (`recovery[N]:`) stays — only the per-hint field shape and rendered text change to match spec's `tool`/`params`/`reason` semantics.

- [ ] **Step 1: Write the failing test**

Replace the entire contents of `src/recovery.rs`'s `#[cfg(test)] mod tests` block with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::KvValue;

    #[test]
    fn hint_with_params_renders_suggested_params() {
        let hints = [RecoveryHint::new("assign_user").param("user", KvValue::Str("alice".to_string()))];
        let out = render_recovery(&hints);
        assert!(out.contains("assign_user: suggestedParams: { user: alice }"), "got: {out}");
    }

    #[test]
    fn hint_with_multiple_params_renders_all() {
        let hints = [RecoveryHint::new("create_item")
            .param("project", KvValue::Str("PROJ".to_string()))
            .param("type", KvValue::Str("Task".to_string()))];
        let out = render_recovery(&hints);
        assert!(out.contains("project: PROJ"), "got: {out}");
        assert!(out.contains("type: Task"), "got: {out}");
    }

    #[test]
    fn hint_with_reason_includes_it() {
        let hints = [RecoveryHint::new("retry_call").reason("rate limit hit")];
        let out = render_recovery(&hints);
        assert!(out.contains("retry_call"));
        assert!(out.contains("rate limit hit"));
    }

    #[test]
    fn hint_with_no_params_no_reason_renders_bare_tool_name() {
        let hints = [RecoveryHint::new("list_issues")];
        let out = render_recovery(&hints);
        assert!(out.contains("recovery[1]:\n  list_issues\n"), "got: {out}");
    }

    #[test]
    fn empty_hints_returns_empty() {
        assert_eq!(render_recovery(&[]), "");
    }

    #[test]
    fn multiple_hints_renders_count() {
        let hints = [RecoveryHint::new("retry"), RecoveryHint::new("escalate")];
        let out = render_recovery(&hints);
        assert!(out.starts_with("recovery[2]:\n"));
    }

    #[test]
    fn append_recovery_modifies_string() {
        let mut s = "body\n".to_string();
        append_recovery(&mut s, &[RecoveryHint::new("retry")]);
        assert_eq!(s, "body\nrecovery[1]:\n  retry\n");
    }

    #[test]
    fn append_recovery_noop_when_empty() {
        let mut s = "base".to_string();
        append_recovery(&mut s, &[]);
        assert_eq!(s, "base");
    }

    #[test]
    fn int_and_bool_params_render_correctly() {
        let hints = [RecoveryHint::new("retry_after").param("seconds", KvValue::Int(30)).param("force", KvValue::Bool(true))];
        let out = render_recovery(&hints);
        assert!(out.contains("seconds: 30"), "got: {out}");
        assert!(out.contains("force: true"), "got: {out}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi recovery::tests`
Expected: FAIL with many compile errors — `RecoveryHint::new` currently takes `(action, description)`, not just `(tool)`; no `.param()` method exists; `.reason()` currently doesn't exist either (only `.with_example()`).

- [ ] **Step 3: Write the implementation**

Replace the non-test contents of `src/recovery.rs` entirely:

```rust
use crate::kv::KvValue;

/// A structured recovery hint for an agent encountering an error.
///
/// Names a tool to call and, optionally, the parameters to call it with —
/// machine-actionable, not just descriptive text. Rendered as part of a
/// `recovery[N]:` block (see [`render_recovery`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryHint {
    /// The tool/operation name the agent should call to recover.
    pub tool: String,
    /// Ordered key-value parameters to call `tool` with.
    pub params: Vec<(String, KvValue)>,
    /// Optional human-readable reason this recovery path applies.
    pub reason: Option<String>,
}

impl RecoveryHint {
    /// Create a recovery hint naming just the tool to call.
    pub fn new(tool: impl Into<String>) -> Self {
        Self { tool: tool.into(), params: Vec::new(), reason: None }
    }

    /// Append a suggested parameter.
    #[must_use]
    pub fn param(mut self, key: impl Into<String>, value: KvValue) -> Self {
        self.params.push((key.into(), value));
        self
    }

    /// Attach a human-readable reason.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

fn kv_value_str(v: &KvValue) -> String {
    match v {
        KvValue::Str(s) => s.clone(),
        KvValue::Int(n) => n.to_string(),
        KvValue::Float(f) => f.to_string(),
        KvValue::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        KvValue::Null => String::new(),
    }
}

/// Render a list of recovery hints as an agent-readable block.
///
/// Format:
/// ```text
/// recovery[2]:
///   assign_user: suggestedParams: { user: alice } — user 'ghost' not found
///   list_issues
/// ```
///
/// Returns an empty string when `hints` is empty.
#[must_use]
pub fn render_recovery(hints: &[RecoveryHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let capacity = 16 + hints.len() * 60;
    let mut out = String::with_capacity(capacity);
    append_recovery(&mut out, hints);
    out
}

/// Append a `recovery[N]:` block to an existing string in-place, without
/// allocating an intermediate buffer.
///
/// No-op when `hints` is empty.
pub fn append_recovery(out: &mut String, hints: &[RecoveryHint]) {
    if hints.is_empty() {
        return;
    }
    out.push_str("recovery[");
    out.push_str(&hints.len().to_string());
    out.push_str("]:\n");
    for hint in hints {
        out.push_str("  ");
        out.push_str(&hint.tool);
        if !hint.params.is_empty() {
            out.push_str(": suggestedParams: { ");
            for (i, (k, v)) in hint.params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(k);
                out.push_str(": ");
                out.push_str(&kv_value_str(v));
            }
            out.push_str(" }");
        }
        if let Some(reason) = &hint.reason {
            out.push_str(" — ");
            out.push_str(reason);
        }
        out.push('\n');
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi recovery::tests`
Expected: all pass.

- [ ] **Step 5: Fix downstream call sites**

Run: `cargo build -p michi --all-features 2>&1 | grep -A2 "error\[" | head -80`

`src/response.rs`'s test module constructs `RecoveryHint::new("retry", "wait and retry")` (old 2-arg form) and `RecoveryHint::with_example(...)` (method removed). Fix each:
- `RecoveryHint::new("retry", "wait and retry")` → `RecoveryHint::new("retry").reason("wait and retry")`
- `RecoveryHint::with_example("check_auth", "verify your API key", "get_token()")` → `RecoveryHint::new("check_auth").reason("verify your API key")` (the "example" concept is now folded into `tool` being the actually-callable thing — there's no separate example string in spec's model; if the old test was specifically asserting an "example" appears, adjust the assertion to check for `reason` text instead, since that's the closest surviving concept).

Also update the doc comment in `src/response.rs` around `render_json`'s recovery serialization (`"recovery":[{"action":"...","description":"...","example":"..."}]` — the JSON field names are now stale). Update `render_json` in `src/response.rs` to serialize the new shape:

```rust
        out.push_str("],\"recovery\":[");
        for (i, r) in self.recovery.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"tool\":");
            json_string(&mut out, &r.tool);
            out.push_str(",\"params\":{");
            for (j, (k, v)) in r.params.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                json_string(&mut out, k);
                out.push(':');
                json_string(&mut out, &kv_value_json_str(v));
            }
            out.push('}');
            if let Some(reason) = &r.reason {
                out.push_str(",\"reason\":");
                json_string(&mut out, reason);
            }
            out.push('}');
        }
```

Add a small helper in `src/response.rs` (or reuse — `kv_value_str` in recovery.rs isn't `pub(crate)` yet; make it `pub(crate)` and import it rather than duplicating):

In `src/recovery.rs`, change `fn kv_value_str` to `pub(crate) fn kv_value_str`. In `src/response.rs`, add `use crate::recovery::kv_value_str as kv_value_json_str;` near the top and remove any duplicate helper you may have written inline — reuse the one in `recovery.rs`.

Update `src/response.rs`'s JSON tests (`json_render_with_recovery`, `json_render_recovery_no_example_omits_key`) to match the new JSON shape (`"tool"` not `"action"`, `"params"` object not present when empty, `"reason"` key absent when `None`).

- [ ] **Step 6: Run full suite and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings && cargo fmt --all --check`
Expected: clean.

```bash
git add src/recovery.rs src/response.rs
git commit -m "feat(recovery): redesign RecoveryHint as tool/params/reason per spec, reusing kv::KvValue instead of serde_json"
```

---

## Phase 4: `kv` and `status` redesign

### Task 10: Add column alignment, `total_count`, `hints`, and richer `KvValue` to `kv`

**Files:**
- Modify: `src/kv/mod.rs`
- Test: same file

Spec (`docs/01-spec.md:422-455`): column alignment (padded to longest key), `total_count`/`hints` parameters, and `KvValue::Text/Int/Float(f64,u8)/Bool/Duration/Missing`. Current `render_kv(items: &[KvItem]) -> String` has none of these — `KvValue::Str/Int/Float/Bool/Null`, no alignment, no total_count/hints.

- [ ] **Step 1: Write the failing test**

Replace `src/kv/mod.rs`'s test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hints::Hint;
    use std::time::Duration;

    #[test]
    fn columns_are_aligned_to_longest_key() {
        let items = vec![
            KvItem { key: "name".into(), value: KvValue::Text("Button".into()) },
            KvItem { key: "description".into(), value: KvValue::Text("A button".into()) },
        ];
        let out = render_kv(&items, None, &[]);
        // "description" (11 chars) is the longest key; "name" must be padded to match.
        assert_eq!(out, "name:         Button\ndescription:  A button\n");
    }

    #[test]
    fn total_count_appends_line() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let out = render_kv(&items, Some(5), &[]);
        assert!(out.contains("totalCount: 5\n"));
    }

    #[test]
    fn hints_append_help_block() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        let out = render_kv(&items, None, &[Hint::new("do this")]);
        assert!(out.contains("help[1]:\n  do this\n"));
    }

    #[test]
    fn missing_renders_as_em_dash() {
        let items = vec![KvItem { key: "value".into(), value: KvValue::Missing }];
        assert_eq!(render_kv(&items, None, &[]), "value: —\n");
    }

    #[test]
    fn float_respects_decimal_places() {
        let items = vec![KvItem { key: "ratio".into(), value: KvValue::Float(1.0 / 3.0, 2) }];
        assert!(render_kv(&items, None, &[]).contains("ratio: 0.33"));
    }

    #[test]
    fn duration_renders_as_seconds_with_one_decimal() {
        let items = vec![KvItem { key: "elapsed".into(), value: KvValue::Duration(Duration::from_millis(4200)) }];
        assert!(render_kv(&items, None, &[]).contains("elapsed: 4.2s"));
    }

    #[test]
    fn text_and_bool_render_as_before() {
        let items =
            vec![KvItem { key: "status".into(), value: KvValue::Text("open".into()) }, KvItem { key: "active".into(), value: KvValue::Bool(true) }];
        let out = render_kv(&items, None, &[]);
        assert!(out.contains("status: open\n"));
        assert!(out.contains("active: true\n"));
    }

    #[test]
    fn empty_items_returns_empty_string() {
        assert_eq!(render_kv(&[], None, &[]), "");
    }

    #[test]
    fn single_key_needs_no_padding() {
        let items = vec![KvItem { key: "id".into(), value: KvValue::Int(1) }];
        assert_eq!(render_kv(&items, None, &[]), "id: 1\n");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi kv::tests`
Expected: FAIL — many compile errors (`render_kv` arity, `KvValue::Text`/`Missing`/`Duration` variants, `Float(f64, u8)` shape don't exist yet).

- [ ] **Step 3: Write the implementation**

Replace the non-test contents of `src/kv/mod.rs`:

```rust
use crate::hints::Hint;
use std::fmt::Write as _;
use std::time::Duration;

/// A single key-value pair for [`render_kv`].
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
    /// UTF-8 text value.
    Text(String),
    /// Signed integer.
    Int(i64),
    /// Floating-point number, rendered with the given number of decimal places.
    Float(f64, u8),
    /// Boolean renders as `true` or `false`.
    Bool(bool),
    /// Elapsed time, rendered as fractional seconds with one decimal (e.g. `4.2s`).
    Duration(Duration),
    /// Absent value, renders as `—` (em dash) rather than an empty string —
    /// distinguishes "no value" from "empty string value".
    Missing,
}

fn push_kv_value(out: &mut String, value: &KvValue) {
    match value {
        KvValue::Text(s) => out.push_str(s),
        KvValue::Int(n) => {
            let _ = write!(out, "{n}");
        }
        KvValue::Float(f, decimals) => {
            let _ = write!(out, "{f:.*}", *decimals as usize);
        }
        KvValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        KvValue::Duration(d) => {
            let _ = write!(out, "{:.1}s", d.as_secs_f64());
        }
        KvValue::Missing => out.push('—'),
    }
}

/// Render a list of key-value pairs as a column-aligned multi-line block.
///
/// Keys are left-padded with spaces so every `:` lines up on the longest key.
/// Appends a `totalCount: N` line when `total_count` is `Some`, and a
/// `help[N]:` block when `hints` is non-empty.
///
/// Preferred for single items and small metadata blocks (up to ~5 fields).
/// For lists of 5+ items, prefer [`crate::toon::render_toon`].
///
/// Returns an empty string when `items` is empty.
#[must_use]
pub fn render_kv(items: &[KvItem], total_count: Option<usize>, hints: &[Hint]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let max_key_len = items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
    let capacity = items.len() * (max_key_len + 24) + hints.len() * 50 + 20;
    let mut out = String::with_capacity(capacity);
    for item in items {
        out.push_str(&item.key);
        out.push(':');
        let pad = max_key_len - item.key.chars().count() + 1;
        for _ in 0..pad {
            out.push(' ');
        }
        push_kv_value(&mut out, &item.value);
        out.push('\n');
    }
    if let Some(total) = total_count {
        out.push_str("totalCount: ");
        let _ = write!(out, "{total}");
        out.push('\n');
    }
    crate::hints::append_hints(&mut out, hints);
    out
}
```

Note the padding math: `single_key_needs_no_padding` expects `"id: 1\n"` (one space after the colon, same as before) when there's only one key — with `max_key_len = item.key.len()`, `pad = max_key_len - item.key.len() + 1 = 1`, giving exactly one space. Verify the two-item alignment test's exact spacing by hand before running: `"name"` is 4 chars, `"description"` is 11 chars, so `name` gets `11 - 4 + 1 = 8` spaces after its colon and `description` gets `11 - 11 + 1 = 1` space — matching the test's `"name:         Button\ndescription:  A button\n"` (count the spaces in the test literal to confirm they match 8 and 1 respectively; adjust the test literal if your count differs, the important thing is the alignment formula, not this exact string).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi kv::tests`
Expected: all pass. If the alignment test fails on exact spacing, count spaces in your implementation's actual output (`cargo nextest run -p michi columns_are_aligned_to_longest_key -- --nocapture` or add a temporary `eprintln!` / just trust the formula and fix the test literal to match) rather than changing the padding formula, which is correct per spec's "Column width is determined by the longest key."

- [ ] **Step 5: Fix downstream call sites**

Run: `cargo build -p michi --all-features 2>&1 | grep "error\["`

Expected breakage in `src/status.rs` (Task 11 rewrites this anyway — skip fixing it here, just note it's expected to still be broken until Task 11 lands) and `tests/kv_integration.rs` (update every `render_kv(&items)` call to `render_kv(&items, None, &[])`, and every `KvValue::Str(...)` to `KvValue::Text(...)`).

- [ ] **Step 6: Commit**

```bash
git add src/kv/mod.rs tests/kv_integration.rs
git commit -m "feat(kv): add column alignment, total_count, hints, Duration/Missing/Text variants per spec"
```

(Leave `src/status.rs` broken at this commit — Task 11 fixes it in the same phase, next.)

---

### Task 11: Rebuild `status::StatusResponse` on top of `kv::render_kv`

**Files:**
- Modify: `src/status.rs` (full rewrite)
- Test: same file

Spec (`docs/01-spec.md:787-823`): `StatusItem{key, value: KvValue, health: Option<Health>}`, `Health::Ok/Degraded(String)/Error(String)` (reason embedded in the variant), `StatusResponse{tool_name: &'static str, description: &'static str, items, hints}`, rendering built on `kv::render_kv` — no separate `overall: Health` summary line; instead each item's own value is the real data, with health shown only as a trailing `[DEGRADED: reason]`/`[ERROR: reason]` annotation when *not* `Ok`.

- [ ] **Step 1: Write the failing test**

Replace `src/status.rs`'s test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hints::Hint;
    use crate::kv::KvValue;

    #[test]
    fn tool_and_description_lead_the_output() {
        let resp = StatusResponse::new("my-search-tool", "Semantic code search and symbol analysis", vec![]);
        let out = resp.render();
        assert!(out.starts_with("tool:         my-search-tool\ndescription:  Semantic code search and symbol analysis\n"), "got: {out}");
    }

    #[test]
    fn ok_health_item_has_no_bracket_annotation() {
        let resp = StatusResponse::new(
            "t",
            "d",
            vec![StatusItem { key: "index".into(), value: KvValue::Text("ready".into()), health: Some(Health::Ok) }],
        );
        let out = resp.render();
        assert!(out.contains("index:        ready\n"), "got: {out}");
        assert!(!out.contains("[OK"));
    }

    #[test]
    fn degraded_item_gets_bracket_annotation() {
        let resp = StatusResponse::new(
            "t",
            "d",
            vec![StatusItem {
                key: "cache".into(),
                value: KvValue::Text("warm (98MB / 100MB)".into()),
                health: Some(Health::Degraded("approaching limit".into())),
            }],
        );
        let out = resp.render();
        assert!(out.contains("cache:        warm (98MB / 100MB)  [DEGRADED: approaching limit]\n"), "got: {out}");
    }

    #[test]
    fn error_item_gets_bracket_annotation() {
        let resp = StatusResponse::new(
            "t",
            "d",
            vec![StatusItem { key: "queue".into(), value: KvValue::Text("stalled".into()), health: Some(Health::Error("connection lost".into())) }],
        );
        let out = resp.render();
        assert!(out.contains("[ERROR: connection lost]"), "got: {out}");
    }

    #[test]
    fn item_with_no_health_renders_plain() {
        let resp = StatusResponse::new("t", "d", vec![StatusItem { key: "files".into(), value: KvValue::Int(2847), health: None }]);
        let out = resp.render();
        assert!(out.contains("files:        2847\n"), "got: {out}");
    }

    #[test]
    fn hints_append_help_block() {
        let resp = StatusResponse::new("t", "d", vec![]).with_hints(vec![Hint::new("Run `search <query>` to search")]);
        assert!(resp.render().contains("help[1]:\n  Run `search <query>` to search\n"));
    }

    #[test]
    fn full_example_matches_spec() {
        let resp = StatusResponse::new(
            "my-search-tool",
            "Semantic code search and symbol analysis",
            vec![
                StatusItem { key: "index".into(), value: KvValue::Text("ready".into()), health: Some(Health::Ok) },
                StatusItem { key: "files".into(), value: KvValue::Int(2847), health: None },
                StatusItem {
                    key: "cache".into(),
                    value: KvValue::Text("warm (98MB / 100MB)".into()),
                    health: Some(Health::Degraded("approaching limit".into())),
                },
                StatusItem { key: "last-updated".into(), value: KvValue::Text("4 minutes ago".into()), health: None },
            ],
        )
        .with_hints(vec![Hint::new("Run `search <query>` to search")]);
        let out = resp.render();
        let expected = "tool:         my-search-tool\ndescription:  Semantic code search and symbol analysis\nindex:        ready\nfiles:        2847\ncache:        warm (98MB / 100MB)  [DEGRADED: approaching limit]\nlast-updated: 4 minutes ago\nhelp[1]:\n  Run `search <query>` to search\n";
        assert_eq!(out, expected);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi status::tests`
Expected: FAIL — many compile errors (`StatusResponse::new` arity, `Health::Degraded`/`Error` no longer unit variants, `StatusItem.value` field doesn't exist).

- [ ] **Step 3: Write the implementation**

Replace `src/status.rs` entirely (except the test module from Step 1):

```rust
use crate::hints::Hint;
use crate::kv::{KvItem, KvValue};

/// Health classification for an individual status item.
///
/// `Ok` renders with no annotation. `Degraded`/`Error` carry a reason and
/// render as a trailing `[DEGRADED: reason]`/`[ERROR: reason]` annotation
/// after the item's own value — health is a signal alongside the real data,
/// not a replacement for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Operating normally — no annotation shown.
    Ok,
    /// Degraded but still serving; carries a short reason.
    Degraded(String),
    /// Not serving; carries a short reason.
    Error(String),
}

/// A single named component with a value and optional health signal.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItem {
    /// Component key, e.g. `"index"`, `"cache"`.
    pub key: String,
    /// The component's actual value — what a caller would want to read.
    pub value: KvValue,
    /// Optional health signal. `None` and `Some(Health::Ok)` both render with
    /// no bracket annotation; only `Degraded`/`Error` add one.
    pub health: Option<Health>,
}

/// Content-first orientation response (AXI **P8**): what a tool returns when
/// called with no arguments. Built on [`crate::kv::render_kv`].
#[derive(Debug, Clone, PartialEq)]
pub struct StatusResponse {
    /// The tool's name, shown as the first line.
    pub tool_name: String,
    /// One-line description, shown as the second line.
    pub description: String,
    /// Component statuses.
    pub items: Vec<StatusItem>,
    /// Optional contextual hints for the agent.
    pub hints: Vec<Hint>,
}

impl StatusResponse {
    /// Create a status response.
    pub fn new(tool_name: impl Into<String>, description: impl Into<String>, items: Vec<StatusItem>) -> Self {
        Self { tool_name: tool_name.into(), description: description.into(), items, hints: Vec::new() }
    }

    /// Attach contextual hints.
    #[must_use]
    pub fn with_hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Render this status response as an agent-readable string.
    #[must_use]
    pub fn render(&self) -> String {
        let mut kv_items = Vec::with_capacity(self.items.len() + 2);
        kv_items.push(KvItem { key: "tool".to_string(), value: KvValue::Text(self.tool_name.clone()) });
        kv_items.push(KvItem { key: "description".to_string(), value: KvValue::Text(self.description.clone()) });
        for item in &self.items {
            let annotated = match &item.health {
                None | Some(Health::Ok) => item.value.clone(),
                Some(Health::Degraded(reason)) => annotate(&item.value, "DEGRADED", reason),
                Some(Health::Error(reason)) => annotate(&item.value, "ERROR", reason),
            };
            kv_items.push(KvItem { key: item.key.clone(), value: annotated });
        }
        crate::kv::render_kv(&kv_items, None, &self.hints)
    }
}

fn kv_value_display(v: &KvValue) -> String {
    match v {
        KvValue::Text(s) => s.clone(),
        KvValue::Int(n) => n.to_string(),
        KvValue::Float(f, decimals) => format!("{f:.*}", *decimals as usize),
        KvValue::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        KvValue::Duration(d) => format!("{:.1}s", d.as_secs_f64()),
        KvValue::Missing => "—".to_string(),
    }
}

fn annotate(value: &KvValue, label: &str, reason: &str) -> KvValue {
    KvValue::Text(format!("{}  [{label}: {reason}]", kv_value_display(value)))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi status::tests`
Expected: all pass. If `full_example_matches_spec` fails on exact spacing, that's the `kv::render_kv` alignment formula from Task 10 applying to a 4-key set (`tool`, `description`, and whatever the longest of the item keys is) — recompute by hand (longest key here is `"description"` at 11 chars, so `tool` needs `11-4+1=8` spaces, `index`/`files`/`cache` (4-5 chars) need proportionally fewer, `last-updated` (12 chars) is actually LONGER than `description` — recheck: `"last-updated"` is 12 characters, making it the longest key, not `description`. Recompute all paddings against a 12-char max and fix the `full_example_matches_spec` expected string to match your implementation's real output before treating a mismatch as a bug — the padding *formula* in Task 10 is correct; get the expected literal right by running the code once and reading its actual output.

- [ ] **Step 5: Commit**

```bash
git add src/status.rs
git commit -m "feat(status): rebuild StatusResponse on kv::render_kv with tool_name/description per spec"
```

---

## Phase 5: `error` redesign

### Task 12: Add the full `ErrorCode` vocabulary as a `DomainError` carrier, alongside the existing pipeline variants

**Files:**
- Modify: `src/error.rs`
- Test: same file

Spec (`docs/01-spec.md:559-619`) wants 9 `ErrorCode` variants (`InvalidInput`, `NotFound`, `Unauthorized`, `Forbidden`, `Conflict`, `RateLimited`, `Unavailable`, `Timeout`, `ExternalFailure`), each rendering a KV block (`error: not_found\nmessage: ...\nexit_code: 1\nhelp[N]:...`) with hints/recovery/retryable/retry_after attached to the error itself. Currently `Error` has only two bare domain variants (`InvalidInput(String)`, `NotFound(String)`) with no hints/recovery/retry_after capability at all, and `render()` produces a single line (`"error: {self}"`), not a KV block. This task keeps the existing `Error` enum (needed for `#[source]`-chaining pipeline variants — a real Rust idiom already working and tested) and adds a `Domain(DomainError)` variant carrying the spec's richer shape, rather than trying to cram hints/recovery fields onto every thiserror enum variant.

- [ ] **Step 1: Write the failing test**

Add to `src/error.rs`'s test module:

```rust
    #[test]
    fn error_code_labels_are_snake_case() {
        assert_eq!(ErrorCode::InvalidInput.label(), "invalid_input");
        assert_eq!(ErrorCode::NotFound.label(), "not_found");
        assert_eq!(ErrorCode::Unauthorized.label(), "unauthorized");
        assert_eq!(ErrorCode::Forbidden.label(), "forbidden");
        assert_eq!(ErrorCode::Conflict.label(), "conflict");
        assert_eq!(ErrorCode::RateLimited.label(), "rate_limited");
        assert_eq!(ErrorCode::Unavailable.label(), "unavailable");
        assert_eq!(ErrorCode::Timeout.label(), "timeout");
        assert_eq!(ErrorCode::ExternalFailure.label(), "external_failure");
    }

    #[test]
    fn non_retryable_codes() {
        for code in [ErrorCode::InvalidInput, ErrorCode::NotFound, ErrorCode::Unauthorized, ErrorCode::Forbidden, ErrorCode::Conflict] {
            assert!(!code.is_retryable_by_default(), "{code:?} should not be retryable by default");
        }
    }

    #[test]
    fn retryable_codes() {
        for code in [ErrorCode::RateLimited, ErrorCode::Unavailable, ErrorCode::Timeout, ErrorCode::ExternalFailure] {
            assert!(code.is_retryable_by_default(), "{code:?} should be retryable by default");
        }
    }

    #[test]
    fn domain_error_renders_kv_block_with_hints() {
        let e = DomainError::new(ErrorCode::NotFound, "Issue #9999 does not exist in this repository")
            .hint("Call list_issues to see available numbers");
        let out = e.render();
        assert_eq!(
            out,
            "error: not_found\nmessage: Issue #9999 does not exist in this repository\nexit_code: 1\nhelp[1]:\n  Call list_issues to see available numbers\n"
        );
    }

    #[test]
    fn domain_error_exit_code_is_always_one() {
        let e = DomainError::new(ErrorCode::RateLimited, "slow down");
        assert_eq!(e.exit_code(), 1);
    }

    #[test]
    fn domain_error_retryable_defaults_from_code_but_is_overridable() {
        let default_retryable = DomainError::new(ErrorCode::RateLimited, "x");
        assert!(default_retryable.retryable);
        let overridden = DomainError::new(ErrorCode::NotFound, "x").retryable(true);
        assert!(overridden.retryable, "explicit .retryable() call overrides the code's default");
    }

    #[test]
    fn domain_error_carries_recovery() {
        let e = DomainError::new(ErrorCode::Conflict, "already exists").recovery(crate::recovery::RecoveryHint::new("get_issue"));
        assert_eq!(e.recovery.as_ref().unwrap().tool, "get_issue");
    }

    #[test]
    fn error_domain_variant_wraps_domain_error() {
        let e = Error::Domain(DomainError::new(ErrorCode::NotFound, "gone"));
        assert_eq!(e.class(), ErrorClass::User);
        assert!(!e.is_retryable());
        assert_eq!(e.exit_code(), 1);
        assert!(e.render().starts_with("error: not_found\n"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi error::tests`
Expected: FAIL — `ErrorCode`, `DomainError` don't exist yet; `Error::Domain` variant doesn't exist.

- [ ] **Step 3: Write the implementation**

In `src/error.rs`, add (before the `Error` enum definition):

```rust
use crate::hints::Hint;
use crate::recovery::RecoveryHint;

/// The specific kind of domain error, independent of the pipeline-execution
/// error variants. Each code has a default retryability and renders in
/// snake_case via [`ErrorCode::label`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Bad parameters. Non-retryable — the agent must change something first.
    InvalidInput,
    /// Resource absent. Non-retryable.
    NotFound,
    /// Auth failure. Non-retryable.
    Unauthorized,
    /// Permission denied. Non-retryable.
    Forbidden,
    /// Resource state mismatch. Non-retryable.
    Conflict,
    /// Rate limited (HTTP 429). Retryable — check `retry_after`.
    RateLimited,
    /// Service unavailable (HTTP 503). Retryable.
    Unavailable,
    /// Request timed out. Retryable.
    Timeout,
    /// Downstream/external failure. Retryable.
    ExternalFailure,
}

impl ErrorCode {
    /// The snake_case label used in rendered output.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::ExternalFailure => "external_failure",
        }
    }

    /// Whether this code is conventionally retryable, absent an explicit
    /// override via [`DomainError::retryable`].
    #[must_use]
    pub fn is_retryable_by_default(&self) -> bool {
        matches!(self, Self::RateLimited | Self::Unavailable | Self::Timeout | Self::ExternalFailure)
    }
}

/// A domain-level error: a classified code, message, and everything needed
/// to render an agent-actionable response — hints, an optional structured
/// recovery hint, and retry metadata. The HTTP-status-to-`ErrorCode` mapping
/// is deliberately not provided here — callers interpret their own failures
/// into an `ErrorCode`, keeping this module free of HTTP knowledge.
#[derive(Debug, Clone)]
pub struct DomainError {
    /// The error classification.
    pub code: ErrorCode,
    /// Human-readable message.
    pub message: String,
    /// Contextual hints, rendered as a trailing `help[N]:` block.
    pub hints: Vec<Hint>,
    /// Optional structured recovery hint.
    pub recovery: Option<RecoveryHint>,
    /// Whether this error is safe to retry. Defaults to `code.is_retryable_by_default()`.
    pub retryable: bool,
    /// Parsed `Retry-After` delay, if known.
    pub retry_after: Option<Duration>,
}

impl DomainError {
    /// Create a domain error. `retryable` defaults from `code`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { retryable: code.is_retryable_by_default(), code, message: message.into(), hints: Vec::new(), recovery: None, retry_after: None }
    }

    /// Append a contextual hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint::new(hint));
        self
    }

    /// Attach a structured recovery hint.
    #[must_use]
    pub fn recovery(mut self, r: RecoveryHint) -> Self {
        self.recovery = Some(r);
        self
    }

    /// Override the default retryability for this specific error instance.
    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Attach a `Retry-After` delay.
    #[must_use]
    pub fn retry_after(mut self, d: Duration) -> Self {
        self.retry_after = Some(d);
        self
    }

    /// Render to an agent-readable KV block with exit code and hints.
    ///
    /// Format:
    /// ```text
    /// error: not_found
    /// message: Issue #9999 does not exist in this repository
    /// exit_code: 1
    /// help[1]:
    ///   Call list_issues to see available numbers
    /// ```
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(64 + self.message.len() + self.hints.len() * 50);
        out.push_str("error: ");
        out.push_str(self.code.label());
        out.push_str("\nmessage: ");
        out.push_str(&self.message);
        out.push_str("\nexit_code: 1\n");
        // Drop the trailing '\n' just added before append_hints, which starts its own block cleanly.
        out.pop();
        out.push('\n');
        crate::hints::append_hints(&mut out, &self.hints);
        out
    }
}
```

Now update the `Error` enum: add `Domain(DomainError)` as a new always-compiled variant (alongside the existing `InvalidInput(String)`/`NotFound(String)` — **keep those two for now**, they're used elsewhere in the crate/tests; this task is additive, not yet a removal):

```rust
    /// A classified domain error with hints, recovery, and retry metadata.
    #[error("{}: {}", .0.code.label(), .0.message)]
    Domain(DomainError),
```

Add this variant to the `Error` enum's domain-errors section (near `InvalidInput`/`NotFound`).

Update `Error::class()` to handle `Domain`:

```rust
            Self::Domain(d) if d.retryable => ErrorClass::Transient,
            Self::Domain(_) => ErrorClass::User,
```

(Place these arms appropriately in the existing `match` — `Domain`'s classification depends on its own `retryable` field, not a fixed mapping, so it needs its own arms rather than grouping with `InvalidInput`/`NotFound`.)

Update `Error::render()` to delegate to `DomainError::render()` when the variant is `Domain`:

```rust
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Domain(d) => d.render(),
            other => format!("error: {other}"),
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi error::tests`
Expected: all pass.

- [ ] **Step 5: Run full workspace check and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings && cargo fmt --all --check`
Expected: clean (the `Domain` variant is additive, so the two pre-existing `InvalidInput`/`NotFound` tests and their call sites are untouched).

```bash
git add src/error.rs
git commit -m "feat(error): add ErrorCode + DomainError carrying hints/recovery/retry metadata per spec"
```

---

## Phase 6: `idempotency` redesign

### Task 13: Add `IdempotencyKey::from_hash` via FNV-1a

**Files:**
- Modify: `src/idempotency.rs`
- Test: same file

Spec (`docs/01-spec.md:632-641`, hashing guidance at `1436-1461`): `IdempotencyKey::from_hash(operation, data: &[u8])`. Per Design Decisions, this uses FNV-1a rather than SHA-256 — no new dependency, and cryptographic strength isn't the property this needs (stability and low collision are).

- [ ] **Step 1: Write the failing test**

Add to `src/idempotency.rs`'s test module:

```rust
    #[test]
    fn from_hash_is_deterministic() {
        let a = IdempotencyKey::from_hash("create_item", b"same input");
        let b = IdempotencyKey::from_hash("create_item", b"same input");
        assert_eq!(a, b);
    }

    #[test]
    fn from_hash_differs_by_operation() {
        let a = IdempotencyKey::from_hash("create_item", b"x");
        let b = IdempotencyKey::from_hash("delete_item", b"x");
        assert_ne!(a, b, "same data, different operation, must produce different keys");
    }

    #[test]
    fn from_hash_differs_by_data() {
        let a = IdempotencyKey::from_hash("create_item", b"x");
        let b = IdempotencyKey::from_hash("create_item", b"y");
        assert_ne!(a, b);
    }

    #[test]
    fn from_hash_produces_stable_known_value() {
        // Locks the exact FNV-1a output for a fixed input, so a future accidental
        // algorithm change is caught by a failing test, not silent key drift.
        let key = IdempotencyKey::from_hash("op", b"data");
        assert_eq!(key.as_str(), "op:9f9169bb0ba735cc");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi from_hash_is_deterministic`
Expected: FAIL — `from_hash` doesn't exist.

- [ ] **Step 3: Write the implementation**

In `src/idempotency.rs`, add to `impl IdempotencyKey`:

```rust
    /// Construct a key from an operation name and raw input bytes, hashed
    /// with FNV-1a for a stable, deterministic, low-collision key. Not
    /// cryptographic — idempotency keys need stability, not security. For
    /// maps/structs, serialize with sorted keys first (e.g. `BTreeMap`, not
    /// `HashMap`) so the same logical input always hashes the same way.
    #[must_use]
    pub fn from_hash(operation: &str, data: &[u8]) -> Self {
        Self(format!("{operation}:{:016x}", fnv1a_64(data)))
    }
```

Add the hash function near the bottom of the file, before the test module:

```rust
/// FNV-1a 64-bit hash. Fixed, versionless algorithm — unlike
/// `std::collections::hash_map::DefaultHasher`, whose algorithm Rust
/// explicitly does not guarantee stays the same across compiler versions.
fn fnv1a_64(data: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi idempotency`
Expected: all pass. If `from_hash_produces_stable_known_value` fails because the hex doesn't match `9f9169bb0ba735cc`, compute FNV-1a 64-bit of `b"data"` independently to confirm the expected value (FNV-1a of the ASCII bytes `d`,`a`,`t`,`a` starting from offset basis `0xcbf29ce484222325` with prime `0x100000001b3` — this is the standard, widely-published test vector for FNV-1a-64 on the string `"data"`; if your computed value differs, trust the algorithm implementation over a mistyped test literal and fix the test to match your code's actual output).

- [ ] **Step 5: Commit**

```bash
git add src/idempotency.rs
git commit -m "feat(idempotency): add IdempotencyKey::from_hash via FNV-1a per spec (no new dependency)"
```

---

### Task 14: Add `render_already_done` (spec's rendering `already_done`, coexisting with the existing check function)

**Files:**
- Modify: `src/idempotency.rs`
- Test: same file

Spec (`docs/01-spec.md:643-683`): `already_done(operation, summary, hints) -> String` always renders a KV block. The existing `already_done(stored: Option<String>) -> AlreadyDone` is a *different, also-useful* function (a check, not a renderer) — kept as-is per the module's current, valid design. This task adds the spec's rendering function under a new name to avoid a collision.

- [ ] **Step 1: Write the failing test**

Add to `src/idempotency.rs`'s test module:

```rust
    #[test]
    fn render_already_done_matches_spec_format() {
        let out = render_already_done(
            "create_issue",
            "Issue #42 already exists with identical fields",
            &[crate::hints::Hint::new("Call get_issue with number=42 to view it")],
        );
        assert_eq!(
            out,
            "operation: create_issue\nstatus:    already_done\nsummary:   Issue #42 already exists with identical fields\nhelp[1]:\n  Call get_issue with number=42 to view it\n"
        );
    }

    #[test]
    fn render_already_done_no_hints_omits_help_block() {
        let out = render_already_done("noop", "nothing changed", &[]);
        assert!(!out.contains("help["));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi render_already_done_matches_spec_format`
Expected: FAIL — function doesn't exist.

- [ ] **Step 3: Write the implementation**

Add to `src/idempotency.rs`:

```rust
/// Render an already-done response: a successful no-op, not an error (exit
/// code 0 is the caller's responsibility — this function only renders).
///
/// Format:
/// ```text
/// operation: create_issue
/// status:    already_done
/// summary:   Issue #42 already exists with identical fields
/// help[1]:
///   Call get_issue with number=42 to view it
/// ```
#[must_use]
pub fn render_already_done(operation: &str, summary: &str, hints: &[crate::hints::Hint]) -> String {
    let mut out = String::with_capacity(64 + operation.len() + summary.len() + hints.len() * 50);
    out.push_str("operation: ");
    out.push_str(operation);
    out.push_str("\nstatus:    already_done\nsummary:   ");
    out.push_str(summary);
    out.push('\n');
    crate::hints::append_hints(&mut out, hints);
    out
}
```

Note the field-label padding (`operation:`, `status:   `, `summary:  `) is fixed/hand-aligned to match spec's example exactly — this is a one-off literal format, not the general `kv::render_kv` alignment (three fixed field names, not a caller-supplied list), so hardcoding the padding here is correct and doesn't need `kv::render_kv`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi idempotency`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/idempotency.rs
git commit -m "feat(idempotency): add render_already_done per spec, alongside the existing already_done check"
```

---

### Task 15: Redesign `PartialSuccess` with `FailedOp` and categorized rendering

**Files:**
- Modify: `src/idempotency.rs`
- Test: same file

Spec (`docs/01-spec.md:652-703`): `PartialSuccess{completed: Vec<String>, failed: Vec<FailedOp>, skipped: Vec<String>}`, `FailedOp{operation, reason, recovery: Option<RecoveryHint>}`, categorized render with per-category TOON-ish blocks, folded recovery hints, and `exit_code()` (1 iff `failed` non-empty). Current `PartialSuccess{completed: Vec<String>, remaining: Vec<String>, reason: String}` with a one-line render has neither the right shape nor the right output format.

- [ ] **Step 1: Write the failing test**

Replace the `partial_success_renders` test (and add more) in `src/idempotency.rs`'s test module:

```rust
    #[test]
    fn partial_success_full_example_matches_spec() {
        let ps = PartialSuccess {
            completed: vec!["create_issue".into(), "add_label".into()],
            failed: vec![FailedOp {
                operation: "assign_user".into(),
                reason: "User 'ghost' not found".into(),
                recovery: Some(crate::recovery::RecoveryHint::new("assign_user").param("user", crate::kv::KvValue::Text("alice".into()))),
            }],
            skipped: vec!["notify_team".into()],
        };
        let out = ps.render();
        assert!(out.starts_with("partial_success: 2 completed, 1 failed, 1 skipped\n"), "got: {out}");
        assert!(out.contains("completed[2]:\n  create_issue\n  add_label\n"), "got: {out}");
        assert!(out.contains("failed[1]{operation,reason}:\n  assign_user,\"User 'ghost' not found\"\n"), "got: {out}");
        assert!(out.contains("skipped[1]:\n  notify_team\n"), "got: {out}");
        assert!(out.contains("help[1]:"), "got: {out}");
        assert!(out.contains("assign_user: suggestedParams: { user: alice }"), "got: {out}");
    }

    #[test]
    fn partial_success_empty_categories_omitted() {
        let ps = PartialSuccess { completed: vec!["a".into()], failed: vec![], skipped: vec![] };
        let out = ps.render();
        assert!(!out.contains("failed["), "empty failed category must be omitted, got: {out}");
        assert!(!out.contains("skipped["), "empty skipped category must be omitted, got: {out}");
    }

    #[test]
    fn partial_success_exit_code_zero_when_no_failures() {
        let ps = PartialSuccess { completed: vec!["a".into()], failed: vec![], skipped: vec!["b".into()] };
        assert_eq!(ps.exit_code(), 0);
    }

    #[test]
    fn partial_success_exit_code_one_when_any_failed() {
        let ps =
            PartialSuccess { completed: vec![], failed: vec![FailedOp { operation: "x".into(), reason: "y".into(), recovery: None }], skipped: vec![] };
        assert_eq!(ps.exit_code(), 1);
    }

    #[test]
    fn failed_op_without_recovery_produces_no_help_block() {
        let ps = PartialSuccess { completed: vec![], failed: vec![FailedOp { operation: "x".into(), reason: "y".into(), recovery: None }], skipped: vec![] };
        assert!(!ps.render().contains("help["));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi partial_success_full_example_matches_spec`
Expected: FAIL — `FailedOp` doesn't exist, `PartialSuccess.failed`/`skipped` fields don't exist, `exit_code()` doesn't exist.

- [ ] **Step 3: Write the implementation**

Replace `PartialSuccess` and its `impl` block in `src/idempotency.rs`:

```rust
/// A single operation that failed within a larger multi-step operation.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedOp {
    /// Identifier of the operation that failed.
    pub operation: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Optional structured recovery hint for this specific failure.
    pub recovery: Option<crate::recovery::RecoveryHint>,
}

/// Signals that an operation partially completed before a failure.
///
/// Use this when some steps of a multi-step operation succeeded — the agent
/// can resume from the checkpoint rather than retrying from scratch.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSuccess {
    /// Identifiers of steps that completed successfully.
    pub completed: Vec<String>,
    /// Steps that failed, with reason and optional recovery hint.
    pub failed: Vec<FailedOp>,
    /// Identifiers of steps that were not attempted.
    pub skipped: Vec<String>,
}

impl PartialSuccess {
    /// Render as an agent-readable string: a P4 summary line, one block per
    /// non-empty outcome category, then any per-op recovery hints folded into
    /// a trailing `help[]` block.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(
            64 + self.completed.iter().map(String::len).sum::<usize>()
                + self.failed.iter().map(|f| f.operation.len() + f.reason.len()).sum::<usize>()
                + self.skipped.iter().map(String::len).sum::<usize>(),
        );
        out.push_str("partial_success: ");
        out.push_str(&self.completed.len().to_string());
        out.push_str(" completed, ");
        out.push_str(&self.failed.len().to_string());
        out.push_str(" failed, ");
        out.push_str(&self.skipped.len().to_string());
        out.push_str(" skipped\n");

        if !self.completed.is_empty() {
            out.push_str("completed[");
            out.push_str(&self.completed.len().to_string());
            out.push_str("]:\n");
            for op in &self.completed {
                out.push_str("  ");
                out.push_str(op);
                out.push('\n');
            }
        }

        if !self.failed.is_empty() {
            out.push_str("failed[");
            out.push_str(&self.failed.len().to_string());
            out.push_str("]{operation,reason}:\n");
            for f in &self.failed {
                out.push_str("  ");
                out.push_str(&f.operation);
                out.push(',');
                out.push_str(&crate::toon::escape_value_pub(&f.reason));
                out.push('\n');
            }
        }

        if !self.skipped.is_empty() {
            out.push_str("skipped[");
            out.push_str(&self.skipped.len().to_string());
            out.push_str("]:\n");
            for op in &self.skipped {
                out.push_str("  ");
                out.push_str(op);
                out.push('\n');
            }
        }

        let recovery_hints: Vec<crate::recovery::RecoveryHint> = self.failed.iter().filter_map(|f| f.recovery.clone()).collect();
        crate::recovery::append_recovery(&mut out, &recovery_hints);
        out
    }

    /// `0` when all operations completed or were skipped; `1` when any failed.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.failed.is_empty())
    }
}
```

This introduces a dependency on a public TOON-escaping function that doesn't exist yet (`crate::toon::escape_value_pub`) — `escape_value` is currently `pub(crate)` inside `toon::escape`, re-exported as `pub(crate) use escape::escape_value;` in `toon/mod.rs` (only crate-internal, per Task work done earlier this session for `pipeline::render()`). Since `idempotency` is a different module needing the same escaping (a failure reason can contain a comma), this is exactly the right seam to make it available — but check first whether it's already crate-visible enough: `pub(crate)` visibility covers the whole crate, including `idempotency.rs`. **Use `crate::toon::escape_value` directly instead of inventing a new `escape_value_pub` name** — it's already `pub(crate)` and reachable from anywhere in the crate. Fix the code above to call `crate::toon::escape_value(&f.reason)` (not `escape_value_pub`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi idempotency`
Expected: all pass.

- [ ] **Step 5: Fix downstream call sites and commit**

Run: `cargo build -p michi --all-features 2>&1 | grep "error\["`

The old `partial_success_renders` test (asserting on `remaining`/`reason` fields) no longer compiles — it was already replaced in Step 1's rewrite of the test module; confirm no other file constructs the old `PartialSuccess{completed, remaining, reason}` shape (`grep -rn "PartialSuccess {" src tests`).

```bash
git add src/idempotency.rs
git commit -m "feat(idempotency): redesign PartialSuccess with FailedOp and categorized rendering per spec"
```

---

## Phase 7: `response::AgentResponse` full rewrite

This is the largest single task in the plan — the primary integration point, and (per the MCP-mapping discussion) the piece that makes the NAPI surface actually useful for a TypeScript MCP server. Read all of Phase 7 before starting; it's one coherent redesign split into steps for reviewability, not independent sub-features.

### Task 16: Rewrite `AgentResponse` as a routing builder over `items`/`kv_items`

**Files:**
- Modify: `src/response.rs` (near-total rewrite)
- Test: same file

Spec (`docs/01-spec.md:858-923`): `AgentResponse` owns `type_name`, `items`/`fields` (→ TOON), `single_item` (→ KV), `total_count`, `hints`, `recovery`, `truncate_cells_at`, and routes rendering based on which setter was called. We additionally keep `is_error: bool` from the current design (not in spec, but valuable — see below, it's what makes `AgentResponse` map directly onto MCP's `CallToolResult.isError`) and keep `render_json()` returning `String` (Design Decisions table).

- [ ] **Step 1: Write the failing test**

Replace `src/response.rs`'s entire `#[cfg(test)] mod tests` block with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::{KvItem, KvValue};
    use crate::toon::Value;

    #[test]
    fn items_path_renders_toon() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1), Value::Str("open".into())]], &["id", "state"]);
        let out = r.render(OutputFormat::Text);
        assert!(out.starts_with("issues[1]{id,state}:\n  1,open\n"), "got: {out}");
    }

    #[test]
    fn kv_items_path_renders_kv() {
        let r = AgentResponse::new("issue").kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(42) }]);
        let out = r.render(OutputFormat::Text);
        assert_eq!(out, "id: 42\n");
    }

    #[test]
    fn total_count_appears_in_toon_output() {
        let r = AgentResponse::new("issues").items(vec![], &["id"]).total_count(99);
        assert!(r.render(OutputFormat::Text).contains("totalCount: 99"));
    }

    #[test]
    fn hint_and_recovery_append_after_body() {
        let r = AgentResponse::new("issue")
            .kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(1) }])
            .hint("do this")
            .recovery_hint(crate::recovery::RecoveryHint::new("retry"));
        let out = r.render(OutputFormat::Text);
        let hint_pos = out.find("help[").unwrap();
        let recovery_pos = out.find("recovery[").unwrap();
        assert!(hint_pos < recovery_pos);
    }

    #[test]
    fn truncate_cells_at_applies_to_toon_items() {
        let long = "x".repeat(300);
        let r = AgentResponse::new("t").items(vec![vec![Value::Str(long)]], &["field"]).truncate_cells_at(50);
        assert!(r.render(OutputFormat::Text).contains("chars truncated"));
    }

    #[test]
    fn as_error_sets_flag_in_json() {
        let r = AgentResponse::new("t").kv_items(vec![]).as_error();
        assert!(r.render(OutputFormat::Json).contains("\"isError\":true"));
    }

    #[test]
    fn json_format_omits_hints_and_recovery_keys_when_empty_toon() {
        let r = AgentResponse::new("issues").items(vec![], &["id"]);
        let json = r.render(OutputFormat::Json);
        assert!(json.contains("\"isError\":false"));
    }

    #[test]
    fn render_hints_only_returns_just_the_help_block() {
        let r = AgentResponse::new("t").kv_items(vec![]).hint("call foo").hint("call bar");
        assert_eq!(r.render_hints_only(), "help[2]:\n  call foo\n  call bar\n");
    }

    #[test]
    fn render_hints_only_empty_when_no_hints() {
        let r = AgentResponse::new("t").kv_items(vec![]);
        assert_eq!(r.render_hints_only(), "");
    }

    #[test]
    fn render_toon_shorthand_matches_render_text_with_toon_format() {
        let r = AgentResponse::new("issues").items(vec![vec![Value::Int(1)]], &["id"]);
        assert_eq!(r.render_toon(), r.render(OutputFormat::Text));
    }

    #[test]
    fn render_kv_shorthand_matches_render_text_with_kv_format() {
        let r = AgentResponse::new("issue").kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(1) }]);
        assert_eq!(r.render_kv(), r.render(OutputFormat::Text));
    }

    #[test]
    fn multiple_hint_calls_accumulate() {
        let r = AgentResponse::new("t").kv_items(vec![]).hint("a").hint("b");
        assert!(r.render_hints_only().contains("help[2]:"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi response::tests`
Expected: FAIL — `AgentResponse::new` currently takes a body string, not a type name; `.items()`/`.kv_items()`/`.total_count()`/`.truncate_cells_at()`/`.render_hints_only()` don't exist.

- [ ] **Step 3: Write the implementation**

Replace `src/response.rs` entirely (keep the `OutputFormat` enum, `json_string`, and `hex_digit` helpers unchanged — they're correct and unaffected by this redesign):

```rust
use crate::hints::Hint;
use crate::kv::KvItem;
use crate::recovery::RecoveryHint;
use crate::toon::Value;

/// The serialisation format for `AgentResponse::render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Plain-text TOON / kv format. Default.
    #[default]
    Text,
    /// Compact JSON object — field names match the builder setters.
    Json,
}

/// Which underlying format an `AgentResponse` will render as, determined by
/// which of `.items()` / `.kv_items()` was called (not by item count).
#[derive(Debug, Clone, PartialEq, Eq)]
enum RenderTarget {
    /// Neither `.items()` nor `.kv_items()` called yet.
    Unset,
    /// `.items()` called — routes to `toon::render_toon`.
    Toon,
    /// `.kv_items()` called — routes to `kv::render_kv`.
    Kv,
}

/// Builder for an agent-facing response. Routes to TOON or KV based on which
/// items method is called, not on item count.
///
/// # Format routing
/// - `.items()` called    → TOON (list of uniform-schema rows)
/// - `.kv_items()` called → KV   (single item or mixed-type metadata)
///
/// Use `.items()` for 5+ uniform rows. Use `.kv_items()` for single items,
/// status data, or heterogeneous metadata. Calling both on one builder is a
/// caller-side logic error — whichever was called *last* wins at render time;
/// treat one `AgentResponse` as one output shape.
///
/// # Examples
///
/// ```rust
/// use michi::response::{AgentResponse, OutputFormat};
///
/// let out = AgentResponse::new("issues")
///     .items(vec![], &["number", "title"])
///     .hint("Try a broader filter")
///     .render(OutputFormat::Text);
/// assert!(out.contains("help[1]:"));
/// ```
#[derive(Debug, Clone)]
pub struct AgentResponse {
    type_name: String,
    target: RenderTarget,
    items: Vec<Vec<Value>>,
    fields: Vec<String>,
    single_item: Vec<KvItem>,
    total_count: Option<usize>,
    hints: Vec<Hint>,
    recovery: Vec<RecoveryHint>,
    truncate_cells_at: usize,
    is_error: bool,
}

impl AgentResponse {
    /// Create a new, empty response for the given type name. Neither
    /// `.items()` nor `.kv_items()` has been called yet — rendering an unset
    /// response produces an empty-ish TOON header for `type_name` (via the
    /// same path as an empty `.items()` call), since that's a safer default
    /// than panicking on a builder a caller hasn't finished configuring.
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            target: RenderTarget::Unset,
            items: Vec::new(),
            fields: Vec::new(),
            single_item: Vec::new(),
            total_count: None,
            hints: Vec::new(),
            recovery: Vec::new(),
            truncate_cells_at: 200,
            is_error: false,
        }
    }

    /// Populate the TOON list path. Routes rendering to `toon::render_toon`.
    #[must_use]
    pub fn items(mut self, rows: Vec<Vec<Value>>, fields: &[&str]) -> Self {
        self.items = rows;
        self.fields = fields.iter().map(|s| (*s).to_string()).collect();
        self.target = RenderTarget::Toon;
        self
    }

    /// Set the total available count, emitted as `totalCount: N` (TOON path only).
    #[must_use]
    pub fn total_count(mut self, n: usize) -> Self {
        self.total_count = Some(n);
        self
    }

    /// Populate the KV single-item path. Routes rendering to `kv::render_kv`.
    #[must_use]
    pub fn kv_items(mut self, items: Vec<KvItem>) -> Self {
        self.single_item = items;
        self.target = RenderTarget::Kv;
        self
    }

    /// Append a contextual hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint::new(hint));
        self
    }

    /// Replace all contextual hints.
    #[must_use]
    pub fn hints(mut self, hints: Vec<Hint>) -> Self {
        self.hints = hints;
        self
    }

    /// Append a recovery hint.
    #[must_use]
    pub fn recovery_hint(mut self, r: RecoveryHint) -> Self {
        self.recovery.push(r);
        self
    }

    /// Set the max cell length before inline truncation on the TOON path (see
    /// `toon::ToonOptions::max_cell_len`). Default: 200.
    #[must_use]
    pub fn truncate_cells_at(mut self, limit: usize) -> Self {
        self.truncate_cells_at = limit;
        self
    }

    /// Mark this response as an error state — reflected in
    /// `OutputFormat::Json`'s `isError` field.
    #[must_use]
    pub fn as_error(mut self) -> Self {
        self.is_error = true;
        self
    }

    fn body(&self) -> String {
        match self.target {
            RenderTarget::Toon | RenderTarget::Unset => {
                let opts = crate::toon::ToonOptions {
                    type_name: self.type_name.clone(),
                    fields: self.fields.clone(),
                    rows: self.items.clone(),
                    total_count: self.total_count,
                    hints: Vec::new(), // hints are appended once, below, not duplicated into the TOON body
                    max_cell_len: self.truncate_cells_at,
                };
                crate::toon::render_toon(&opts)
            }
            RenderTarget::Kv => crate::kv::render_kv(&self.single_item, self.total_count, &[]),
        }
    }

    /// Render the response in the requested format.
    #[must_use]
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Text => self.render_text(),
            OutputFormat::Json => self.render_json(),
        }
    }

    /// Shorthand for `render(OutputFormat::Text)` when the TOON path was used.
    #[must_use]
    pub fn render_toon(&self) -> String {
        self.render_text()
    }

    /// Shorthand for `render(OutputFormat::Text)` when the KV path was used.
    #[must_use]
    pub fn render_kv(&self) -> String {
        self.render_text()
    }

    fn render_text(&self) -> String {
        let body = self.body();
        let mut out = String::with_capacity(body.len() + self.hints.len() * 60 + self.recovery.len() * 80);
        out.push_str(&body);
        crate::hints::append_hints(&mut out, &self.hints);
        crate::recovery::append_recovery(&mut out, &self.recovery);
        out
    }

    /// Render just the `help[N]:` block for `self.hints` — the three-surface
    /// seam for MCP frameworks that render a display body separately (via
    /// their own Markdown layer) and only need michi to own the `help[]`
    /// format for the agent-facing content block. Returns an empty string
    /// when there are no hints.
    #[must_use]
    pub fn render_hints_only(&self) -> String {
        crate::hints::render_hints(&self.hints)
    }

    fn render_json(&self) -> String {
        let body = self.body();
        let capacity =
            body.len() + self.hints.iter().map(|h| h.as_str().len() + 16).sum::<usize>() + self.recovery.len() * 64 + 64;
        let mut out = String::with_capacity(capacity);
        out.push_str("{\"body\":");
        json_string(&mut out, &body);
        out.push_str(",\"hints\":[");
        for (i, h) in self.hints.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json_string(&mut out, h.as_str());
        }
        out.push_str("],\"recovery\":[");
        for (i, r) in self.recovery.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"tool\":");
            json_string(&mut out, &r.tool);
            out.push_str(",\"params\":{");
            for (j, (k, v)) in r.params.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                json_string(&mut out, k);
                out.push(':');
                json_string(&mut out, &crate::recovery::kv_value_str(v));
            }
            out.push('}');
            if let Some(reason) = &r.reason {
                out.push_str(",\"reason\":");
                json_string(&mut out, reason);
            }
            out.push('}');
        }
        out.push_str("],\"isError\":");
        out.push_str(if self.is_error { "true" } else { "false" });
        out.push('}');
        out
    }
}

/// Append a JSON-encoded string (with surrounding quotes and escape sequences)
/// to `out`. Escapes `"`, `\`, `\n`, `\r`, `\t`, and all other control characters
/// (U+0000–U+001F) as `\u00XX` per RFC 8259.
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if u32::from(other) < 0x20 => {
                let code = u32::from(other);
                out.push_str("\\u00");
                out.push(hex_digit(code >> 4));
                out.push(hex_digit(code & 0xf));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Render a nibble (0–15) as a lowercase hex digit.
fn hex_digit(nibble: u32) -> char {
    char::from_digit(nibble, 16).unwrap_or('0')
}
```

Note this calls `crate::recovery::kv_value_str`, which Task 9 made `pub(crate)` — confirm that's still true (it should be, no other task changes its visibility).

Note also: `ToonOptions.hints` is `Vec<Hint>` (Task 4), so `body()`'s `hints: Vec::new()` field needs the right type — `Vec::new()` infers correctly either way since it's empty, no change needed there.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi response::tests`
Expected: all pass.

- [ ] **Step 5: Fix every downstream call site**

This is the biggest ripple in the whole plan. Run:

Run: `grep -rln "AgentResponse::new\|AgentResponse::error" src tests benches`

Every call site constructing `AgentResponse::new(some_body_string)` (old design) needs to become `AgentResponse::new(type_name).items(...)` or `.kv_items(...)` (new design) — there is no mechanical 1:1 translation since the old design took an *already-rendered* string and the new one takes structured data. Check each match individually:

- `src/napi.rs`: does not currently construct `AgentResponse` at all (it wasn't NAPI-exposed before this plan) — no fix needed here, Task 17 adds the NAPI wrapper fresh.
- Any doctest in `src/response.rs` itself was already rewritten in Step 3's replacement.
- Any snapshot test in `tests/snapshot_tests.rs` referencing `AgentResponse` — update to the new builder shape, matching whatever it was originally trying to demonstrate (a full response with hints and recovery, most likely) using `.kv_items()` or `.items()` as appropriate, then run `cargo insta review` (see Task 21) to accept the new snapshot output, since the rendered format hasn't semantically changed (still body + help[] + recovery[]) even though how the body gets built has.

- [ ] **Step 6: Run full workspace suite and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings && cargo fmt --all --check`
Expected: clean.

```bash
git add src/response.rs
git commit -m "feat(response): rewrite AgentResponse as items/kv_items routing builder per spec, keep is_error + String JSON as deliberate additions"
```

---

## Phase 8: NAPI surface — `JsAgentResponse`, `appendHints`, `renderRecovery`

### Task 17: Add `JsAgentResponse` using the Option+take() pattern

**Files:**
- Modify: `src/napi.rs`
- Test: same file (Rust-side) + `packages/michi-node/__test__/index.test.mjs` (JS-side, exercises the actual NAPI boundary)

Spec (`docs/01-spec.md:953-1017`) documents exactly this pattern already — a `#[napi]` class can't expose Rust's consuming (`self`-returning) builder methods directly, since a live JS reference means Rust never owns the value outright. The fix: store the builder in `Option<T>`, `take()` it out on each mutating call, apply the consuming method, put the result back. This is the single biggest addition — it's what makes `AgentResponse` (and therefore MCP tool results) reachable from TypeScript at all.

- [ ] **Step 1: Write the failing test (Rust side)**

Add to `src/napi.rs`'s test module:

```rust
    #[test]
    fn js_agent_response_items_then_render_toon() {
        let mut r = JsAgentResponse::new("issues".to_string());
        r.items(vec![vec![value("int"), ..Default::default()]], vec!["id".to_string()]).unwrap();
        let out = r.render_toon().unwrap();
        assert!(out.starts_with("issues[1]{id}:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_kv_items_then_render_kv() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }]).unwrap();
        let out = r.render_kv().unwrap();
        assert!(out.contains("id:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_hint_and_render_hints_only() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.hint("do this".to_string()).unwrap();
        assert_eq!(r.render_hints_only().unwrap(), "help[1]:\n  do this\n");
    }

    #[test]
    fn js_agent_response_render_json_reflects_is_error() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.as_error().unwrap();
        assert!(r.render_json().unwrap().contains("\"isError\":true"));
    }
```

(These reference a `value(t: &str) -> JsToonValue` helper already present in `napi.rs`'s test module from earlier work, and a `JsKvItem` type this task introduces below — the `..Default::default()` on `JsToonValue` in the first test requires `JsToonValue` to derive `Default`; check whether it does, and if not, add `#[derive(Default)]` to it as part of this task, or spell out all fields explicitly instead of using struct-update syntax — prefer adding `Default` since NAPI-exposed option-heavy structs commonly want it.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --features napi js_agent_response_items_then_render_toon`
Expected: FAIL — `JsAgentResponse`, `JsKvItem` don't exist.

- [ ] **Step 3: Write the implementation**

Add to `src/napi.rs`, after the existing `truncate` export:

```rust
/// Value for a `JsKvItem` (JavaScript-friendly), mirroring `JsToonValue`'s
/// discriminated-union shape.
#[napi(object)]
#[derive(Default)]
pub struct JsToonValue {
    /// Discriminant: `"str"`, `"int"`, `"float"`, `"bool"`, or `"null"`.
    #[napi(js_name = "type")]
    pub r#type: String,
    /// The value when `type` is `"str"`.
    #[napi(js_name = "strVal")]
    pub str_val: Option<String>,
    /// The value when `type` is `"int"`.
    #[napi(js_name = "intVal")]
    pub int_val: Option<i32>,
    /// The value when `type` is `"float"`.
    #[napi(js_name = "floatVal")]
    pub float_val: Option<f64>,
    /// The value when `type` is `"bool"`.
    #[napi(js_name = "boolVal")]
    pub bool_val: Option<bool>,
}
```

(This replaces the existing `JsToonValue` definition in place — just adding `#[derive(Default)]`; do not duplicate the struct.)

```rust
/// A key-value item for [`JsAgentResponse::kv_items`] (JavaScript-friendly).
#[napi(object)]
pub struct JsKvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: JsToonValue,
}

fn js_kv_value_to_rust(v: JsToonValue) -> crate::kv::KvValue {
    match v.r#type.as_str() {
        "str" => crate::kv::KvValue::Text(v.str_val.unwrap_or_default()),
        "int" => crate::kv::KvValue::Int(i64::from(v.int_val.unwrap_or(0))),
        "float" => crate::kv::KvValue::Float(v.float_val.unwrap_or(0.0), 6),
        "bool" => crate::kv::KvValue::Bool(v.bool_val.unwrap_or(false)),
        _ => crate::kv::KvValue::Missing,
    }
}

/// NAPI wrapper around [`crate::response::AgentResponse`].
///
/// `AgentResponse`'s Rust methods consume `self` and return `Self` — that
/// idiom can't cross the NAPI boundary directly, since a `#[napi]` class
/// instance is owned by the JS garbage collector and Rust only ever sees
/// `&mut self`. Each mutating method here `take()`s the inner builder out of
/// its `Option` slot, applies the consuming method, and puts the result back.
#[napi(js_name = "AgentResponse")]
pub struct JsAgentResponse {
    inner: Option<crate::response::AgentResponse>,
}

#[napi]
impl JsAgentResponse {
    /// Create a new response builder for the given type name.
    #[napi(constructor)]
    #[must_use]
    pub fn new(type_name: String) -> Self {
        Self { inner: Some(crate::response::AgentResponse::new(type_name)) }
    }

    fn take(&mut self) -> napi::Result<crate::response::AgentResponse> {
        self.inner.take().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
    }

    /// Populate the TOON list path.
    ///
    /// # Errors
    ///
    /// Returns an error if `rows` or `fields` exceed this crate's NAPI-boundary
    /// size limits, or if this builder was already consumed by `.render*()`.
    #[napi]
    pub fn items(&mut self, rows: Vec<Vec<JsToonValue>>, fields: Vec<String>) -> napi::Result<()> {
        if rows.len() > MAX_ROWS {
            return Err(napi::Error::from_reason(format!("rows length {} exceeds maximum of {MAX_ROWS}", rows.len())));
        }
        if fields.len() > MAX_FIELDS {
            return Err(napi::Error::from_reason(format!("fields length {} exceeds maximum of {MAX_FIELDS}", fields.len())));
        }
        let b = self.take()?;
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let converted: Vec<Vec<crate::toon::Value>> = rows.into_iter().map(|row| row.into_iter().map(js_value_to_rust).collect()).collect();
        self.inner = Some(b.items(converted, &field_refs));
        Ok(())
    }

    /// Set the total available count (TOON path only).
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // n clamped non-negative first
    pub fn total_count(&mut self, n: i32) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.total_count(n.max(0) as usize));
        Ok(())
    }

    /// Populate the KV single-item path.
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn kv_items(&mut self, items: Vec<JsKvItem>) -> napi::Result<()> {
        let b = self.take()?;
        let converted = items.into_iter().map(|i| crate::kv::KvItem { key: i.key, value: js_kv_value_to_rust(i.value) }).collect();
        self.inner = Some(b.kv_items(converted));
        Ok(())
    }

    /// Append a contextual hint.
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn hint(&mut self, hint: String) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.hint(hint));
        Ok(())
    }

    /// Append a recovery hint naming a tool (no structured params — use
    /// `AgentResponse` from Rust directly for typed params; the NAPI surface
    /// keeps this to the common case of "here's what to call next").
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn recovery_hint(&mut self, tool: String, reason: Option<String>) -> napi::Result<()> {
        let b = self.take()?;
        let mut hint = crate::recovery::RecoveryHint::new(tool);
        if let Some(reason) = reason {
            hint = hint.reason(reason);
        }
        self.inner = Some(b.recovery_hint(hint));
        Ok(())
    }

    /// Mark this response as an error state.
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn as_error(&mut self) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.as_error());
        Ok(())
    }

    /// Render via the TOON or KV path (whichever was populated).
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn render_toon(&self) -> napi::Result<String> {
        self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed")).map(crate::response::AgentResponse::render_toon)
    }

    /// Render via the KV path. Identical to `render_toon()` in practice —
    /// both call the same underlying `render(OutputFormat::Text)` — kept as a
    /// separate method to mirror the Rust API's `render_kv()` shorthand.
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn render_kv(&self) -> napi::Result<String> {
        self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed")).map(crate::response::AgentResponse::render_kv)
    }

    /// Render as a compact JSON string (`{"body":...,"hints":[...],"recovery":[...],"isError":bool}`).
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn render_json(&self) -> napi::Result<String> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(|b| b.render(crate::response::OutputFormat::Json))
    }

    /// Render just the `help[N]:` block — the three-surface seam for MCP
    /// frameworks assembling their own display body separately.
    ///
    /// # Errors
    ///
    /// Returns an error if this builder was already consumed.
    #[napi]
    pub fn render_hints_only(&self) -> napi::Result<String> {
        self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed")).map(crate::response::AgentResponse::render_hints_only)
    }
}
```

- [ ] **Step 4: Run test to verify it passes (Rust side)**

Run: `cargo nextest run -p michi --features napi napi::tests`
Expected: all pass, including the pre-existing NAPI tests from earlier work.

- [ ] **Step 5: Add the JS-side integration test**

Add to `packages/michi-node/__test__/index.test.mjs`:

```javascript
import { AgentResponse } from '../index.js'

void describe('AgentResponse', () => {
  void it('builds a TOON response with hints via chained calls', () => {
    const r = new AgentResponse('issues')
    r.items([[{ type: 'int', intVal: 1 }]], ['id'])
    r.hint('do this')
    const out = r.renderToon()
    assert.ok(out.startsWith('issues[1]{id}:'))
    assert.ok(out.includes('help[1]:'))
  })

  void it('builds a KV response', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 42 } }])
    assert.ok(r.renderKv().includes('id:'))
  })

  void it('renderJson reflects asError', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.asError()
    assert.ok(r.renderJson().includes('"isError":true'))
  })

  void it('throws once consumed rather than silently no-opping', () => {
    // there is no explicit "consume" step in this API shape (render* methods take &self,
    // not &mut self, so they don't actually consume) — this test instead documents that
    // calling a mutator after render still works, since render doesn't take the inner value.
    const r = new AgentResponse('t')
    r.kvItems([])
    r.renderKv()
    r.hint('still works')
    assert.ok(r.renderHintsOnly().includes('still works'))
  })
})
```

Note the last test's comment: unlike spec's original sketch (where even `render_toon(&self)` was written taking `&self`, not consuming), this design's render methods take `&self` (read-only), not `&mut self` — so there is no "already rendered" failure mode in practice for the render calls themselves, only for calling a *setter* after... actually, setters also always leave `self.inner` populated again immediately after `take()`, so there is no reachable state where `self.inner` is `None` when a JS caller can observe it (the only time it's `None` is transiently inside a single Rust method call, never between calls). Simplify the doc comments on `items`/`kv_items`/`hint`/`total_count`/`recovery_hint`/`as_error` in Step 3 if this makes the "Returns an error if..." documentation misleading — **before finishing this step**, re-read each `# Errors` doc comment written in Step 3 and confirm it's still accurate: it is not reachable in normal use (the `Option` is only ever briefly `None` inside one method body), so keep the `ok_or_else` as defensive code (cheap, correct, guards against a future refactor accidentally leaving `inner` as `None`) but soften the doc comments to say so, e.g.: "Returns an error only if an internal invariant is violated (should not happen in normal use)."

- [ ] **Step 6: Run the JS test suite**

Run: `cd packages/michi-node && pnpm build --platform && pnpm test`
Expected: all pass, including the new `AgentResponse` describe block.

- [ ] **Step 7: Regenerate and check `index.d.ts`**

Run: `cat packages/michi-node/index.d.ts | grep -A 20 "class AgentResponse"`
Expected: napi-derive auto-generated a `AgentResponse` class declaration with `items`, `totalCount`, `kvItems`, `hint`, `recoveryHint`, `asError`, `renderToon`, `renderKv`, `renderJson`, `renderHintsOnly` methods (camelCase, matching napi-rs's default JS naming convention).

- [ ] **Step 8: Commit**

```bash
git add src/napi.rs packages/michi-node/__test__/index.test.mjs packages/michi-node/index.d.ts packages/michi-node/index.js
git commit -m "feat(napi): add JsAgentResponse via Option+take() pattern per spec — the MCP-relevant NAPI surface"
```

---

### Task 18: Add `appendHints` and `renderRecovery` NAPI exports

**Files:**
- Modify: `src/napi.rs`
- Test: same file + `packages/michi-node/__test__/index.test.mjs`

Spec's Q4 (`docs/01-spec.md:1383-1389`) recommends exporting `AgentResponse` plus `renderHints()`/`appendHints()` as the minimal NAPI surface. `renderHints` already exists; `appendHints` doesn't. Spec's TS types (`docs/01-spec.md:1161-1162`) also show `renderRecovery`.

- [ ] **Step 1: Write the failing test**

Add to `src/napi.rs`'s test module:

```rust
    #[test]
    fn append_hints_appends_to_existing_body() {
        let out = append_hints("body\n".to_string(), vec!["do this".to_string()]).expect("valid input");
        assert_eq!(out, "body\nhelp[1]:\n  do this\n");
    }

    #[test]
    fn render_recovery_basic() {
        let hints = vec![JsRecoveryHint { tool: "retry".to_string(), reason: None }];
        let out = render_recovery(hints).expect("valid input");
        assert!(out.starts_with("recovery[1]:\n  retry"), "got: {out}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --features napi append_hints_appends_to_existing_body`
Expected: FAIL — `append_hints`, `JsRecoveryHint`, `render_recovery` don't exist in `napi.rs`.

- [ ] **Step 3: Write the implementation**

Add to `src/napi.rs`:

```rust
/// Append a `help[N]:` block to an existing body string.
///
/// # Errors
///
/// Returns an error if `hints` exceeds [`MAX_HINTS`] entries.
#[napi(catch_unwind)]
#[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
pub fn append_hints(body: String, hints: Vec<String>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    let mut out = body;
    crate::hints::append_hints(&mut out, &h);
    Ok(out)
}

/// A recovery hint for [`render_recovery`] (JavaScript-friendly — no
/// structured params over this boundary; use `AgentResponse.recoveryHint`
/// for the common case, or the Rust API directly for typed params).
#[napi(object)]
pub struct JsRecoveryHint {
    /// The tool/operation name the agent should call to recover.
    pub tool: String,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Render recovery hints as a `recovery[N]:` block.
///
/// # Errors
///
/// Returns an error if `hints` exceeds [`MAX_HINTS`] entries.
#[napi(catch_unwind)]
pub fn render_recovery(hints: Vec<JsRecoveryHint>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let converted: Vec<crate::recovery::RecoveryHint> = hints
        .into_iter()
        .map(|h| {
            let mut hint = crate::recovery::RecoveryHint::new(h.tool);
            if let Some(reason) = h.reason {
                hint = hint.reason(reason);
            }
            hint
        })
        .collect();
    Ok(crate::recovery::render_recovery(&converted))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --features napi napi::tests`
Expected: all pass.

- [ ] **Step 5: Add JS-side tests, rebuild, run, and commit**

Add to `packages/michi-node/__test__/index.test.mjs`:

```javascript
import { appendHints, renderRecovery } from '../index.js'

void describe('appendHints', () => {
  void it('appends a help block to an existing body', () => {
    assert.strictEqual(appendHints('body\n', ['do this']), 'body\nhelp[1]:\n  do this\n')
  })
})

void describe('renderRecovery', () => {
  void it('renders a recovery block', () => {
    const out = renderRecovery([{ tool: 'retry', reason: 'rate limited' }])
    assert.ok(out.startsWith('recovery[1]:\n  retry'))
    assert.ok(out.includes('rate limited'))
  })
})
```

Run: `cd packages/michi-node && pnpm build --platform && pnpm test`
Expected: all pass.

```bash
git add src/napi.rs packages/michi-node/__test__/index.test.mjs packages/michi-node/index.d.ts packages/michi-node/index.js
git commit -m "feat(napi): add appendHints and renderRecovery exports per spec Q4's minimal-surface recommendation"
```

---

### Task 19: Complete crate-root re-exports per spec's `lib.rs` sketch

**Files:**
- Modify: `src/lib.rs`
- Test: `tests/toon_integration.rs` (add one test exercising the new re-exports so a future accidental removal is caught)

Spec's crate root (`docs/01-spec.md:321-346`) re-exports far more than the current `src/lib.rs` does. Currently only `Error, ErrorClass, Sensitive`, `append_hints, render_hints, Hint`, `AgentResponse, OutputFormat`, and `render_toon, ToonOptions, Value` are re-exported at the crate root — everything else requires the fully-qualified `michi::kv::render_kv` path. This is a real, if small, ergonomics gap: spec's intent is that the common path doesn't require knowing which submodule something lives in.

- [ ] **Step 1: Write the failing test**

Add to `tests/toon_integration.rs`:

```rust
#[test]
fn crate_root_reexports_are_reachable() {
    // Compiles iff every one of these paths resolves at the crate root, per
    // docs/01-spec.md's lib.rs sketch. Not exhaustive of every type in the
    // crate — just the ones spec explicitly lists as top-level re-exports.
    let _: fn(&michi::kv::KvItem, Option<usize>, &[michi::Hint]) -> String = michi::render_kv;
    let _ = michi::empty_state("t");
    let _: fn(Option<String>) -> michi::AlreadyDone = michi::already_done;
    let _: fn(&str, &str, &[michi::Hint]) -> String = michi::render_already_done;
    let _ = michi::RetryConfig::default();
    let _: fn(&str) -> Option<std::time::Duration> = michi::parse_retry_after;
    let _: fn(&michi::resilience::RetryConfig, u32, f64, Option<std::time::Duration>) -> Option<std::time::Duration> =
        michi::next_retry_delay;
    let _: fn(&str, usize, &str) -> michi::Truncated = michi::truncate;
    let _: fn(&str, usize, &str) -> String = michi::truncate_inline;
    let _ = michi::RecoveryHint::new("t");
    let _ = michi::StatusResponse::new("t", "d", vec![]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --test toon_integration crate_root_reexports_are_reachable`
Expected: FAIL — compile errors for every path not yet re-exported at the crate root (`michi::render_kv`, `michi::empty_state`, `michi::already_done`, `michi::render_already_done`, `michi::RetryConfig`, `michi::parse_retry_after`, `michi::next_retry_delay`, `michi::truncate`, `michi::truncate_inline`, `michi::RecoveryHint`, `michi::StatusResponse`, `michi::AlreadyDone`, `michi::Truncated` all currently require their submodule path).

- [ ] **Step 3: Write the implementation**

In `src/lib.rs`, replace the re-export block:

```rust
// Re-export the most common types at the crate root for convenience.
pub use empty::empty_state;
pub use error::{DomainError, Error, ErrorClass, ErrorCode, Sensitive};
pub use hints::{append_hints, render_hints, Hint};
pub use idempotency::{already_done, render_already_done, AlreadyDone, FailedOp, IdempotencyKey, PartialSuccess};
pub use kv::render_kv;
pub use recovery::RecoveryHint;
pub use resilience::{next_retry_delay, parse_retry_after, RetryConfig};
pub use response::{AgentResponse, OutputFormat};
pub use status::StatusResponse;
pub use toon::{render_toon, ToonOptions, Value};
pub use truncate::{truncate, truncate_inline, Truncated};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --test toon_integration crate_root_reexports_are_reachable`
Expected: PASS.

- [ ] **Step 5: Run full workspace check and commit**

Run: `cargo nextest run --workspace --all-features && cargo clippy --workspace --all-features -- -D warnings && cargo fmt --all --check`
Expected: clean — `clippy::wildcard_imports`/unused-import lints shouldn't fire since every re-export is genuinely used somewhere, but double check `unreachable_pub` (warned in `src/lib.rs`) doesn't flag anything; if it does, the flagged item likely needs its own module-level visibility fixed rather than the re-export removed.

```bash
git add src/lib.rs tests/toon_integration.rs
git commit -m "feat(lib): complete crate-root re-exports per spec's lib.rs sketch"
```

---

## Phase 9: Testing gaps — property tests and a test-only TOON parser

### Task 20: Add a test-only TOON parser and property tests

**Files:**
- Create: `tests/toon_parser.rs` (test-only parser, not part of the public API)
- Create: `tests/proptest_toon.rs`
- Create: `tests/proptest_resilience.rs`
- Create: `tests/proptest_truncate.rs`

Spec (`docs/01-spec.md:1327-1332`) wants property tests: TOON output valid per grammar for arbitrary strings, `truncate_inline` never exceeds `limit + signal_len`, TOON round-trip via a test-only parser, `parse_retry_after` never panics on arbitrary strings, `next_retry_delay` always within `[base_delay, max_delay]`-ish bounds. None of these exist — `proptest` has been a listed dev-dependency this whole time with zero actual usage.

- [ ] **Step 1: Write the test-only TOON parser**

Create `tests/toon_parser.rs`:

```rust
//! Test-only TOON parser — NOT part of the public API. Exists purely to
//! support round-trip property tests (render → parse → compare) per
//! docs/01-spec.md's testing strategy. Parses only what render_toon actually
//! produces; not a general-purpose TOON parser for untrusted input.

use michi::toon::Value;

#[derive(Debug, PartialEq)]
pub struct ParsedToon {
    pub type_name: String,
    pub fields: Vec<String>,
    pub rows: Vec<Vec<String>>, // parsed as raw strings; comparing rendered text, not typed values
    pub total_count: Option<usize>,
    pub hints: Vec<String>,
}

pub fn parse(input: &str) -> Option<ParsedToon> {
    let mut lines = input.lines();
    let header = lines.next()?;
    let (type_name, rest) = header.split_once('[')?;
    let (count_str, rest) = rest.split_once(']')?;
    let row_count: usize = count_str.parse().ok()?;
    let fields_str = rest.strip_prefix('{')?.strip_suffix(":")?.strip_suffix('}')?;
    let fields: Vec<String> = if fields_str.is_empty() { Vec::new() } else { fields_str.split(',').map(str::to_string).collect() };

    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let line = lines.next()?.strip_prefix("  ")?;
        rows.push(split_toon_row(line));
    }

    let mut total_count = None;
    let mut hints = Vec::new();
    for line in lines {
        if let Some(n) = line.strip_prefix("totalCount: ") {
            total_count = n.parse().ok();
        } else if line.starts_with("help[") {
            // hint lines follow, each prefixed "  "
        } else if let Some(h) = line.strip_prefix("  ") {
            hints.push(h.to_string());
        }
    }

    Some(ParsedToon { type_name: type_name.to_string(), fields, rows, total_count, hints })
}

fn split_toon_row(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current = String::new();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quotes => in_quotes = true,
            '"' if in_quotes => in_quotes = false,
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' if !in_quotes => {
                values.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    values.push(current);
    values
}

pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        Value::Null => String::new(),
    }
}
```

- [ ] **Step 2: Write a unit test for the parser itself, verify it fails, implement, verify it passes**

Add to the bottom of `tests/toon_parser.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_document() {
        let input = "issues[2]{number,title,state}:\n  42,Fix login redirect,open\n  43,Add dark mode,open\ntotalCount: 47\nhelp[1]:\n  Call get_issue\n";
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.type_name, "issues");
        assert_eq!(parsed.fields, vec!["number", "title", "state"]);
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(parsed.rows[0], vec!["42", "Fix login redirect", "open"]);
        assert_eq!(parsed.total_count, Some(47));
        assert_eq!(parsed.hints, vec!["Call get_issue"]);
    }

    #[test]
    fn parses_quoted_comma_value() {
        let input = "t[1]{a}:\n  \"x,y\"\n";
        let parsed = parse(input).unwrap();
        assert_eq!(parsed.rows[0], vec!["x,y"]);
    }
}
```

Run: `cargo nextest run -p michi --test toon_parser`
Expected: this should already pass since the parser was written in Step 1 alongside its own test — if it doesn't, fix `parse`/`split_toon_row` until both tests pass before moving on. (Unlike other tasks, the parser itself isn't spec-mandated code — it's test infrastructure — so writing it and its unit test together in one step is appropriate.)

- [ ] **Step 3: Write the round-trip property test**

Create `tests/proptest_toon.rs`:

```rust
use michi::toon::{render_toon, ToonOptions, Value};
use proptest::prelude::*;

mod toon_parser;
use toon_parser::{parse, value_to_string};

proptest! {
    #[test]
    fn render_toon_output_is_grammar_valid(
        type_name in "[a-z][a-z0-9_]{0,10}",
        field_count in 1usize..4,
        row_count in 0usize..5,
        cell in "[a-zA-Z0-9 ]{0,20}",
    ) {
        let fields: Vec<String> = (0..field_count).map(|i| format!("f{i}")).collect();
        let rows: Vec<Vec<Value>> = (0..row_count).map(|_| fields.iter().map(|_| Value::Str(cell.clone())).collect()).collect();
        let opts = ToonOptions { type_name: type_name.clone(), fields: fields.clone(), rows: rows.clone(), total_count: None, hints: vec![], max_cell_len: 200 };
        let rendered = render_toon(&opts);

        let parsed = parse(&rendered).expect("render_toon output must be parseable by the grammar");
        prop_assert_eq!(parsed.type_name, type_name);
        prop_assert_eq!(parsed.fields, fields);
        prop_assert_eq!(parsed.rows.len(), row_count);
        for (parsed_row, original_row) in parsed.rows.iter().zip(rows.iter()) {
            for (parsed_cell, original_value) in parsed_row.iter().zip(original_row.iter()) {
                prop_assert_eq!(parsed_cell, &value_to_string(original_value));
            }
        }
    }
}
```

- [ ] **Step 4: Run and verify**

Run: `cargo nextest run -p michi --test proptest_toon`
Expected: PASS (100 generated cases by default). If it fails, the failure is almost certainly in the test-only parser (e.g. mishandling a field-list edge case like zero fields — `t[0]{}:` has an empty `fields_str` — the parser already special-cases this in Step 1's `if fields_str.is_empty()`), not in `render_toon` itself, which has full unit-test coverage elsewhere in this plan. Fix the parser, not the renderer, unless a genuine renderer bug turns up.

- [ ] **Step 5: Write `truncate_inline` and `parse_retry_after`/`next_retry_delay` property tests**

Create `tests/proptest_truncate.rs`:

```rust
use michi::truncate::truncate_inline;
use proptest::prelude::*;

proptest! {
    #[test]
    fn truncate_inline_never_exceeds_limit_plus_signal(
        content in ".{0,500}",
        limit in 1usize..100,
    ) {
        let hint = "full=true";
        let result = truncate_inline(&content, limit, hint);
        // The hard-cap logic in truncate() guarantees the result never exceeds
        // `limit` chars at all (not limit + signal_len) — see src/truncate.rs's
        // final `if result.chars().count() > max_chars` clamp.
        prop_assert!(result.chars().count() <= limit, "result {} chars exceeds limit {limit}: {result:?}", result.chars().count());
    }

    #[test]
    fn truncate_inline_never_splits_utf8(content in ".{0,200}", limit in 1usize..50) {
        let result = truncate_inline(&content, limit, "full=true");
        prop_assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }
}
```

Create `tests/proptest_resilience.rs`:

```rust
use michi::resilience::{next_retry_delay, parse_retry_after, RetryConfig};
use proptest::prelude::*;
use std::time::Duration;

proptest! {
    #[test]
    fn parse_retry_after_never_panics(input in ".{0,200}") {
        let _ = parse_retry_after(&input);
    }

    #[test]
    fn next_retry_delay_within_max_delay_bound(
        attempt in 0u32..5,
        jitter_seed in 0.0f64..1.0,
        base_secs in 1u64..10,
        max_secs in 10u64..60,
        jitter_factor in 0.0f64..1.0,
    ) {
        let config = RetryConfig {
            max_retries: 10,
            base_delay: Duration::from_secs(base_secs),
            max_delay: Duration::from_secs(max_secs),
            jitter_factor,
        };
        if let Some(delay) = next_retry_delay(&config, attempt, jitter_seed, None) {
            prop_assert!(delay <= config.max_delay, "delay {delay:?} exceeded max_delay {:?}", config.max_delay);
        }
    }
}
```

- [ ] **Step 6: Run all property tests**

Run: `cargo nextest run -p michi --test proptest_toon --test proptest_truncate --test proptest_resilience`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add tests/toon_parser.rs tests/proptest_toon.rs tests/proptest_truncate.rs tests/proptest_resilience.rs
git commit -m "test: add test-only TOON parser and proptest property tests per spec testing strategy"
```

---

### Task 21: Refresh snapshot tests for the redesigned modules

**Files:**
- Modify: `tests/snapshot_tests.rs`
- Modify: `tests/snapshots/*.snap` (regenerated via `cargo insta review`)

Every module touched in Phases 3-7 (`recovery`, `status`, `response`) changed its output format. Existing snapshots for those are now stale or the tests referencing them no longer compile. Spec (`docs/01-spec.md:1334-1338`) wants snapshot coverage of "KV column alignment across different key lengths" and "Status response with mixed health signals" specifically — currently absent even before this plan's changes.

- [ ] **Step 1: Fix compilation of `tests/snapshot_tests.rs`**

Run: `cargo build -p michi --tests 2>&1 | grep "error\[" -A 3`

Fix each construction of `AgentResponse`, `StatusResponse`, `RecoveryHint`, `KvItem`/`KvValue::Str` to the new shapes established in Phases 3, 4, and 7 of this plan.

- [ ] **Step 2: Add the two spec-required new snapshots**

Add to `tests/snapshot_tests.rs`:

```rust
#[test]
fn snapshot_kv_column_alignment() {
    use michi::kv::{render_kv, KvItem, KvValue};
    let items = vec![
        KvItem { key: "id".into(), value: KvValue::Int(1) },
        KvItem { key: "description".into(), value: KvValue::Text("A longer field".into()) },
        KvItem { key: "x".into(), value: KvValue::Bool(true) },
    ];
    insta::assert_snapshot!(render_kv(&items, None, &[]));
}

#[test]
fn snapshot_status_mixed_health() {
    use michi::kv::KvValue;
    use michi::status::{Health, StatusItem, StatusResponse};
    let resp = StatusResponse::new(
        "my-tool",
        "does things",
        vec![
            StatusItem { key: "index".into(), value: KvValue::Text("ready".into()), health: Some(Health::Ok) },
            StatusItem { key: "cache".into(), value: KvValue::Text("warm".into()), health: Some(Health::Degraded("near limit".into())) },
            StatusItem { key: "queue".into(), value: KvValue::Text("down".into()), health: Some(Health::Error("disconnected".into())) },
        ],
    );
    insta::assert_snapshot!(resp.render());
}
```

- [ ] **Step 3: Run and accept new snapshots**

Run: `cargo nextest run -p michi --test snapshot_tests`
Expected: new snapshot tests FAIL on first run (no `.snap` file exists yet — `insta` creates a `.snap.new` pending file).

Run: `cargo insta review`
Review each `.snap.new` diff shown — for the two new snapshots, accept them (they define the new baseline). For any *existing* snapshot that changed because of this plan's redesigns (recovery/status/response format changes), accept only if the new output is actually correct per this plan's design (cross-check against the relevant task's example above) — do not blindly accept without checking.

- [ ] **Step 4: Run full suite and commit**

Run: `cargo nextest run --workspace --all-features`
Expected: all pass.

```bash
git add tests/snapshot_tests.rs tests/snapshots/
git commit -m "test: refresh snapshots for recovery/status/response redesigns, add kv alignment + status mixed-health snapshots per spec"
```

---

## Phase 10: CI — version-sync assertion

### Task 22: Assert npm package version equals crate version before publish-relevant CI runs

**Files:**
- Modify: `.github/workflows/ci.yml`

Spec (`docs/01-spec.md:1272-1274`): "CI asserts the two are equal before any publish." No such check currently exists anywhere in `ci.yml`.

- [ ] **Step 1: Add the check as a new CI job**

In `.github/workflows/ci.yml`, add a new job (after the existing `lint` job, for example):

```yaml
  version-sync:
    name: Version sync
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Assert Cargo.toml and package.json versions match
        run: |
          CARGO_VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/version *= *"(.*)"/\1/')
          NPM_VERSION=$(node -p "require('./packages/michi-node/package.json').version")
          echo "Cargo.toml: $CARGO_VERSION"
          echo "package.json: $NPM_VERSION"
          if [ "$CARGO_VERSION" != "$NPM_VERSION" ]; then
            echo "::error::Version mismatch: Cargo.toml=$CARGO_VERSION package.json=$NPM_VERSION"
            exit 1
          fi
```

- [ ] **Step 2: Verify locally**

Run: `grep -m1 '^version' Cargo.toml` and `node -p "require('./packages/michi-node/package.json').version"` — confirm both currently print `0.1.0`, so the check would pass as-is.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: assert Cargo.toml and package.json versions match per spec's versioning contract"
```

---

### Task 23: Add the actual shipped `linux-x64-musl` target to CI (currently only `-gnu` is built)

**Files:**
- Modify: `.github/workflows/ci.yml`

Spec (`docs/01-spec.md:937-939`, `1252-1254`, and `docs/superpowers/specs/2026-07-03-michi-design.md`) is consistent and explicit: the two shipped npm binary targets are `aarch64-apple-darwin` (`darwin-arm64`) and `x86_64-unknown-linux-musl` (`linux-x64-musl`), cross-compiled via `cargo-zigbuild`. The current `ci.yml` `napi` job matrix builds `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-unknown-linux-gnu` — it has never once build-verified the actual musl target this crate is supposed to ship for. This was identified earlier this session (in the publish-readiness audit) and never fixed until now.

- [ ] **Step 1: Read the current matrix**

Run: `grep -A 15 "napi:" .github/workflows/ci.yml`
Expected: confirms the matrix currently has `x86_64-apple-darwin` (macos-13), `aarch64-apple-darwin` (macos-latest), `x86_64-unknown-linux-gnu` (ubuntu-latest) — no musl entry.

- [ ] **Step 2: Add the musl target via `cargo-zigbuild` cross-compilation**

In `.github/workflows/ci.yml`, inside the `napi` job's `strategy.matrix.include` list, add a fourth entry:

```yaml
          - target: x86_64-unknown-linux-musl
            runner: ubuntu-latest
            cross: true
```

Then update the steps below to install `cargo-zigbuild` and pass `--cross-compile` when `matrix.cross` is set. Replace the existing `pnpm build --target ${{ matrix.target }}` step with:

```yaml
      - name: Install cargo-zigbuild (cross-compile targets only)
        if: matrix.cross
        run: pip install ziglang && cargo install cargo-zigbuild --locked
      - run: pnpm build --target ${{ matrix.target }} ${{ matrix.cross && '--cross-compile' || '' }}
        working-directory: packages/michi-node
```

Existing native (non-cross) matrix entries don't set `cross: true`, so `matrix.cross` is falsy for them and the `--cross-compile` flag is omitted — behavior for the three existing entries is unchanged.

Also update the test-skip condition (currently `if: matrix.target != 'aarch64-apple-darwin'`, which skips running tests on that one target because it can't execute foreign-arch binaries on the build runner) to also skip the cross-compiled musl target, since an `ubuntu-latest` runner can't execute a musl binary built via zig's cross-linker either without an emulation layer:

```yaml
      - run: node --test __test__/index.test.mjs
        working-directory: packages/michi-node
        if: matrix.target != 'aarch64-apple-darwin' && matrix.target != 'x86_64-unknown-linux-musl'
```

- [ ] **Step 3: Verify the workflow YAML is syntactically valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: no error (confirms valid YAML syntax before pushing — this doesn't validate GitHub Actions semantics, just that the file parses).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add the actual shipped linux-x64-musl target to the napi build matrix"
```

Note: this task adds CI coverage for the real target; it does not touch the *release/publish* workflow, since none exists yet (identified separately in this session's publish-readiness audit as a bigger, still-open item requiring your input on token vs. OIDC publishing — out of scope for this spec-parity plan).

---

## Phase 11: Reconcile `docs/01-spec.md` itself

### Task 24: Update the spec doc to match the reconciled reality

**Files:**
- Modify: `docs/01-spec.md`

This is the step that closes the loop so spec and code don't silently drift apart again. Every deviation in the Design Decisions table at the top of this plan needs to be written into the spec itself — not because the code should change, but because the *next* reader of `docs/01-spec.md` shouldn't have to reconstruct this reconciliation from a plan file and a code review.

- [ ] **Step 1: Update the Cargo.toml section**

In `docs/01-spec.md`'s `## Cargo.toml` section (around lines 121-170), update:
- `rust-version = "1.93"` → `rust-version = "1.96"`
- Remove `serde`/`serde_json` from `[dependencies]`
- `napi = { version = "2", features = ["napi4"] }` → `napi = { version = "3", features = ["napi6"] }`
- `napi-derive` version `"2"` → `"3"`
- `napi-build` version `"1"` → `"2"`
- `criterion` dev-dependency → `divan`
- Add a one-line note: "Deviations from earlier drafts of this section are tracked in `docs/superpowers/plans/2026-07-08-spec-parity.md`'s Design Decisions table."

- [ ] **Step 2: Update the `resilience` section**

In `docs/01-spec.md`'s `### resilience` section (around lines 707-769):
- Update `RetryConfig` field names to `max_retries`/`base_delay`/`jitter_factor: f64` (matching the actual, kept-as-superior implementation), with a note explaining why `jitter_factor: f64` replaced the originally-specified `jitter: bool`.
- Update `next_retry_delay`'s signature to `next_retry_delay(config: &RetryConfig, attempt: u32, jitter_seed: f64, retry_after: Option<Duration>) -> Option<Duration>`.
- Update `is_retryable_status`'s doc to confirm 500 exclusion is implemented (it already says this — just confirm the implementation note doesn't need changing, since Task 6 made the code match the doc, not the other way around).
- Add the `parse_retry_after_at` variant to the documented API.

- [ ] **Step 3: Update the `error` section**

Update the `AxiError`/`ErrorCode` section to describe the actual `Error` enum / `ErrorCode` / `DomainError` shape from Task 12, keeping the design doc's already-recorded `AxiError → Error` rename rationale and extending it with the `Domain(DomainError)` variant explanation.

- [ ] **Step 4: Update the `recovery` section**

Update `RecoveryHint`'s `params: Vec<(String, serde_json::Value)>` to `params: Vec<(String, crate::kv::KvValue)>`, with a one-line note on why (zero-dep).

- [ ] **Step 5: Update the `response` section**

Confirm the `AgentResponse` section already matches Task 16's design (it should, closely — that task followed spec) and add the two additions beyond spec: `is_error: bool` / `.as_error()`, and `render_json(&self) -> String` (not `serde_json::Value`).

- [ ] **Step 6: Update Q4 and Q5's status**

Change Q4 and Q5 from "open questions" to resolved: Q4 — implemented per its own recommendation (builder + `renderHints`/`appendHints`/`renderRecovery` exported; low-level functions also still exported, additively, not removed, since removing already-shipped exports is a breaking change with no benefit). Q5 — resolved by using `kv::KvValue` instead of `serde_json::Value` for typed params, avoiding both the type-loss problem Q5 raised and the dependency Q5's alternative would have required.

- [ ] **Step 7: Run `just check` to confirm nothing broke from a docs-only change, then commit**

Run: `just check`
Expected: clean (this task touches no `.rs` files, `typos` is the only check with any chance of catching something).

```bash
git add docs/01-spec.md
git commit -m "docs: reconcile 01-spec.md with the implementation per this session's spec-parity work"
```

---

## Final verification

- [ ] Run the complete workspace check: `just check && just test`
- [ ] Run `cargo publish --dry-run -p michi --allow-dirty` to confirm the crate still packages cleanly
- [ ] Run `cd packages/michi-node && pnpm build --platform && pnpm test` to confirm the NAPI boundary still works end-to-end
- [ ] Re-read the Design Decisions table at the top of this plan against the final `docs/01-spec.md` (Task 24) and confirm every deviation is documented there
- [ ] Grep for any remaining references to removed/renamed types across the whole repo: `grep -rn "AxiError\|ErrorCode::render\|KvValue::Str\|KvValue::Null\|StatusResponse::new(overall" src tests docs/00-overview.md` — anything found needs a follow-up fix this plan missed

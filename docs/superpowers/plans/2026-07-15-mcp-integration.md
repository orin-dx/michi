# MCP Integration, DX Polish, and Doc/Hygiene Reconciliation — Implementation Plan

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the primitives → MCP `CallToolResult` gap with an always-compiled `mcp` module and a real-object NAPI export, ship the two DX fixes worth shipping (opt-in `serde` feature + `toon::list()`), and correct every stale/wrong documentation claim found during the doc audit — all per `docs/superpowers/specs/2026-07-09-mcp-integration-design.md`.

**Architecture:** Two independent workstreams that can run in either order or in parallel: Phase 1 is pure documentation correction (no code, no design ambiguity). Phases 2–4 are new code — a zero-dependency `src/mcp.rs` core, a NAPI wrapper that's the only place `serde_json` enters (scoped to the `napi` feature), and a separate opt-in `serde` feature for Rust-side ergonomics. Explicitly not in scope: NAPI setter chainability (deliberately left as-is per the design spec), and the git push to GitHub (needs its own separate go/no-go, not part of this plan).

**Tech Stack:** Rust (stable, workspace `rust-version = "1.96"`), `napi`/`napi-derive` v3 (`napi6` feature), `cargo nextest`, `insta` snapshots, `proptest`, new optional `serde`/`serde_json` dependencies (both gated behind features, never in the default build).

---

## Phase 1: Documentation & hygiene reconciliation

### Task 1: Rename the npm package from `michi` to `michin`

**Files:**
- Modify: `packages/michi-node/package.json`
- Modify: `README.md`
- Modify: `packages/michi-node/README.md`
- Modify: `docs/01-spec.md`
- Modify: `docs/superpowers/specs/2026-07-03-michi-design.md`

The npm registry already has an unrelated package published under the name `michi` (a URL
router, since 2014). `michin` is unclaimed. The Rust crate name (`michi`, root `Cargo.toml`)
is unaffected — crates.io has no such crate and this plan does not touch the Rust package
name.

- [ ] **Step 1: Confirm current state**

Run: `grep -rn '"michi"' packages/michi-node/package.json`
Expected: `"name": "michi",` — confirms this is the line to change.

- [ ] **Step 2: Rename in `package.json`**

In `packages/michi-node/package.json`, change:
```json
  "name": "michi",
```
to:
```json
  "name": "michin",
```

- [ ] **Step 3: Update `README.md`'s install/quick-start section**

In `README.md`, find:
```bash
pnpm add michi     # or npm install / yarn add
```
Change to:
```bash
pnpm add michin     # or npm install / yarn add
```

Find the TypeScript quick-start import:
```typescript
import { renderToon } from "michi";
```
Change to:
```typescript
import { renderToon } from "michin";
```

- [ ] **Step 4: Update `packages/michi-node/README.md`**

Change:
```bash
pnpm add michi
```
to:
```bash
pnpm add michin
```

Change:
```typescript
import { renderToon } from "michi";
```
to:
```typescript
import { renderToon } from "michin";
```

- [ ] **Step 5: Update `docs/01-spec.md`**

Find and update the two occurrences:
```
name = "michi"
```
(in the `packages/michi-node/Cargo.toml` snippet — this is the **Rust** package name for
the NAPI cdylib crate itself, which may legitimately stay `michi` or may need to change
too; check the surrounding context: if this line is under a `[package]` table for
`packages/michi-node/Cargo.toml`, it's the *Rust* crate name for the cdylib and does not
need to change — only the *npm* `package.json` `"name"` field changes. Read the surrounding
~15 lines before editing to confirm which one this is.)

```
  package.json                  # name: "michi"
```
Change to:
```
  package.json                  # name: "michin"
```
(this one is unambiguous — it's a comment about `package.json`, which is npm-only).

- [ ] **Step 6: Update the design doc**

In `docs/superpowers/specs/2026-07-03-michi-design.md`, find:
```
      package.json              ← name: "michi", published to npm
```
Change to:
```
      package.json              ← name: "michin"
```
(also drop "published to npm" — nothing has been published yet; see Task 2's note on `01-mvp.md` for the same "don't assert something happened that hasn't" pattern.)

- [ ] **Step 7: Verify no stale references remain**

Run: `grep -rln '"michi"' packages/michi-node/package.json README.md packages/michi-node/README.md docs/01-spec.md docs/superpowers/specs/2026-07-03-michi-design.md`
Expected: no matches for the npm-context occurrences (a match on the **Rust** crate name
`name = "michi"` inside a Cargo.toml snippet is fine and expected — only npm-context
mentions should be gone). Manually eyeball any remaining hits to confirm they're Rust-crate
references, not npm ones.

- [ ] **Step 8: Commit**

```bash
git add packages/michi-node/package.json README.md packages/michi-node/README.md docs/01-spec.md docs/superpowers/specs/2026-07-03-michi-design.md
git commit -m "docs: rename npm package michi -> michin (name collision on npm registry)"
```

---

### Task 2: Mark `docs/projects/01-mvp.md` superseded and fix its false scope claim

**Files:**
- Modify: `docs/projects/01-mvp.md`
- Modify: `docs/00-overview.md`

`01-mvp.md` currently states *"No features are deferred — v0.1.0 ships the complete public
API"* — false. It was superseded by `docs/superpowers/plans/2026-07-03-michi-core.md`, which
explicitly scopes Plan 2 (pipeline execution, fuzzy, cache, cli, mcp adapters) out from the
start and is the plan that was actually followed.

- [ ] **Step 1: Add a superseded notice to the top of `01-mvp.md`**

In `docs/projects/01-mvp.md`, immediately after the existing header block (after the `---`
following `> orin-dx · Draft · June 2026`), insert:

```markdown
> **Superseded.** This is an earlier, session-parallelized draft of the same v0.1.0 goal.
> `docs/superpowers/plans/2026-07-03-michi-core.md` is the plan that was actually executed —
> it has an explicit, correct scope note carving Plan 2 (pipeline execution, `fuzzy`,
> `cache`, `cli`, `mcp` adapters) out from the start. This document's "no features are
> deferred" claim below is **incorrect** and kept only for historical reference; see
> `michi-core.md`'s own "Not in this plan (Plan 2)" list for the accurate scope.
```

- [ ] **Step 2: Correct the false claim in place**

In `docs/projects/01-mvp.md`, find:
```
The MVP delivers the complete `michi` crate as specified in `docs/01-spec.md`:
all 11 modules, the `AgentResponse` builder, the NAPI npm wrapper, a full test
suite, and benchmarks. No features are deferred — v0.1.0 ships the complete
public API.
```
Change to:
```
The MVP delivers the pure-primitives `michi` crate as specified in `docs/01-spec.md`:
all 11 default-feature modules, the `AgentResponse` builder, the NAPI npm wrapper, a full
test suite, and benchmarks. Plan 2 (pipeline execution, `fuzzy`, `cache`, `cli`, `mcp`
adapters) is explicitly deferred — see the superseded notice above.
```

- [ ] **Step 3: Update `docs/00-overview.md`'s document index**

In `docs/00-overview.md`, find the "Documents in this repo" table row:
```
| `docs/projects/01-mvp.md` | MVP implementation plan — agent sessions, constraints |
```
Change to:
```
| `docs/projects/01-mvp.md` | Superseded early MVP draft — see `michi-core.md` instead   |
```

- [ ] **Step 4: Verify**

Run: `grep -n "No features are deferred\|Superseded" docs/projects/01-mvp.md`
Expected: the "No features are deferred" line is gone (replaced), and "Superseded" appears
in the new notice.

- [ ] **Step 5: Commit**

```bash
git add docs/projects/01-mvp.md docs/00-overview.md
git commit -m "docs: mark 01-mvp.md superseded, correct its false 'no features deferred' claim"
```

---

### Task 3: Update `packages/michi-node/README.md`'s API table

**Files:**
- Modify: `packages/michi-node/README.md`

The table currently lists only 4 of the functions the npm package actually exports. This
task adds the missing ones that exist **today** (before Phase 3 adds `toCallToolResult` —
that gets added to this same table in Task 10, not here).

- [ ] **Step 1: Confirm the current NAPI export surface**

Run: `grep -n "pub fn \|impl JsAgentResponse" /Users/gabe/Projects/michi/.worktrees/spec-parity/src/napi.rs`
Expected output includes (among others): `render_toon`, `empty_state`, `render_hints`,
`append_hints`, `render_recovery`, `truncate`, plus the `JsAgentResponse` class with methods
`items`, `totalCount`, `kvItems`, `hint`, `recoveryHint`, `asError`, `renderToon`, `renderKv`,
`renderJson`, `renderHintsOnly` (camelCase per napi-rs's JS naming convention).

- [ ] **Step 2: Replace the API table**

In `packages/michi-node/README.md`, find:
```markdown
## API

| Function | Signature |
|---|---|
| `renderToon` | `(opts: JsToonOptions) => string` — TOON list rendering |
| `emptyState` | `(typeName: string) => string` — definitive empty-state block |
| `renderHints` | `(hints: string[]) => string` — `help[N]:` block |
| `truncate` | `(content: string, maxChars: number, hint: string) => string` |

Full type definitions ship in `index.d.ts`. See the
[main repo](https://github.com/orin-dx/michi) for the complete primitive set, the TOON
grammar, and the Rust API this wraps.
```

Replace with:
```markdown
## API

### Free functions

| Function | Signature |
|---|---|
| `renderToon` | `(opts: JsToonOptions) => string` — TOON list rendering |
| `emptyState` | `(typeName: string) => string` — definitive empty-state block |
| `renderHints` | `(hints: string[]) => string` — `help[N]:` block |
| `appendHints` | `(body: string, hints: string[]) => string` — append `help[N]:` to an existing body |
| `renderRecovery` | `(hints: JsRecoveryHint[]) => string` — `recovery[N]:` block |
| `truncate` | `(content: string, maxChars: number, hint: string) => string` |

### `AgentResponse` class

The primary integration point — a builder that composes TOON/KV rendering, hints, and
recovery into one response. Setters return `void`, not `this` (see the main repo's
`docs/01-spec.md` for why chaining isn't supported across the NAPI boundary) — call methods
as sequential statements, not a chain:

| Method | Signature |
|---|---|
| `constructor` | `(typeName: string)` |
| `.items` | `(rows: JsToonValue[][], fields: string[]) => void` |
| `.totalCount` | `(n: number) => void` |
| `.kvItems` | `(items: JsKvItem[]) => void` |
| `.hint` | `(hint: string) => void` |
| `.recoveryHint` | `(tool: string, reason?: string) => void` |
| `.asError` | `() => void` |
| `.renderToon` | `() => string` — slot-specific, reads only `items`/`fields` |
| `.renderKv` | `() => string` — slot-specific, reads only `kvItems` |
| `.renderJson` | `() => string` — compact JSON: `{"body":...,"hints":[...],"recovery":[...],"isError":bool}` |
| `.renderHintsOnly` | `() => string` — just the `help[N]:` block |

Full type definitions ship in `index.d.ts`. See the
[main repo](https://github.com/orin-dx/michi) for the complete primitive set, the TOON
grammar, and the Rust API this wraps.
```

- [ ] **Step 3: Verify**

Run: `grep -c "^|" packages/michi-node/README.md`
Expected: more table rows than before (sanity check the replace didn't drop content).
Read the file back to confirm formatting is correct markdown.

- [ ] **Step 4: Commit**

```bash
git add packages/michi-node/README.md
git commit -m "docs: update npm package README API table to match actual NAPI export surface"
```

---

### Task 4: Fix dangling references to the never-written `michi-pipeline.md` plan doc

**Files:**
- Modify: `src/pipeline/executor.rs`
- Modify: `src/sink/mod.rs`

Both files point to `docs/superpowers/plans/2026-07-03-michi-pipeline.md`, which does not
exist. Per the design spec, fix the comments to point at content that does exist rather than
inventing a placeholder plan document.

- [ ] **Step 1: Confirm current content**

Run: `cat src/pipeline/executor.rs src/sink/mod.rs`
Expected:
```
// Plan 2: PipelineExecutor, PipelineContext — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
// Plan 2: OutputSink, AgentEvent, NoopSink — requires `pipeline` feature.
// See docs/superpowers/plans/2026-07-03-michi-pipeline.md
```

- [ ] **Step 2: Fix `src/pipeline/executor.rs`**

Replace the full file content with:
```rust
// Plan 2: PipelineExecutor, PipelineContext — requires `pipeline` feature.
// Not yet started. See docs/superpowers/specs/2026-07-03-michi-design.md for
// the architectural notes that exist today; a dedicated Plan 2 implementation
// plan has not been written yet.
```

- [ ] **Step 3: Fix `src/sink/mod.rs`**

Replace the full file content with:
```rust
// Plan 2: OutputSink, AgentEvent, NoopSink — requires `pipeline` feature.
// Not yet started. See docs/superpowers/specs/2026-07-03-michi-design.md for
// the architectural notes that exist today; a dedicated Plan 2 implementation
// plan has not been written yet.
```

- [ ] **Step 4: Verify the crate still builds (comment-only change, should be a no-op)**

Run: `cargo build -p michi --all-features`
Expected: clean, no errors (these files have no code, only comments — this just confirms
nothing else was accidentally touched).

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/executor.rs src/sink/mod.rs
git commit -m "docs: fix dangling references to a Plan 2 pipeline doc that was never written"
```

---

## Phase 2: `src/mcp.rs` — the `CallToolResult` mapping core

### Task 5: Create `src/mcp.rs` with `Audience`, `ContentBlock`, `CallToolResult`

**Files:**
- Create: `src/mcp.rs`
- Modify: `src/lib.rs`
- Test: `src/mcp.rs` (inline `#[cfg(test)]`)

Always-compiled, zero new dependencies — same hand-rolled-JSON-adjacent pattern as
`response.rs`. This task creates the types only; `AgentResponse::to_call_tool_result()`
(which constructs them) is Task 6.

- [ ] **Step 1: Write the failing test**

Create `src/mcp.rs`:
```rust
//! MCP `CallToolResult` mapping — the shape a tool call actually returns to
//! an MCP client. Always compiled: this is pure struct construction, no new
//! dependencies, so there's no reason to gate it behind a feature flag.
//!
//! michi does not know about the rest of the MCP protocol (no JSON-RPC, no
//! tool registration, no server bootstrapping — see `docs/01-spec.md`'s
//! Non-goals). This module owns exactly one thing: turning an already-built
//! [`crate::response::AgentResponse`] into the `content`/`isError`/
//! `structuredContent` shape MCP's `tools/call` response expects.

/// Which surface a [`ContentBlock`] is meant for. Mirrors MCP's
/// `annotations.audience`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// The compact, token-efficient surface — what michi renders today.
    Assistant,
    /// A human-readable surface, supplied by the caller. michi does not
    /// generate this text itself (see this crate's Non-goals: no
    /// display-format Markdown) — it only carries it correctly through to
    /// the protocol shape when a caller has one.
    User,
}

/// One text content block, tagged with its intended audience.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentBlock {
    /// The block's text content.
    pub text: String,
    /// Which surface this block is meant for.
    pub audience: Audience,
}

/// The MCP `CallToolResult` shape: what a tool call returns to a client.
/// Built via [`crate::response::AgentResponse::to_call_tool_result`], never
/// hand-constructed by a caller.
#[derive(Debug, Clone, PartialEq)]
pub struct CallToolResult {
    /// Text content blocks — the primary `assistant`-audience block first,
    /// then an optional `user`-audience block if the caller supplied one.
    pub content: Vec<ContentBlock>,
    /// Whether this is a tool execution error, per MCP's error-reporting
    /// model (`isError: true` in the result, not a JSON-RPC protocol error —
    /// see MCP's tools spec, "Tool Execution Errors").
    pub is_error: bool,
    /// The same data as `content[0]`, as a JSON string — MCP's
    /// `structuredContent` companion. Always populated: michi already builds
    /// this JSON for `AgentResponse::render(OutputFormat::Json)`, so
    /// including it costs nothing and gives a JSON-aware client a typed
    /// alternative to parsing the compact text.
    pub structured_content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_block_carries_text_and_audience() {
        let b = ContentBlock { text: "hello".to_string(), audience: Audience::Assistant };
        assert_eq!(b.text, "hello");
        assert_eq!(b.audience, Audience::Assistant);
    }

    #[test]
    fn call_tool_result_is_constructible() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: Audience::Assistant }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        assert_eq!(r.content.len(), 1);
        assert!(!r.is_error);
        assert_eq!(r.structured_content, "{}");
    }
}
```

Note: `Audience` needs `PartialEq` for the test above to compile (`assert_eq!` requires it) —
already included in the `#[derive(...)]` list.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --test-threads 1 mcp::tests` — actually these are unit
tests inside `src/mcp.rs`, so: `cargo nextest run -p michi mcp::tests`
Expected: FAIL — `src/mcp.rs` isn't wired into `src/lib.rs` yet, so the module doesn't exist
from the crate's perspective (compile error: file exists but isn't a registered module).

- [ ] **Step 3: Wire the module into `src/lib.rs`**

In `src/lib.rs`, add the module declaration alongside the other always-compiled modules
(alphabetical, so between `kv` and `pipeline`... check current order first — the existing
list is `empty, error, hints, idempotency, kv, napi(gated), pipeline, recovery, resilience,
response, sink, status, telemetry, toon, truncate`; insert `mcp` between `kv` and `napi`):

```rust
/// MCP `CallToolResult` mapping — turns an `AgentResponse` into the shape a
/// tool call returns to an MCP client.
pub mod mcp;
```

Add the crate-root re-export alongside the existing re-export block:
```rust
pub use mcp::{Audience, CallToolResult, ContentBlock};
```
(insert alphabetically in the existing `pub use` list — after `kv::render_kv` and before
`recovery::RecoveryHint`, matching the module's alphabetical position).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi mcp::tests`
Expected: 2 tests pass.

- [ ] **Step 5: Run full targeted suite and commit**

Run: `cargo nextest run -p michi --lib -E 'not test(pipeline::verify_finding)'` (excludes
the known-unrelated stray scratch tests in `src/pipeline/mod.rs` if that file still has
uncommitted local modifications in this worktree — check `git status src/pipeline/mod.rs`
first; if it's clean, just run `cargo nextest run -p michi --lib`).

Run: `cargo clippy -p michi --all-features -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/mcp.rs src/lib.rs
git commit -m "feat(mcp): add Audience/ContentBlock/CallToolResult types, always-compiled"
```

---

### Task 6: Add `AgentResponse::human_content()` and `to_call_tool_result()`

**Files:**
- Modify: `src/response.rs`
- Test: same file

**Files:**
- Modify: `src/response.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

First read the current `src/response.rs` in full to confirm the exact current `AgentResponse`
struct fields and builder method style (it should match what's shown in this plan — if it's
drifted, adapt field names to match reality, not this plan).

Add to `src/response.rs`'s `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn to_call_tool_result_uses_render_text_as_assistant_block() {
        let r = AgentResponse::new("issue").kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(1) }]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].text, r.render_kv());
        assert_eq!(result.content[0].audience, crate::mcp::Audience::Assistant);
        assert!(!result.is_error);
    }

    #[test]
    fn to_call_tool_result_reflects_is_error() {
        let r = AgentResponse::new("t").kv_items(vec![]).as_error();
        let result = r.to_call_tool_result();
        assert!(result.is_error);
    }

    #[test]
    fn to_call_tool_result_includes_human_content_block_when_set() {
        let r = AgentResponse::new("t").kv_items(vec![]).human_content("Here's a friendly summary.");
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].text, "Here's a friendly summary.");
        assert_eq!(result.content[1].audience, crate::mcp::Audience::User);
    }

    #[test]
    fn to_call_tool_result_omits_human_block_when_not_set() {
        let r = AgentResponse::new("t").kv_items(vec![]);
        let result = r.to_call_tool_result();
        assert_eq!(result.content.len(), 1);
    }

    #[test]
    fn to_call_tool_result_structured_content_is_valid_json_matching_render_json_body() {
        let r = AgentResponse::new("issue").kv_items(vec![KvItem { key: "id".into(), value: KvValue::Int(1) }]);
        let result = r.to_call_tool_result();
        // structured_content is the same JSON render() produces for OutputFormat::Json.
        assert_eq!(result.structured_content, r.render(OutputFormat::Json));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi response::tests::to_call_tool_result`
Expected: FAIL — `human_content` and `to_call_tool_result` don't exist yet on `AgentResponse`.

- [ ] **Step 3: Add the `human_content` field and builder method**

In `src/response.rs`, add a new field to the `AgentResponse` struct (alongside the existing
fields — read the current struct definition first and add this in a sensible position, e.g.
right after `is_error`):
```rust
    human_content: Option<String>,
```

Update the `new()` constructor to initialize it:
```rust
            human_content: None,
```
(add this line to the `Self { ... }` literal in `new()`, alongside the other field
initializers).

Add the builder method, placed near `as_error()`:
```rust
    /// Attach a human-facing companion block (`audience: user`) for MCP
    /// callers. Optional — most callers won't set this. michi does not
    /// generate this text itself; the caller supplies it.
    #[must_use]
    pub fn human_content(mut self, text: impl Into<String>) -> Self {
        self.human_content = Some(text.into());
        self
    }
```

- [ ] **Step 4: Add `to_call_tool_result()`**

Add to the `impl AgentResponse` block, near `render_hints_only()`:
```rust
    /// Build the MCP `CallToolResult` for this response: the rendered body
    /// as the primary `assistant`-audience content block, `human_content`
    /// (if set) as a second `user`-audience block, `is_error`, and
    /// `structured_content` as the JSON-rendered form of the same data.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        let mut content = vec![crate::mcp::ContentBlock { text: self.render_text(), audience: crate::mcp::Audience::Assistant }];
        if let Some(human) = &self.human_content {
            content.push(crate::mcp::ContentBlock { text: human.clone(), audience: crate::mcp::Audience::User });
        }
        crate::mcp::CallToolResult { content, is_error: self.is_error, structured_content: self.render(OutputFormat::Json) }
    }
```

Note: `render_text()` is the private method that backs `render(OutputFormat::Text)` — check
it's still named that in the current file (it should be, per the crate's existing
`render_toon`/`render_kv`/`render_text`/`render_json` split); use whatever the current
private method that produces the `target`-dispatched text body is actually called if it
differs.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p michi response::tests`
Expected: all pass, including the 5 new tests.

- [ ] **Step 6: Run full targeted suite and commit**

Run: `cargo nextest run -p michi --lib && cargo clippy -p michi --all-features -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/response.rs
git commit -m "feat(response): add human_content() and to_call_tool_result() per MCP integration design"
```

---

### Task 7: Retire the empty `mcp` Cargo feature

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`

The `mcp` feature (`mcp = ["pipeline"]`) has no code behind it — grep confirms zero
`#[cfg(feature = "mcp")]` items anywhere in the tree. It was reserved for Plan 2 ("MCP server
that runs pipelines"), which hasn't started. Since `src/mcp.rs` (Task 5) is always-compiled
and needs no feature gate, retire the flag now; the name is free to reuse when Plan 2 is
real.

- [ ] **Step 1: Confirm nothing depends on it**

Run: `grep -rn 'feature = "mcp"' src/ tests/ benches/`
Expected: no matches (confirms it's safe to remove).

- [ ] **Step 2: Remove from `Cargo.toml`**

In `Cargo.toml`, find:
```toml
[features]
default  = []
pipeline = ["dep:tokio", "dep:tokio-util", "dep:async-trait", "dep:uuid"]
fuzzy    = ["dep:nucleo-matcher", "pipeline"]
cache    = ["dep:moka", "dep:sha2", "pipeline"]
cli      = ["dep:indicatif", "dep:inquire", "dep:crossterm", "dep:ctrlc", "pipeline"]
mcp      = ["pipeline"]
napi     = ["dep:napi", "dep:napi-derive"]
full     = ["pipeline", "fuzzy", "cache", "cli", "mcp"]
```
Replace with (removes the `mcp` line; `full`'s composition drops `mcp` too — Tasks 9 and 11
will add `serde_json`/`serde` back into `napi`/a new `serde` feature and `full` respectively,
so don't add those here, just remove `mcp`):
```toml
[features]
default  = []
pipeline = ["dep:tokio", "dep:tokio-util", "dep:async-trait", "dep:uuid"]
fuzzy    = ["dep:nucleo-matcher", "pipeline"]
cache    = ["dep:moka", "dep:sha2", "pipeline"]
cli      = ["dep:indicatif", "dep:inquire", "dep:crossterm", "dep:ctrlc", "pipeline"]
napi     = ["dep:napi", "dep:napi-derive"]
full     = ["pipeline", "fuzzy", "cache", "cli"]
```

Also update the crate keywords list — find:
```toml
keywords    = ["axi", "agent", "mcp", "cli", "llm"]
```
Leave this as-is. `mcp` as a *keyword* (crates.io search term) is still accurate — the crate
now has real, always-compiled MCP integration via `src/mcp.rs`. Only the *Cargo feature flag*
was the misleading, empty one.

- [ ] **Step 3: Update the feature-flag doc table in `src/lib.rs`**

In `src/lib.rs`'s module doc comment, find:
```rust
//! | Feature | Adds |
//! |---|---|
//! | `pipeline` | `PipelineExecutor`, `CheckpointStore`, `OutputSink`, `CircuitBreaker` |
//! | `fuzzy` | `FuzzyMatcher`, `FuzzyResolver` |
//! | `cache` | Two-tier `Cache` (moka + disk) |
//! | `cli` | CLI surface adapters (indicatif, inquire) |
//! | `mcp` | MCP surface adapters |
//! | `napi` | NAPI exports (used by `packages/michi-node`) |
//! | `full` | All of the above except `napi` |
```
Replace with:
```rust
//! | Feature | Adds |
//! |---|---|
//! | `pipeline` | `PipelineExecutor`, `CheckpointStore`, `OutputSink`, `CircuitBreaker` (Plan 2, not yet implemented) |
//! | `fuzzy` | `FuzzyMatcher`, `FuzzyResolver` (Plan 2, not yet implemented) |
//! | `cache` | Two-tier `Cache` (moka + disk) (Plan 2, not yet implemented) |
//! | `cli` | CLI surface adapters (indicatif, inquire) (Plan 2, not yet implemented) |
//! | `napi` | NAPI exports (used by `packages/michi-node`) |
//! | `full` | All of the above except `napi` |
//!
//! MCP integration (`AgentResponse::to_call_tool_result()`) is always compiled
//! — no feature flag needed, see the `mcp` module.
```

(This also fixes a pre-existing doc-accuracy gap this task's audit surfaced: the table never
noted that `pipeline`/`fuzzy`/`cache`/`cli` are Plan 2 stubs with no code behind them yet —
worth being explicit here too, since a reader of this exact table is the most likely person
to be surprised by it, same as this whole plan's trigger.)

- [ ] **Step 4: Update the project's `CLAUDE.md`**

In `CLAUDE.md`, find:
```
- Feature flags: `pipeline` (tokio), `fuzzy`, `cache`, `cli`, `mcp`, `napi`
```
Change to:
```
- Feature flags: `pipeline` (tokio), `fuzzy`, `cache`, `cli`, `napi`, `serde` (opt-in Serialize/Deserialize + `toon::list()`)
```
(the `serde` feature doesn't exist yet at this point in the plan — it's added in Task 10 —
but this line only needs to be correct by the time the whole plan is done, and touching it
once here rather than twice avoids a needless second edit; if executing tasks out of order,
defer this exact line to after Task 10 lands instead.)

- [ ] **Step 5: Verify the crate still builds without the removed feature**

Run: `cargo check -p michi --all-features`
Expected: clean (the `mcp` feature name no longer exists, so nothing can reference it —
confirmed by Step 1's grep already).

Run: `cargo check -p michi --features full`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/lib.rs CLAUDE.md
git commit -m "feat(cargo): retire the empty mcp feature flag; MCP integration is always-compiled"
```

---

## Phase 3: NAPI `toCallToolResult()`

### Task 8: Add `serde_json` scoped to the `napi` feature only

**Files:**
- Modify: `Cargo.toml`

This is the one place `serde_json` enters the crate, and only inside `napi` — the default
build stays exactly as zero-dep as it is today.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, in the `[dependencies]` section, find the `# napi feature` block:
```toml
# napi feature
napi        = { version = "3", features = ["napi6"], optional = true }
napi-derive = { version = "3", optional = true }
```
Change to:
```toml
# napi feature
napi        = { version = "3", features = ["napi6", "serde-json"], optional = true }
napi-derive = { version = "3", optional = true }
serde_json  = { version = "1", optional = true }
```

Update the `napi` feature definition to require the new optional dep:
```toml
napi     = ["dep:napi", "dep:napi-derive", "dep:serde_json"]
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p michi --features napi`
Expected: clean. This confirms `napi`'s `serde-json` cargo feature (which enables
`impl FromNapiValue`/`ToNapiValue for serde_json::Value`) resolves correctly against napi
v3/napi6. **If this fails** (e.g. the feature name differs in the resolved napi-rs version),
run `cargo doc -p napi --open` or check the installed napi crate's `Cargo.toml` for its
actual feature list (`find ~/.cargo/registry -path '*napi-3*/Cargo.toml' -exec grep -A5 '\[features\]' {} \;`)
and adjust the feature name in this task to match reality before proceeding — do not
silently skip this verification.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "feat(cargo): add serde_json scoped to the napi feature, for typed structuredContent"
```

---

### Task 9: Add `JsAgentResponse::toCallToolResult()`

**Files:**
- Modify: `src/napi.rs`
- Test: same file (Rust side) + `packages/michi-node/__test__/index.test.mjs` (JS side)

- [ ] **Step 1: Write the failing test (Rust side)**

Read the current `src/napi.rs` in full first (it's been touched by earlier work — confirm
`JsAgentResponse`'s exact current shape matches what's shown below before editing).

Add to `src/napi.rs`'s test module:
```rust
    #[test]
    fn js_agent_response_to_call_tool_result_basic() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("int") }]).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].audience, "assistant");
        assert!(!result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_reflects_is_error() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.as_error().unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_structured_content_is_parsed_json() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        let result = r.to_call_tool_result().unwrap();
        // structured_content is a real serde_json::Value, not a string — confirm it's
        // an object with the expected top-level key, not a re-stringified blob.
        assert!(result.structured_content.get("isError").is_some(), "got: {:?}", result.structured_content);
    }
```

(`value("int")` reuses the existing `value(t: &str) -> JsToonValue` test helper already in
this file's test module.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p michi --features napi js_agent_response_to_call_tool_result_basic`
Expected: FAIL — `to_call_tool_result` doesn't exist on `JsAgentResponse` yet.

- [ ] **Step 3: Write the implementation**

Add to `src/napi.rs`, after the existing `JsAgentResponse` impl block's last method
(`render_hints_only`) but still inside `impl JsAgentResponse { ... }` — insert before the
closing `}` of the impl block:

```rust
    /// Build the MCP `CallToolResult` for this response. Returns a real
    /// object, not a JSON string — `structuredContent` is a parsed
    /// `serde_json::Value`, so TypeScript callers don't need to
    /// `JSON.parse()` it themselves.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn to_call_tool_result(&self) -> napi::Result<JsCallToolResult> {
        let inner = self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))?;
        let result = inner.to_call_tool_result();
        let structured_content = serde_json::from_str(&result.structured_content)
            .map_err(|e| napi::Error::from_reason(format!("structured_content was not valid JSON: {e}")))?;
        Ok(JsCallToolResult {
            content: result
                .content
                .into_iter()
                .map(|c| JsContentBlock {
                    text: c.text,
                    audience: match c.audience {
                        crate::mcp::Audience::Assistant => "assistant".to_string(),
                        crate::mcp::Audience::User => "user".to_string(),
                    },
                })
                .collect(),
            is_error: result.is_error,
            structured_content,
        })
    }
```

Add the two new `#[napi(object)]` structs **before** the `JsAgentResponse` struct
definition (so they're defined before first use — Rust doesn't strictly require this
ordering, but it matches this file's existing top-to-bottom convention of defining object
types before the class that uses them):

```rust
/// One MCP content block (JavaScript-friendly).
#[napi(object)]
pub struct JsContentBlock {
    /// The block's text content.
    pub text: String,
    /// `"assistant"` or `"user"` — which surface this block is meant for.
    pub audience: String,
}

/// The MCP `CallToolResult` shape, returned by [`JsAgentResponse::to_call_tool_result`].
#[napi(object)]
pub struct JsCallToolResult {
    /// Text content blocks.
    pub content: Vec<JsContentBlock>,
    /// Whether this is a tool execution error.
    #[napi(js_name = "isError")]
    pub is_error: bool,
    /// The same data as `content[0]`, as a real parsed JSON value — not a
    /// string the caller has to `JSON.parse()` themselves.
    #[napi(js_name = "structuredContent")]
    pub structured_content: serde_json::Value,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p michi --features napi napi::tests`
Expected: all pass, including the 3 new tests.

- [ ] **Step 5: Add the JS-side integration test**

Add to `packages/michi-node/__test__/index.test.mjs` (check the file's current top-of-file
import style first and match it):
```javascript
void describe('toCallToolResult', () => {
  void it('returns a real object with content/isError/structuredContent', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 1 } }])
    const result = r.toCallToolResult()
    assert.strictEqual(result.content.length, 1)
    assert.strictEqual(result.content[0].audience, 'assistant')
    assert.strictEqual(result.isError, false)
    // structuredContent must be a real object already, not a JSON string.
    assert.strictEqual(typeof result.structuredContent, 'object')
    assert.strictEqual(result.structuredContent.isError, false)
  })

  void it('includes a second user-audience block when humanContent-equivalent data is present', () => {
    // Rust-side human_content() has no NAPI setter in this plan (NAPI surface stays
    // minimal per the design spec) — this test only exercises the single-block path.
    const r = new AgentResponse('t')
    r.kvItems([])
    r.asError()
    const result = r.toCallToolResult()
    assert.strictEqual(result.isError, true)
    assert.strictEqual(result.structuredContent.isError, true)
  })
})
```

Note: this plan does **not** add a NAPI setter for `human_content()` — the design spec
doesn't call for one, and the JS test above reflects that (only exercises what's actually
exposed). If a NAPI `humanContent()` setter turns out to be wanted later, that's a follow-up,
not part of this plan.

- [ ] **Step 6: Rebuild the NAPI binary and run the JS suite**

Run: `cd packages/michi-node && pnpm build --platform && pnpm test`
Expected: all pass, including the 2 new tests, against the actual compiled binary.

- [ ] **Step 7: Run full verification and commit**

Run: `cargo clippy -p michi --features napi -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add src/napi.rs packages/michi-node/__test__/index.test.mjs packages/michi-node/index.d.ts packages/michi-node/index.js
git commit -m "feat(napi): add JsAgentResponse.toCallToolResult() returning a real parsed object"
```

(If `index.d.ts`/`index.js` are gitignored in this repo — check `git status` after the
`pnpm build` step — only `git add` the files that are actually tracked; don't force-add
gitignored generated artifacts.)

---

## Phase 4: Optional `serde` feature + `toon::list()`

### Task 10: Add the `serde` feature and derive `Serialize`/`Deserialize` on core types

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/toon/render.rs` (the `Value` enum)
- Modify: `src/kv/mod.rs` (the `KvValue` enum)
- Modify: `src/hints.rs` (the `Hint` type)
- Modify: `src/recovery.rs` (the `RecoveryHint` type)
- Modify: `src/mcp.rs` (`Audience`, `ContentBlock`, `CallToolResult`)
- Test: each file's existing test module

- [ ] **Step 1: Add the feature and dependencies**

In `Cargo.toml`, add a new dependency block after the `# napi feature` block:
```toml
# serde feature
serde      = { version = "1", features = ["derive"], optional = true }
```

Note: `serde_json` is already an optional dependency (added in Task 8, scoped to `napi`).
Cargo allows the same optional dependency to be required by multiple features without
duplication — add it to `serde`'s feature requirement too:

In `[features]`, add:
```toml
serde    = ["dep:serde", "dep:serde_json"]
```
And update `full` to include it (matches `full`'s existing role as "everything testable"):
```toml
full     = ["pipeline", "fuzzy", "cache", "cli", "serde"]
```

Also, since `toon::list()` (Task 11) needs field-order preservation when serializing structs
to build TOON rows, and `serde_json::Map` defaults to alphabetical (BTreeMap-backed) key
order without this, enable the ordering-preserving feature on `serde_json` itself:
```toml
serde_json = { version = "1", optional = true, features = ["preserve_order"] }
```
(this replaces the plain `serde_json = { version = "1", optional = true }` line added in
Task 8 — edit that same line, don't add a duplicate).

- [ ] **Step 2: Verify the new feature compiles standalone**

Run: `cargo check -p michi --features serde`
Expected: clean (nothing uses `serde`/`serde_json` yet, so this just confirms the dependency
resolves).

- [ ] **Step 3: Write the failing tests**

Add to `src/toon/render.rs`'s existing test module (or a new one if none exists at this
exact location — check first):
```rust
    #[test]
    #[cfg(feature = "serde")]
    fn value_serializes_and_deserializes() {
        let v = Value::Int(42);
        let json = serde_json::to_string(&v).expect("serializes");
        let back: Value = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(v, back);
    }
```

Add equivalent round-trip tests to `src/kv/mod.rs`, `src/hints.rs`, `src/recovery.rs`,
`src/mcp.rs` test modules, each gated `#[cfg(feature = "serde")]`, each constructing one
representative value of the type and round-tripping it through `serde_json::to_string` /
`serde_json::from_str`. Example for `src/kv/mod.rs`:
```rust
    #[test]
    #[cfg(feature = "serde")]
    fn kv_value_serializes_and_deserializes() {
        let v = KvValue::Text("hello".to_string());
        let json = serde_json::to_string(&v).expect("serializes");
        let back: KvValue = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(v, back);
    }
```
Add to `src/hints.rs`'s existing test module:
```rust
    #[test]
    #[cfg(feature = "serde")]
    fn hint_serializes_and_deserializes() {
        let h = Hint::new("call list_issues next");
        let json = serde_json::to_string(&h).expect("serializes");
        let back: Hint = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(h, back);
    }
```

Add to `src/recovery.rs`'s existing test module:
```rust
    #[test]
    #[cfg(feature = "serde")]
    fn recovery_hint_serializes_and_deserializes() {
        let h = RecoveryHint::new("assign_user")
            .param("user", KvValue::Text("alice".to_string()))
            .reason("user 'ghost' not found");
        let json = serde_json::to_string(&h).expect("serializes");
        let back: RecoveryHint = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(h, back);
    }
```

Add to `src/mcp.rs`'s existing test module:
```rust
    #[test]
    #[cfg(feature = "serde")]
    fn audience_serializes_and_deserializes() {
        let a = Audience::Assistant;
        let json = serde_json::to_string(&a).expect("serializes");
        let back: Audience = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(a, back);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn call_tool_result_serializes_and_deserializes() {
        let r = CallToolResult {
            content: vec![ContentBlock { text: "body".to_string(), audience: Audience::Assistant }],
            is_error: false,
            structured_content: "{}".to_string(),
        };
        let json = serde_json::to_string(&r).expect("serializes");
        let back: CallToolResult = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(r, back);
    }
```

Note: `Audience` needs `PartialEq` for these `assert_eq!` calls — already present from Task 5's
derive list, so no change needed there.

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo nextest run -p michi --features serde serializes_and_deserializes`
Expected: FAIL — compile errors, `Serialize`/`Deserialize` not implemented for these types
yet.

- [ ] **Step 5: Add the derives**

In `src/toon/render.rs`, find the `Value` enum's derive line and add a `cfg_attr`:
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
```
Change to:
```rust
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Value {
```

Apply the identical pattern (add `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`
immediately above the existing `#[derive(...)]` line) to:
- `KvValue` in `src/kv/mod.rs`
- `Hint` in `src/hints.rs`
- `RecoveryHint` in `src/recovery.rs`
- `Audience`, `ContentBlock`, `CallToolResult` in `src/mcp.rs`

Read each type's current derive line first — some may have `Copy`/`Eq` in addition to
`Debug, Clone, PartialEq`; keep whatever's already there and only *add* the `cfg_attr` line,
don't remove existing derives.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features serde`
Expected: all pass, including every new round-trip test.

- [ ] **Step 7: Confirm the default build is unaffected**

Run: `cargo build -p michi` (no features — the default build)
Expected: clean. This is the check that matters most: confirms `serde`/`serde_json` are
genuinely not pulled in unless the feature is explicitly requested.

Run: `cargo tree -p michi --no-default-features | grep -i serde`
Expected: no output (confirms zero serde in the dependency tree by default).

- [ ] **Step 8: Run full verification and commit**

Run: `cargo clippy -p michi --features serde -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

```bash
git add Cargo.toml src/toon/render.rs src/kv/mod.rs src/hints.rs src/recovery.rs src/mcp.rs
git commit -m "feat(serde): add opt-in serde feature, derive Serialize/Deserialize on core value types"
```

---

### Task 11: Add `toon::list()` convenience API

**Files:**
- Modify: `src/toon/mod.rs`
- Test: `tests/toon_integration.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/toon_integration.rs`:
```rust
#[test]
#[cfg(feature = "serde")]
fn list_builds_toon_options_from_serializable_struct_slice() {
    #[derive(serde::Serialize)]
    struct Issue {
        number: u64,
        title: String,
        state: String,
    }

    let issues =
        vec![Issue { number: 51815, title: "[Bug]: Telegram plugin".to_string(), state: "open".to_string() }];
    let opts = michi::toon::list("issues", &issues);
    let out = michi::toon::render_toon(&opts);
    assert!(out.starts_with("issues[1]{number,title,state}:\n"), "got: {out}");
    assert!(out.contains("51815,[Bug]: Telegram plugin,open"), "got: {out}");
}

#[test]
#[cfg(feature = "serde")]
fn list_handles_empty_slice() {
    #[derive(serde::Serialize)]
    struct Empty {
        x: i32,
    }
    let items: Vec<Empty> = vec![];
    let opts = michi::toon::list("nothing", &items);
    assert_eq!(opts.fields.len(), 0);
    assert_eq!(opts.rows.len(), 0);
}

#[test]
#[cfg(feature = "serde")]
fn list_stringifies_nested_values_losslessly() {
    #[derive(serde::Serialize)]
    struct WithNested {
        id: u64,
        tags: Vec<String>,
    }
    let items = vec![WithNested { id: 1, tags: vec!["a".to_string(), "b".to_string()] }];
    let opts = michi::toon::list("t", &items);
    // tags is a nested array — falls back to a compact JSON string, not an error.
    assert_eq!(opts.rows[0][1], michi::toon::Value::Str(r#"["a","b"]"#.to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p michi --features serde --test toon_integration list_`
Expected: FAIL — `michi::toon::list` doesn't exist yet.

- [ ] **Step 3: Write the implementation**

In `src/toon/mod.rs`, add (near the bottom of the file, after `render_toon` and its existing
helpers — read the current file structure first to place this sensibly):

```rust
/// Convert one field's JSON value into a TOON [`Value`]. Scalars map
/// directly; nested objects/arrays fall back to a compact JSON string —
/// lossless, never a hard error.
#[cfg(feature = "serde")]
fn json_value_to_toon_value(v: Option<&serde_json::Value>) -> Value {
    match v {
        None | Some(serde_json::Value::Null) => Value::Null,
        Some(serde_json::Value::Bool(b)) => Value::Bool(*b),
        Some(serde_json::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Str(n.to_string())
            }
        }
        Some(serde_json::Value::String(s)) => Value::Str(s.clone()),
        Some(other) => Value::Str(other.to_string()),
    }
}

/// Build [`ToonOptions`] from a slice of `Serialize`-able items sharing the
/// same shape. Field order follows the first item's serialized key order;
/// scalar values (string/number/bool/null) map directly to [`Value`],
/// anything else (a nested object or array) is serialized to a compact JSON
/// string and carried as `Value::Str` — lossless, never a hard error. Items
/// that don't serialize to a JSON object (e.g. a bare number or string slice)
/// produce an empty row rather than panicking.
///
/// Requires the `serde` feature.
///
/// ```rust
/// # #[cfg(feature = "serde")] {
/// use michi::toon;
///
/// #[derive(serde::Serialize)]
/// struct Issue { number: u64, title: String, state: String }
///
/// let issues = vec![Issue { number: 51815, title: "Bug".to_string(), state: "open".to_string() }];
/// let opts = toon::list("issues", &issues);
/// let out = toon::render_toon(&opts);
/// assert!(out.starts_with("issues[1]{number,title,state}:\n"));
/// # }
/// ```
#[cfg(feature = "serde")]
#[must_use]
pub fn list<T: serde::Serialize>(type_name: impl Into<String>, items: &[T]) -> ToonOptions {
    let mut fields: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(items.len());
    for item in items {
        let obj = match serde_json::to_value(item) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        if fields.is_empty() && !obj.is_empty() {
            fields = obj.keys().cloned().collect();
        }
        rows.push(fields.iter().map(|f| json_value_to_toon_value(obj.get(f))).collect());
    }
    ToonOptions { type_name: type_name.into(), fields, rows, total_count: None, hints: Vec::new(), max_cell_len: 200 }
}
```

Check `ToonOptions`'s exact current field list before writing the final struct literal above
(it should be `type_name, fields, rows, total_count, hints, max_cell_len` per the crate's
current shape — adjust if it's drifted).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p michi --features serde --test toon_integration list_`
Expected: all 3 pass.

- [ ] **Step 5: Add the crate-root re-export**

In `src/lib.rs`, `toon::list` is **not** added to the unconditional crate-root re-export
list (`pub use toon::{render_toon, ToonOptions, Value};`) — it's feature-gated and the
existing re-export block has no precedent for conditional re-exports. Leave it reachable via
`michi::toon::list` only; don't add a `#[cfg(feature = "serde")] pub use toon::list;` unless
you confirm the crate already has a pattern for conditional crate-root re-exports elsewhere
(it doesn't, as of this plan — `napi` is a whole gated *module*, not an individual gated
re-export). Skip this step; `michi::toon::list(...)` is the intended call site.

- [ ] **Step 6: Update the quick-start examples to stop being aspirational**

In `README.md` and `docs/00-overview.md`, the existing quick-start examples already show
`#[derive(serde::Serialize)] struct Issue` feeding what looks like a `toon::list(...)` call
— check both files' current quick-start code blocks. If either shows a call shape that
doesn't match this task's actual `list(type_name, items)` signature, correct the example to
match reality (exact args, exact output). If they already match, leave them as-is and just
confirm by running the example logic mentally against the real function signature.

- [ ] **Step 7: Run full verification and commit**

Run: `cargo nextest run -p michi --features serde && cargo clippy -p michi --features serde -- -D warnings && cargo fmt -p michi -- --check`
Expected: clean.

Run: `cargo build -p michi` (default, no features)
Expected: clean — confirms `toon::list` being `#[cfg(feature = "serde")]`-gated means it
doesn't exist at all in the default build, so nothing about the default build's zero-dep
guarantee changed.

```bash
git add src/toon/mod.rs tests/toon_integration.rs README.md docs/00-overview.md
git commit -m "feat(toon): add list() convenience API for Serialize-able slices, behind the serde feature"
```

---

## Final verification

- [ ] Run the complete workspace check across all feature combinations:
  ```bash
  cargo build -p michi
  cargo build -p michi --features serde
  cargo build -p michi --features napi
  cargo build -p michi --all-features
  cargo nextest run -p michi --all-features
  cargo clippy -p michi --all-features -- -D warnings
  cargo fmt -p michi -- --check
  ```
  Expected: all clean. (The pre-existing, unrelated `pipeline::verify_finding::*` test
  failures from stray uncommitted scratch code, if that file is still dirty in this
  worktree, are not this plan's concern — confirm via `git status src/pipeline/mod.rs`
  whether that's still present, and if so, note it in the final report rather than either
  fixing it or being surprised by it.)
- [ ] Run `cd packages/michi-node && pnpm build --platform && pnpm test` — confirm the NAPI
  boundary works end-to-end including `toCallToolResult()`.
- [ ] Run `cargo publish --dry-run -p michi --allow-dirty` — confirm the crate still
  packages cleanly with the new module and features.
- [ ] Grep for any remaining stale `"michi"` npm-context references missed by Task 1:
  `grep -rn 'npm.*"michi"\|"michi".*npm\|add michi\b' README.md packages/michi-node/README.md docs/`
  (expect zero hits outside Rust-crate-name contexts).
- [ ] Confirm the `mcp` Cargo feature is genuinely gone: `grep -n '^mcp' Cargo.toml` (expect
  no output).
- [ ] Re-read `docs/superpowers/specs/2026-07-09-mcp-integration-design.md` against the
  final state of the code and confirm every numbered item in Parts 1, 2, and 4 has a
  corresponding completed task above, and Part 3's "explicitly not changing" item (NAPI
  chainability) was genuinely left untouched.

# michi — MCP integration, DX polish, and doc/hygiene reconciliation

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> orin-dx · Design · 09 Jul 2026

**Context:** Follow-up to a re-evaluation of michi against axi.md and the 2026-07-28 MCP
release candidate (see conversation; no separate artifact for the eval itself). The eval
found one real gap (michi renders correct text but doesn't help a caller assemble an MCP
`CallToolResult`), a few DX rough edges the crate's own docs already half-acknowledge, and,
separately, a full documentation-vs-source audit found stale/incorrect claims across
several docs plus a real npm publishing blocker. This spec covers all of it.

---

## Goal

Close the primitives → MCP `CallToolResult` gap without moving michi's protocol-knowledge
boundary, fix the two DX edges worth fixing, and correct every documentation claim found to
be stale or wrong — without reopening decisions that were already made deliberately and
correctly (NAPI setter chainability, "no display-format Markdown", zero-dep default build).

---

## Part 1 — `src/mcp.rs`: the `CallToolResult` mapping

### Why always-compiled, no feature gate

The mapping is pure struct construction plus the JSON-string building `response.rs` already
does — zero new dependencies. Gating it behind the (currently empty, zero-dependency-adding)
`mcp` Cargo feature would be free in theory but confusing in practice: a consumer would have
to discover and opt into a feature flag to get a function that costs nothing and needs
nothing. It ships in the default build, next to `toon`/`kv`/`response`.

The existing `mcp = ["pipeline"]` feature entry is retired. It was wired for a capability
("MCP server that runs pipelines," Plan 2) that doesn't exist yet and was never a dependency
of anything real — confirmed by grepping the tree for any `#[cfg(feature = "mcp")]` code
(there is none) and by `docs/superpowers/plans/2026-07-03-michi-core.md`'s own scope note,
which named this a Plan 2 stub from the start. crates.io has never had a version of this
crate published, so removing an unused feature name is not a breaking change to anyone. The
name can come back verbatim when Plan 2 actually needs it.

### Shape

```rust
/// Which surface a content block is meant for. Mirrors MCP's `annotations.audience`.
pub enum Audience {
    /// The compact, token-efficient surface — what michi renders today.
    Assistant,
    /// A human-readable surface, supplied by the caller. michi does not
    /// generate this text (see Non-goals) — it only carries it correctly.
    User,
}

/// One text content block, tagged with its intended audience.
pub struct ContentBlock {
    pub text: String,
    pub audience: Audience,
}

/// The MCP `CallToolResult` shape: what a tool call actually returns to a
/// client. Constructed from an `AgentResponse`, never hand-built by a caller.
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    pub is_error: bool,
    /// Same data as `content[0]`, as a JSON string — MCP's `structuredContent`
    /// companion. Always populated: it costs nothing (michi already builds
    /// this JSON for `render(OutputFormat::Json)`), and give a JSON-aware
    /// client a typed alternative to parsing the compact text.
    pub structured_content: String,
}
```

`AgentResponse` gains:

```rust
/// Attach a human-facing companion block (`audience: user`). Optional — most
/// callers won't set this. michi does not generate this text; the caller
/// supplies it (see Non-goals: no display-format Markdown).
pub fn human_content(mut self, text: impl Into<String>) -> Self

/// Build the MCP `CallToolResult` for this response: the rendered body as
/// the primary `assistant`-audience block, `human_content` (if set) as a
/// second `user`-audience block, `is_error`, and `structured_content` as the
/// JSON-rendered form of the same data.
pub fn to_call_tool_result(&self) -> mcp::CallToolResult
```

This resolves the audience question from the design conversation as **plumbing, not a new
renderer**: michi correctly carries a human-facing block through to the protocol shape when
a caller has one, but still never generates human-pretty text itself.

### NAPI: `toCallToolResult()`

`JsAgentResponse` gains a method returning a real `#[napi(object)]` struct, not a string.
This is the one place `serde_json` enters the picture, and only inside the `napi` feature
(already the crate's one `unsafe`-permitted, dependency-heavier boundary) — the default
build stays exactly as zero-dep as it is today. This is what actually closes DX edge #3
(`render_json()` requiring a manual `JSON.parse()`): the TypeScript caller gets
`structuredContent` as a real parsed object, not text they parse themselves.

### Non-goals (unchanged from `docs/01-spec.md`, restated for this addition)

- Still no image/audio/resource content blocks — michi renders text, `ContentBlock` is
  text-only. Multi-modal content is a caller concern if they ever need it.
- Still no human-pretty rendering. `human_content()` is a pass-through slot, not a formatter.
- Still no MCP server bootstrapping, tool registration, or JSON-RPC handling.

### Testing

- Unit tests: `to_call_tool_result()` on TOON path, KV path, error state, with and without
  `human_content()`.
- Snapshot test: exact `CallToolResult` shape for one representative response.
- Property test: for arbitrary `AgentResponse` construction, `structured_content` always
  parses as valid JSON and `content[0].text` always equals the existing `render_toon()` /
  `render_kv()` output (no drift between the two paths).
- NAPI/JS integration test: `toCallToolResult()` returns a real object with `content`,
  `isError`, `structuredContent` as a parsed value, exercised against the compiled binary.

---

## Part 2 — optional `serde` feature

New Cargo feature, off by default, no interaction with `default = []`:

- Derives `Serialize`/`Deserialize` on michi's public data types (`Value`, `KvValue`,
  `Hint`, `RecoveryHint`, the `mcp` types from Part 1, etc.) for Rust callers already using
  serde-based MCP SDKs or wanting to persist/transmit these types directly.
- Powers a new ergonomic entry point: `toon::list<T: Serialize>(type_name: &str, items: &[T]) -> ToonOptions`
  (exact name/shape confirmed at plan time) that builds `ToonOptions` from any
  `Serialize`-able slice instead of hand-written `Vec<Vec<Value>>`. This is the API
  `docs/00-overview.md`'s own quick-start already illustrates and calls "may land later" —
  it lands here, gated behind `serde` rather than unconditionally, so the zero-dep default
  build is untouched.

### Non-goals

- Not a replacement for the explicit `ToonOptions`/`Value` construction — that stays the
  zero-dep default path. `toon::list()` is strictly additive sugar.
- Not used anywhere in the default build's own rendering logic — `serde` stays a pure
  opt-in convenience, never a hidden dependency of core rendering.

### Testing

- Unit tests behind `#[cfg(feature = "serde")]`: derive round-trips, `toon::list()` against
  a representative `#[derive(Serialize)]` struct, matching the crate's quick-start example
  exactly (so the docs stop being aspirational).

---

## Part 3 — explicitly not changing

- **NAPI setter chainability** stays as-is. `JsAgentResponse`'s Option+take() pattern and
  void-returning setters were a deliberate, already-reviewed, already-correctly-documented
  choice (see `docs/01-spec.md`'s "builder boundary problem" section). Revisiting it would
  mean redesigning how state crosses the NAPI boundary for a cosmetic ergonomics gain, not
  fixing a defect. Out of scope here.

---

## Part 4 — documentation & hygiene reconciliation

Corrective, not design work — listed here for completeness and single-PR traceability, not
because any of it needs the design-approval gate above.

1. **npm package rename, `michi` → `michin`**, everywhere the old name appears:
   `packages/michi-node/package.json`'s `name` field, `README.md`, `packages/michi-node/README.md`,
   `docs/01-spec.md`, `docs/superpowers/specs/2026-07-03-michi-design.md`. The Rust crate
   name (`michi`, in the root `Cargo.toml`) is unaffected — it's available on crates.io and
   is a separate namespace from npm.
2. **`docs/projects/01-mvp.md`** — mark it superseded by
   `docs/superpowers/plans/2026-07-03-michi-core.md` at the top of the file, and correct or
   remove the false "no features are deferred" claim rather than leave it standing.
   `docs/00-overview.md`'s document index gets a note pointing to the superseding plan.
3. **`packages/michi-node/README.md`**'s API table — regenerate/update to include
   `AgentResponse` (and its methods), `appendHints`, `renderRecovery`, and (once Part 1
   lands) `toCallToolResult`, alongside the existing four functions.
4. **`docs/superpowers/specs/2026-07-03-michi-design.md`** — fix the file-tree comment
   asserting `package.json ← name: "michi", published to npm` (wrong name, and "published"
   was never true — nothing has been published to any registry yet).
5. **Dangling reference cleanup** — `src/pipeline/executor.rs` and `src/sink/mod.rs` both
   point to `docs/superpowers/plans/2026-07-03-michi-pipeline.md`, which does not exist.
   Either write a minimal stub of that plan doc (scope: Plan 2, not started) or change the
   comments to point at the design doc's existing Plan 2 notes instead. Prefer the latter —
   don't invent a plan document with no real content just to satisfy a comment.

### Explicitly excluded from this pass

**Pushing local `main` / the `v0.1.0` tag to GitHub** is not part of this spec. It's a
shared-remote action outside the scope of "fix what's wrong locally," and needs its own
explicit go/no-go when it comes up — not bundled into a docs cleanup.

---

## Rollout

Two independent workstreams, since Part 4 has zero dependency on Parts 1–2 and can run
fully in parallel:

- **Workstream A (Part 4):** mechanical, low-risk, no design ambiguity left to resolve.
- **Workstream B (Parts 1–2):** new code, TDD, full implement → spec-review →
  code-quality-review cycle per unit, matching the discipline used for the spec-parity plan.

Both run inside the existing `spec-parity` git worktree (still the active branch — this
spec's work lands as additional commits there, not a new worktree) unless the prior
worktree's work has since been merged/finished, in which case a fresh worktree is created
per `superpowers:using-git-worktrees`.

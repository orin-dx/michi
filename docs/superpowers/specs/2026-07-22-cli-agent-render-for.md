# `render_for(Audience)` — Dual CLI/Agent Output Design

> orin-dx · michi · 2026-07-22

## Problem

michi already lets an MCP caller attach a human-facing companion block via
`AgentResponse::human_content()`, but that block is only ever reachable through
`to_call_tool_result()` — the MCP-specific path. A CLI consumer that wants the same
dual-audience behavior (agent-optimized TOON/KV for a piped/scripted invocation, something
more human-appropriate for an interactive terminal) has no equivalent. Today it would have to
build its own ad hoc rendering-mode switch from scratch — exactly what monokl's `output.rs` does
today for a much narrower case (pretty vs. compact JSON, not genuinely human-vs-agent).

## Design

**Split the decision from the signal**, the same pattern already used for truncation strategy
(see `PRINCIPLES.md`). Deciding *which* audience a given invocation is for — TTY detection, a
CLI flag, an environment variable — is the caller's job; it's argument-parsing/environment
territory, inside michi's existing "no CLI framework" non-goal. What michi owns is the signal:
given that decision has already been made, which pre-built content to emit.

### 1. Relocate `Audience`

`Audience` (`Assistant`/`User`) currently lives in `src/mcp.rs`, built for MCP first. Using it to
select CLI output too means it's no longer an MCP-specific concept — it never really was; MCP was
just its first consumer. Move it to a new, small, focused module:

- **Create** `src/audience.rs`, holding the enum verbatim (including its existing `serde`
  `cfg_attr` derive and doc comments).
- **Modify** `src/mcp.rs` to import `Audience` from the new location instead of defining it.
- **Modify** `src/lib.rs`'s module list and crate-root re-export: `pub mod audience;` alongside
  the other always-compiled modules; `pub use audience::Audience;` replacing `Audience` in the
  existing `pub use mcp::{Audience, CallToolResult, ContentBlock};` line (which becomes
  `pub use mcp::{CallToolResult, ContentBlock};`).
- **No compat re-export** at the old `mcp::Audience` path — the crate isn't published, there's
  nothing external to break, and a shim would be exactly the kind of unnecessary indirection
  `PRINCIPLES.md` argues against.
- Seven existing internal call sites reference `crate::mcp::Audience` directly (not through the
  crate-root re-export) and need their import path updated: `src/response.rs` (5 occurrences —
  1 in `to_call_tool_result()`'s implementation, 4 in its tests) and `src/napi.rs` (2, in
  `to_call_tool_result()`'s implementation).

### 2. `AgentResponse::render_for(audience: Audience) -> String`

In `src/response.rs`:

```rust
/// Render for the given audience. `Assistant` reads whichever of
/// `.items()`/`.kv_items()` was populated last — identical to
/// `render_text()`. `User` returns `human_content` if the caller set one;
/// if not, it falls back to that same agent-oriented rendering rather than
/// an empty string or a panic.
///
/// That fallback is a real behavior to design around, not just a safety
/// net: TOON/KV text is comma-syntax built for a model to parse, not for a
/// human to read comfortably. A caller intending to actually use the
/// `User` path should call `.human_content()` first — use
/// [`Self::has_human_content`] to check before rendering if the response
/// was built somewhere that may not have set it.
#[must_use]
pub fn render_for(&self, audience: Audience) -> String {
    match audience {
        Audience::Assistant => self.render_text(),
        Audience::User => self.human_content.clone().unwrap_or_else(|| self.render_text()),
    }
}

/// Whether `.human_content()` was set on this builder. Lets a caller
/// downstream of wherever the response was built — e.g. a renderer that
/// only knows the audience, not how the response was constructed — decide
/// what to do before calling `render_for(Audience::User)`, rather than
/// discovering the fallback only by inspecting the text it gets back.
#[must_use]
pub fn has_human_content(&self) -> bool {
    self.human_content.is_some()
}
```

Both take `&self`, matching `render_toon()`/`render_kv()`/`render_hints_only()` — no new
consuming/builder pattern introduced.

### 3. NAPI mirror

In `src/napi.rs`, `JsAgentResponse` gets:

- `renderFor(audience: string) -> napi::Result<String>` — accepts exactly `"assistant"` or
  `"user"` (matching the existing wire-format vocabulary MCP's `annotations.audience` already
  uses at this boundary), returns `napi::Error::from_reason(...)` for anything else. This follows
  the crate's existing convention of plain lowercase strings at the NAPI boundary for audience
  values (see `JsContentBlock.annotations.audience: Array<string>`) rather than introducing a new
  `#[napi]`-derived enum marshaling pattern.
- `hasHumanContent() -> napi::Result<bool>` — mirrors `has_human_content()`.

Both `#[napi(catch_unwind)]`, both read-only (`&self`, following the existing
`renderToon`/`renderKv` pattern of reading through the `Option` slot without `take()`-ing it).

## What this deliberately doesn't do

- **No TTY detection, no flag parsing, no environment inspection anywhere in michi.** The
  decision of which `Audience` to pass in stays entirely the caller's — unchanged non-goal.
- **No new "human display" rendering logic.** `human_content` remains pure plumbing — michi never
  generates the human-facing text itself, exactly as it already works for
  `to_call_tool_result()`. `render_for` only decides *which already-built text* to return.
- **No change to `render_toon()`/`render_kv()`/`render()`/`to_call_tool_result()`'s existing
  behavior.** This is a pure addition; nothing existing changes shape.

## Evidence and honesty check

Per `PRINCIPLES.md`'s cross-tool criterion: this pattern is well-established across the broader
CLI ecosystem generally (TTY-aware dual-mode output is a common Unix convention), but the direct
evidence from this session's own studied consumers is thinner than, say, `help[]` hints clearing
4-for-4. monokl's `output.rs` shows dual-mode output selection via TTY detection today — but
between two JSON renderings (pretty vs. compact), not genuinely human-prose-vs-agent-TOON. rtk has
no equivalent at all; every recognized rtk mode is agent-consumed. Building this now is a bet on a
well-precedented general pattern, not a fully validated one — worth being explicit about, not
worth blocking on given the design costs almost nothing (no new dependency, reuses an existing
type, ~20 lines total).

## Testing plan (detail in the implementation plan)

- `render_for(Assistant)` matches `render_text()` for both the TOON and KV paths.
- `render_for(User)` returns `human_content` verbatim when set.
- `render_for(User)` falls back to agent rendering when `human_content` is unset — for both the
  TOON and KV paths (the design's earlier `to_call_tool_result()` tests only ever exercised the KV
  path; this is exactly the kind of gap `PRINCIPLES.md` flags independent verification for).
- `has_human_content()` reflects `Some`/`None` correctly, before and after `.human_content()` is
  called.
- NAPI: `renderFor("assistant")`, `renderFor("user")` (with and without `humanContent()` set),
  `renderFor("nonsense")` returns an error rather than panicking, `hasHumanContent()`.
- Doc/spec updates: `docs/spec/03-rust-api.md`'s `response` section (new methods),
  `docs/spec/04-mcp-and-napi.md` (NAPI mirror + a note that `Audience` moved, since that file's
  code samples reference it), a new entry in `docs/spec/06-decisions.md` recording the relocation
  and the evidence-tier honesty note above.

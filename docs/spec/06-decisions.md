# Decisions

This file tracks open questions and API-shape rationale. Workspace-level decisions are indexed in [`docs/adr/`](../adr/README.md); current behavior is specified in the relevant API reference.

## Open questions

Tracked by what they block, not by sequence number — "Q1/Q2/Q3" reads too easily as calendar quarters, which these aren't.

### Open until validated — TOON vs. Markdown-KV retrieval accuracy

**Blocks:** recommending TOON as the default for MCP consumers

Token savings have been measured; retrieval accuracy has not. The comparison method and decision scope are in the [research note](../research/toon-retrieval-accuracy.md).

### Open until two consumers agree — measurement-basis rendering

**Blocks:** a shared rendering primitive for measured and modeled values

ProjectAtlas distinguishes observed values, deterministic derivations, and modeled counterfactuals. Lumen may need the same distinction. This supports investigation, not a new Michi domain type.

First test whether the proposed `ProvenanceBlock` and ordinary section fields express this clearly; see [the envelope proposal](07-wisp-and-monokl-envelope.md). Add a `MeasurementBasis`/`Measured<T>` API only when two real consumers need the same wire contract and the design passes the five gates in `PRINCIPLES.md`. Michi renders the basis; callers decide whether a value is observed, derived, or modeled.

This question adds no production dependency. External telemetry or TOON crates remain evaluation/dev dependencies unless an implementation spec proves otherwise.

### Deferred to v2 — `cli` feature scope

**Blocks:** terminal-aware rendering

Reserved for terminal-aware rendering.

- If `--format toon` in a human terminal context should wrap long rows or colourize the header, this is where that lives — likely a `render_terminal()` variant consulting terminal width (`crossterm`) and applying ANSI colour to health signals and the type header.
- Out of scope for v1: michi v1 targets agent consumers only, not human-readable terminal output.

### Resolved at v0.1 — `AgentResponse` builder vs. standalone functions

Both.

- `AgentResponse`/`JsAgentResponse` is exported, along with `renderHints()`/`appendHints()`/`renderRecovery()`/`renderToon()`/`emptyState()`/`truncate()`.
- The low-level functions are exported _additively_, not instead of the builder — removing an already-shipped export is a breaking change with no upside.
- Rust-only consumers reach every primitive directly; TypeScript consumers get both the builder and the standalone functions.
- This also settled the chainable-builder question: `JsAgentResponse`'s setters return `undefined`, not `this` — see [04-mcp-and-napi.md](04-mcp-and-napi.md)'s "Chainable setters" section.

### Resolved at v0.1 — Recovery hint format

`RetryHint.params` uses `kv::KvValue`, an existing already-tested enum, instead of `serde_json::Value` — fixes the Rust-side type-loss concern without adding a dependency.

- **A related bug surfaced during this reconciliation and got fixed alongside it:** independent of which Rust-side type `params` used, `AgentResponse::render_json()`'s JSON _output_ was, for a time, stringifying every param value regardless of its `KvValue` variant (`{"seconds":"30"}` instead of `{"seconds":30}`) — which would have defeated the point of a typed enum entirely.
- `render_json()` now serializes `Int`/`Float`/`Bool` as native JSON literals, `Text` as a JSON string, `Missing` as `null`, so both the Rust-side and JSON-output representations carry real type information.
- Covered by tests in `src/response.rs` and `src/kv/mod.rs`.

---

## Why the API is shaped the way it is

This crate went through more than one draft. None of what follows is "the API used to be wrong" — it's the reasoning behind why the current shape won, in case a future change tempts you back toward an earlier idea that was already tried and rejected for a reason.

### `ToonOptions` bundles everything, not five positional arguments

- An early draft split `render_toon()`'s inputs across five positional parameters plus a small options struct holding only `max_cell_len`/`total_count`.
- The shipped version folds every input — `type_name`, `fields`, `rows`, `hints` included — into `ToonOptions` itself, taken by reference.
- One struct, one thing to construct.

### `append_hints` mutates in place

- It takes `out: &mut String` and appends to it, rather than taking `body: &str` and returning a new `String`.
- That avoids an allocation-and-copy this crate removes everywhere it can — `recovery::append_recovery` follows the identical pattern.
- The content appended is the same either way; this is a signature choice, not a behavior change.

### `truncate()`/`truncate_inline()` take `hint: &str` as a parameter

- An early version hardcoded `"full=true"` for every call site.
- Every example in this spec happening to use that exact string doesn't mean every caller's actual escape-hatch flag is named that — taking it as a parameter costs nothing and removes an assumption michi has no business making.

### `error::DomainError` and `error::Error` are two types, not one

- An early draft called this type `AxiError` and made it the crate's entire error type.
- The shipped design splits it: `DomainError` is the pure data/render type — what [03-rust-api.md](03-rust-api.md) documents.
- `Error` is a `thiserror`-derived enum with a `Domain(DomainError)` variant alongside always-compiled `InvalidInput`/`NotFound` variants; the original intent was room for execution-layer variants (`Http`, `Timeout`, `StepFailed`) once the `pipeline` crate (Plan 2) landed.
- **That's not how it shipped.** `crates/michi-pipeline` defines its own `ExecutionError` enum (`Http`, `Timeout`, `Failed`, `CircuitOpen`, `StepFailed`, `StepCountMismatch`, `UnknownStepId`, `DuplicateStepId`, `CyclicDependency`, `Cancelled`) rather than adding variants to `michi_core::Error`. `impl From<ExecutionError> for DomainError` converts at the boundary instead. `michi_core::Error` itself still has only `InvalidInput`/`NotFound`/`Domain` — verified against current source, not extended. This keeps `michi-pipeline`'s error surface independently versioned alongside the crate it belongs to, consistent with the "independent release cadence" rationale above, rather than coupling it back into `michi-core`.

### `idempotency::already_done()` only checks — it doesn't render

- An early draft had one function do both.
- michi owns no persistence layer, so it has no way to know on its own whether an operation already happened — only your store does.
- Splitting check from render is the honest shape: `already_done()` takes what your lookup found and classifies it; `render_already_done()` renders regardless of how you detected the no-op.

### `IdempotencyKey::new()` takes one pre-combined string, not `(operation, stable_input)`

- A caller that wants an operation-name-plus-input key builds it themselves (`format!("{operation}:{stable_input}")`) before calling `new`, or uses `from_hash` for the hashed variant.
- `from_hash` uses FNV-1a, not SHA-256 — idempotency keys need stability and low collision, not cryptographic security.
- This crate has no `sha2` dependency at all today — it was removed along with the rest of the unused `cache` feature (see the crate-boundary entry below).

### `RetryConfig`'s `jitter_factor: f64`, not `jitter: bool`

- `f64` supports partial jitter, not just full-jitter-or-none — strictly more capable at the same call-site cost.
- **This was also the subject of an adversarially-found bug:** jitter could previously exceed `max_delay`, since nothing capped the jittered result against it. Fixed now.
- `next_retry_delay`'s four-parameter signature (`config`, `attempt`, `jitter_seed`, `retry_after`) reflects that fix plus the `retry_after` integration.
- It returns `Option<Duration>`, not a bare `Duration`, so a caller can tell "one more attempt, with this delay" apart from "retries exhausted."

### `StatusResponse.tool_name`/`.description` are `String`, not `&'static str`

A tool's name and description are runtime-computed at every real call site this crate has been used from — often built from config or a manifest. `&'static str` would force every caller to hardcode a literal or leak an allocation just to satisfy the lifetime.

### `OutputFormat` has two variants, not three

- An early draft had a three-way `Toon`/`Kv`/`Json` split.
- The shipped design decides TOON-vs-KV by which of `.items()`/`.kv_items()` was called (see `target` in [03-rust-api.md](03-rust-api.md)'s `response` section), not by a value passed to `render()` — so `OutputFormat` only needs to select text vs. JSON.

### A multi-section envelope on `AgentResponse`, not a second response type

Wisp's briefings and monokl's batch `QueryResponse` both need one tool result to carry several named sections, one budget report, and one provenance record. `AgentResponse` holds one body: `RenderTarget` is `Unset | Toon | Kv`, `items()`/`kv_items()` set it, and `render()` dispatches on whichever ran last — so a ten-section response silently keeps one. Full design in [07-wisp-and-monokl-envelope.md](07-wisp-and-monokl-envelope.md); the reasoning for the three choices that could have gone otherwise:

- **Sections extend `AgentResponse` rather than arriving as a parallel `BatchResponse` type.** A second response type would duplicate hints, recovery, audience routing, `human_content`, and the whole `to_call_tool_result()` path, and would leave every consumer choosing between two builders on a distinction (one section or several) they often learn only at render time. `items()`/`kv_items()` stay as one-section sugar over the same storage, and a zero- or one-section response renders byte-for-byte as it does today — the existing `insta` snapshots are the test, so multi-section support is a minor bump, not a major one. Widening `PartialSuccess`'s `completed`/`skipped` from `Vec<String>` to index-bearing `OpRef` **is** a major bump, since `completed[2]:` becomes `completed[2]{index,operation}:` and rendered output differs. Taken now because there is no published consumer to break; taken at all because `Vec<String>` cannot express `results[i]` ↔ `ops[i]`, and that correspondence is the whole point of a batch response.
- **The budget and provenance slots take michi's own `BudgetSummary` and `ProvenanceBlock`; michi depends on `wisp-contracts` not at all.** An earlier version of this decision imported `BudgetReport` and `Provenance` from that crate behind an optional `contracts` feature. michi publishes to crates.io before Wisp does, so it cannot name a crate that is not published yet, and an optional dependency still has to resolve — a feature gate does not fix a publish-ordering problem. The vocabulary argument survives untouched for the two repositories it was written for: Wisp and monokl must agree on what a `Provenance` *means*, and third-party harness plugins that cannot link AGPL code have to parse it, which is why that crate is permissively licensed. What a renderer needs is smaller — the numbers and labels a `budget:` or `provenance:` block puts on the page — so michi defines that shape itself. The conversion from the contracts types lives in the consumer, `wisp-output` for Wisp and `monokl-agent` for monokl's CLI, never here. No feature flag either way: these types add no dependency, so gate 2's zero-dep default has nothing to gate. Sections are the same — built from types michi already owns.
- **Never fail closed on overflow — written down here so no future primitive regresses to it.** Serena's `find_symbol` returns *no content* once `max_answer_chars` is exceeded; the agent is left to guess a narrower query knowing nothing about what it missed. Every michi overflow path does the opposite: partial results, `returned_count`/`total_count`, and a hint naming the refinement. This is also why the envelope carries no pagination cursor — MCP defines `cursor`/`nextCursor` only for list operations, `tools/call` results have no cursor field, and the 2026-07-28 release candidate does not add one, so a cursor here would be a private convention no client honors.

### `AgentResponse` has no `unsafe impl Send`/`Sync`

- Every field is an owned, non-interior-mutable type (`String`, `Vec`, `Option<usize>`, `bool`, ...), so `Send`/`Sync` already hold through Rust's automatic derivation.
- An explicit `unsafe impl` would be both redundant and a violation of this crate's own "no `unsafe` outside the NAPI boundary" rule.

### `napi` v3 with the `napi6` feature, not v2 with `napi4`

- `napi4` was, at the time, the lowest level exposing `ThreadsafeFunction` and the async support napi-rs needs, with Node.js 12 as the floor.
- The move to v3/`napi6` happened because v3 dropped the old Docker-image cross-compilation requirement — not because michi needed any new capability.
- Its exports are synchronous either way; the choice is entirely about build tooling.

### `content[0]` is TOON/KV text, not a JSON mirror of `structured_content`

- The MCP spec says a tool returning `structuredContent` _should also_ mirror that same data as serialized JSON in a text block, for clients that don't read `structuredContent` yet.
- michi doesn't do this literally — `content[0].text` comes from `render_toon()`/`render_kv()`, never from JSON-stringifying `structured_content`.
- Complying with the letter of that guidance would mean putting verbose JSON in the primary agent-facing block, which defeats the entire reason TOON exists — token efficiency is michi's whole premise, and JSON is never the token-cheaper option for list data (see [02-toon-format.md](02-toon-format.md)).
- The two representations already carry the same underlying data, both built from the same `AgentResponse` — just shaped for different audiences: TOON for the model to read, JSON for a client to parse programmatically.
- That's a deliberate deviation from the spec's backwards-compatibility recommendation, made with eyes open, not an oversight.

### `Audience` lives in its own module, not inside `mcp.rs`

- It was originally defined in `mcp.rs` because MCP was its first consumer, but the concept itself was never MCP-specific — `render_for()`/`has_human_content()` use it for CLI output selection with no MCP involvement at all.
- Moved to `src/audience.rs` once it had a second real consumer, rather than left in a module named after the first one.

### `render_for(Audience::User)` falls back to agent rendering instead of returning `None` or an empty string

- The alternative — an `Option<String>`-returning method — would be more strictly honest about "did you actually get human content," but would force every caller into `Option`-handling ceremony even in the common case of just wanting text to print.
- **The resolution:** keep `render_for` returning a plain `String` (matching `render_toon()`/`render_kv()`'s existing family), but add `has_human_content()` as a separate, cheap predicate.
- A caller who's downstream of wherever the response was built (a generic renderer that only knows the audience, not how the response was constructed) can check first, without forcing the common case to handle a case it usually doesn't need to think about.

### Evidence for `render_for` is real but thinner than `help[]` hints had

- Dual-mode CLI output (agent vs. human, or some analogous split) is a well-established pattern across the broader CLI ecosystem generally, but this session's own studied consumers only partially confirm it: monokl's `output.rs` does TTY-detected dual-mode output today, but between two JSON renderings (pretty vs. compact), not genuinely human-prose-vs-agent-TOON.
- Built anyway, since the design costs almost nothing — no new dependency, reuses the existing `Audience` type, roughly 20 lines total.
- Worth being explicit that this one is a bet on a well-precedented general pattern, not a fully validated one, unlike hints clearing 4-for-4 across every studied tool.

### A separate crate is justified by dependency weight and coupling, not just "this is a distinct feature"

The decision rule, for this crate and future ones like it — split into a separate crate when:

1. It pulls in dependencies heavy/opinionated enough that Cargo's feature-unification model would genuinely surprise a consumer of the default build who never asked for them — the strongest signal here, since michi's entire identity is "zero-dep by default" in a way most crates don't promise as centrally.
2. It has low coupling to the core crate's internals, reaching it only through the public API.
3. It would benefit from an independent release cadence rather than forcing version bumps on the stable core.

A dependency stays a _feature_, not a crate, when it's low-consequence even if unified in accidentally (`serde`'s derives — most Rust consumers already have `serde` somewhere) or would just create combinatorial Cargo.toml complexity with no real benefit.

**Applied to Plan 2:**

- `pipeline` (the executor) becomes its own crate — heavy dependency (tokio), clean boundary (operates only on the already-public `Pipeline` data type), independent maturity. **Shipped as designed**: `crates/michi-pipeline`, depending on `michi-core` + `michi-resilience` + `tokio`, not re-exported through the root `michi` facade.
- The async halves of `resilience` (`CircuitBreaker`, the retry wrapper) fold _into_ that same crate rather than becoming a fourth crate of their own — they exist specifically to wrap pipeline step execution and have no independent use case, so criterion (2) fails for splitting them out further. **Shipped as designed**: `CircuitBreaker` lives in `michi-pipeline` itself, not a separate crate; `michi-resilience` continues to hold only the sync retry-delay math both `michi-pipeline` and NAPI callers share.
- `fuzzy` and `cache` each become their own crate, downstream of `pipeline` — the intended coupling is a `Resolution<T>`-shaped type used in pipeline-step context, so `fuzzy` implies `pipeline` by design. Verified against current source: neither `Resolution<T>` nor any `fuzzy`/`cache` code exists yet anywhere in this workspace — this remains a forward design intent, not a shipped fact.
- `cli` becomes its own crate too — a genuinely different kind of consumer (a terminal, not an agent) with no coupling to pipeline internals beyond calling into it like any other dependent would. Also not yet built.

None of this is scaffolded ahead of time. A crate gets created in the same work that writes its first real implementation, not before — an empty crate sitting in the workspace is the identical ambiguity problem the deleted `pipeline`/`sink`/`resilience` stub files caused, just moved up one level of granularity.

---

## Implementation notes

### `IdempotencyKey::from_hash` needs deterministic input

For the key to be canonical across calls with the same logical input, the bytes you hash must be produced deterministically:

- **Maps and structs:** sort keys before serializing. `serde_json::to_vec` on a `HashMap` produces non-deterministic key order — use `BTreeMap` or a sorted intermediate representation.
- **Floats:** avoid them in idempotency keys; representation varies by architecture. Round to a fixed decimal or convert to integers.
- **Timestamps:** exclude request-time fields (`created_at`, `request_id`) unless you actually want a per-request key rather than a per-operation one.

`from_hash` takes raw `&[u8]` — michi has no opinion on how you produce those bytes, and doesn't itself depend on `serde_json`. The example below uses it only because JSON is a common choice for callers already producing it elsewhere; any deterministic serialization works equally well as long as it follows the rules above.

```rust
// Correct: BTreeMap serialises with sorted keys
let mut params: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
params.insert("project", json!("PROJ"));
params.insert("type",    json!("Task"));
let key = IdempotencyKey::from_hash("create_item", &serde_json::to_vec(&params)?);

// Incorrect: HashMap key order is non-deterministic
let params: HashMap<&str, _> = /* ... */;
let key = IdempotencyKey::from_hash("create_item", &serde_json::to_vec(&params)?);
// ^ same logical input may produce a different key on different calls
```

### Why `is_retryable_status()` excludes 500

The default retryable set is `{ 429, 502, 503, 504 }`.

- 429 and the 50x gateway errors represent transient conditions — rate limits, upstream unavailability — where the same request will likely succeed after a delay.
- 500 represents a server-side bug: retrying without changing anything reproduces the same 500, and for write operations, may duplicate side effects if the server processed the request before erroring.
- Callers that genuinely need to retry 500s (some APIs use it for transient conditions) can layer their own predicate on top:

```rust
// Default: 429, 502, 503, 504 only
if is_retryable_status(status) { ... }

// Custom: caller adds 500 for a specific API known to use it transiently
let retryable = is_retryable_status(status) || status == 500;
```

### `render_hints_only()` — the three-surface seam

Supports MCP frameworks implementing a three-surface pattern:

- One content block for the agent (`audience: ["assistant"]`).
- One for display (`audience: ["user"]`).
- A `structuredContent` payload for client tooling.

The display body is rendered by the framework's own Markdown/rich-text layer; the agent-facing block combines TOON output with a `help[]` trailer. Without `render_hints_only()`, framework code that's already rendered a display body would need to reconstruct an entire `AgentResponse` just to get a formatted `help[]` block to append.

```typescript
// Example: MCP framework TypeScript (simplified)
function assembleToolResult(opts: RespondOpts): ToolResult {
  const userBody = renderDisplayMarkdown(opts.body); // display surface → audience:["user"]
  const hintsBlock = michi.renderHintsOnly(opts.hints); // michi → append to assistant body
  const assistantBody =
    opts.assistant ?? // caller-provided TOON
    userBody + "\n" + hintsBlock; // fallback: display body + hints

  return {
    content: [
      {
        type: "text",
        text: assistantBody,
        annotations: { audience: ["assistant"], priority: 1.0 },
      },
      {
        type: "text",
        text: userBody,
        annotations: { audience: ["user"], priority: 0.5 },
      },
    ],
    structuredContent: opts.structured ?? null,
  };
}
```

### `parse_retry_after()` format details

Accepts both forms from RFC 7231 §7.1.3:

| Form            | Example                           | Notes                              |
| --------------- | --------------------------------- | ---------------------------------- |
| Integer seconds | `"120"`                           | Seconds to wait from response time |
| HTTP-date       | `"Wed, 21 Oct 2026 07:28:00 GMT"` | Absolute datetime, UTC only        |

- Returns `None` for anything malformed — callers should treat that as "use backoff only" and call `next_retry_delay()` with `retry_after: None`.
- Doesn't validate that an HTTP-date is in the future: a server returning a past date (clock skew, a bug) produces a zero or very small duration from this function alone, but `next_retry_delay()` clamps the final result to at least `config.initial_delay` regardless, so no extra handling is needed at the call site.

### `render_toon()`'s capacity estimate

A heuristic, not a hard bound:

```
capacity = 60 + row_count × (field_count × 12 + 10) + hints.len() × 60
```

- The `60` covers the type name, bracketed count, and brace-wrapped field list.
- `× 12 + 10` per row estimates a typical short cell (an id, a short title fragment, a state word) plus its delimiter.
- `hints.len() × 60` covers the trailing `help[]` block.
- The estimate intentionally runs slightly high so the common case never reallocates — a wildly under-estimated row (very long untruncated cells) may trigger one reallocation, acceptable since `max_cell_len` truncation caps the realistic upper bound.
- None of these are configurable — they're tuning constants, not part of the public contract.

### `outputSchema` is in the LLM context; `structuredContent` isn't

Matters for token budget planning in MCP servers using progressive disclosure or deferred tool loading.

- `outputSchema` on a tool definition is part of the `tools/list` response — it _is_ injected into the context window at session start alongside `inputSchema`, adding roughly +15% on the schema component (an approximate, workload-dependent figure).
- It's a one-time cost that amortizes across the session.
- `structuredContent` in a tool result is client-consumed only and never enters the context window.

This matters for deferred tool loading specifically: adding `outputSchema` to tools that aren't loaded yet adds schema tokens before those tools are even called, defeating the point of deferral.

- Only define `outputSchema` on always-loaded tools, or exclude it from `tools/list` for deferred ones and supply it only when the tool is actually invoked.
- If your MCP framework auto-registers `outputSchema` from tool definitions, make sure it respects that boundary.

### Build-time P4 — pre-computed aggregates at CI time

AXI Principle 4 usually gets discussed as a runtime convention: always return `totalCount`, pre-filter before responding, summarize instead of dumping raw data. The same principle applies at build time, and that's where the bigger wins are.

The pattern: any deterministic computation a tool needs repeatedly is a candidate for pre-computing in CI instead of running it per request.

**Canonical example — a design system component library.** At build time, a bundler plugin scans every component's definitions, resolves references, and writes a static lookup index shipped with the MCP server package. When an agent calls `searchComponents()`, the handler reads that static structure — no scanning, no resolution, no index construction per request. The agent pays zero turn cost for the computation; the CI pipeline pays it once.

| Computation | Runtime | Build time |
| --- | --- | --- |
| Filtering by user input | ✓ | — |
| Counting available items | Runtime (`totalCount` field) | Build time (pre-count, embed in manifest) |
| Resolving references between definitions | — | ✓ (bundler plugin) |
| Indexing for search / BM25 | — | ✓ (write index to disk at build) |
| Building dependency graphs | — | ✓ (static graph serialised to JSON) |
| Generating tool output schemas | — | ✓ (schema library `toJsonSchema()` at registration) |

This is a design decision, not a code pattern — it shows up during architecture planning, not implementation. Ask: does this computation change per request, or is it stable across many requests? Stable computation belongs at build time.

### Resolved at v0.1 — Cargo workspace crate splitting

`michi` was split into standalone Layer 1 & 2 workspace crates (`crates/michi-truncate`, `crates/michi-resilience`, `crates/michi-toon`, `crates/michi-core`) with `michi` as a unified facade.

- **Why:** Downstream tools that only require char-boundary string truncation or backoff math can depend on `michi-truncate` or `michi-resilience` with zero transitive dependencies.
- **Facade Crate:** `michi` re-exports all primitives so existing Rust callers have zero breaking changes.

### Resolved at v0.1 — Small-String Optimization (`compact_str`)

`Value::Str` in `michi-toon` uses `compact_str::CompactString` instead of heap `String`.

- **Why:** Up to 24-byte cell strings (IDs, status codes, field names) are stored on the stack without heap allocation (`malloc`), eliminating up to 90% of allocations during TOON list rendering.

### Resolved 2026-09-04 — License changed from AGPL-3.0-or-later to FSL-1.1-MIT

`michi`'s declared license (`Cargo.toml`, `package.json`, `LICENSE`, `README.md`) moved from `AGPL-3.0-or-later` to `FSL-1.1-MIT`.

- **Why:** Suite consistency — Lumen and Wisp's upper layers are both `FSL-1.1-MIT`, and an AGPL michi sitting underneath an FSL Wisp created a real redistribution conflict: FSL code cannot link AGPL code. Relicensing removes that conflict rather than working around it. FSL is also adoptable in environments that ban AGPL outright, and converts to MIT two years after each release, which AGPL never does.
- **No permissive-core crate is designated yet.** Unlike Lumen, which splits a permissive Layer 1/1.5 from an FSL Layer 2/2.5/4, michi has no crate carved out as permissive today — every crate in the workspace is `FSL-1.1-MIT`. If a permissive core is designated later, Lumen's `deny.toml` containment pattern (banning the FSL crate from the permissive crate's dependency graph) is the model to follow.

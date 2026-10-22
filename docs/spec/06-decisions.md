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
- Covered by tests in `crates/michi-core/src/response.rs` and `crates/michi-core/src/kv/mod.rs`.

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
- **That's not how it shipped.** `crates/michi-pipeline` defines its own `ExecutionError` enum (`Http`, `Timeout`, `Failed`, `CircuitOpen`, `StepFailed`, `Cancelled`, `Interrupted`, `AmbiguousOutcome`, `Panicked`; plan validation has its own `PlanError` since the 2026-10-21 redesign) rather than adding variants to `michi_core::Error`. `impl From<ExecutionError> for DomainError` converts at the boundary instead. `michi_core::Error` itself still has only `InvalidInput`/`NotFound`/`Domain` — verified against current source, not extended. This keeps `michi-pipeline`'s error surface independently versioned alongside the crate it belongs to, consistent with the "independent release cadence" rationale above, rather than coupling it back into `michi-core`.

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
- **The budget and provenance slots take michi's own `BudgetSummary` and `ProvenanceBlock`; michi depends on `wisp-contracts` not at all.** An earlier version of this decision imported `BudgetReport` and `Provenance` from that crate behind an optional `contracts` feature. michi publishes to crates.io before Wisp does, so it cannot name a crate that is not published yet, and an optional dependency still has to resolve — a feature gate does not fix a publish-ordering problem. The vocabulary argument survives untouched for the two repositories it was written for: Wisp and monokl must agree on what a `Provenance` _means_, and third-party harness plugins that cannot link AGPL code have to parse it, which is why that crate is permissively licensed. What a renderer needs is smaller — the numbers and labels a `budget:` or `provenance:` block puts on the page — so michi defines that shape itself. The conversion from the contracts types lives in the consumer, `wisp-output` for Wisp and `monokl-agent` for monokl's CLI, never here. No feature flag either way: these types add no dependency, so gate 2's zero-dep default has nothing to gate. Sections are the same — built from types michi already owns.
- **Never fail closed on overflow — written down here so no future primitive regresses to it.** Serena's `find_symbol` returns _no content_ once `max_answer_chars` is exceeded; the agent is left to guess a narrower query knowing nothing about what it missed. Every michi overflow path does the opposite: partial results, `returned_count`/`total_count`, and a hint naming the refinement. This is also why the envelope carries no pagination cursor — MCP defines `cursor`/`nextCursor` only for list operations, `tools/call` results have no cursor field, and the 2026-07-28 release candidate does not add one, so a cursor here would be a private convention no client honors.

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
- Moved to its own module (`crates/michi-core/src/audience.rs`) once it had a second real consumer, rather than left in a module named after the first one.

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

Criterion (3) is amended by "Resolved 2026-10-21 — Two crates, one version" below: a separate crate still versions in lockstep with the rest of michi until it is a deliberately stable interface.

A dependency stays a _feature_, not a crate, when it's low-consequence even if unified in accidentally (`serde`'s derives — most Rust consumers already have `serde` somewhere) or would just create combinatorial Cargo.toml complexity with no real benefit.

**Applied to Plan 2:**

- `pipeline` (the executor) becomes its own crate — heavy dependency (tokio), clean boundary (operates only on the already-public `Pipeline` data type), independent maturity. **Shipped as designed**: `crates/michi-pipeline`, depending on `michi-core` + `tokio`, not re-exported through `michi-core`.
- The async halves of `resilience` (`CircuitBreaker`, the retry wrapper) fold _into_ that same crate rather than becoming a fourth crate of their own — they exist specifically to wrap pipeline step execution and have no independent use case, so criterion (2) fails for splitting them out further. **Shipped as designed**: `CircuitBreaker` lives in `michi-pipeline` itself, not a separate crate; `michi_core::resilience` holds only the sync retry-delay math both `michi-pipeline` and NAPI callers share.
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

> Superseded by "Resolved 2026-10-21 — Two crates, one version" below.

`michi` was split into standalone Layer 1 & 2 workspace crates (`crates/michi-truncate`, `crates/michi-resilience`, `crates/michi-toon`, `crates/michi-core`) with `michi` as a unified facade.

- **Why:** Downstream tools that only require char-boundary string truncation or backoff math can depend on `michi-truncate` or `michi-resilience` with zero transitive dependencies.
- **Facade Crate:** `michi` re-exports all primitives so existing Rust callers have zero breaking changes.

### Resolved at v0.1 — Small-String Optimization (`compact_str`)

`Value::Str` in `michi_core::toon` (originally `michi-toon`) uses `compact_str::CompactString` instead of heap `String`.

- **Why:** Up to 24-byte cell strings (IDs, status codes, field names) are stored on the stack without heap allocation (`malloc`), eliminating up to 90% of allocations during TOON list rendering.

### Resolved 2026-09-04 — License changed from AGPL-3.0-or-later to FSL-1.1-MIT

`michi`'s declared license (`Cargo.toml`, `package.json`, `LICENSE`, `README.md`) moved from `AGPL-3.0-or-later` to `FSL-1.1-MIT`.

- **Why:** Suite consistency — Lumen and Wisp's upper layers are both `FSL-1.1-MIT`, and an AGPL michi sitting underneath an FSL Wisp created a real redistribution conflict: FSL code cannot link AGPL code. Relicensing removes that conflict rather than working around it. FSL is also adoptable in environments that ban AGPL outright, and converts to MIT two years after each release, which AGPL never does.
- **No permissive-core crate is designated yet.** Unlike Lumen, which splits a permissive Layer 1/1.5 from an FSL Layer 2/2.5/4, michi has no crate carved out as permissive today — every crate in the workspace is `FSL-1.1-MIT`. If a permissive core is designated later, Lumen's `deny.toml` containment pattern (banning the FSL crate from the permissive crate's dependency graph) is the model to follow.

### Resolved 2026-10-21 — Two crates, one version

Before the first crates.io publish, the four sync crates (`michi-truncate`, `michi-resilience`, `michi-toon`, `michi-core`) and the `michi` facade merged into a single crate, `michi-core`, with `toon`, `truncate`, and `resilience` as modules. `michi-pipeline` stays a separate crate. Every package (`michi-core`, `michi-pipeline`, and the `@orin-dx/michi` npm package built from `packages/michi-node`) versions in lockstep through a Callisto `[[fixed-group]]` named `michi`.

- **Why one sync crate:** separate crates only contain breaking changes when they version independently, and michi's parts all change at the same fast pre-1.0 pace. Independent versions bought release overhead (cascading bumps across crates on every change) without protecting any consumer.
- **Build time was not a reason to split:** in a measured clean release build, michi's own crates took about 1.9s of a 3.96s total.
- **Why `michi-pipeline` stays separate:** the existing dependency-weight rule in "A separate crate is justified by dependency weight and coupling" still applies — it pulls in Tokio, which the default build must not. It depends on `michi-core` through an exact `=` version pin so a mixed set never resolves.
- **When a part splits out again:** only for a heavy dependency, or once it becomes a deliberately stable interface worth versioning on its own. It is then re-exported from `michi-core`, so consumers see no change.
- **Why no facade:** `michi` on crates.io is taken by an unrelated project, so the published names are `michi-core` and `michi-pipeline`. Consumers who want `use michi::...` rename the dependency: `michi = { package = "michi-core", version = "0.3" }`.

### Resolved 2026-10-21 — Validation failures are errors on every surface

`PRINCIPLES.md` said every `render*()` returns `String` and always succeeds, while `render_toon()` returns `Result` and `AgentResponse` turned a validation failure into an `error: toon_validation_failed` body with `isError: false`. A client checking `isError` read an error as a success.

- **Decision:** input that can always be made safe is sanitized and rendered infallibly. Input whose shape can be wrong (row length against fields, structural characters in header names) is validated, and the free functions return `Result`. `AgentResponse` still always produces a response, but a validation failure now sets the error flag on the text body, the JSON `isError` and MCP `isError`, and `AgentResponse::is_error()` reports it.
- **No `debug_assert!` fallback:** debug and release builds behave the same. The old principle asked for one; the code already bans it in the TOON renderer.

### Resolved 2026-10-21 — Line breaks in caller text are escaped, never passed through or dropped

Several line-based renderers (hints, empty-state names, error messages, recovery tool/reason/parameter keys, KV keys, already-done fields, partial-success ids) wrote CR/LF raw, so a value such as `x\nstatus: failed` produced a second, fake `status:` line. Others (KV values, status fields, TOON cells) stripped them, silently changing the data.

- **Decision:** every renderer writes CR and LF in caller text as the two-character escapes `\r` and `\n`. Nothing starts a new line and nothing is dropped.
- **TOON cells:** a value with a line break is quoted and the escapes go inside the quotes, alongside `\"` and `\\`, so cells round-trip exactly. Quoted backslashes are now doubled; before, `C:\src\a,b` parsed back as `C:srca,b`, and a value ending in `\` escaped its closing quote and merged two cells.
- **TOON hints:** only line breaks are escaped; ordinary punctuation such as `,` and `{` is kept rather than replaced with `_`. Header tokens still replace structural characters, since the header has no escape syntax.
- **Ambiguity accepted:** outside quoted TOON cells there is no escape for the backslash itself, so caller text that already contains the characters `\n` reads the same as an escaped line break. These blocks have no parser contract, and readability for agents wins.

### Resolved 2026-10-21 — Integers keep their exact digits

`Value::from(u64)` clamped values above `i64::MAX` to `9223372036854775807`, and serde `list()` rounded them through `f64` (`18446744073709552000`). Neither is faithful for IDs, hashes or counters.

- **Decision:** values above `i64::MAX` become `Value::Str` holding the exact decimal digits, which render the same as an integer cell. `usize` follows `u64`.

### Resolved 2026-10-21 — `toon::list()` headers are the union of every item's fields

`list()` took its columns from the first item only, so a field skipped on row one (`#[serde(skip_serializing_if = "Option::is_none")]`) vanished from every row.

- **Decision:** the header lists every field any item serialized, in first-seen order, and rows missing a field get an empty cell. Rejecting differing schemas was the alternative; it would make an ordinary optional field an error.

### Resolved 2026-10-21 — Schemas and serde describe the wire form

The `schemars` schemas for `ContentBlock` and `CallToolResult` were derived from the Rust structs (`is_error`, string `structured_content`, top-level `audience`) while serde emitted the MCP wire form (`isError`, parsed JSON `structuredContent`, `annotations.audience`). `RecoveryHint` serialized `params` as `[key, {"Text": ...}]` pairs and an absent reason as `null`, while the hand-built structured companions wrote a `params` object of plain values and omitted the reason.

- **Decision:** each type's schema is the schema of the private wire type serde actually emits, and `RecoveryHint` serializes through a wire type that reuses the companions' value writer, so serde, schema and companion are one shape. Tests compare generated schemas and serde output against real serialized values.
- **`schemars` enables `serde`:** a schema of the wire form needs the wire types, which only exist with `serde`.
- **Deserializing `RecoveryHint` is exact or fails:** strings, booleans, `null`, integers in `i64` and floats (with the decimal places needed to reproduce the value, so `1e-6` stays `0.000001`) are accepted; arrays, objects and integers outside `i64` are rejected with an error naming the parameter, never rewritten. The schema restricts parameter values to those scalar types. A `Duration` parameter serializes as a string such as `"1.5s"` and reads back as `Text`.
- **`ContentBlock` is text-only:** a wire block whose `type` is anything but `"text"` fails to deserialize instead of becoming a text block, and the schema advertises only `"text"`.
- **`compact_str/serde` follows michi's `serde` feature:** the default build no longer pulls in serde through `compact_str`.

### Resolved 2026-10-21 — `michi-pipeline` retry and breaker semantics

- **One replay rule, internal and outward.** Every failed attempt is classified by whether it could have taken effect, and the same classification decides whether `CircuitBreaker::call` re-invokes the step and whether the returned error tells an outer caller that replay is safe (`ExecutionError::is_retryable()`, `DomainError.retryable`).
  - **Not executed:** circuit open, cancelled. The step never ran; safe to try again.
  - **Rejected:** HTTP 429 or 503, or `Failed { retryable: true }`. The server (or the step) says the request was not processed. Retried per the retry policy for every step.
  - **Ambiguous:** a timeout (the breaker's own or step-returned, also inside `StepFailed`), HTTP 502 or 504. The attempt may have committed. Re-invoked only if `Step::is_idempotent()` is `true` (default `false`; `FnStep::idempotent()` for closures).
  - **Definitive:** everything else (other 4xx, 500, `Failed { retryable: false }`). Not retried.
- **Outward guidance:** when a non-idempotent step ends in an ambiguous outcome, the breaker returns `ExecutionError::AmbiguousOutcome { source }`. It is not retryable, and its `DomainError` keeps the inner code, drops `retry_after`, and tells the agent to verify whether the operation completed before retrying. An idempotent step's ambiguous outcome is returned bare and stays retryable. A wrapper variant beats a `replay_safe` field because steps build `Http`/`Timeout` themselves and cannot know their own replay safety; only the breaker, which holds the step, can.
- **Contract on steps:** 429 and 503 mean "not processed" and are retried even for a non-idempotent step, so a step that may have committed its side effect must not report them. It reports 502, 504, a timeout or a non-retryable error instead.
- **Why 502/504 are ambiguous:** a gateway error means an upstream may have received and applied the request before the proxy gave up (RFC 9110 §9.2.2: clients should not automatically retry non-idempotent requests). 429/503 are explicit refusals.
- **Duplicate ids and run state:** both `execute_pipeline` and `execute_pipeline_parallel` reject duplicate ids with `DuplicateStepId` before running anything. Each sequential run resets every status to `Pending`, and every step after a failure, or not started at cancellation, is `Skipped`. IDs stay caller-chosen and readable rather than generated UUIDs: agents read them and copy them into dependency edges and recovery calls, and a UUID costs about 20 tokens without telling an agent anything.
- **Open:** a `StepFailed` describes only the failed step's replay safety; replaying a whole pipeline re-runs steps that already completed. Resume-from-failure, binding runners to step ids instead of list position, and `StepStatus` variants that tell "not run (circuit open / cancelled / prerequisite failed)" apart from "ran and failed" are pending a decision. **Superseded** by "Resolved 2026-10-21 — Pipeline execution redesign" below, which also replaces the `execute_pipeline`/`execute_pipeline_parallel` run-state rules in the bullet above.
- **Breaker state:** phase, probe ownership and failure count are one mutex-guarded state machine with a generation counter. An outcome from a call admitted under an earlier generation is ignored: it cannot reopen a recovered breaker, extend the open window, or reset the failure count. Some breakers (resilience4j) still count late results; michi deliberately does not.
- **Cycle detection** is iterative, so a 100,000-node graph no longer overflows the stack.

### Resolved 2026-10-21 — Pipeline execution redesign

`michi-pipeline`'s positional executors (`execute_pipeline`, `execute_pipeline_parallel`, `StepDependencies`) are replaced by a validated `Plan`, one scheduler, a returned `RunRecord` and `resume`. The research behind it is [`docs/research/pipeline-design.md`](../research/pipeline-design.md); its nine ranked changes were approved and all are adopted. The API is [08-pipeline-execution.md](08-pipeline-execution.md). Unpublished, so the old API is removed without deprecation.

- **Identity:** `StepId` and `RunId` are caller-chosen slugs (1 to 64 ASCII letters, digits, `_`, `-`, `.`, no `:`), so agents can read and echo them and `run_id:step_id` is an unambiguous idempotency key. Display names are separate and may repeat. Rejecting an empty id deviates from `PRINCIPLES.md`'s "empty strings are accepted everywhere": an id is an identity, not display text. `RunId::unique` avoids a `uuid` dependency.
- **Definition vs run:** an immutable `Plan` is validated once, reporting every violation, and an invalid plan is an `Err` before anything runs. A run returns a `#[must_use]` `RunRecord` with private fields instead of mutating `Pipeline`. The plan fingerprint (FNV-1a over ids and dependencies, order-independent) binds a record to its plan for resume, since closures cannot be hashed.
- **Status model:** `StepStatus` gains `Running`, `CompletedEarlier`, `OutcomeUnknown(cause)`, `Skipped(reason)`, `NotRun(reason)` and `Panicked`, each mapping to one recovery verb. `Skipped(Cancelled)` means cancelled before taking effect; a step dropped mid-flight is ambiguous. `RunOutcome` is `Succeeded` only if every step succeeded; otherwise `OutcomeUnknown` outranks `Cancelled`, which outranks `Failed`, because verifying comes before resuming. Root causes are separated from consequential skips. `Pipeline::render` gains a `reason` column (`null` when there is none) rather than folding the reason into the status label, so the status vocabulary stays small.
- **Replay:** the four-way rule above is kept and extended to cancellation (`ExecutionError::Interrupted` is ambiguous), timeouts and resume. `Step::is_idempotent() -> bool` becomes `Step::replay() -> Replay`. An ambiguous outcome of a non-idempotent step ends `OutcomeUnknown` and is never replayed automatically; an idempotent step's ends `Failed`, which resume re-runs. A step-returned `CircuitOpen` is still retried after its `retry_after`; the executor's own open breaker ends the step `NotRun` without retry.
- **Resume:** a separate `resume()` runs under the record's `RunId` (keys stay stable), refuses a fingerprint mismatch, never re-runs completed steps, re-runs failed, skipped, not-run and pending ones, and re-runs `OutcomeUnknown`, `Panicked` or `Running` steps only if idempotent or verified by the caller (`Verification::Applied` / `NotApplied`). Treating a panic and a crash-time `Running` as ambiguous goes beyond the report's list, for the same reason as a timeout: the effect may have happened. Documented as at least once per step and not incremental recomputation; exactly-once needs a deduplicating downstream.
- **Scheduling:** one loop on a call-owned `JoinSet` with a remaining-prerequisite count and a ready set ordered by declaration index. `Sequential` is the cap-1 case of the same loop. The `Parallel { max_concurrency: NonZeroUsize }` cap is enforced at dispatch, not with a semaphore inside spawned tasks, so admission order is deterministic. `SkipDependents` is the default, including in sequential mode (a behaviour change); `FailFast` cancels the run token. Panics surface as `JoinError` and become `Panicked`.
- **Cancellation and time:** the in-task race drops an attempt once the run is cancelled and the grace period (default 5 s) has passed, which is how fail-fast's "grace, then abort" is implemented while keeping each step's record. Per-attempt `tokio::time::timeout` (default 30 s) plus an optional total step deadline; backoff sleeps race the token.
- **Breakers and budgets:** a `BreakerRegistry` keyed by `ExecStep::breaker_key` (default: step id), shareable across runs, replaces the single shared breaker, keeping the mutex-and-generation state machine per key. Cancellation and interruption are not breaker failures. A per-run `RetryBudget` (pure, in `michi_core::resilience`, so Node callers can share it later) caps retries at 10% of successes after a reserve of 10.
- **Observer:** `StepObserver` is called synchronously from the scheduler loop on start and on each terminal record. With `StepRecord::in_flight` and `RunRecord::from_parts`, callers can persist progress and resume after a crash.
- **Adopted vs built:** adopted tokio's `JoinSet` and `tokio::time` (already a dependency) and **tokio-util 0.7.20's `CancellationToken`** (new mandatory dependency, tokio-rs maintained, awaitable cancellation and child tokens that an `AtomicBool` cannot give; wrapped in michi's own `CancellationToken` so its 0.x version never appears in the public API). Built: the scheduler (one data structure on `JoinSet`; `buffer_unordered` loses panic isolation and tower's `ConcurrencyLimit` is not a DAG dispatcher), Kahn validation (about 40 lines that report michi-typed violations; petgraph only pays off for SCC reports or DOT export), the retry loop (fused with replay classes, breaker admission, budget, deadline and cancellation; a retry crate would add a second source of backoff truth), the retry budget (modelled on tower's `TpsBudget` without a tower dependency) and the keyed breaker (recloser and failsafe lack generation counters, tower-resilience needs the `Service` model). Avoided: tower `Service` steps, `TaskTracker` (keeps no results), dagrs, saga coordination and persistence.
- **Deliberate limits:** no data passing between steps, no dynamic dependencies, no cross-run concurrency limits, no concise render mode yet. A caller who reuses a `RunId` for a different plan or inputs defeats downstream deduplication; michi cannot detect it.

### Resolved 2026-10-21 — No zero-dependency claim, no WASM target

- **Dependencies:** the default build has always depended on `thiserror` and `compact_str`, so "zero-dep by default" was false. `PRINCIPLES.md` gate 2 now states the real rule: a small default set, everything else behind a feature, and a recorded decision for any new mandatory dependency.
- **WASM:** the `check-wasm` task, its `ci` dependency and the `wasm32-unknown-unknown` toolchain target are removed. No consumer needs a WASM build, and the check cost setup for every contributor. Nothing in `michi-core` depends on the OS, so restoring the check later is a one-line task.

### Resolved 2026-10-21 — TOON output conforms to TOON 4.4

michi called its list format TOON but diverged from the [TOON specification](https://github.com/toon-format/spec/blob/main/SPEC.md) in ways a conforming decoder rejects or misreads. michi now targets `toon-spec: 4.4` (Working Draft, 2026-10-08) with the default options. `tests/toon_spec.rs` covers each applicable encoder rule by section, a strict test-only decoder (`tests/support/toon_parser.rs`) rejects any non-canonical encoder output, and `tests/toon_spec_fixtures.rs` runs the official encode fixtures in michi's subset (vendored from `toon-format/spec` at `d09c3d3`, MIT).

- **Empty list (§9.1, §6):** `issues[0]{}:` → `issues: []`. An empty field list is a syntax error and the legacy `key[0]:` header must not be emitted. `empty_state()` and `Pipeline::render()` follow.
- **Rows without fields (§9.3, §9.4, §10):** rows with zero fields are empty objects, which may not use tabular form, so they render as `key[N]:` followed by N bare `-` items instead of blank rows. Rejecting them was considered; list form keeps the data and needs no new error.
- **Null and empty string (§2, §7.2):** `Value::Null` and a key missing from a serde `list()` item render as `null` instead of an empty cell; the empty string renders as `""`.
- **String quoting (§7.2):** a string is quoted exactly when the spec requires it: empty, leading/trailing space or tab, `true`/`false`/`null`, numeric-looking, contains `:` `"` `\` `[` `]` `{` `}` a comma or a control character, or starts with `-` or `#`. Previously only `,` `"` LF CR triggered quotes, so `-x`, `42`, `true` and `a:b` decoded as other types or structure, and backslashes stayed bare. `PartialSuccess`'s `failed[]` reason column is no longer quoted unconditionally.
- **Escapes (§7.1):** `\t` and `\u00xx` (lowercase hex) for the other C0 controls join `\\ \" \n \r`.
- **Numbers (§2, §3):** NaN and ±Infinity render as `null` instead of `"NaN"`/`"inf"`; `-0` as `0`; floats outside `1e-6 <= |n| < 1e21` use a lowercase signed exponent (`1e+21`) instead of hundreds of digits. Integers above `i64::MAX` are a new `Value::UInt(u64)` variant (from `From<u64>`, `From<usize>` and serde `list()`) rendered as unquoted numbers; as `Value::Str` they were subject to string truncation (`max_cell_len(5)` cut `u64::MAX` to a fragment) and would now have been quoted as numeric-looking strings.
- **Header keys (§7.3):** the type name and field names are emitted bare when they match `^[A-Za-z_][A-Za-z0-9_.]*$` and quoted otherwise, so any name is representable; structural characters are no longer replaced with `_`. New public `toon::escape_key`. Validation still exists but checks what would make the document invalid: `ToonError::InvalidTypeName` now means the type name is a reserved root key (`totalCount` or `help`, which would duplicate a sibling key, §14.3) and `ToonError::InvalidFieldName` means a repeated field name (§14.2). The variants keep their names; their meaning and `Display` text changed.
- **Help block (§9.1, §1.4):** `help[N]:` followed by indented lines was an array header with no valid items. Hints are an array of strings, and §1.4/§13.1 make the form follow from the shape, so the block is the inline primitive array `help[N]: h1,h2` — list form (`  - h1`) would be more readable but is non-conformant. Hints are quoted per §7.2, notably when they contain a comma. `hints::append_hints`/`render_hints` are shared, so the KV, `DomainError`, `Error`, status, empty-state and `render_already_done` help blocks changed the same way.
- **Root document and whitespace (§5, §12):** a list document is a root object (list entry, `totalCount`, `help`), the empty type name is the quoted empty key `""`, and documents end without a trailing newline: `render_toon`, `list`, `ToonDocument::render`, `empty_state(_with_hints)`, `Pipeline::render`, and every `AgentResponse` text output (TOON and KV paths, so the two end the same way; the JSON `body` likewise). Block helpers meant for concatenation — `render_hints`, `append_hints`, `render_kv`, `render_recovery`, `DomainError::render`, `StatusResponse::render`, `PartialSuccess::render` — keep their trailing newline, and `append_hints` starts its block on a new line when the buffer does not end in one.
- **Delimiter (§6, §11):** unchanged — the comma is declared by the absence of a delimiter symbol in the bracket segment, which michi already did.
- **`AgentResponse` validation error body:** the `message:` value is quoted per §7.2, since a `ToonError` message contains `"`.

Deliberate remaining deviations and limits:

- **Nested values in `list()`** become a quoted string of their compact JSON text (a documented §3 host-type normalization) instead of nested TOON objects or field groups; michi emits flat lists only.
- **Truncation** is unchanged: a truncated cell carries the `(N chars truncated — use full=true)` suffix, which changes the data. Only its quoting conforms now (a truncated cell starting with a space is quoted). Truncation is a separate follow-up.
- **Not TOON:** the KV block pads keys for alignment (`key:   value`; §12 requires exactly one space after the colon), does not quote keys or values per §7.2/§7.3, renders floats with fixed decimals and `Missing` as `—`, and ends with a newline. `StatusResponse` and `render_already_done` are KV blocks with the same issues. `DomainError`/`Error::render` is a KV-shaped block whose `message` is unquoted. The `recovery[N]:` block's items (`  tool: suggestedParams: { k: v } — reason`) are not valid list items, and `PartialSuccess` uses it for its help block; its `completed[N]:`/`skipped[N]:` blocks have bare item lines and its summary value contains unquoted commas. Making these TOON means: one space after every colon and §7.2/§7.3 quoting via `escape_value`/`escape_key` (losing KV alignment), canonical numbers and `null` for `Missing`; `completed`/`skipped` as inline arrays; and recovery hints as a tabular array (e.g. `recovery[N]{tool,params,reason}:` with `params` as JSON text) or list-form objects. Left for a maintainer decision, since the alignment and the recovery line format are deliberate readability choices.
- **Supersedes** the "TOON hints" bullet of "Line breaks in caller text are escaped, never passed through or dropped" above: hints are now quoted per §7.2 instead of keeping punctuation bare, and header tokens are quoted instead of having structural characters replaced.

### Resolved 2026-10-21 — Truncation reports what was cut and never invents a hint

- **Count:** the suffix is `" ({removed} more chars — use {hint})"`, where `removed` is the input's char count minus the chars kept. The old `" ({original} chars truncated — …)"` gave the original length while part of it was still shown, so an agent could not tell how much was missing. The kept count is chosen with the suffix's own digits included, so the result stays within `max_chars`.
- **Hint:** the `— use {hint}` clause is the caller's instruction. `truncate()`/`truncate_inline()` keep their `&str` parameter (the NAPI `truncate` export is unchanged) and omit the clause for an empty hint. The TOON renderer no longer hard-codes `full=true`, which named a parameter the calling tool may not have: `ToonOptions::truncation_hint` and `AgentResponse::truncation_hint` (NAPI `truncationHint`) default to none, and without one the suffix only reports the count. This finishes the "`truncate()`/`truncate_inline()` take `hint: &str`" decision above, which the renderer had bypassed.
- **Tiny caps:** when the full suffix plus one kept char does not fit, the result is the first `max_chars - 1` chars and `…` (`…` alone at 1, empty at 0) instead of a hard-capped fragment such as `" (500 chars tr"`. `was_truncated` stays `true`; `signal` is `"…"` (or `None` at 0).
- **Escaping:** `max_cell_len` / `truncate_cells_at` count chars before TOON escaping and quoting, so an escaped or quoted cell can be longer than the cap. Kept deliberately: the cap bounds the content an agent reads, and counting the escaped form would make the kept text depend on how many quotes or line breaks it holds.
- **Supersedes** the "Truncation" bullet of "TOON output conforms to TOON 4.4" above.

### Resolved 2026-10-21 — Every agent-facing block is TOON 4.4

The "Not TOON" bullet of "TOON output conforms to TOON 4.4" left the KV, status, already-done, error, recovery and partial-success blocks as michi's own line formats. An agent then had to parse two formats, and the recovery and partial-success blocks were not even consistent with each other. Every block is now TOON 4.4 encoder output (default options), checked by `tests/toon_blocks.rs` against the strict validator in `tests/support/toon_parser.rs`. TOON §1.4 makes the form follow from the value's shape and position, so each choice below is a choice of data model; the form is then the one the spec prescribes for it.

- **Composition rule:** every renderer returns a finished document with no trailing newline (§12), and `append_hints`/`append_recovery` join a block onto a document with one LF (none when the buffer is empty or already ends in one). The previous rule — documents without, blocks with a trailing newline — made `render_kv() + render_hints()` valid but `AgentResponse` had to pop and re-add newlines, and a block on its own was not a valid document. A composed document is one root object; keys stay unique because the composed keys `totalCount`, `help` and `recovery` are reserved (next bullet). Supersedes the block-helper sentence of the "Root document and whitespace" bullet above.
- **Reserved root keys:** `totalCount`, `help` and `recovery` are rejected as a list type name (`ToonError::InvalidTypeName`, `recovery` is new) and as KV keys, whether or not the document actually contains that sibling, so validity never depends on the data. Considered: rejecting only when the sibling is present — rejected as data-dependent.
- **KV (`render_kv`):** a root object of primitive fields (§8). One space after each colon, losing column alignment (§12 allows no other spacing); keys quoted per §7.3 and text per §7.2. `Missing` → `null` (§2), distinct from `""`. `Duration` stays the string `1.5s` — it is a unit-bearing string, and needs no quotes. **Float:** `KvValue::Float(f, decimals)` rounds `f` to `decimals` places (the correctly rounded `{f:.decimals}`), then writes the rounded value canonically (§2): `decimals` bounds the precision but never pads, so `Float(1.5, 3)` is `1.5` and `Float(-0.001, 2)` is `0`. Keeping the caller's digits verbatim would emit trailing zeros (`1.500`), which §2 forbids; emitting the unrounded value canonically would ignore the caller's precision. NaN/±Infinity → `null` (§3). The JSON companion (`kv_value_to_json`) is unchanged. A repeated key cannot be represented (§14.3), so `render_kv` now returns `Result<String, KvError>` (`DuplicateKey`, `ReservedKey`); deduplicating would drop data silently. `AgentResponse`'s KV path turns the error into an `error: kv_validation_failed` body with `is_error`, mirroring `toon_validation_failed`.
- **Recovery hints:** an array of records `{tool, params, reason}`, rendered as the tabular array `recovery[N]{tool,params,reason}:` (§9.3) with `params` a string holding the compact JSON object (the same writer and text as the JSON companions' `params`, `"{}"` when empty) and `reason` `null` when absent. Every element then has the same keys and only primitive columns, so §9.3 prescribes exactly this table for every input. Considered: `params` as a nested object — §1.4 would then make the form depend on the data (a nested-field-group table `{tool,params{project,type},reason}` when every hint has the same param keys, list-form objects per §10 when params or reason differ or a hint has no params, since empty objects disqualify tabular form), so agents would see three shapes for one block; list-form objects for every input are not an option, as a conforming encoder must use the table when detection succeeds. JSON text in a cell costs `\"` escapes but is unambiguous, typed, and the same convention as `toon::list()`'s nested values (§3 host-type normalization). `DomainError`, `AgentResponse` and `PartialSuccess` all append this one block.
- **Status (`StatusResponse::render`):** `tool` and `description` fields, then the table `items[N]{key,value,health,reason}:` (`items: []` when empty), then `help`. Item keys become cells instead of root keys, so a status item may be named anything — `help`, `tool`, or a repeated key — without breaking §14.3 and without a fallible API. `health` is `ok`, `degraded`, `error`, or `null` for an item with no health signal; `reason` carries the degraded/error reason, else `null`. The old `value  [DEGRADED: reason]` annotation changed the value's text and type. `Health::Ok` and `None` now render differently (`ok` vs `null`), superseding the old "Ok renders byte-identical to None" criterion: `null` says no signal was given, which `ok` would misstate. Considered: separate root fields per item (`cache: warm`, `cache_health: degraded`) — rejected, it needs key mangling and can still collide.
- **Errors:** `DomainError::render` is a root object `error`, `message` (quoted per §7.2), `exit_code`, then `help` and the `recovery` table. `Error::InvalidInput(m)` / `Error::NotFound(m)` use the same shape with `error: invalid_input` / `error: not_found` and `message: m`, instead of `error: Invalid input: m` (whose value needed quoting for its colon anyway), so every error an agent sees has one schema. `Display` is unchanged.
- **Partial success:** the summary stays one field, quoted because of its commas (`partial_success: "2 completed, 1 failed, 1 skipped"`) — restructuring it into numeric fields would duplicate the array lengths. `completed`/`skipped` are inline primitive arrays (§9.1, the form §1.4 prescribes for strings); `failed` stays the `{operation,reason}` table; recovery hints are the shared `recovery` table instead of a `help[N]:` block, since `help` is the array of strings everywhere else. Empty categories are still omitted.
- **Already done:** `operation`, `status: already_done`, `summary` (quoted per §7.2), `help`.
- **Line breaks:** every caller string now goes through §7.2 quoting and §7.1 escaping, so the CR/LF-only escape (`text::escape_line_breaks`) is gone; a line break can still never start a new line. A recovery param key or value is escaped twice — JSON, then TOON — and round-trips through both decoders (`tests/line_injection.rs`).
- **Supersedes** the "Not TOON" bullet and the block-helper sentence of the "Root document and whitespace" bullet of "TOON output conforms to TOON 4.4" above, and the `primitives#38` harness limit: serde_json's `float_roundtrip` is enabled for michi-core's `serde` feature (and its dev-dependency), so the fixture runs.

### Resolved 2026-10-21 — Capacity limits

A capacity investigation found inputs that cost far more than their size suggested. Measured on an Apple Silicon laptop, release builds:

| Case | Before | After |
| --- | --- | --- |
| `ToonOptions::validate` with 50,000 fields (O(F²) duplicate scan) | 1.12–1.35 s | rejected in ~1 µs (`TooManyFields`); 1,000 fields validate in ~14 µs (was ~0.7 ms) |
| serde `list()` over 10,000 items with one distinct key each (10k × 10k table) | 2.84 GiB | rejected in ~1.3 ms after the key-union pass (`TooManyFields`) |
| 1 MB `truncation_hint` × 100,000 truncated cells (hint counted per cell) | 3.0 s | rejected in validation (`TruncationHintTooLong`); a 256-char hint over 100k truncated cells renders in ~28 ms |
| NAPI `renderToon` / `AgentResponse.items` rejecting 100,001 × 100 rows | 3.0–3.4 s, 2.4–2.6 GiB | ~0.1 ms, no allocation |
| NAPI 10k × 10 cells sharing one 100 KB JS string | 1.6 s, +11 GiB RSS (copied per cell) | rejected in ~11 ms at 64 MiB (`input byte total …`) |
| `AgentResponse::is_error()` on 1,000,000 cells (cloned rows and rendered) | ~14 ms | ~30 µs (validates only) |
| `michi_pipeline::resume` over 50,000 carried-over steps (linear `prior.step()` per step) | 1.6–2.4 s | ~21 ms |
| `Plan::new`, one step depending on 50,000 others (O(deg²) dedup) | ~107 ms | ~21 ms |

Decisions:

- **Core shape caps (michi-core):** `toon::MAX_FIELDS` = 1,000 header fields, `toon::MAX_CELLS` = 1,000,000 cells (`rows × fields`), `toon::MAX_TRUNCATION_HINT_CHARS` = 256 chars, enforced by `ToonDocument::validate` as `ToonError::TooManyFields`, `TooManyCells` and `TruncationHintTooLong` (`ToonError` is `#[non_exhaustive]`, so adding variants is not breaking). 1,000 fields matches the NAPI per-row cap that already existed; 1,000,000 cells is far beyond any list an agent should read in one response but keeps a worst-case render in the tens of milliseconds; 256 chars is ample for an instruction like `full=true` or `--no-truncate`. Duplicate fields are found with a set. `list()` checks the field and cell caps right after the key-union pass, before materializing rows. Considered: no cap, documenting the cost — rejected, since one oversized call blocks a server's thread and a sparse `list()` input is easy to produce by accident.
- **Render path:** `AgentResponse` stores its TOON slot as `ToonOptions` and renders by reference instead of cloning rows per render; `is_error()` validates without rendering; hints and recovery are appended to the body in place. The truncation hint's char count is measured once per document. Output is byte-identical for valid input.
- **NAPI budgets:** exports that take collections (`renderToon`, `renderHints`, `appendHints`, `renderRecovery`, `renderKv`, `renderAlreadyDone`, `renderDomainError`, `renderStatus`, `AgentResponse.items`/`kvItems`) take untyped napi values and convert incrementally, because napi-derive converts typed parameters in full before the body runs. Lengths are checked first; then each call gets 1,000,000 cells and 64 MiB of string bytes, with 1 MiB per cell, hint, reason, description or message and 4 KiB per name (type name, field name, key, tool, code, operation, truncation hint). A string's UTF-8 length is charged before it is copied, so shared JS strings cannot amplify. The single-string `AgentResponse` setters keep `String` parameters (napi copies their one argument exactly once) and apply the same caps after the copy. `index.d.ts` declares the same types (`ts_args_type`); the Rust-typed functions keep their names for Rust callers. Considered: checking lengths in the body of the typed exports — rejected, the conversion has already happened by then.
- **Pipeline:** `resume` indexes the prior record once (first record wins, as with `RunRecord::step`); `Plan::new` dedups dependencies with an O(1) stamp per edge. `RunRecord::step` stays a linear scan, documented as such, rather than adding a cached index to a serialized, `PartialEq` type.
- **Compatibility:** previously accepted oversize inputs are now rejected, so this ships as a minor release of the fixed `michi` group.

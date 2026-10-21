# Decisions

Why things are the way they are. If a choice in the code looks arbitrary, the reasoning is probably here.

## Open questions

Tracked by what they block, not by sequence number — "Q1/Q2/Q3" reads too easily as calendar quarters, which these aren't.

### Open until validated — TOON vs. Markdown-KV retrieval accuracy

**Blocks:** recommending TOON as the default for MCP consumers

- AXI benchmarks the token-efficiency gains at the CLI/MCP level, but not the downstream LLM retrieval accuracy of TOON vs. Markdown-KV for list data.
- Before treating TOON as universally superior for the `audience: ["assistant"]` surface, run a retrieval accuracy experiment: same list data, TOON vs. Markdown-KV, a retrieval task, across a few model sizes.
- If Markdown-KV wins on accuracy despite the higher token cost, the right call for MCP consumers may be Markdown-KV even for lists.
- The crate ships either way — the open question is which format callers should default to.

### Open until a real integration exists — crates.io publish readiness

**Blocks:** crates.io release

For external Rust binaries to depend on `michi-core` (and `michi-pipeline`):

- **(a) crates.io publish** — cleanest for public consumers once the API is stable.
- **(b) git dep with tag** — `michi-core = { git = "https://github.com/orin-dx/michi", tag = "v0.x.y" }`, good for early adopters before crates.io publish.
- **(c) private cargo registry** — overhead not worth it.

**Recommendation:** (b) during development, (a) at first stable release. crates.io publish is gated behind at least one real consumer integration test.

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
- **Why `michi-pipeline` stays separate:** the existing dependency-weight rule in "A separate crate is justified by dependency weight and coupling" still applies — it pulls in Tokio, which the zero-dep default build must not. It depends on `michi-core` through an exact `=` version pin so a mixed set never resolves.
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
- **Deserializing `RecoveryHint` is best-effort for non-text values:** a `Duration` parameter serializes as a string such as `"1.5s"` and reads back as `Text`, and a float takes its decimal places from the JSON number.

### Resolved 2026-10-21 — `michi-pipeline` retry and breaker semantics

- **Retry eligibility:** a timeout is an ambiguous outcome — the attempt may already have committed — so the retry loop re-invokes a step after a timeout only if the step opts in with `Step::is_idempotent()` (default `false`; `FnStep::idempotent()` for closures). Errors the step classifies itself still follow `is_retryable()`. A 504 the step returns is still trusted as the step classifies it; whether it should also need the opt-in is open.
- **Duplicate ids:** `execute_pipeline_parallel` rejects any repeated step id up front. `execute_pipeline` (sequential) still accepts them, as an existing test pins; whether to align it is open.
- **Breaker state:** phase, probe ownership and failure count are one mutex-guarded state machine with a generation counter. An outcome from a call admitted under an earlier generation is ignored: it cannot reopen a recovered breaker, extend the open window, or reset the failure count. Some breakers (resilience4j) still count late results; michi deliberately does not.
- **Cycle detection** is iterative, so a 100,000-node graph no longer overflows the stack.

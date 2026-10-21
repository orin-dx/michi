# Scope, Versioning, and Quality

## What michi formalizes vs. what stays in your app

### Conventions that are usually implicit, now formal

| Concept | Where it usually lives | Provided as |
| --- | --- | --- |
| Contextual hints after output | Implicit strings per tool | `hints::Hint`, `render_hints()`, `append_hints()` |
| Total count on list responses | Number passed ad hoc | `ToonOptions::total_count`, `totalCount:` line |
| Recovery hints on failure | Ad hoc per server/tool | `recovery::RecoveryHint`, `render_recovery()` |
| Retry delay calculation | Inside caller's retry loop | `resilience::next_retry_delay()` |
| Retry-After header parsing | Ad hoc string parsing | `resilience::parse_retry_after()` |
| Retryable status classification | Hardcoded booleans per caller | `resilience::is_retryable_status()` |
| Error classification shape | Ad hoc per tool | `error::Error`, `error::DomainError`, `error::ErrorCode` |
| Empty state handling | Ad hoc per tool (often silent) | `empty::empty_state()` |
| Truncation | Ad hoc per tool, no standard signal | `truncate::truncate()`, `truncate_inline()` |

### Genuinely new — no shared primitive existed before this

| What | Why |
| --- | --- |
| TOON renderer | New format — token-efficient list encoding for agent context windows |
| `kv::render_kv()` | Single-item Markdown-KV with aligned columns; no standard Rust equivalent |
| `status::StatusResponse` | Typed P8 content-first orientation response |
| `idempotency::already_done()` | Typed already-done signal with canonical output format |
| `idempotency::PartialSuccess` | General partial-success reporting with per-op recovery hints |
| `idempotency::IdempotencyKey` | Canonical key construction with stable hashing |
| `AgentResponse` builder | One construction API across every consumer |
| NAPI wrapper | Cross-language bridge — TypeScript authors never write Rust |
| Formal TOON grammar | A canonical spec enabling interoperability and testability |
| `render_hints_only()` | Append hints to an existing body without re-rendering |
| `mcp::CallToolResult` + `AgentResponse::to_call_tool_result()` | Assembles a wire-conformant MCP `tools/call` response from an already-built `AgentResponse` |
| `AgentResponse::render_for()` + `has_human_content()` | Dual agent/human output selection for any consumer, not just MCP — no prior shared primitive covered CLI output mode selection |

### Stays in your application code

| What | Why |
| --- | --- |
| Display Markdown formatters | Display-surface formatting; `audience: ["user"]` |
| Full MCP protocol (JSON-RPC, registration, bootstrapping) | Protocol knowledge beyond struct assembly |
| `outputSchema` wiring | Tied to your schema validation library |
| Tool annotations | MCP SDK concern |
| MCP server bootstrapping | Protocol-specific |
| HTTP client + auth | Too many deployment shapes to standardize |
| LRU cache | General utility, not AXI-specific |
| Full async retry loop | Requires an async runtime |
| Structured logging | `tracing` or similar — your choice |
| Schema validation | Tied to your type system |

---

## Feature flags

```toml
[features]
default  = []
napi     = ["dep:napi", "dep:napi-derive", "dep:serde_json", "michi-core/serde"]
serde    = ["michi-core/serde", "michi-toon/serde"]
schemars = ["michi-core/schemars", "michi-toon/schemars"]
miette   = ["michi-core/miette"]
```

No async runtime dependency, no tokio, no async-std — every public function is sync.

- `serde` is opt-in Rust-side ergonomics: `Serialize`/`Deserialize` on the core value types, plus `toon::list()`. See [01-overview-and-setup.md](01-overview-and-setup.md) for the full dependency rationale.
- `schemars` derives `JsonSchema` on DTO types (`DomainError`, `StatusResponse`, `ToonOptions`, and others) for automatic MCP `outputSchema` generation.
- `miette` implements `miette::Diagnostic` for `DomainError` to render colorized CLI diagnostic cards.
- `cli` is **not** a Cargo feature of this crate at all. Terminal-aware rendering (line wrapping, colour codes for the `[DEGRADED: ...]` health signals in `status::StatusResponse`) is out of scope for v1, since michi v1 targets agent consumers only.
- When that work is actually built, it lands as its own crate, downstream of `pipeline`, rather than a feature flag here — see [ARCHITECTURE.md](../../ARCHITECTURE.md) and [06-decisions.md](06-decisions.md) for the crate-boundary rule and the v2 scope sketch.

---

## Versioning and release

- Intended to publish to [crates.io](https://crates.io) as `michi` — not yet published
- Intended to publish the npm package to [npmjs.com](https://npmjs.com) as `@orin-axi/michi` — not yet published
- NAPI binary cross-compiled via `cargo-zigbuild`:
  - `darwin-arm64` (`aarch64-apple-darwin`)
  - `linux-x64-musl` (`x86_64-unknown-linux-musl`)

**SemVer contract:**

| Bump      | Meaning                                                       |
| --------- | ------------------------------------------------------------- |
| **Patch** | Bug fixes; TOON output identical or more correct              |
| **Minor** | New API surface (new module, new method on `AgentResponse`)   |
| **Major** | TOON format change; any existing rendered output would differ |

Format changes are major versions. Treat the rendered string as a contract — the snapshot tests exist to make accidental format drift a failing build, not a silent breaking change.

**MSRV policy:** `rust-version = "1.96"` is the declared minimum. A bump happens only when a needed language or `std` feature requires it, gets recorded in the CHANGELOG, and counts as a **minor** bump — never a patch.

**Version sync:** the npm package version tracks the crate version exactly.

- A `version-sync` CI job asserts the two are equal on every push/PR, so a `michi` crate at `0.3.1` and the `@orin-axi/michi` npm package at `0.3.1` always describe the same source.
- `.github/workflows/release.yml` uses Callisto to open version PRs and publish after a verified merge. The first release establishes versioned packages for downstream integration; consumer integration is a post-release validation, not a publication prerequisite (see [ADR 4](../adr/0004-publish-before-consumer-integration.md)).
- When no native binary matches a consumer's platform, the TypeScript fallback export loads instead.

---

## Performance contract

| Operation                              | Target  | Basis                               |
| -------------------------------------- | ------- | ----------------------------------- |
| `render_toon()` — 100 items, 4 fields  | < 500µs | Simple string allocation, no I/O    |
| `render_toon()` — 1000 items, 4 fields | < 3ms   | Linear in N×fields                  |
| `render_kv()` — 10 items               | < 20µs  | Column-width scan + string join     |
| `render_hints()` — 5 hints             | < 10µs  | Trivial string join                 |
| `truncate_inline()`                    | < 5µs   | Single pass, char boundary safe     |
| `parse_retry_after()`                  | < 2µs   | String parse only                   |
| `next_retry_delay()`                   | < 1µs   | Pure arithmetic                     |
| NAPI boundary overhead                 | < 5µs   | Established from napi-rs benchmarks |

**Allocation:**

- `render_toon()` pre-allocates via `String::with_capacity` before any writes (see [06-decisions.md](06-decisions.md) for the exact heuristic).
- Every function is allocation-bounded — no references held across call boundaries.
- `escape.rs` avoids heap allocation for the common case (no special chars in a cell).

**Thread safety:**

- Every public function is a stateless pure function.
- `AgentResponse` is `Send + Sync` (all fields owned, no interior mutability).
- The NAPI wrapper uses napi-rs's safe threading model.

Benchmarks run via `divan`, in CI, on any PR touching `src/`.

---

## Testing strategy

**Unit tests** (in-crate, `#[cfg(test)]`):

- TOON grammar: one test per grammar production
- KV: alignment, unicode keys, missing values, every `KvValue` variant
- Escaping: commas, quotes, null, empty string, unicode
- Truncation: exact boundary, under, over, unicode char boundary safety
- Hints: empty, single, multiple, long strings
- Empty state: with and without hints
- Recovery: single, multiple, with/without params and reason
- Error: every `ErrorCode` variant — render, exit code, `Display`
- Idempotency: `already_done` format, `PartialSuccess` with mixed results
- Resilience: `parse_retry_after` (integer seconds, HTTP-date, malformed); `next_retry_delay` (backoff progression, jitter bounds, `Retry-After` respect); `is_retryable_status` (covered and uncovered codes)
- Status: every health state, degraded rendering, error rendering
- `AgentResponse` builder: every method combination, correct format dispatch

**Property tests** (`proptest`, `tests/`):

- `render_toon()` output is grammar-valid for arbitrary string inputs
- `truncate_inline()` never returns a string longer than `limit + signal_len`
- TOON round-trip: render → parse (test-only parser) → compare values
- `parse_retry_after()` never panics on arbitrary strings
- `next_retry_delay()` always returns a value within `[initial_delay, max_delay]`

**Snapshot tests** (`insta`, `tests/snapshot_tests.rs`):

- Canonical TOON examples from this spec — exact byte-for-byte match
- KV column alignment across different key lengths
- Status response with mixed health signals
- Format stability, to catch accidental drift across versions

**NAPI integration tests** (Node's built-in `node:test`, `packages/michi-node/__test__/`):

- Every NAPI export with representative inputs
- Error cases: mismatched row lengths, oversized inputs, null values
- `AgentResponse` builder via NAPI, every path — including the `Option`/`take` "already rendered" guard
- Platform binary loading on the CI matrix (darwin-arm64, linux-x64-musl)

**Consumer integration tests:**

- MCP consumer: `audience: ["assistant"]` block is valid TOON
- CLI consumer: `--format toon` output is valid TOON

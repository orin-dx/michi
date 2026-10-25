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
| Truncation | Ad hoc per tool, no standard signal | `truncate::truncate()`; for lists, `TruncationOutcome` and `AgentResponse::truncation()` |

### Genuinely new — no shared primitive existed before this

| What | Why |
| --- | --- |
| TOON renderer | New format — token-efficient list encoding for agent context windows |
| `kv::render_kv()` | Single item as TOON `key: value` lines; no standard Rust equivalent |
| `status::StatusResponse` | Typed P8 content-first orientation response |
| `resilience::already_done()` + `render_already_done()` | Typed already-done signal with canonical output format |
| `idempotency::PartialSuccess` | General partial-success reporting with per-op recovery hints |
| `resilience::IdempotencyKey` | Canonical key construction with stable hashing |
| `AgentResponse` builder | One construction API across every consumer |
| NAPI wrapper | Cross-language bridge — TypeScript authors never write Rust |
| Formal TOON grammar | A canonical spec enabling interoperability and testability |
| `render_hints_only()` | Just a response's `help[N]` block, to append to a body rendered elsewhere |
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

All feature flags live on `michi-core` (`crates/michi-core/Cargo.toml`):

```toml
[features]
default = []
serde = ["dep:serde", "dep:serde_json", "serde_json/float_roundtrip", "compact_str/serde"]
schemars = ["dep:schemars", "serde"]
miette = ["dep:miette"]
napi = ["dep:napi", "dep:napi-derive", "serde"]
```

No async runtime dependency, no tokio, no async-std — every public function is sync.

- `serde` is opt-in Rust-side ergonomics: `Serialize`/`Deserialize` on the core value types, plus `toon::list()`. See [01-overview-and-setup.md](01-overview-and-setup.md) for the full dependency rationale.
- `schemars` (implies `serde`) implements `JsonSchema` on the types serde serializes (`DomainError`, `CallToolResult`, `ContentBlock`, `RecoveryHint`, `KvValue`, `toon::Value`, the pipeline status types and others), each describing its serde wire form, for MCP `outputSchema` generation. It uses schemars 1.x, so schemas are JSON Schema 2020-12 (`$defs`, `prefixItems`, `const`). Render-only types such as `StatusResponse` and `ToonOptions` have no schema.
- `miette` implements `miette::Diagnostic` for `DomainError` to render colorized CLI diagnostic cards.
- `cli` is **not** a Cargo feature of `michi-core` at all. Terminal-aware rendering (line wrapping, colour codes for the `degraded`/`error` health signals in `status::StatusResponse`) is out of scope for v1, since michi v1 targets agent consumers only.
- When that work is actually built, it lands as its own crate, downstream of `pipeline`, rather than a feature flag here — see [ARCHITECTURE.md](../../ARCHITECTURE.md) and [06-decisions.md](06-decisions.md) for the crate-boundary rule and the v2 scope sketch.

---

## Versioning and release

- Published to [crates.io](https://crates.io) as three crates, `michi-core`, `michi-pipeline` (first release 0.3.1) and `michi-graph` (first release 0.5.0). The name `michi` on crates.io belongs to an unrelated project
- Published to [npmjs.com](https://npmjs.com) as `@orin-dx/michi`, with one native package per platform
- NAPI binaries for six targets, built natively or cross-compiled with `cargo-zigbuild` (see [04-mcp-and-napi.md](04-mcp-and-napi.md)):
  - macOS: `darwin-arm64`, `darwin-x64`
  - Linux glibc: `linux-x64-gnu`, `linux-arm64-gnu`
  - Linux musl: `linux-x64-musl`, `linux-arm64-musl`

**SemVer contract:**

| Bump      | Meaning                                                       |
| --------- | ------------------------------------------------------------- |
| **Patch** | Bug fixes; TOON output identical or more correct              |
| **Minor** | New API surface (new module, new method on `AgentResponse`)   |
| **Major** | TOON format change; any existing rendered output would differ |

Format changes are major versions. Treat the rendered string as a contract — the snapshot tests exist to make accidental format drift a failing build, not a silent breaking change.

**MSRV policy:** `rust-version = "1.93"` is the declared minimum. A bump happens only when a needed language or `std` feature requires it, gets recorded in the CHANGELOG, and counts as a **minor** bump — never a patch.

**Third-party types in the public API:** a few public types expose dependency or representation details. `toon::Value::Str` holds a `compact_str::CompactString`, so a `compact_str` major bump is a michi breaking release. `StepContext::deadline()` and `attempt_deadline()` in `michi-pipeline` return `tokio::time::Instant`. `Hint`, `IdempotencyKey` and `Sensitive<T>` have public tuple fields (`Hint(pub String)`, `IdempotencyKey(pub String)`, `Sensitive<T>(pub T)`), so their representation is part of the API. This is a known pre-1.0 limitation to revisit before 1.0, not a promise.

**Version sync:** `michi-core`, `michi-pipeline`, `michi-graph` and the npm package share one version (each crate's own `version`, written by Callisto), released in lockstep as a Callisto fixed group named `michi`. `michi-pipeline` depends on `michi-core` and `michi-graph` through exact `=` pins, so a mixed set never resolves.

- A `version-sync` CI job asserts on every push/PR that the three crates' versions and the npm package version are equal, so `michi-core` at `0.7.0` and the `@orin-dx/michi` npm package at `0.7.0` always describe the same source.
- `.github/workflows/callisto-release.yml` uses Callisto to open version PRs and publish after a verified merge. Consumer integration is a post-release validation, not a publication prerequisite (see [ADR 4](../adr/0004-publish-before-consumer-integration.md)).
- When no native binary matches a consumer's platform, loading the package throws; there is no JavaScript fallback.

---

## Performance contract

| Operation                              | Target  | Basis                               |
| -------------------------------------- | ------- | ----------------------------------- |
| `render_toon()` — 100 items, 4 fields  | < 500µs | Simple string allocation, no I/O    |
| `render_toon()` — 1000 items, 4 fields | < 3ms   | Linear in N×fields                  |
| `render_kv()` — 10 items               | < 20µs  | Key check + one string pass         |
| `render_hints()` — 5 hints             | < 10µs  | Trivial string join                 |
| `truncate()`                           | < 5µs   | Single pass, char boundary safe     |
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

Benchmarks live in `crates/michi-core/benches/` and run via `divan`; CI builds them on every push/PR (`cargo bench --no-run --workspace`).

---

## Testing strategy

**Unit tests** (in-crate, `#[cfg(test)]`):

- TOON grammar: one test per grammar production
- KV: one space after the colon (no alignment), unicode keys, missing values, every `KvValue` variant, reserved and duplicate keys
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
- `truncate()` never returns a string longer than `max_chars` chars, and `removed` plus the kept chars equals the input char count
- TOON round-trip: render → parse (test-only parser) → compare values
- `parse_retry_after()` never panics on arbitrary strings
- `next_retry_delay()` always returns a value within `[base_delay, max_delay]`

**Snapshot tests** (`insta`, `tests/snapshot_tests.rs`):

- Canonical TOON examples from this spec — exact byte-for-byte match
- KV items with mixed value types and key lengths
- Status response with mixed health signals
- Format stability, to catch accidental drift across versions

**NAPI integration tests** (Node's built-in `node:test`, `packages/michi-node/__test__/`):

- Every NAPI export with representative inputs
- Error cases: mismatched row lengths, oversized inputs, null values
- `AgentResponse` builder via NAPI, every path — including the `Option`/`take` "already consumed" guard
- Platform binary loading on the CI matrix, on every target whose runner can execute the binary (darwin-arm64, linux-x64-gnu, linux-arm64-gnu)

**Consumer integration tests:**

- MCP consumer: `audience: ["assistant"]` block is valid TOON
- CLI consumer: `--format toon` output is valid TOON

# MCP and the NAPI/npm Boundary

> Targets MCP spec version **2025-11-25** (the current stable release). A release candidate for **2026-07-28** exists — described by its own announcement as "the largest revision since launch" — but hasn't shipped yet. Nothing in this doc should be read as covering the RC; revisit this note once it's finalized.

## `mcp` module

Always compiled — no feature gate, no new dependency in the default build. Owns exactly one thing: turning an already-built `AgentResponse` into the shape MCP's `tools/call` response expects.

```rust
// Audience lives in michi-core's src/audience.rs — not MCP-specific, michi's CLI output
// path uses it too (see 03-rust-api.md's render_for()/has_human_content()).
pub struct ContentBlock {
    pub text: String,
    pub audience: Vec<Audience>,   // MCP's annotations.audience is an array
}

pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    pub is_error: bool,
    pub structured_content: String,   // JSON text; see field doc for what's actually structured
}
```

`audience` is a `Vec`, not a scalar.

- MCP's real `annotations.audience` is an array — one block can target more than one audience.
- michi always populates exactly one element per block today, but the type matches the wire shape, so no translation is needed at the serialization boundary.

Under the `serde` feature, `ContentBlock`/`CallToolResult` don't derive `Serialize`/`Deserialize` directly onto their Rust-shaped fields.

- A naive derive would emit `is_error`/`structured_content` (snake_case) and a bare `audience` — neither valid MCP JSON.
- Each type instead converts through a private `*Wire` struct (`serde(into = "...", from = "...")`) that:
  - adds the `"type": "text"` discriminator
  - nests `audience` under `annotations`
  - renames fields to camelCase
  - for `CallToolResult`, parses `structured_content` into a real embedded JSON value rather than a double-encoded string
- `Audience` itself serializes lowercase (`"assistant"`/`"user"`), matching MCP's `Role` type.

The wire rules, for output and input:

- **Output:** `structuredContent` is the parsed JSON object and is omitted when `structured_content` is empty. MCP requires an object there, so serializing a `CallToolResult` whose `structured_content` is neither empty nor the text of a JSON object fails with a serde error (no `null`, array or double-encoded string is ever emitted). `CallToolResult::try_new` runs the same check up front and returns `StructuredContentError::InvalidJson` or `NotAnObject`.
- **Input is tolerant where MCP is:** only `content` is required. `isError` defaults to `false`; an absent or `null` `structuredContent` becomes the empty string; `annotations` and `annotations.audience` are optional and default to no audience; other annotation fields (such as `priority`) are ignored. Still rejected: a `structuredContent` that is not an object, and a content block whose `type` is not `"text"`.

`AgentResponse::to_call_tool_result()` (see [03-rust-api.md](03-rust-api.md)) is the only intended constructor. It builds:

- the primary `assistant`-audience block from whichever of `render_toon()`/`render_kv()` is active
- an optional second `user`-audience block from `human_content()`, if the caller set one
- `structured_content` from `render(OutputFormat::Json)`

---

## NAPI npm package — `@orin-dx/michi`

A thin napi-rs wrapper around `michi-core`, built with its `napi` feature enabled:

- The `#[napi]` exports live in `crates/michi-core/src/napi.rs`, compiled only with `michi-core`'s `napi` feature; `packages/michi-node` is a `publish = false` cdylib whose `src/lib.rs` is `pub use michi_core::napi::*;`
- `napi-rs` with `napi-derive` proc-macros — clean Rust in, C-ABI glue and TypeScript types out, generated at compile time
- Typed `#[napi(object)]` structs (`JsToonValue`, `JsKvItem`, ...) for the dynamic FFI boundary (cell values, recovery params) — kept as tagged Rust enums converted by hand, not `serde_json::Value`. `serde_json` does enter the tree once `napi` is enabled (for `toCallToolResult()`'s typed `structuredContent`), but the two are independent choices — `JsToonValue`/`JsKvItem` stay their own explicit shape because a `Value` would silently accept malformed input instead of matching a documented variant.
- Platform-aware binary loading via the generated `index.js`
- TypeScript fallback export for environments without the native binary
- Six published targets, one `packages/michi-node/npm/<platform>` package each (`napi.targets` in `package.json`):
  - `aarch64-apple-darwin` (`darwin-arm64`)
  - `x86_64-apple-darwin` (`darwin-x64`)
  - `x86_64-unknown-linux-gnu` (`linux-x64-gnu`)
  - `x86_64-unknown-linux-musl` (`linux-x64-musl`)
  - `aarch64-unknown-linux-gnu` (`linux-arm64-gnu`)
  - `aarch64-unknown-linux-musl` (`linux-arm64-musl`)

### Why `napi` v3 / the `napi6` feature

napi-rs gates capabilities behind Node-API version features. This crate targets `napi` v3 with the `napi6` feature.

- **Why v3:** it dropped the Docker-image requirement v2 had for cross-compilation — that's the actual reason for the move (see [06-decisions.md](06-decisions.md)).
- michi's exports are synchronous pure functions either way, so the async machinery either feature level gates isn't something michi exercises. The choice is about build tooling, not a capability michi needs.

### The builder-boundary problem

This is the one genuinely hard part of wrapping michi in napi-rs, worth spelling out. The Rust `AgentResponse` is a **consuming** builder — every setter takes `self` by value and returns `Self`:

```rust
pub fn items(mut self, rows: Vec<Vec<Value>>, fields: &[&str]) -> Self
```

That idiom can't cross the napi boundary directly.

- A `#[napi]` class instance is owned by the JavaScript garbage collector.
- Rust only ever gets `&self` or `&mut self`, never ownership — because a live JS reference still points at the underlying struct.
- `pub fn items(self, ...) -> Self` simply isn't writable on a napi class.

The fix: store the consuming Rust builder in an `Option` slot, and `take()` it out on each mutation — apply the consuming method, put the result back.

```rust
#[napi(js_name = "AgentResponse")]
pub struct JsAgentResponse {
    inner: Option<michi_core::AgentResponse>,
}

#[napi]
impl JsAgentResponse {
    #[napi(constructor)]
    pub fn new(type_name: String) -> Self {
        Self { inner: Some(michi_core::AgentResponse::new(type_name)) }
    }

    /// `&mut self` setter: take the consuming builder out of the Option,
    /// apply the consuming method, put the result back. Returns `()`, not
    /// `Self` — see below for why chaining doesn't survive the NAPI boundary.
    #[napi(catch_unwind)]
    pub fn items(
        &mut self,
        rows: Vec<Vec<JsToonValue>>, // typed cell value, not serde_json::Value
        fields: Vec<String>,
    ) -> napi::Result<()> {
        let b = self.inner.take()
            .ok_or_else(|| napi::Error::from_reason("response already rendered"))?;
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let converted = convert_rows(rows);              // JsToonValue → michi_core::toon::Value
        self.inner = Some(b.items(converted, &field_refs));
        Ok(())
    }

    #[napi(catch_unwind)]
    pub fn render_toon(&self) -> napi::Result<String> {
        self.inner.as_ref()
            .ok_or_else(|| napi::Error::from_reason("response already consumed"))
            .map(|b| b.render_toon())
    }
}
```

`Option` acts as a nullable ownership slot: `take()` moves the value out and leaves `None`, the consuming method runs, the result gets reassigned.

### Chainable setters — deliberately not implemented

Every setter on `JsAgentResponse` returns `napi::Result<()>` (void on success), not `this`.

- Making a setter return `this` across the napi boundary means handing back a JS-visible reference to the _same_ wrapped object from a `&mut self` method.
- napi-rs's `#[napi]` class methods don't support that without a more invasive change to the `Option`+`take()` pattern above (e.g. holding a JS-side handle back to `self` and threading it through every mutator's return).
- Given the pattern above already works and is fully tested, that wasn't worth it for a purely cosmetic JS ergonomics win.

TypeScript callers use sequential statements, not method chaining:

```typescript
const r = new AgentResponse("issues");
r.items(rows, ["number", "title"]); // returns undefined, not r
r.hint("Try a broader filter"); // ditto
const out = r.renderToon();
```

not:

```typescript
// This does NOT work — items()/hint() return undefined, not `this`.
const out = new AgentResponse("issues")
  .items(rows, ["number", "title"])
  .hint("Try a broader filter")
  .renderToon();
```

(Async `&mut self` is unsound in napi-rs — it can't enforce exclusivity across the event loop, and would require an `unsafe` marker. michi's setters are all synchronous, so this never comes up.)

### Error handling at the boundary

Any `#[napi]` function returning `napi::Result<T>` throws a JS `Error` on `Err`. Two practices apply here:

- Wrap fallible conversions (row shape mismatches, oversized inputs) in `Result` and surface them as `napi::Error::from_reason(...)`. Inputs that cannot form a valid TOON document throw with a prefixed message: `toon_validation_failed: …` from `renderToon`, `emptyState` and `emptyStateWithHints` (a reserved `typeName`), `kv_validation_failed: …` from `renderKv` (a duplicate key, or a reserved `totalCount`/`help`/`recovery` key). `renderPipeline` throws for a status or reason that does not name a `StepStatus` (`unknown step status "done": expected one of pending, …`, `step status "skipped" needs a reason: prerequisite_failed or cancelled`, `step status "completed" takes no reason, got "timeout"`).
- Annotate exports that call into non-trivial rendering with `#[napi(catch_unwind)]`.
  - Without it: **a Rust panic crashes the entire Node process** — there's no isolation boundary.
  - With it: a panic becomes a thrown JS `Error` (or a rejected `Promise` for async) instead of process death.

### Input limits

`catch_unwind` does not stop a single call from blocking Node's event loop with a huge allocation, so every export that takes caller collections bounds the work before doing it. napi-derive converts a typed parameter such as `Vec<Vec<JsToonValue>>` or `String` in full before the export's body runs; a check in the body comes too late (a rejected 100,001 × 100 `rows` array cost 3.4 s and 2.6 GiB that way, and 10k × 10 cells sharing one 100 KB JS string were copied out once per cell, ~10 GB). These exports therefore take untyped napi values and convert them incrementally (`crates/michi-core/src/napi/input.rs`):

- `renderToon`, `renderHints`, `appendHints`, `renderRecovery`, `renderKv`, `renderAlreadyDone`, `renderDomainError`, `renderStatus`, `emptyStateWithHints`, `renderPartialSuccess`, `renderPipeline`, `alreadyDone`, and `AgentResponse.items` / `AgentResponse.kvItems`.
- Array lengths are checked first: at most 100,000 rows, 1,000 fields, values per row, KV items or status items, 10,000 hints, 10,000 `renderDomainError` causes, and 10,000 entries in each of `renderPartialSuccess`' `completed`, `failed` and `skipped` and `renderPipeline`'s `steps`. For rows, every row's length is read and summed before any cell is converted.
- Then each value is converted against a per-call budget: at most **1,000,000 cells** and **64 MiB of string data** in all. A string's UTF-8 length is read, and charged, before it is copied, so a JS string referenced many times is counted every time it is used.
- Per-string caps: **1 MiB** for a cell value (`strVal`), hint, `renderDomainError` cause, recovery or failed-op `reason`, status `description`/`health`, error `message`, `summary` or pipeline step `name`; **4 KiB** for `typeName`, a field name, a KV or status key, `truncationHint`, a recovery `tool`, `toolName`, an error `code`, `operation`, a completed or skipped step id, and a pipeline step's `id`, `status` and `reason`. `appendHints`' `body` and `alreadyDone`'s `stored` count toward the 64 MiB budget only.
- Errors keep the usual wording: `rows length 100001 exceeds maximum of 100000`, `cell count 1000100 exceeds maximum of 1000000`, `input byte total 67202037 exceeds maximum of 67108864`, `strVal byte length 1200000 exceeds maximum of 1048576`. A string whose UTF-16 length already exceeds the cap is rejected without scanning it, reported as `byte length (at least N)`.
- Only the payload a cell's `type` names is read (`strVal` for `"str"`, `floatVal` and `decimalsVal` for `"float"`, …). As before, an object field set to `null` is a wrong-typed value, not an absent one.
- The single-string `AgentResponse` members (the constructor's `typeName`, `hint`, `recoveryHint`, `truncationHint`, `humanContent`) and `emptyState` keep plain `string` parameters, since napi copies their one argument exactly once; they reject a value over the caps after the copy: 4 KiB for `typeName`, a recovery `tool` and `truncationHint`, 1 MiB for a hint, a recovery `reason` and `humanContent`. `AgentResponse.hint`/`recoveryHint` also keep their cumulative 10,000-call caps.
- **Cumulative budget per `AgentResponse`:** one object holds at most **64 MiB** of caller strings across all its calls: the type name, `items` fields and cells, `kvItems` keys and values, hints, recovery tools and reasons, the truncation hint and `humanContent`. `hint` and `recoveryHint` add to the total; `items`, `kvItems`, `truncationHint` and `humanContent` replace their slot, so the old value's bytes come back first. A call that would pass the total throws `cumulative byte total 67108865 exceeds maximum of 67108864` and leaves the builder unchanged. Before, each call had its own budget, so 65 separate 1 MiB hints were accepted and rendered 68 MB synchronously.
- michi-core's own shape caps (1,000 fields, 1,000,000 cells, a 256-char truncation hint; see [02 — Size limits](02-toon-format.md#size-limits)) still apply after conversion and surface as `toon_validation_failed: …`.

The typed Rust functions (`render_toon(JsToonOptions)`, `JsAgentResponse::items(Vec<Vec<JsToonValue>>, Vec<String>)`, …) keep their names and signatures for Rust callers and unit tests; the JS exports are separate Rust functions (`render_toon_js`, `items_js`, …) with `js_name` and `ts_args_type`, so `index.d.ts` still declares the same plain types.

### Numeric boundary

Every JavaScript number that crosses into a rendering or resilience call is validated at the boundary through a newtype in `crates/michi-core/src/napi/num.rs` before any michi code runs; an out-of-domain value throws a JS `Error` instead of being narrowed. `napi_get_value_double` (a non-coercing read) is used for every converted position, so `napi_get_value_int32`'s `ToInt32` wraparound is never in the path.

- `totalCount` (`JsToonOptions.totalCount`, `renderKv`'s `totalCount` parameter, and `AgentResponse.totalCount`) is rejected unless it is a finite, non-negative integer no greater than `Number.MAX_SAFE_INTEGER` (2^53 - 1).
- `decimalsVal` is rejected unless it is a finite integer in `[0, 20]`. When it is absent, a KV or status float keeps every digit it needs (the rule `RecoveryHint` deserialization uses), so `1e-7` renders `1e-7` and `5e-324` renders `5e-324`. When it is given, the float rounds to that many places, as asked: `1e-7` with `decimalsVal: 2` renders `0`.
- `maxChars` (`truncate`'s parameter) is rejected unless it is a finite, non-negative integer no greater than `Number.MAX_SAFE_INTEGER`.
- `intVal` is rejected unless it is a finite integer in `[-9007199254740991, 9007199254740991]` — the range over which a JS number and an `i64` agree exactly.
- `floatVal` is rejected unless it is finite; `NaN`, `Infinity`, and `-Infinity` are all rejected.
- `baseDelayMs`, `maxDelayMs`, and `retryAfterMs` are rejected unless finite, non-negative, and Duration-representable (`v / 1000.0 < u64::MAX`).
- `jitterFactor` and `jitterSeed` are rejected unless finite and in `[0.0, 1.0]`.
- `maxRetries` and `attempt` are rejected unless a finite integer in `[0, 4294967295]`.
- `status` is rejected unless a finite integer in `[100, 599]`.

None of these thirteen inputs is ever silently clamped, truncated, or defaulted on an out-of-domain value — rejection happens before any michi rendering or resilience code runs.

### Platform binary loading (`index.js`)

`@napi-rs/cli` generates `index.js` at build time.

- It tries a locally compiled `.node` file first, so `napi build` during development needs no reinstall.
- It then falls back to the per-platform optional-dependency npm package matching `process.platform`/`process.arch`.
- On Linux it also branches on glibc vs. musl via `detect-libc`.
- Each platform binary ships as its own npm package under `optionalDependencies`, with `cpu`/`os` fields so package managers install only the matching one.
- If neither a local `.node` nor a matching optional dep is present, the require throws — no silent degradation, so the package's TypeScript fallback export should surface a clear error.

> Known pitfall: npm sometimes omits optional platform deps from `package-lock.json` when the lockfile was generated on a different architecture (npm/cli#4828). Prefer `pnpm`, or regenerate the lockfile on the target arch.

### TypeScript type generation

`index.d.ts` is **never hand-written**.

- The `napi-derive` macro emits type metadata and the CLI assembles the `.d.ts` at build time — function signatures, the `AgentResponse` class, `Promise<T>` for async, `T | null` for `Option<T>`.
- Where an inferred type is too loose (the dynamic cell-value arrays, for instance), the wrapper uses `#[napi(ts_arg_type = "...")]` / `#[napi(ts_return_type = "...")]` overrides so the published types match the contract below.

### Cross-compilation and CI

`@napi-rs/cli`'s `--cross-compile` flag selects `cargo-zigbuild` as the linker for non-native Linux targets (and `cargo-xwin` for Windows, unused here).

CI builds all six NAPI targets. The macOS targets build on the arm64 `macos-latest` runner, `x86_64-apple-darwin` through rustup's target and Apple's universal SDK (no extra linker); `x86_64-unknown-linux-gnu` builds natively on `ubuntu-latest`; the musl targets and `aarch64-unknown-linux-gnu` link with `cargo-zigbuild`, `aarch64-unknown-linux-gnu` on GitHub's `ubuntu-24.04-arm` runner. The Node tests run where the runner loads the binary natively: `darwin-arm64`, `linux-x64-gnu` and `linux-arm64-gnu`.

`.github/workflows/callisto-release.yml` uses Callisto 0.11. Ordinary pushes to `main` create or update a version PR when changesets are present. A verified merge of that managed PR, carrying its `.callisto/release-decision.json`, runs Callisto's CI release: `callisto release plan`, a release build of all six targets (the runner and cross-compile choice per target come from `callisto matrix`, except that `x86_64-apple-darwin` builds on `macos-latest` rather than Callisto 0.11's retired `macos-13`; `aarch64` Linux cross-compiles with `cargo-zigbuild` on `ubuntu-latest`), then `callisto release execute`, which publishes the committed `packages/michi-node/npm/<platform>` packages before `@orin-dx/michi`, then pushes the release tags (no GitHub releases: no package lists `github-release` in `publish-to`). The build's generated `index.js` and `index.d.ts` ship in the npm package; neither they nor the `.node` files are committed.

The pipeline has not produced a release yet: neither `michi-core`/`michi-pipeline` on crates.io nor `@orin-dx/michi` on npm exists. The three packages ship one version through the Callisto fixed group, and the first release establishes the published versions for downstream integration; consumer integration follows publication (see [ADR 4](../adr/0004-publish-before-consumer-integration.md)). The `version-sync` CI job checks that the npm package version matches the workspace version in the root `Cargo.toml`.

**Windows is a non-goal for v1.** michi's agent consumers (MCP servers, CLIs invoked by coding agents) run on Linux and macOS hosts; a `windows-x64` target would add `cargo-xwin` tooling and another CI lane for no current consumer. Nothing blocks adding it later.

### TypeScript types (`index.d.ts`)

This is auto-generated, never hand-written — `napi build` regenerates it from the `#[napi]` attributes in `crates/michi-core/src/napi.rs` every time.

- Every object type is `Js`-prefixed, matching its Rust struct name: `JsToonValue`, `JsToonOptions`, `JsKvItem`, `JsRecoveryHint`, `JsStatusItem`, `JsFailedOp`, `JsPipelineStep`, `JsAlreadyDone`, `JsContentBlock`, `JsAnnotations`, `JsCallToolResult`.
- There's no unprefixed `ToonValue`/`CallToolResult`/etc. in the real output, whatever an older sketch of this section might have suggested.

What's actually in `packages/michi-node/index.d.ts` today (rendering-domain surface; the resilience-, status-, and error-domain functions follow further down):

<details>
<summary>Full rendering-domain <code>index.d.ts</code> (118 lines)</summary>

```typescript
/** Scalar TOON/KV cell value. Discriminate via `type`. */
export interface JsToonValue {
  type: string; // "str" | "int" | "float" | "bool" | "null"
  strVal?: string;
  intVal?: number;
  floatVal?: number;
  boolVal?: boolean;
  /** Decimal places when `type` is `"float"` (KV and status render only). Rejected if outside [0, 20].
   * Defaults to the value's shortest exact decimals, so `1e-7` renders `1e-7` rather than `0`. */
  decimalsVal?: number;
}

export interface JsToonOptions {
  typeName: string;
  fields: Array<string>;
  /** Rows, each parallel to `fields`. Capped at MAX_ROWS rows, MAX_FIELDS values per row. */
  rows: Array<Array<JsToonValue>>;
  totalCount?: number;
  /** Required, not optional — pass an empty array for no hints. Capped at MAX_HINTS. */
  hints: Array<string>;
  /** The caller's instruction for getting a truncated cell in full (e.g. `"full=true"`), appended as
   * `" — use {hint}"`. Omitted: the suffix only reports how many chars were cut. */
  truncationHint?: string;
}

/** Throws if rows/fields/hints, or any row, exceed this module's per-call size
 * limits — protects Node's single-threaded event loop from unbounded
 * synchronous allocation on a crafted call. */
export declare function renderToon(opts: JsToonOptions): string;

/** Render a definitive empty-state TOON response: `typeName: []\ntotalCount: 0` (no trailing newline).
 * Throws `toon_validation_failed: …` for a reserved `typeName` (`totalCount`, `help`, `recovery`), like `renderToon`. */
export declare function emptyState(typeName: string): string;

/** `emptyState` plus a `help[N]: …` block; the same `toon_validation_failed` error for a reserved `typeName`. */
export declare function emptyStateWithHints(typeName: string, hints: Array<string>): string;

/** The TOON 4.4 inline array `help[N]: h1,h2` (no trailing newline); an empty string when hints is empty. */
export declare function renderHints(hints: Array<string>): string;

/** Appends the help block to a TOON document: one LF first unless `body` is empty or already ends in one. */
export declare function appendHints(body: string, hints: Array<string>): string;

/** Only the inline form crosses the NAPI boundary — unlike Rust's `truncate()`,
 * which returns a richer `Truncated` struct, this returns the final string directly.
 * Suffix: `" ({removed} more chars — use {hint})"`, or `" ({removed} more chars)"` for an
 * empty `hint`; a cap too small for it gives `maxChars - 1` chars and `…`. */
export declare function truncate(content: string, maxChars: number, hint: string): string;

export interface JsRecoveryHint {
  tool: string;
  /** No structured params over this boundary — use `AgentResponse.recoveryHint()`
   * for the common case, or the Rust API directly for typed params. */
  reason?: string;
}

/** The TOON 4.4 table `recovery[N]{tool,params,reason}:`; params is always `"{}"` over this boundary and an
 * absent reason is `null`. */
export declare function renderRecovery(hints: Array<JsRecoveryHint>): string;

export interface JsAnnotations {
  /** `["assistant"]` or `["user"]` — michi has no concept of MCP's optional `priority`. */
  audience: Array<string>;
}

export interface JsContentBlock {
  /** Always `"text"` — michi only ever produces text content blocks. */
  type: string;
  text: string;
  annotations: JsAnnotations;
}

/** `structuredContent` is a real parsed value here (unlike `renderJson()`'s
 * string), since it crosses the boundary as a typed `#[napi(object)]` field,
 * not hand-built JSON text. */
export interface JsCallToolResult {
  content: Array<JsContentBlock>;
  isError: boolean;
  structuredContent: any;
}

export interface JsKvItem {
  key: string;
  value: JsToonValue;
}

/** High-level builder — mirrors the Rust `AgentResponse` API. Every mutator
 * returns `void`, not `this` — see "Chainable setters," above; callers use
 * sequential statements, not method chaining. */
export declare class AgentResponse {
  constructor(typeName: string);
  items(rows: Array<Array<JsToonValue>>, fields: Array<string>): void;
  totalCount(n: number): void;
  kvItems(items: Array<JsKvItem>): void;
  hint(hint: string): void;
  recoveryHint(tool: string, reason?: string): void;
  /** Appends `" — use {hint}"` to truncated TOON cells; not called, the suffix only reports the chars cut. */
  truncationHint(hint: string): void;
  /** Marks this response as an error state, reflected in `renderJson()`'s `isError` field. */
  asError(): void;
  /** Attaches a `user`-audience companion block, included by `toCallToolResult()`
   * and readable via `renderFor("user")`. */
  humanContent(text: string): void;
  /** Renders for `"assistant"` or `"user"`. `"user"` falls back to the
   * agent rendering when `humanContent()` was never set — see
   * `hasHumanContent()`. Throws for any other audience string. */
  renderFor(audience: string): string;
  /** Whether `.humanContent()` was set on this builder. */
  hasHumanContent(): boolean;
  /** Reads the TOON slot unconditionally — see 03-rust-api.md. */
  renderToon(): string;
  /** Reads the KV slot unconditionally — see 03-rust-api.md. */
  renderKv(): string;
  /** Returns a JSON *string* — `{"body":...,"hints":[...],"recovery":[...],"isError":bool}`
   * — not a parsed value; kept as hand-built text for consistency with the Rust side's
   * `render(OutputFormat::Json)`, which stays `serde_json`-free (see the `mcp` module
   * section above for where this crate *does* use `serde_json`, and why). */
  renderJson(): string;
  renderHintsOnly(): string;
  toCallToolResult(): JsCallToolResult;
}
export type JsAgentResponse = AgentResponse;
```

</details>

The resilience-, status-, and error-domain surface, added by SPEC-ARCH-004 and the status/error NAPI wrappers — every numeric position here is validated through the newtype kernel in `crates/michi-core/src/napi/num.rs` per the "Numeric boundary" section below, not left to `ToInt32`/implicit coercion:

<details>
<summary>Full resilience-, error-, and status-domain <code>index.d.ts</code> (57 lines)</summary>

```typescript
/** Return `true` if the HTTP status code is conventionally retryable (429, 502, 503, 504). Only 429 and 503 say the
 * request was not processed; after a 502 or 504 it may have been applied, so retry those only for an idempotent request. */
export declare function isRetryableStatus(status: number): boolean;

/**
 * Calculate the next retry delay in milliseconds.
 *
 * `jitterSeed` must be in `[0.0, 1.0]`; pass a per-call random value from
 * your preferred RNG to avoid thundering-herd retries across concurrent
 * callers. Delay inputs must be finite and non-negative; `retryAfterMs` may
 * be omitted or `null`. Returns `null` when `attempt >= maxRetries`.
 */
export declare function nextRetryDelay(
  maxRetries: number,
  baseDelayMs: number,
  maxDelayMs: number,
  jitterFactor: number,
  jitterSeed: number,
  attempt: number,
  retryAfterMs?: number | undefined | null,
): number | null;

/** Parse an RFC 9110 `Retry-After` header value into seconds: delay-seconds (saturating at `u64::MAX`) or an
 * HTTP-date, where a past date (before 1970 too) is `0`. Returns `null` for a malformed value. */
export declare function parseRetryAfter(headerValue: string): number | null;

/** Render the TOON 4.4 `already_done` document: `operation`, `status: already_done`, `summary`, `help`. */
export declare function renderAlreadyDone(
  operation: string,
  summary: string,
  hints: Array<string>,
): string;

/** Render a classified `DomainError` as a TOON 4.4 document (`error`, `message`, `causes`, `exit_code`, `help`), or
 * a GitHub Actions annotation (message only) when `githubAnnotation` is true. `causes` are why it happened, outermost
 * first, rendered as their own `causes[N]: …` key; `hints` are what to do next. */
export declare function renderDomainError(
  code: string,
  message: string,
  hints: Array<string>,
  githubAnnotation?: boolean | undefined | null,
  causes?: Array<string> | undefined | null,
): string;

/** A component status item for `renderStatus`. */
export interface JsStatusItem {
  /** Component key. */
  key: string;
  value: JsToonValue;
  /** Health state: `"ok"`, `"degraded: <reason>"`, or `"error: <reason>"`. */
  health?: string;
}

/** Render a P8 content-first orientation response: `tool`, `description`, the TOON 4.4 table
 * `items[N]{key,value,health,reason}:` and `help`. An item without `health` renders `ok`. */
export declare function renderStatus(
  toolName: string,
  description: string,
  items: Array<JsStatusItem>,
  hints?: Array<string> | undefined | null,
): string;

/** One failed operation for `renderPartialSuccess`. */
export interface JsFailedOp {
  operation: string;
  reason: string;
  /** No structured params over this boundary. */
  recovery?: JsRecoveryHint | null;
}

/** Render `PartialSuccess`: a quoted summary, `completed[N]: …`, the `failed[N]{operation,reason}:` table,
 * `skipped[N]: …` (empty categories omitted), then one `recovery[N]{tool,params,reason}:` table. */
export declare function renderPartialSuccess(
  completed: Array<string>,
  failed: Array<JsFailedOp>,
  skipped: Array<string>,
): string;

/** One step for `renderPipeline`: the labels `StepStatus::label` and `StepStatus::reason` write. */
export interface JsPipelineStep {
  id: string;
  name: string;
  status:
    | "pending"
    | "running"
    | "completed"
    | "completed_earlier"
    | "failed"
    | "outcome_unknown"
    | "skipped"
    | "not_run"
    | "panicked";
  /** Required for `outcome_unknown`, `skipped` and `not_run`; absent or `null` otherwise. */
  reason?:
    | "timeout"
    | "gateway"
    | "cancelled"
    | "interrupted"
    | "prerequisite_failed"
    | "circuit_open"
    | null;
}

/** Render pipeline state as `step[N]{id,name,status,reason}:`, or `step: []` with no steps. */
export declare function renderPipeline(steps: Array<JsPipelineStep>): string;

export interface JsAlreadyDone {
  done: boolean;
  /** The stored result when `done`. */
  result?: string;
}

/** Classify an idempotency lookup: any stored string (even empty) is done; `null`/`undefined` is not. */
export declare function alreadyDone(stored?: string | undefined | null): JsAlreadyDone;
```

`truncate` is already the inline form (`truncate_inline`), so there is no separate `truncateInline` export. Rust's richer `truncate()` result (`Truncated`) does not cross the boundary.

</details>

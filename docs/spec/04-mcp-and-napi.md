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
- Cross-compiled via `cargo-zigbuild`:
  - `aarch64-apple-darwin` (`darwin-arm64`)
  - `x86_64-unknown-linux-musl` (`linux-x64-musl`)

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

- Wrap fallible conversions (row shape mismatches, oversized inputs) in `Result` and surface them as `napi::Error::from_reason(...)`.
- Annotate exports that call into non-trivial rendering with `#[napi(catch_unwind)]`.
  - Without it: **a Rust panic crashes the entire Node process** — there's no isolation boundary.
  - With it: a panic becomes a thrown JS `Error` (or a rejected `Promise` for async) instead of process death.

### Numeric boundary

Every JavaScript number that crosses into a rendering or resilience call is validated at the boundary through a newtype in `crates/michi-core/src/napi/num.rs` before any michi code runs; an out-of-domain value throws a JS `Error` instead of being narrowed. `napi_get_value_double` (a non-coercing read) is used for every converted position, so `napi_get_value_int32`'s `ToInt32` wraparound is never in the path.

- `totalCount` (`JsToonOptions.totalCount`, `renderKv`'s `totalCount` parameter, and `AgentResponse.totalCount`) is rejected unless it is a finite, non-negative integer no greater than `Number.MAX_SAFE_INTEGER` (2^53 - 1).
- `decimalsVal` is rejected unless it is a finite integer in `[0, 20]`.
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

In CI, three of the four targets build natively on their own runner:

- `x86_64-apple-darwin` on `macos-13`
- `aarch64-apple-darwin` on `macos-latest`
- `x86_64-unknown-linux-gnu` on `ubuntu-latest`

Only `x86_64-unknown-linux-musl` cross-compiles, from an `ubuntu-latest` runner:

```bash
cargo install cargo-zigbuild
napi build --release --cross-compile --target x86_64-unknown-linux-musl
```

Today's `.github/workflows/ci.yml` has a `napi` job with a `fail-fast: false` matrix that **builds and tests** four targets on every push: `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` (the last cross-compiled via `cargo-zigbuild`).

- `.github/workflows/release.yml` ("Release & Publish") runs on every push to `main`, invoking the `orin-dx/callisto` action with `publish: "true"` — an automated publish pipeline exists.
- As of this writing, that pipeline has never actually produced a release: neither `michi-core`/`michi-pipeline` on crates.io nor `@orin-dx/michi` on npm exists yet (verified directly against both registries, not assumed). crates.io publish is gated behind at least one real consumer integration test (see [`docs/spec/06-decisions.md`](06-decisions.md)) — the pipeline exists, but the gate condition it's waiting on hasn't been met.
- What callisto's release job actually does internally (version detection, per-platform `.node` artifact assembly, `napi prepublish`) is external to this repo — `orin-dx/callisto`'s own source is the reference, not restated here.

The `version-sync` job already asserts the npm package version equals the crate version on every push/PR (see [05-scope-and-quality.md](05-scope-and-quality.md)) — that check exists today and would gate a publish job once one is built.

**Windows is a non-goal for v1.** michi's agent consumers (MCP servers, CLIs invoked by coding agents) run on Linux and macOS hosts; a `windows-x64` target would add `cargo-xwin` tooling and another CI lane for no current consumer. Nothing blocks adding it later.

### TypeScript types (`index.d.ts`)

This is auto-generated, never hand-written — `napi build` regenerates it from the `#[napi]` attributes in `crates/michi-core/src/napi.rs` every time.

- Every object type is `Js`-prefixed, matching its Rust struct name: `JsToonValue`, `JsToonOptions`, `JsKvItem`, `JsRecoveryHint`, `JsContentBlock`, `JsAnnotations`, `JsCallToolResult`.
- There's no unprefixed `ToonValue`/`CallToolResult`/etc. in the real output, whatever an older sketch of this section might have suggested.

What's actually in `packages/michi-node/index.d.ts` today (rendering-domain surface; the resilience-, status-, and error-domain functions follow further down):

<details>
<summary>Full rendering-domain <code>index.d.ts</code> (110 lines)</summary>

```typescript
/** Scalar TOON/KV cell value. Discriminate via `type`. */
export interface JsToonValue {
  type: string; // "str" | "int" | "float" | "bool" | "null"
  strVal?: string;
  intVal?: number;
  floatVal?: number;
  boolVal?: boolean;
  /** Decimal places when `type` is `"float"` (KV render only). Rejected if outside [0, 20]. Defaults to 6. */
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
 * No `hints` parameter over this boundary — use `appendHints()` to add one. */
export declare function emptyState(typeName: string): string;

/** Returns an empty string when hints is empty. */
export declare function renderHints(hints: Array<string>): string;

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
<summary>Full resilience-, error-, and status-domain <code>index.d.ts</code> (56 lines)</summary>

```typescript
/** Return `true` if the HTTP status code is conventionally retryable (429, 502, 503, 504). */
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

/** Parse an RFC 7231 `Retry-After` header value (delta-seconds or HTTP-date) into seconds. */
export declare function parseRetryAfter(headerValue: string): number | null;

/** Render an explicit `already_done` status block. */
export declare function renderAlreadyDone(
  operation: string,
  summary: string,
  hints: Array<string>,
): string;

/** Render a classified `DomainError` card, or a GitHub Actions annotation when `githubAnnotation` is true. */
export declare function renderDomainError(
  code: string,
  message: string,
  hints: Array<string>,
  githubAnnotation?: boolean | undefined | null,
): string;

/** A component status item for `renderStatus`. */
export interface JsStatusItem {
  /** Component key. */
  key: string;
  value: JsToonValue;
  /** Health state: `"ok"`, `"degraded: <reason>"`, or `"error: <reason>"`. */
  health?: string;
}

/** Render a P8 content-first orientation response. */
export declare function renderStatus(
  toolName: string,
  description: string,
  items: Array<JsStatusItem>,
  hints?: Array<string> | undefined | null,
): string;
```

</details>

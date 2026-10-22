# Rust API

## Crate Root (`michi_core`)

`michi-core` is the one crate holding every sync primitive. Each primitive is a module (`michi_core::toon`, `michi_core::kv`, `michi_core::truncate`, `michi_core::resilience`, …), and the most-used types are re-exported at the crate root:

```rust
pub use error::{DomainError, Error, ErrorClass, ErrorCode, Sensitive};
pub use response::{AgentResponse, OutputFormat};
pub use kv::{render_kv, KvError, KvItem, KvValue};
// … audience, empty, hints, idempotency, mcp, recovery, status
pub use resilience::{
    already_done, is_retryable_status, next_retry_delay, parse_retry_after, parse_retry_after_at,
    render_already_done, AlreadyDone, IdempotencyKey, RetryBudget, RetryConfig, RetryConfigError,
};
pub use truncate::{truncate, truncate_inline, Truncated};

pub type Result<T> = std::result::Result<T, Error>;
```

The root re-exports are explicit lists (0.4.0 replaced the `resilience::*` and `truncate::*` globs), so a new item in a module does not appear at the root by accident. `toon` is not re-exported at the root; its items are reached as `michi_core::toon::ToonOptions`, `michi_core::toon::list()`, and so on. There is no `michi` crate (the name on crates.io belongs to an unrelated project); consumers who prefer `michi::` paths rename the dependency with `michi = { package = "michi-core", version = "0.3" }`.

## `toon` — List Rendering

`michi_core::toon` formats tabular lists using Token-Oriented Object Notation (**AXI P1**). Small cell strings (≤ 24 bytes) are stack-inlined via `compact_str::CompactString`.

```rust
#[non_exhaustive]
pub struct ToonOptions {
    pub type_name: String,
    pub fields: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: Option<usize>,
    pub hints: Vec<String>,
    /// Max string cell length in chars, counted before escaping/quoting (default 200).
    pub max_cell_len: usize,
    /// The caller's "get it in full" instruction for the truncation suffix (default None: no `— use …` clause).
    pub truncation_hint: Option<String>,
}

impl ToonOptions {
    pub fn new(type_name: impl Into<String>, fields: Vec<String>, rows: Vec<Vec<Value>>) -> Self;
    pub fn total_count(self, total: Option<usize>) -> Self;
    pub fn hints(self, hints: Vec<String>) -> Self;
    pub fn max_cell_len(self, len: usize) -> Self;
    pub fn truncation_hint(self, hint: impl Into<String>) -> Self;
    /// Validate structural invariants before rendering; rendering consumes the returned proof.
    /// Returns Err if type_name is a reserved document key (`totalCount`, `help`, `recovery`),
    /// there are more than MAX_FIELDS fields, a field name is repeated, rows × fields is more than
    /// MAX_CELLS, truncation_hint is longer than MAX_TRUNCATION_HINT_CHARS chars, or any row's length
    /// differs from fields.len() (checked in that order). Any other name is valid and is quoted per TOON 4.4 §7.3.
    pub fn validate(&self) -> Result<ToonDocument<'_>, ToonError>;
}

pub const MAX_FIELDS: usize = 1_000;
pub const MAX_CELLS: usize = 1_000_000;
pub const MAX_TRUNCATION_HINT_CHARS: usize = 256;

#[non_exhaustive]
pub enum Value {
    Str(compact_str::CompactString),
    Int(i64),
    UInt(u64),  // From<u64>/From<usize> above i64::MAX; always an exact unquoted number
    Float(f64), // NaN/±Infinity render as null; serde writes them as "NaN"/"Infinity"/"-Infinity" and reads them back
    Bool(bool),
    Null,
}

impl From<&str> for Value { ... }
impl From<String> for Value { ... }
impl From<i64> for Value { ... }
impl From<f32> for Value { ... } // keeps the f32's shortest digits: 0.1f32 renders 0.1
impl From<f64> for Value { ... }
impl From<bool> for Value { ... }

#[non_exhaustive]
pub enum ToonError {
    InvalidTypeName { name: String },
    InvalidFieldName { name: String },
    RowLengthMismatch { row_index: usize, expected: usize, actual: usize },
    InvalidItem { row_index: usize, reason: String },
    TooManyFields { count: usize, max: usize },
    TooManyCells { rows: usize, fields: usize, max: usize },
    TruncationHintTooLong { len: usize, max: usize },
}
```

See [02 — Size limits](02-toon-format.md#size-limits) for why the caps exist and how they surface.

### Serde Integration (`list()`)

When the `serde` feature is enabled, `toon::list()` serializes any `T: serde::Serialize` slice to a rendered TOON string:

```rust
#[cfg(feature = "serde")]
pub fn list<T: serde::Serialize>(
    type_name: impl Into<String>,
    items: &[T],
) -> Result<String, ToonError>;
```

Items are serialized to JSON text and read back (so an `f32` field keeps its own shortest digits, `0.1` rather than `0.10000000149011612`) and must produce JSON objects (structs or maps). Non-object items and a reserved `type_name` (`totalCount`, `help`, `recovery`) return `Err(ToonError)`, and so does a key union wider than `MAX_FIELDS` or an `items.len() × fields` table larger than `MAX_CELLS` (checked before any row is built). A key missing from some items renders as `null` in those rows; nested arrays/objects become their compact JSON text, quoted.

## Response Compositor (`error`, `response`, `idempotency`)

### `DomainError`

`DomainError` provides classified domain error responses (**AXI P6**):

```rust
#[non_exhaustive]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
    pub hints: Vec<Hint>,
    pub recovery: Option<RecoveryHint>,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
}

impl DomainError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self;
    pub fn hint(self, hint: impl Into<String>) -> Self;
    pub fn recovery(self, r: RecoveryHint) -> Self;
    pub fn retryable(self, retryable: bool) -> Self;
    pub fn retry_after(self, d: Duration) -> Self;
    /// Always returns `1`.
    pub fn exit_code(&self) -> i32;
    pub fn render(&self) -> String;
    pub fn render_github_annotation(&self) -> String;
    pub fn render_json(&self) -> String;
    pub fn to_call_tool_result(&self) -> CallToolResult;
}
```

- `render()` outputs a TOON 4.4 document: `error: <code>`, `message: …` (quoted per §7.2), `exit_code: 1`, then `help[N]: …` and `recovery[1]{tool,params,reason}:` when set. `Error::render()` uses the same shape for every variant (`error: invalid_input` / `error: not_found` for the non-domain variants). See [02-toon-format.md](02-toon-format.md#agent-facing-blocks).
- `render_github_annotation()` outputs a GitHub Actions annotation (`::error title=code::message`). Newlines and CRs are percent-encoded.
- `render_json()` outputs a JSON object: `{"isError":true,"error":"...","message":"...","retryable":bool,"hints":[...],"recovery":null|{...}}`.
- `to_call_tool_result()` builds an MCP `CallToolResult` with `is_error:true` and the rendered JSON as `structured_content`.
- When the `schemars` feature is enabled, `DomainError` implements `JsonSchema` (the schema of `render_json()`'s object). Only types serde actually serializes have a schema: `StatusResponse`, `ToonOptions`, `ErrorClass` and the other render-only types do not.

### `AgentResponse` Builder

`AgentResponse` composes lists, key-value single items, hints, and audience routing. It is a **consuming** builder — every setter takes `self` by value and returns `Self`, so calls chain (`AgentResponse::new("t").items(...).hint(...)`), not `&mut self` mutation:

```rust
pub struct AgentResponse { ... }

impl AgentResponse {
    pub fn new(type_name: impl Into<String>) -> Self;
    pub fn items(self, rows: Vec<Vec<Value>>, fields: &[&str]) -> Self;
    pub fn total_count(self, n: usize) -> Self;
    pub fn kv_items(self, items: Vec<KvItem>) -> Self;
    pub fn hint(self, hint: impl Into<String>) -> Self;
    /// Replaces the full hint list (contrast `hint()`, which appends).
    pub fn hints(self, hints: Vec<Hint>) -> Self;
    pub fn recovery_hint(self, r: RecoveryHint) -> Self;
    /// Overrides the default TOON cell truncation limit (200 chars, counted before escaping/quoting); TOON path only.
    pub fn truncate_cells_at(self, limit: usize) -> Self;
    /// Passed through to `ToonOptions::truncation_hint`; unset by default.
    pub fn truncation_hint(self, hint: impl Into<String>) -> Self;
    pub fn as_error(self) -> Self;
    pub fn human_content(self, text: impl Into<String>) -> Self;

    /// Dispatches on which of `.items()`/`.kv_items()` was called last.
    pub fn render(&self, format: OutputFormat) -> String;
    /// Reads the TOON slot unconditionally, regardless of which builder path was last set.
    pub fn render_toon(&self) -> String;
    /// Reads the KV slot unconditionally, regardless of which builder path was last set.
    pub fn render_kv(&self) -> String;
    /// Just the `help[N]: …` document for `.hint()`/`.hints()`.
    pub fn render_hints_only(&self) -> String;
    /// `"user"` falls back to `render(OutputFormat::Text)` when `.human_content()` was never set.
    pub fn render_for(&self, audience: Audience) -> String;
    pub fn has_human_content(&self) -> bool;
    pub fn to_call_tool_result(&self) -> CallToolResult;
}
```

`render_json()` is not part of the public API — it's reached only via `render(OutputFormat::Json)`.

The TOON slot is stored as a `ToonOptions` and validated and rendered by reference, so a render never copies the rows. `is_error()` validates without rendering. Hints and recovery are appended to the rendered body in place.

`total_count` applies to both paths. On the KV path with no items the body is just `totalCount: N` (the free `render_kv` still returns `""` for empty items), so a count the caller set is never dropped; with neither items nor a count the body is empty.

### Idempotency

```rust
// In michi_core::idempotency (also at michi_core::PartialSuccess, michi_core::FailedOp)
#[non_exhaustive]
#[derive(Default)]
pub struct PartialSuccess {
    pub completed: Vec<String>,
    pub failed: Vec<FailedOp>,
    pub skipped: Vec<String>,
}

impl PartialSuccess {
    pub fn new(completed: Vec<String>, failed: Vec<FailedOp>, skipped: Vec<String>) -> Self;
    pub fn render(&self) -> String;
    pub fn exit_code(&self) -> i32;   // 0 = no failures, 1 = any failed
}

#[non_exhaustive]
pub struct FailedOp {
    pub operation: String,
    pub reason: String,
    pub recovery: Option<RecoveryHint>,
}

impl FailedOp {
    pub fn new(operation: impl Into<String>, reason: impl Into<String>) -> Self;
    pub fn recovery(self, hint: RecoveryHint) -> Self;
}

// In michi_core::resilience (also at michi_core::already_done, michi_core::AlreadyDone)
#[non_exhaustive]
pub enum AlreadyDone { Yes { result: String }, No }
pub fn already_done(stored: Option<String>) -> AlreadyDone;
pub fn render_already_done(operation: &str, summary: &str, hints: &[String]) -> String;
pub struct IdempotencyKey(pub String);
impl IdempotencyKey {
    pub fn new(s: impl Into<String>) -> Self;
    pub fn from_hash(operation: &str, data: &[u8]) -> Self;
}
```

### Key-value, hints, recovery and status blocks

Every block renderer returns a finished TOON 4.4 document with no trailing newline; the `append_*` helpers join a block onto a document with one LF. The forms are listed in [02-toon-format.md](02-toon-format.md#agent-facing-blocks).

```rust
// michi_core::kv
#[non_exhaustive]
pub struct KvItem { pub key: String, pub value: KvValue }
impl KvItem { pub fn new(key: impl Into<String>, value: KvValue) -> Self; }
#[non_exhaustive]
pub enum KvValue { Text(String), Int(i64), Float(f64, u8), Bool(bool), Duration(Duration), Missing }
#[non_exhaustive]
pub enum KvError { ReservedKey { key: String }, DuplicateKey { key: String } }
/// `key: value` lines, then `totalCount` and `help`. Err for a repeated key or a reserved
/// `totalCount`/`help`/`recovery` key (a duplicate root key, §14.3). Empty items: Ok("").
pub fn render_kv(items: &[KvItem], total_count: Option<usize>, hints: &[Hint]) -> Result<String, KvError>;

// michi_core::hints
pub fn render_hints(hints: &[Hint]) -> String;               // help[N]: h1,h2
pub fn append_hints(out: &mut String, hints: &[Hint]);

// michi_core::recovery
#[non_exhaustive]
pub struct RecoveryHint { pub tool: String, pub params: Vec<(String, KvValue)>, pub reason: Option<String> }
impl RecoveryHint {
    pub fn new(tool: impl Into<String>) -> Self;
    pub fn param(self, key: impl Into<String>, value: KvValue) -> Self;
    pub fn reason(self, reason: impl Into<String>) -> Self;
}
pub fn render_recovery(hints: &[RecoveryHint]) -> String;    // recovery[N]{tool,params,reason}:
pub fn append_recovery(out: &mut String, hints: &[RecoveryHint]);

// michi_core::status
#[non_exhaustive]
pub struct StatusItem { pub key: String, pub value: KvValue, pub health: Option<Health> }
impl StatusItem {
    pub fn new(key: impl Into<String>, value: KvValue) -> Self;   // health: None
    pub fn health(self, health: Health) -> Self;
}
impl StatusResponse { pub fn render(&self) -> String; }      // tool, description, items[N]{key,value,health,reason}:, help

// michi_core::pipeline
#[non_exhaustive]
pub struct PipelineStep { pub id: String, pub name: String, pub status: StepStatus }
impl PipelineStep { pub fn new(id: impl Into<String>, name: impl Into<String>, status: StepStatus) -> Self; }
#[non_exhaustive]
#[derive(Default)]
pub struct Pipeline { pub id: String, pub steps: Vec<PipelineStep> }
impl Pipeline {
    pub fn new(id: impl Into<String>, steps: Vec<PipelineStep>) -> Self;
    pub fn render(&self) -> String;                           // step[N]{id,name,status,reason}:
}
```

The structs above are `#[non_exhaustive]` from 0.4.0: build them with their constructors, then read or assign their public fields as before. Struct literals and exhaustive destructuring work only inside michi-core, so adding a field later is not a breaking change. `Value`, `KvValue`, `AlreadyDone` and `RetryConfigError` are `#[non_exhaustive]` enums: a `match` outside michi-core needs a `_` arm.

`KvValue::Float(f, decimals)` rounds `f` to `decimals` places and writes the rounded value in canonical TOON number form, so `Float(1.5, 3)` is `1.5`, not `1.500`; `Missing` is `null`; `Duration` is the string `1.5s`. `AgentResponse`'s KV path turns a `KvError` into a `kv_validation_failed` error body and sets `is_error`, like `toon_validation_failed` on the TOON path.

## `truncate` — Char-Boundary Truncation

```rust
pub struct Truncated {
    pub content: String,
    /// Byte length (not char count) of the input, truncated or not.
    pub original_len: usize,
    pub was_truncated: bool,
    /// The signal embedded at the end of content, leading space stripped: "(N more chars — use hint)",
    /// "(N more chars)" or "…". None when not truncated or when max_chars is 0.
    pub signal: Option<String>,
}

pub fn truncate(content: &str, max_chars: usize, hint: &str) -> Truncated;
pub fn truncate_inline(content: &str, max_chars: usize, hint: &str) -> String;
```

Truncates strings safely at Unicode scalar boundaries without splitting UTF-8 code points. `max_chars` counts chars, and the result never exceeds it:

| Input     | `max_chars` | `hint`        | `content`                                        |
| --------- | ----------- | ------------- | ------------------------------------------------ |
| 200 × `a` | 50          | `"full=true"` | 17 × `a` + `" (183 more chars — use full=true)"` |
| 100 × `a` | 40          | `""`          | 24 × `a` + `" (76 more chars)"`                  |
| 100 × `a` | 3           | `"x"`         | `"aa…"`                                          |
| 100 × `a` | 1 / 0       | any           | `"…"` / `""`                                     |

- The count is the chars **removed** (input chars minus kept chars), sized so the suffix's own digits fit within `max_chars`.
- The `— use {hint}` clause is the caller's instruction and is omitted when `hint` is empty; michi never supplies one. The `&str` signature stays so the NAPI `truncate(content, maxChars, hint)` export is unchanged.
- When the suffix plus one kept char does not fit, the result is the first `max_chars - 1` chars and `…`, never a fragment of the suffix. `was_truncated` is still `true`.

## `resilience` — Retry & Idempotency Primitives

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_factor: f64,
}

impl RetryConfig {
    /// Normalizing constructor — clamps out-of-range values.
    pub fn new(max_retries: u32, base_delay: Duration, max_delay: Duration, jitter_factor: f64) -> Self;
    /// Strict constructor — returns Err for out-of-range values.
    pub fn try_new(...) -> Result<Self, RetryConfigError>;
}

#[non_exhaustive]
pub enum RetryConfigError {
    MaxDelayIsZero,
    BaseDelayExceedsMaxDelay { base: Duration, max: Duration },
    JitterFactorOutOfRange { factor: f64 },
}

pub fn next_retry_delay(
    config: &RetryConfig,
    attempt: u32,
    jitter_seed: f64,          // per-call RNG value in [0.0, 1.0]
    retry_after: Option<Duration>,
) -> Option<Duration>;

pub fn parse_retry_after(header_value: &str) -> Option<Duration>;
pub fn is_retryable_status(status: u16) -> bool;
```

Calculates exponential backoff with caller-supplied jitter and parses RFC 7231 `Retry-After` HTTP header values (delta-seconds and HTTP-date formats).

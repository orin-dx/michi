# Edge-Case Architecture: Systemic Fixes

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> Produced 2026-08-04 following a comprehensive four-agent audit of all michi modules.
> This document records architectural decisions for 7 systemic issues found in that audit.
> Implementation is driven by the decisions below — agents must not deviate without
> updating this document first.

---

## Background

A parallel audit across all michi modules found 29 edge-case issues. Most are
clear bugs with unambiguous fixes. Seven point to structural gaps in how
invariants are enforced — escaping applied inconsistently at call sites, render
functions that cannot signal errors, validation only in debug builds, unreachable
enum variants, and missing integration paths. This document records the
architectural decisions for those seven.

**Audit findings reference:** `docs/superpowers/plans/` — see the edge-cases
artifact for the full 29-finding list.

---

## Issue A — Escaping is opt-in at the call site

**Root cause:** `ToonOptions` stores `type_name`, `fields`, and `hints` as plain
`String` with no type-level distinction between "validated header token" and
"arbitrary user string." `render()` applies `escape_value()` only to cell values —
the only position that has explicit handling. Header positions and hints are
unprotected.

**Decision: A1 + A2 combined**

Add `sanitize_header_token()` and `sanitize_hint()` in `escape.rs`, call them
from `render.rs`. Add `ToonOptions::validate()` for callers who need explicit
error signals. Do not add newtypes — the API friction cascades into `list()` and
every downstream usage, disproportionate to the benefit.

```rust
// escape.rs — new functions
/// Strip newlines from a TOON header token (type_name or field name).
/// The TOON format has no escaping syntax for header positions; newlines
/// are the only character michi can fix silently. Structural characters
/// (`[`, `]`, `{`, `}`, `,`) must be excluded by the caller.
pub(crate) fn sanitize_header_token(s: &str) -> std::borrow::Cow<'_, str> {
    if s.bytes().any(|b| b == b'\n' || b == b'\r') {
        std::borrow::Cow::Owned(s.chars().filter(|&c| c != '\n' && c != '\r').collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

pub(crate) use sanitize_header_token as sanitize_hint;
```

```rust
// lib.rs — new error type and validate() method
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToonError {
    /// `type_name` contains a structural character that cannot be escaped.
    InvalidTypeName { name: String },
    /// A field name contains a structural character that cannot be escaped.
    InvalidFieldName { name: String },
    /// Row at `row_index` has `actual` values but `fields` expects `expected`.
    RowLengthMismatch { row_index: usize, expected: usize, actual: usize },
}

impl std::fmt::Display for ToonError { /* label each variant */ }
impl std::error::Error for ToonError {}

impl ToonOptions {
    /// Validate structural invariants before rendering.
    ///
    /// `render_toon()` handles bad input gracefully, but this gives callers
    /// explicit error signals when they need to detect problems.
    pub fn validate(&self) -> Result<(), ToonError> {
        if self.type_name.contains(['[', ']', '{', '}', '\n', '\r']) {
            return Err(ToonError::InvalidTypeName { name: self.type_name.clone() });
        }
        for field in &self.fields {
            if field.contains([',', '{', '}', '\n', '\r']) {
                return Err(ToonError::InvalidFieldName { name: field.clone() });
            }
        }
        for (i, row) in self.rows.iter().enumerate() {
            if row.len() != self.fields.len() {
                return Err(ToonError::RowLengthMismatch {
                    row_index: i,
                    expected: self.fields.len(),
                    actual: row.len(),
                });
            }
        }
        Ok(())
    }
}
```

**Resolves:** C1. `ToonError::RowLengthMismatch` also subsumes Issue B.

---

## Issue B — `render()` cannot signal errors

**Root cause:** `render()` was designed on the assumption of pre-validated input,
with a `debug_assert!` as the only guard. Because `ToonOptions` has public fields,
post-construction mutation can invalidate any invariant, making construction-time
validation insufficient on its own.

**Decision: B2 — Make render robust, remove debug_assert**

Replace the `debug_assert!` in `render.rs` with graceful clamping: extra values
in a row are ignored, missing values emit empty (Null) cells. The rendering
contract becomes: "produces valid TOON for any input; mismatched rows are padded
or clamped." Callers who need to detect the mismatch call `validate()` (from
Issue A).

Do **not** change `render_toon()` to return `Result<String, ToonError>` — the
cascading change touches `AgentResponse`, `render()`, `render_text()`, and
`to_call_tool_result()`, all currently returning `String`. The robustness approach
is consistent with how `truncate()` handles degenerate inputs.

**Resolves:** C2 (silent corruption in release builds).

---

## Issue C — `render_json()` tested only via JS

**Root cause:** `render_json()` was first exercised via the NAPI integration layer;
JS tests were written there and no Rust-level test was added.

**Decision: C1 — Add Rust unit tests in response.rs**

Policy: Rust-layer behavior has Rust tests; NAPI-boundary behavior has JS tests.
No architectural change needed. Add tests covering:

```rust
#[test]
fn render_json_basic_structure() { /* isError:false, body, hints:[], recovery:[] */ }

#[test]
fn render_json_is_error_flag() { /* as_error() -> isError:true */ }

#[test]
fn render_json_recovery_int_param_is_numeric_not_string() {
    // Regression: must be `"limit":10`, not `"limit":"10"`
    // This is the exact bug class previously found and fixed; must stay green.
}

#[test]
fn render_json_hint_is_string_escaped() { /* quotes in hint text */ }
```

**Resolves:** H1.

---

## Issue D — `ErrorClass::Internal` is unreachable

**Root cause:** `Error::class()` was written with two mental models (user vs.
transient). `Internal` was declared when the taxonomy was designed but the match
never evolved to produce it. Infrastructure errors with `retryable: false` fall
through to `ErrorClass::User`, which is semantically wrong.

**Decision: D2 (classification on ErrorCode) + D1 (use it in Error::class())**

```rust
impl ErrorCode {
    /// The default error class for this code, before considering `DomainError.retryable`.
    #[must_use]
    pub fn default_class(&self) -> ErrorClass {
        match self {
            Self::InvalidInput | Self::NotFound | Self::Unauthorized
            | Self::Forbidden | Self::Conflict => ErrorClass::User,
            Self::RateLimited | Self::Unavailable | Self::Timeout
            | Self::ExternalFailure => ErrorClass::Internal,
        }
    }
}

impl Error {
    #[must_use]
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Domain(d) if d.retryable => ErrorClass::Transient,
            Self::Domain(d) => d.code.default_class(),
            Self::InvalidInput(_) | Self::NotFound(_) => ErrorClass::User,
        }
    }
}
```

Updated `ErrorClass::Internal` doc: "An infrastructure or downstream failure that
is not the caller's fault and is not expected to self-resolve without intervention.
michi does not recommend automatic retry for `Internal` errors."

`is_retryable()` stays as `matches!(self.class(), ErrorClass::Transient)` —
`Internal` is not retryable by default.

**Resolves:** H4. Informs Issue G (Internal errors need a clean MCP error path).

---

## Issue E — `TruncateResult::signal` and `content` can diverge

**Root cause:** `signal` is computed from the full suffix string before the
hard-cap at lines 65–68. When `max_chars` is smaller than the suffix length,
`content` is truncated mid-suffix but `signal` retains the full pre-cap string.

**Decision: E1 — Recompute `signal` as text actually embedded in `content`**

After hard-capping, recompute `signal` as the bytes in `result` that follow
the kept content. If the cap leaves no room for any signal text, `signal` is
`None`. Updated doc for `Truncated.signal`:

> The truncation signal text that was actually embedded in `content`, starting
> immediately after the kept characters (leading space stripped). `None` when
> not truncated or when `max_chars` was so small that no signal text fit.

Add a test for the degenerate case (max_chars smaller than the suffix length).

**Resolves:** M5.

---

## Issue F — `RetryConfig` has no validation

**Root cause:** `RetryConfig::new()` was written as a simple struct literal
constructor. Invariants are documented in comments but not enforced. `max_delay =
Duration::ZERO` silently discards a server's explicit `retry_after`. The michi
principles prohibit `unwrap()`/`expect()` in lib code — a `debug_assert!` is
equivalent in nature.

**Decision: F1 — `RetryConfig::new()` returns `Result<Self, RetryConfigError>`**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum RetryConfigError {
    /// `max_delay` must be non-zero (a zero cap silently drops `retry_after`).
    MaxDelayIsZero,
    /// `base_delay` must not exceed `max_delay`.
    BaseDelayExceedsMaxDelay { base: Duration, max: Duration },
    /// `jitter_factor` must be in the range `[0.0, 1.0]`.
    JitterFactorOutOfRange { factor: f64 },
}

impl std::fmt::Display for RetryConfigError { /* one sentence per variant */ }
impl std::error::Error for RetryConfigError {}

impl RetryConfig {
    pub fn new(
        max_retries: u32,
        base_delay: Duration,
        max_delay: Duration,
        jitter_factor: f64,
    ) -> Result<Self, RetryConfigError> {
        if max_delay.is_zero() {
            return Err(RetryConfigError::MaxDelayIsZero);
        }
        if base_delay > max_delay {
            return Err(RetryConfigError::BaseDelayExceedsMaxDelay {
                base: base_delay,
                max: max_delay,
            });
        }
        if !(0.0..=1.0).contains(&jitter_factor) {
            return Err(RetryConfigError::JitterFactorOutOfRange { factor: jitter_factor });
        }
        Ok(Self { max_retries, base_delay, max_delay, jitter_factor })
    }
}
```

`Default` stays infallible — it already produces valid values.

**Resolves:** M6 (max_delay=0 silently drops retry_after), L3 (jitter_factor unenforced).

---

## Issue G — No MCP integration path for `DomainError`

**Root cause:** The MCP bridge was built around the success path.
`AgentResponse::to_call_tool_result()` handles errors only when the caller has
already called `AgentResponse::as_error()`. `DomainError` has a rich render
pipeline but no path to `CallToolResult`. Not documented as deliberate in
`decisions.md` — an oversight.

**Decision: G1 — Add `DomainError::to_call_tool_result()` and `DomainError::render_json()`**

Both live in `michi-core/src/error.rs`. No new dependencies.

```rust
impl DomainError {
    /// Build an MCP `CallToolResult` representing this error.
    ///
    /// Sets `is_error: true`, renders the error via [`DomainError::render()`] as the
    /// assistant-facing text block, and attaches a compact JSON companion for
    /// `structured_content`.
    #[must_use]
    pub fn to_call_tool_result(&self) -> crate::mcp::CallToolResult {
        crate::mcp::CallToolResult {
            content: vec![crate::mcp::ContentBlock {
                text: self.render(),
                audience: vec![crate::audience::Audience::Assistant],
            }],
            is_error: true,
            structured_content: self.render_json(),
        }
    }

    /// Render this error as a compact JSON object for `structured_content` or telemetry.
    ///
    /// Produces: `{"error":"<code>","message":"<msg>","retryable":<bool>}`
    #[must_use]
    pub fn render_json(&self) -> String {
        let mut out = String::with_capacity(64 + self.message.len());
        out.push_str("{\"error\":");
        crate::kv::json_escape_str(&mut out, self.code.label());
        out.push_str(",\"message\":");
        crate::kv::json_escape_str(&mut out, &self.message);
        out.push_str(",\"retryable\":");
        out.push_str(if self.retryable { "true" } else { "false" });
        out.push('}');
        out
    }
}
```

Uses the same hand-rolled zero-allocation JSON approach as `AgentResponse::render_json()`.
No `serde` required. `json_escape_str` must be accessible from `kv` — make
`pub(crate)` if not already.

**Resolves:** M9. Pairs with Issue D — Internal-classified errors now have a
correct class and a clean path to MCP wire format.

---

## Decision Summary

| Issue | Decision | Primary files | Resolves |
|-------|----------|---------------|---------|
| A — Escaping opt-in | `sanitize_header_token()` + `sanitize_hint()` in render; `ToonError` + `validate()` | `michi-toon/src/escape.rs`, `render.rs`, `lib.rs` | C1; `ToonError` shared with B |
| B — No render errors | Graceful clamp/pad in `render::render()`; remove `debug_assert!` | `michi-toon/src/render.rs` | C2 |
| C — render_json untested | Add Rust unit tests (basic structure, isError, typed params regression, hint escaping) | `michi-core/src/response.rs` | H1 |
| D — Internal unreachable | `ErrorCode::default_class()`; fix `Error::class()` to return `Internal` for infra codes | `michi-core/src/error.rs` | H4 |
| E — signal/content diverge | Recompute `signal` after hard-cap as text actually in `content` | `michi-truncate/src/lib.rs` | M5 |
| F — RetryConfig unvalidated | `RetryConfigError` type; `RetryConfig::new()` → `Result<Self, RetryConfigError>` | `michi-resilience/src/lib.rs` | M6, L3 |
| G — No DomainError→MCP path | `DomainError::to_call_tool_result()` + `DomainError::render_json()` in error.rs | `michi-core/src/error.rs` | M9 |

---

## What this document does NOT cover

The remaining 22 findings from the audit are clear bugs with no systemic
ambiguity — each has an obvious correct fix. They are not listed here because
they do not require a design decision:

- **C2** — covered by Issue B's decision
- **H2** — already fixed (dead files deleted)
- **H3** — `toon::list()` non-object items: return `Err(ToonError)`, not empty rows
- **H5** — `parse_http_date`: validate days-in-month against actual month length
- **M1** — `FailedOp.operation`: apply `escape_value_quoted` (same as `reason`)
- **M2** — `KvValue::Text` with `\n`: strip or escape, same as TOON cell values
- **M3/M4** — Add correctness tests for the `retry_after: Some(_)` path and HTTP-date parsing
- **M7** — `serializer.rs`: dead code; remove or wire into `toon::list()`
- **M8** — Add insta snapshot tests for `error`, `hints`, `recovery`, `idempotency`
- **L1** — `u64` near MAX: document the clamp or return `ToonError`
- **L2** — `Value::Float(NAN/INFINITY)`: render as quoted string or return `ToonError`
- **L4** — `is_retryable_status`: add tests for 429, 503, 500 (not retryable), 200
- **L5** — `AlreadyDone`/`PartialSuccess` boundary: add doc comments
- **L6** — `decisions.md`: rename `initial_delay` → `base_delay`
- **L7** — Spec: document RFC 850/asctime non-support in `parse_retry_after`
- **L8** — Pipeline step ID uniqueness: document policy in doc comments
- **L9** — KV key padding: document Unicode limitation in `render_kv` doc comment
- **L10** — Empty strings: document acceptance policy in `PRINCIPLES.md`
- **S2** — `michi-resilience`: port `PartialSuccess` and `FailedOp` from `src/idempotency.rs`
- **S3** — `as_error()`, `render_for()`, `has_human_content()`, `render_hints_only()`: add Rust-level tests

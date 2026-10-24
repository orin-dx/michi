# Edge-Case Fixes Implementation Plan

> **Historical record.** This dated document shows what was planned or decided at the time; much of it has since changed. Don't implement from it. For current behavior, see [`docs/spec/`](../../spec/README.md), [`ARCHITECTURE.md`](../../../ARCHITECTURE.md) and the [ADRs](../../adr/README.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all 29 audit findings plus 1 gap finding across michi's module clusters, enforce the unified invariant policy in code, and close every test coverage gap identified in the adversarial review.

**Architecture:** Seven systemic decisions recorded in `docs/superpowers/specs/2026-08-04-edge-case-architecture.md` govern the structural fixes. The remaining findings are clear bugs with unambiguous local fixes. Tasks are ordered by dependency: TOON → michi-core/error → michi-core/response → michi-resilience → michi-truncate → docs.

**Tech Stack:** Rust, `cargo nextest`, `insta` (snapshots), `proptest`, `compact_str`, `thiserror`

**Model guidance per task:**
- `[haiku]` — mechanical, 1-2 files, complete spec
- `[sonnet]` — multi-file or requires integration judgment
- `[opus]` — subtle algorithm, broad codebase understanding, or adversarially sensitive

---

## File Map

| Task | Creates | Modifies |
|------|---------|----------|
| 1 | — | `crates/michi-toon/src/escape.rs`, `crates/michi-toon/src/render.rs` |
| 2 | — | `crates/michi-toon/src/lib.rs`, `crates/michi-toon/src/render.rs` |
| 3 | — | `crates/michi-toon/src/lib.rs` (remove `mod serializer`), delete `crates/michi-toon/src/serializer.rs` |
| 4 | — | `crates/michi-core/src/kv/mod.rs` |
| 5 | — | `crates/michi-core/src/error.rs` |
| 6 | — | `crates/michi-core/src/error.rs`, `crates/michi-core/src/mcp.rs` |
| 7 | `crates/michi-core/src/idempotency.rs` | `crates/michi-core/src/lib.rs`, `tests/snapshot_tests.rs` |
| 8 | — | `crates/michi-core/src/response.rs`, `crates/michi-core/src/mcp.rs` |
| 9 | — | `crates/michi-core/src/response.rs`, `tests/snapshot_tests.rs` |
| 10 | — | `crates/michi-resilience/src/lib.rs`, `src/napi.rs` |
| 11 | — | `crates/michi-resilience/src/lib.rs` (tests), `src/resilience/mod.rs` (reference) |
| 12 | — | `crates/michi-resilience/src/lib.rs` |
| 13 | — | `crates/michi-truncate/src/lib.rs` |
| 14 | — | `crates/michi-core/src/pipeline/mod.rs`, `crates/michi-core/src/kv/mod.rs`, `crates/michi-resilience/src/lib.rs` |

---

## Task 1 — michi-toon: `sanitize_header_token` + wire into `render()` `[sonnet]`

**Findings resolved:** C1 (header escaping), A (partial)

**Files:**
- Modify: `crates/michi-toon/src/escape.rs`
- Modify: `crates/michi-toon/src/render.rs`

**Context:** `render()` calls `escape_value()` for cell values but pushes `type_name`, field names, and hints raw. A newline in `type_name` splits the header line; a comma in a field name silently creates an extra column. The fix adds `sanitize_header_token()` that replaces structural chars AND newlines with `_`, then wires it into the three render positions.

- [ ] **Step 1: Write failing tests in `escape.rs`**

```rust
#[cfg(test)]
mod sanitize_tests {
    use super::*;

    #[test]
    fn plain_token_is_borrowed() {
        let result = sanitize_header_token("file_path");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "file_path");
    }

    #[test]
    fn newline_in_type_name_is_replaced() {
        assert_eq!(sanitize_header_token("foo\nbar"), "foo_bar");
    }

    #[test]
    fn carriage_return_is_replaced() {
        assert_eq!(sanitize_header_token("foo\rbar"), "foo_bar");
    }

    #[test]
    fn structural_chars_are_replaced() {
        assert_eq!(sanitize_header_token("a[b]c"), "a_b_c");
        assert_eq!(sanitize_header_token("a{b}c"), "a_b_c");
        assert_eq!(sanitize_header_token("a,b"), "a_b");
    }

    #[test]
    fn multiple_structural_chars() {
        assert_eq!(sanitize_header_token("foo{bar},baz\n"), "foo_bar__baz_");
    }
}
```

Run: `cargo nextest run -p michi-toon -- sanitize_tests`
Expected: FAIL ("sanitize_header_token not found")

- [ ] **Step 2: Add `sanitize_header_token` to `escape.rs`**

```rust
/// Sanitize a TOON header token (type_name or field name) for safe embedding.
///
/// Replaces `\n`, `\r`, and structural characters (`[`, `]`, `{`, `}`, `,`)
/// with `_`. Header positions have no escaping syntax in the TOON grammar —
/// replacement is the only safe option.
pub(crate) fn sanitize_header_token(s: &str) -> std::borrow::Cow<'_, str> {
    const STRUCTURAL: &[char] = &['[', ']', '{', '}', ',', '\n', '\r'];
    if s.chars().any(|c| STRUCTURAL.contains(&c)) {
        std::borrow::Cow::Owned(
            s.chars()
                .map(|c| if STRUCTURAL.contains(&c) { '_' } else { c })
                .collect(),
        )
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

// Hint positions have the same structural constraint as header tokens.
pub(crate) use sanitize_header_token as sanitize_hint;
```

- [ ] **Step 3: Run tests to confirm passing**

Run: `cargo nextest run -p michi-toon -- sanitize_tests`
Expected: all pass

- [ ] **Step 4: Wire `sanitize_header_token` into `render.rs`**

In `render()`, change the three push positions:

```rust
// type_name position (was: out.push_str(type_name))
out.push_str(&super::escape::sanitize_header_token(type_name));

// field names (was: out.push_str(field))
out.push_str(&super::escape::sanitize_header_token(field));

// hints (was: out.push_str(hint))
out.push_str(&super::escape::sanitize_hint(hint));
```

- [ ] **Step 5: Add render-level tests for the sanitization**

Add inside `render.rs` tests module:

```rust
#[test]
fn type_name_with_newline_is_sanitized() {
    let out = render("foo\nbar", &[], &[], None, &[], 200);
    assert!(out.starts_with("foo_bar[0]{}:"), "got: {out}");
}

#[test]
fn field_with_comma_is_sanitized() {
    let out = render("t", &["a,b".to_string()], &[vec![Value::from("v")]], None, &[], 200);
    assert!(out.contains("{a_b}"), "got: {out}");
}

#[test]
fn hint_with_newline_is_sanitized() {
    let out = render("t", &[], &[], None, &["line1\nline2".to_string()], 200);
    assert!(out.contains("line1_line2"), "got: {out}");
    assert!(!out.contains('\n'), "hint newline must not appear in output");
}
```

Run: `cargo nextest run -p michi-toon`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-toon/src/escape.rs crates/michi-toon/src/render.rs
git commit -m "fix(toon): sanitize structural chars and newlines in header positions"
```

---

## Task 2 — michi-toon: `ToonError` + `validate()` + non-object `list()` fix + Float specials `[sonnet]`

**Findings resolved:** A (ToonError + validate()), B (keep debug_assert! + clamp/pad), H3 (non-object list), L2 (Float NaN/Inf)

**Files:**
- Modify: `crates/michi-toon/src/lib.rs`
- Modify: `crates/michi-toon/src/render.rs`

**Context:** `ToonError` is the public error type for the TOON subsystem. `validate()` gives callers an explicit signal for malformed options (it will be wired into `AgentResponse::toon_body()` in Task 8). The `render()` `debug_assert!` stays but is joined by graceful clamp/pad for release. `toon::list()` currently produces silent empty rows for non-object items; should return `Err(ToonError)`. `Value::Float(NAN)` currently renders as `NaN` which is not valid TOON — render as quoted string.

- [ ] **Step 1: Write failing tests for ToonError and validate()**

Add to `lib.rs` tests:

```rust
#[cfg(test)]
mod validate_tests {
    use super::*;

    #[test]
    fn validate_ok_for_valid_options() {
        let opts = ToonOptions {
            type_name: "file".into(),
            fields: vec!["path".into(), "size".into()],
            rows: vec![
                vec![Value::from("a.rs"), Value::from(100i64)],
            ],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(opts.validate().is_ok());
    }

    #[test]
    fn validate_rejects_type_name_with_bracket() {
        let opts = ToonOptions {
            type_name: "foo[bar".into(),
            fields: vec![],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(opts.validate(), Err(ToonError::InvalidTypeName { .. })));
    }

    #[test]
    fn validate_rejects_field_with_comma() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a,b".into()],
            rows: vec![],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(opts.validate(), Err(ToonError::InvalidFieldName { .. })));
    }

    #[test]
    fn validate_rejects_row_length_mismatch() {
        let opts = ToonOptions {
            type_name: "t".into(),
            fields: vec!["a".into()],
            rows: vec![vec![Value::from("x"), Value::from("y")]],
            hints: vec![],
            max_cell_len: 200,
            total_count: None,
        };
        assert!(matches!(
            opts.validate(),
            Err(ToonError::RowLengthMismatch { row_index: 0, expected: 1, actual: 2 })
        ));
    }
}
```

Run: `cargo nextest run -p michi-toon -- validate_tests`
Expected: FAIL (ToonError doesn't exist yet)

- [ ] **Step 2: Add `ToonError` and `validate()` to `lib.rs`**

```rust
/// Structural validation error for [`ToonOptions`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ToonError {
    /// `type_name` contains a structural character (`[`, `]`, `{`, `}`, `\n`, `\r`).
    InvalidTypeName { name: String },
    /// A field name contains a structural character (`,`, `{`, `}`, `\n`, `\r`).
    InvalidFieldName { name: String },
    /// Row `row_index` has `actual` values but `fields` declares `expected`.
    RowLengthMismatch { row_index: usize, expected: usize, actual: usize },
}

impl std::fmt::Display for ToonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTypeName { name } =>
                write!(f, "type_name {name:?} contains a structural character"),
            Self::InvalidFieldName { name } =>
                write!(f, "field {name:?} contains a structural character"),
            Self::RowLengthMismatch { row_index, expected, actual } =>
                write!(f, "row {row_index} has {actual} values but {expected} fields declared"),
        }
    }
}

impl std::error::Error for ToonError {}
```

Add to `ToonOptions` impl:

```rust
/// Validate structural invariants.
///
/// `render_toon()` sanitizes gracefully; call this when you need explicit
/// error signals. Called automatically by `AgentResponse::toon_body()`.
///
/// Caller opt-in for direct `render_toon()` usage — see PRINCIPLES.md §1.
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
```

- [ ] **Step 3: Replace `debug_assert!` with clamp/pad alongside it in `render.rs`**

Replace the existing `debug_assert!` block (lines ~137-144 in render.rs):

```rust
// Keep debug_assert! for fast TDD failure in dev builds.
// The clamp/pad below is what runs in release — both coexist intentionally.
for (i, row) in rows.iter().enumerate() {
    debug_assert!(
        row.len() == field_count,
        "row {i} has {} values but {field_count} fields",
        row.len()
    );
}
```

In the row-rendering loop, replace `for (i, val) in row.iter().enumerate()` with:

```rust
for row in rows {
    out.push_str("  ");
    // clamp/pad: extra values ignored, missing values emit empty cells
    for i in 0..field_count {
        if i > 0 {
            out.push(',');
        }
        if let Some(val) = row.get(i) {
            match val {
                Value::Str(s) => { /* existing logic */ }
                Value::Int(n) => { let _ = write!(out, "{n}"); }
                Value::Float(f) => {
                    if f.is_nan() || f.is_infinite() {
                        // Not valid TOON numeric scalars — render as quoted string
                        let _ = write!(out, "\"{}\"", f);
                    } else {
                        let _ = write!(out, "{f}");
                    }
                }
                Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                Value::Null => {}
            }
        }
        // missing cell → empty (Null equivalent)
    }
    out.push('\n');
}
```

- [ ] **Step 4: Fix `toon::list()` to return `Err(ToonError)` for non-object items**

In `lib.rs`, find the `list()` function. The `_ => Map::new()` arm must become an error.

Read the current `list()` implementation, then update:

```rust
pub fn list<T: serde::Serialize>(
    type_name: &str,
    items: &[T],
) -> Result<String, ToonError> {
    // ... existing header extraction logic ...
    for (row_index, item) in items.iter().enumerate() {
        let val = serde_json::to_value(item)
            .map_err(|_| ToonError::InvalidTypeName { name: type_name.to_string() })?;
        match val {
            serde_json::Value::Object(map) => { /* existing logic */ }
            _ => return Err(ToonError::InvalidTypeName {
                name: format!("{type_name} (item {row_index} is not a JSON object)"),
            }),
        }
    }
    // ...
}
```

Note: if `list()` currently returns `String`, update the return type to `Result<String, ToonError>` and update all call sites in the codebase (likely only in tests).

- [ ] **Step 5: Add tests for non-object items and Float specials**

```rust
#[test]
fn list_rejects_non_object_items() {
    let result = list("t", &[1u32, 2, 3]);
    assert!(result.is_err(), "non-object items must return Err");
}

#[test]
fn float_nan_renders_as_quoted_string() {
    let opts = ToonOptions {
        type_name: "t".into(),
        fields: vec!["v".into()],
        rows: vec![vec![Value::Float(f64::NAN)]],
        hints: vec![],
        max_cell_len: 200,
        total_count: None,
    };
    let out = opts.render_toon();
    // NaN must not appear unquoted
    assert!(!out.contains(",NaN,") && !out.contains("  NaN\n"), "got: {out}");
    assert!(out.contains("\"NaN\"") || out.contains("\"nan\""), "got: {out}");
}

#[test]
fn float_inf_renders_as_quoted_string() {
    use Value::Float;
    let opts = ToonOptions {
        type_name: "t".into(),
        fields: vec!["v".into()],
        rows: vec![vec![Float(f64::INFINITY)]],
        hints: vec![],
        max_cell_len: 200,
        total_count: None,
    };
    let out = opts.render_toon();
    assert!(out.contains('"'), "inf must render as quoted, got: {out}");
}
```

Run: `cargo nextest run -p michi-toon`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-toon/src/lib.rs crates/michi-toon/src/render.rs
git commit -m "fix(toon): ToonError+validate(), clamp/pad rows, non-object list Err, quote NaN/Inf"
```

---

## Task 3 — michi-toon: delete `serializer.rs` dead code `[haiku]`

**Finding resolved:** M7

**Files:**
- Modify: `crates/michi-toon/src/lib.rs` (remove `mod serializer;`)
- Delete: `crates/michi-toon/src/serializer.rs`

**Context:** `serializer.rs` declares `ValueSerializer` and claims to stream TOON without intermediate allocations, but `toon::list()` never calls it — it goes through `serde_json::to_value()` instead. Zero test coverage, zero reachable callers.

- [ ] **Step 1: Confirm serializer.rs has no callers**

Run: `grep -r "ValueSerializer\|serializer" crates/michi-toon/src/ --include="*.rs"`
Expected: only references are inside `serializer.rs` itself and its `mod serializer;` declaration in `lib.rs`

- [ ] **Step 2: Remove the module declaration from `lib.rs`**

Delete the line `mod serializer;` (or `pub mod serializer;`) from `crates/michi-toon/src/lib.rs`.

- [ ] **Step 3: Delete the file**

```bash
git rm crates/michi-toon/src/serializer.rs
```

- [ ] **Step 4: Confirm build**

Run: `cargo nextest run -p michi-toon`
Expected: all pass, no warnings about serializer

- [ ] **Step 5: Commit**

```bash
git add crates/michi-toon/src/lib.rs
git commit -m "chore(toon): remove dead ValueSerializer scaffolding"
```

---

## Task 4 — michi-core/kv: escape newlines in `KvValue::Text` + guard NaN/Infinity `[haiku]`

**Findings resolved:** M2 (KvValue::Text newline), G1/L2 (NaN/Inf in kv_value_to_json)

**Files:**
- Modify: `crates/michi-core/src/kv/mod.rs`

**Context:** `push_kv_value()` pushes `KvValue::Text` raw — a `\n` breaks the one-line-per-key format. `kv_value_to_json()` calls `write!(out, "{f:.*}", decimals)` for `Float` — `NaN` and `Infinity` produce non-JSON output silently.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn text_value_with_newline_is_stripped() {
    let item = KvItem { key: "msg".into(), value: KvValue::Text("line1\nline2".into()) };
    let out = render_kv(&[item], None, &[]);
    // Newline in value must not create a second line
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 1, "newline in value must not break KV format, got: {out}");
}

#[test]
fn float_nan_in_json_is_quoted_string() {
    let mut out = String::new();
    kv_value_to_json(&mut out, &KvValue::Float(f64::NAN, 2));
    // Must be valid JSON — not the literal NaN token
    assert!(out.starts_with('"'), "NaN must be JSON-quoted, got: {out}");
}

#[test]
fn float_inf_in_json_is_quoted_string() {
    let mut out = String::new();
    kv_value_to_json(&mut out, &KvValue::Float(f64::INFINITY, 0));
    assert!(out.starts_with('"'), "Inf must be JSON-quoted, got: {out}");
}
```

Run: `cargo nextest run -p michi-core -- kv`
Expected: FAIL

- [ ] **Step 2: Fix `push_kv_value` for `KvValue::Text`**

Change the `KvValue::Text(s)` arm in `push_kv_value()`:

```rust
KvValue::Text(s) => {
    // Strip embedded newlines — they break the one-line-per-key KV format
    if s.contains('\n') || s.contains('\r') {
        for ch in s.chars() {
            if ch != '\n' && ch != '\r' {
                out.push(ch);
            }
        }
    } else {
        out.push_str(s);
    }
}
```

- [ ] **Step 3: Fix `kv_value_to_json` for `KvValue::Float` NaN/Infinity**

Change the `KvValue::Float` arm in `kv_value_to_json()`:

```rust
KvValue::Float(f, decimals) => {
    if f.is_nan() || f.is_infinite() {
        // NaN and Infinity are not valid JSON numbers — render as quoted strings
        let s = format!("{f:.*}", *decimals as usize);
        json_escape_str(out, &s);
    } else {
        let _ = write!(out, "{f:.*}", *decimals as usize);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p michi-core -- kv`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/michi-core/src/kv/mod.rs
git commit -m "fix(kv): strip newlines in Text values, quote NaN/Inf in JSON output"
```

---

## Task 5 — michi-core/error: `ErrorCode::default_class()` + fix `Error::class()` `[sonnet]`

**Finding resolved:** D, H4

**Files:**
- Modify: `crates/michi-core/src/error.rs`

**Context:** `Error::class()` currently only returns `User` or `Transient` — `ErrorClass::Internal` is declared but unreachable. Infrastructure codes (`RateLimited`, `Unavailable`, `Timeout`, `ExternalFailure`) with `retryable: false` are misclassified as `User`. Fix adds `ErrorCode::default_class()` and wires it into `Error::class()`. See spec for full rationale and updated `ErrorClass::Internal` doc text.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn unavailable_with_retryable_false_is_internal_not_user() {
    let err = Error::Domain(
        DomainError::new(ErrorCode::Unavailable, "down").retryable(false)
    );
    assert_eq!(err.class(), ErrorClass::Internal,
        "infrastructure error with retryable=false must be Internal, not User");
}

#[test]
fn invalid_input_is_user() {
    let err = Error::Domain(DomainError::new(ErrorCode::InvalidInput, "bad"));
    assert_eq!(err.class(), ErrorClass::User);
}

#[test]
fn rate_limited_retryable_is_transient() {
    let err = Error::Domain(DomainError::new(ErrorCode::RateLimited, "429"));
    assert_eq!(err.class(), ErrorClass::Transient);
}

#[test]
fn default_class_for_all_codes() {
    use ErrorCode::*;
    assert_eq!(InvalidInput.default_class(), ErrorClass::User);
    assert_eq!(NotFound.default_class(), ErrorClass::User);
    assert_eq!(Unauthorized.default_class(), ErrorClass::User);
    assert_eq!(Forbidden.default_class(), ErrorClass::User);
    assert_eq!(Conflict.default_class(), ErrorClass::User);
    assert_eq!(RateLimited.default_class(), ErrorClass::Internal);
    assert_eq!(Unavailable.default_class(), ErrorClass::Internal);
    assert_eq!(Timeout.default_class(), ErrorClass::Internal);
    assert_eq!(ExternalFailure.default_class(), ErrorClass::Internal);
}

#[test]
fn is_retryable_not_affected_by_internal_class() {
    // Internal is not Transient, so is_retryable() must remain false
    let err = Error::Domain(
        DomainError::new(ErrorCode::Unavailable, "down").retryable(false)
    );
    assert!(!err.is_retryable());
}
```

Run: `cargo nextest run -p michi-core -- error`
Expected: FAIL

- [ ] **Step 2: Add `ErrorCode::default_class()`**

In `error.rs`, after the existing `is_retryable_by_default()` impl:

```rust
/// The default [`ErrorClass`] for this code, independent of `DomainError.retryable`.
///
/// Infrastructure codes (`RateLimited`, `Unavailable`, `Timeout`, `ExternalFailure`)
/// return `Internal`; caller-fault codes return `User`. Used by [`Error::class()`].
#[must_use]
pub fn default_class(&self) -> ErrorClass {
    match self {
        Self::InvalidInput | Self::NotFound | Self::Unauthorized
        | Self::Forbidden | Self::Conflict => ErrorClass::User,
        Self::RateLimited | Self::Unavailable | Self::Timeout
        | Self::ExternalFailure => ErrorClass::Internal,
    }
}
```

- [ ] **Step 3: Fix `Error::class()` to use `default_class()`**

Find `Error::class()` in the file and update:

```rust
#[must_use]
pub fn class(&self) -> ErrorClass {
    match self {
        Self::Domain(d) if d.retryable => ErrorClass::Transient,
        Self::Domain(d) => d.code.default_class(),
        Self::InvalidInput(_) | Self::NotFound(_) => ErrorClass::User,
    }
}
```

- [ ] **Step 4: Update `ErrorClass::Internal` doc comment**

```rust
/// A downstream or infrastructure failure that is not the caller's direct error
/// and is not expected to self-resolve without intervention. Note: `RateLimited`
/// falls here when `retryable: false` — the caller may have contributed to the
/// rate-limit condition, but at classification time the error is not self-resolving.
/// michi does not recommend automatic retry for `Internal` errors.
Internal,
```

- [ ] **Step 5: Run all tests**

Run: `cargo nextest run -p michi-core`
Expected: all pass including the new tests

- [ ] **Step 6: Commit**

```bash
git add crates/michi-core/src/error.rs
git commit -m "fix(error): make ErrorClass::Internal reachable via ErrorCode::default_class()"
```

---

## Task 6 — michi-core/error: `DomainError::to_call_tool_result()` + `render_json()` `[sonnet]`

**Findings resolved:** G, M9

**Files:**
- Modify: `crates/michi-core/src/error.rs`
- Modify: `crates/michi-core/src/mcp.rs` (fix unwrap_or(null) on line 89)

**Context:** There is no path from `DomainError` to `CallToolResult`. Add `to_call_tool_result()` and `render_json()`. `render_json()` must include `hints` and `recovery` in the same shape as `AgentResponse::render_json()` — the adversarial review found the schema divergence between the two paths is a real correctness gap. Also fix `mcp.rs:89` where `serde_json::from_str(...).unwrap_or(serde_json::Value::Null)` silently discards structured content on parse failure.

Read `AgentResponse::render_json()` in `crates/michi-core/src/response.rs` to understand the JSON structure before implementing — the hints/recovery shape must match exactly.

- [ ] **Step 1: Read `response.rs` render_json to understand the target shape**

Read `crates/michi-core/src/response.rs` — find `render_json()` and note the exact JSON keys for hints and recovery. The `DomainError::render_json()` must produce the same top-level key names.

- [ ] **Step 2: Write failing tests**

```rust
#[test]
fn domain_error_render_json_basic() {
    let err = DomainError::new(ErrorCode::NotFound, "Item 42 not found");
    let json = err.render_json();
    assert!(json.contains("\"isError\":true"), "got: {json}");
    assert!(json.contains("\"error\":\"not_found\""), "got: {json}");
    assert!(json.contains("\"message\":\"Item 42 not found\""), "got: {json}");
    assert!(json.contains("\"retryable\":false"), "got: {json}");
}

#[test]
fn domain_error_render_json_includes_hints() {
    let err = DomainError::new(ErrorCode::NotFound, "not found")
        .hint("Try searching with get_items");
    let json = err.render_json();
    assert!(json.contains("\"hints\""), "got: {json}");
    assert!(json.contains("Try searching"), "got: {json}");
}

#[test]
fn domain_error_to_call_tool_result_is_error() {
    let err = DomainError::new(ErrorCode::Unavailable, "down");
    let result = err.to_call_tool_result();
    assert!(result.is_error);
    assert!(!result.content.is_empty());
    assert!(result.content[0].text.contains("unavailable"));
}

#[test]
fn domain_error_json_escapes_message() {
    let err = DomainError::new(ErrorCode::InvalidInput, r#"field "name" invalid"#);
    let json = err.render_json();
    // Quotes in message must be JSON-escaped
    assert!(json.contains("\\\"name\\\""), "got: {json}");
}
```

Run: `cargo nextest run -p michi-core -- error`
Expected: FAIL

- [ ] **Step 3: Implement `DomainError::render_json()`**

```rust
/// Render as a JSON object for `structured_content` or telemetry.
///
/// Shape: `{"isError":true,"error":"<code>","message":"<msg>","retryable":<bool>,"hints":[...],"recovery":{...}}`
///
/// Matches the top-level key structure of `AgentResponse::render_json()` so a
/// client reading `structured_content` does not need two schemas for error output.
#[must_use]
pub fn render_json(&self) -> String {
    let mut out = String::with_capacity(128 + self.message.len() + self.hints.len() * 64);
    out.push_str("{\"isError\":true,\"error\":");
    crate::kv::json_escape_str(&mut out, self.code.label());
    out.push_str(",\"message\":");
    crate::kv::json_escape_str(&mut out, &self.message);
    out.push_str(",\"retryable\":");
    out.push_str(if self.retryable { "true" } else { "false" });

    // hints array — same shape as AgentResponse::render_json()
    out.push_str(",\"hints\":[");
    for (i, hint) in self.hints.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        crate::kv::json_escape_str(&mut out, hint.as_str());
    }
    out.push(']');

    // recovery — null if absent, same shape as AgentResponse::render_json()
    out.push_str(",\"recovery\":");
    if let Some(r) = &self.recovery {
        // Render recovery as object: {"tool":"<name>","suggestedParams":{...}}
        // Follow the exact serialization pattern from AgentResponse::render_json()
        // Read response.rs before implementing to ensure field names match.
        crate::recovery::render_recovery_json(&mut out, r);
    } else {
        out.push_str("null");
    }

    out.push('}');
    out
}
```

**Important:** Before implementing `render_recovery_json`, read `crates/michi-core/src/recovery.rs` to understand `RecoveryHint`'s fields, then read `AgentResponse::render_json()` recovery serialization to match its exact key names. Add `pub(crate) fn render_recovery_json(out: &mut String, r: &RecoveryHint)` in `recovery.rs`.

- [ ] **Step 4: Implement `DomainError::to_call_tool_result()`**

```rust
/// Build an MCP [`CallToolResult`] representing this error.
///
/// Sets `is_error: true`. Text content uses [`DomainError::render()`];
/// `structured_content` uses [`DomainError::render_json()`] (includes hints
/// and recovery).
///
/// **Canonical path:** call this directly when you hold a `DomainError`.
/// Do not wrap in `AgentResponse::as_error()` — that loses typed fields.
/// See `ARCHITECTURE.md` §"NAPI Boundary Contract" for the error output path.
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
```

- [ ] **Step 5: Fix `mcp.rs` line 89 — silent null replacement**

Find the `serde_json::from_str(...).unwrap_or(serde_json::Value::Null)` in `mcp.rs`. Replace with an explicit match that either propagates the real content or emits an empty string with a comment explaining why:

```rust
// structured_content is caller-supplied pre-serialized JSON; parse errors
// mean the caller passed non-JSON. Treat as absent rather than silently
// replacing with null, which could hide a serialization bug.
let structured_content = match serde_json::from_str::<serde_json::Value>(&r.structured_content) {
    Ok(_) => r.structured_content.clone(),
    Err(_) => String::new(),
};
```

Adjust surrounding code as needed to use `structured_content` where the old expression was.

- [ ] **Step 6: Run tests**

Run: `cargo nextest run -p michi-core`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add crates/michi-core/src/error.rs crates/michi-core/src/recovery.rs crates/michi-core/src/mcp.rs
git commit -m "feat(error): DomainError::to_call_tool_result() + render_json() with hints/recovery"
```

---

## Task 7 — michi-core: port `PartialSuccess`/`FailedOp`, fix `FailedOp.operation` escaping, add snapshot tests `[sonnet]`

**Findings resolved:** S2 (port PartialSuccess/FailedOp), M1 (FailedOp.operation escaping), M8 (snapshots for error/recovery/idempotency), L5 (doc comment boundary)

**Files:**
- Create: `crates/michi-core/src/idempotency.rs`
- Modify: `crates/michi-core/src/lib.rs` (add `pub mod idempotency;`)
- Modify: `tests/snapshot_tests.rs`

**Context:** `src/idempotency.rs` has `PartialSuccess`, `FailedOp`, `AlreadyDone`, `IdempotencyKey`, and `render_already_done`. `AlreadyDone`/`IdempotencyKey` are already in `crates/michi-resilience`. `PartialSuccess` and `FailedOp` use michi-core types (`RecoveryHint`, `KvValue`, `Hint`) so they belong in `crates/michi-core/src/idempotency.rs`. Port them, fix the escaping bug in `FailedOp.operation` (push raw, should use `escape_value_quoted` like `reason`), add L5 doc comments, and add insta snapshot tests.

- [ ] **Step 1: Read source files**

Read `src/idempotency.rs` (the full file) to understand `PartialSuccess::render()` and its test suite. Also read `tests/snapshot_tests.rs` to understand the snapshot test pattern used.

- [ ] **Step 2: Write failing snapshot tests**

Add to `tests/snapshot_tests.rs`:

```rust
#[test]
fn snapshot_partial_success_full() {
    use michi::idempotency::{PartialSuccess, FailedOp};
    use michi::recovery::RecoveryHint;
    use michi::kv::KvValue;
    let ps = PartialSuccess {
        completed: vec!["create_issue".into(), "add_label".into()],
        failed: vec![FailedOp {
            operation: "assign_user".into(),
            reason: "User 'ghost' not found".into(),
            recovery: Some(
                RecoveryHint::new("assign_user")
                    .param("user", KvValue::Text("alice".into())),
            ),
        }],
        skipped: vec!["notify_team".into()],
    };
    insta::assert_snapshot!(ps.render());
}

#[test]
fn snapshot_partial_success_operation_with_comma_is_escaped() {
    use michi::idempotency::{PartialSuccess, FailedOp};
    // Regression: operation with comma must be quoted to avoid column misalignment
    let ps = PartialSuccess {
        completed: vec![],
        failed: vec![FailedOp {
            operation: "create,item".into(),
            reason: "failed".into(),
            recovery: None,
        }],
        skipped: vec![],
    };
    let out = ps.render();
    // The comma in operation must be inside quotes, not a column separator
    assert!(out.contains(r#""create,item""#), "operation with comma must be quoted, got: {out}");
}

#[test]
fn snapshot_domain_error_render() {
    use michi::error::{DomainError, ErrorCode};
    use michi::hints::Hint;
    let err = DomainError::new(ErrorCode::NotFound, "Item 42 not found")
        .hint("Try searching with get_items first");
    insta::assert_snapshot!(err.render());
}
```

Run: `cargo nextest run -- snapshot_partial`
Expected: FAIL (module not found)

- [ ] **Step 3: Create `crates/michi-core/src/idempotency.rs`**

Port `PartialSuccess`, `FailedOp` from `src/idempotency.rs`. Key changes:
1. Update imports to use `michi_toon::escape_value_quoted` (or re-export it via the toon crate)
2. **Fix M1:** In `PartialSuccess::render()`, the `operation` field must use `escape_value_quoted` just like `reason` does:

```rust
// Was:
out.push_str(&f.operation);
out.push(',');
out.push_str(&escape_value_quoted(&f.reason));

// Must be:
out.push_str(&escape_value_quoted(&f.operation));
out.push(',');
out.push_str(&escape_value_quoted(&f.reason));
```

3. **Add L5 doc comment** to `PartialSuccess`:

```rust
/// # Caller responsibility
///
/// The caller determines which operations belong in `completed`, `failed`, and
/// `skipped` — michi does not validate that these are mutually exclusive or
/// exhaustive. If an operation fully succeeded, use [`render_already_done`]
/// instead; `PartialSuccess` is for multi-step operations where *some* steps
/// failed.
```

4. Port all tests from `src/idempotency.rs`'s test module.

- [ ] **Step 4: Expose from `lib.rs`**

Add to `crates/michi-core/src/lib.rs`:

```rust
pub mod idempotency;
```

- [ ] **Step 5: Run tests and accept snapshots**

Run: `cargo nextest run -- snapshot`
Expected: new snapshots pending

Run: `cargo insta accept` (or `just snapshots`)

Run: `cargo nextest run`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-core/src/idempotency.rs crates/michi-core/src/lib.rs tests/snapshot_tests.rs
git commit -m "feat(core): port PartialSuccess/FailedOp, fix operation escaping, add snapshots"
```

---

## Task 8 — michi-core/response: 5 Rust unit tests + wire `validate()` into `toon_body()` `[haiku]`

**Findings resolved:** H1, C (render_json tests), A (wiring validate into toon_body)

**Files:**
- Modify: `crates/michi-core/src/response.rs`

**Context:** `render_json()` has zero Rust-level tests — only JS coverage. The recovery-param type regression (strings vs. typed literals) has been fixed in code but is unguarded. Also, `validate()` was added to `ToonOptions` in Task 2 but is not called from `AgentResponse::toon_body()` — wire it in so malformed TOON input surfaces as an error response rather than corrupt output.

- [ ] **Step 1: Write the 5 failing tests**

Add to `response.rs` in its `#[cfg(test)]` module:

```rust
#[test]
fn render_json_basic_structure() {
    let r = AgentResponse::new().body("hello world");
    let json = r.render_json();
    assert!(json.contains("\"isError\":false"), "got: {json}");
    assert!(json.contains("\"body\""), "got: {json}");
    assert!(json.contains("\"hints\":[]"), "got: {json}");
}

#[test]
fn render_json_is_error_flag() {
    let r = AgentResponse::new().body("fail").as_error();
    let json = r.render_json();
    assert!(json.contains("\"isError\":true"), "got: {json}");
}

#[test]
fn render_json_recovery_int_param_is_numeric_not_string() {
    // Regression guard: this exact bug was previously found and fixed.
    // params must serialize as typed JSON, not quoted strings.
    use crate::recovery::RecoveryHint;
    use crate::kv::KvValue;
    let recovery = RecoveryHint::new("retry").param("limit", KvValue::Int(10));
    let r = AgentResponse::new().body("hit limit").recovery(recovery);
    let json = r.render_json();
    // Must be `"limit":10` not `"limit":"10"`
    assert!(json.contains("\"limit\":10"), "Int param must be unquoted, got: {json}");
    assert!(!json.contains("\"limit\":\"10\""), "Int param must not be quoted, got: {json}");
}

#[test]
fn render_json_hint_with_quotes_is_escaped() {
    let r = AgentResponse::new().hint(r#"Use "get_item" instead"#);
    let json = r.render_json();
    assert!(json.contains("\\\"get_item\\\""), "quotes in hints must be escaped, got: {json}");
}

#[test]
fn to_call_tool_result_composes_correctly() {
    use crate::audience::Audience;
    let r = AgentResponse::new().body("ok").as_error();
    let result = r.to_call_tool_result();
    assert!(result.is_error);
    assert!(!result.content.is_empty(), "content must not be empty");
    assert!(result.content.iter().any(|b| b.audience.contains(&Audience::Assistant)));
    // structured_content must be parseable JSON
    assert!(result.structured_content.starts_with('{'));
}
```

Run: `cargo nextest run -p michi-core -- response`
Expected: FAIL on most (render_json not tested before)

- [ ] **Step 2: Run tests to confirm green**

These tests should pass if the existing `render_json()` is correct. If any fail, fix `render_json()` rather than the tests.

Run: `cargo nextest run -p michi-core -- response`
Expected: all 5 pass

- [ ] **Step 3: Wire `validate()` into `toon_body()`**

Find `toon_body()` (or the equivalent method that uses `ToonOptions`) in `response.rs`. Add a `validate()` call before rendering:

```rust
// In the method that renders TOON (likely called from render_toon() or items()):
if let Err(e) = toon_opts.validate() {
    // Surface as an error response rather than corrupt TOON output
    return format!("error: toon_validation_failed\nmessage: {e}\n");
}
```

Adjust the exact return type / string format to match what the method currently returns.

- [ ] **Step 4: Add a test for the validation wiring**

```rust
#[test]
fn toon_body_with_invalid_type_name_returns_error_string() {
    // validate() must be wired in — malformed input must not produce corrupt TOON
    let r = AgentResponse::new();
    // Build ToonOptions with an invalid type_name (contains '[')
    // then verify render_toon() / render() produces an error string, not garbage TOON
    // Exact test depends on the API surface — read toon_body() before writing this test
}
```

(The exact test body depends on the `AgentResponse` API for setting `ToonOptions` — read the method before writing.)

- [ ] **Step 5: Run all tests**

Run: `cargo nextest run -p michi-core`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-core/src/response.rs
git commit -m "test(response): add 5 Rust render_json tests, wire ToonOptions::validate() into toon_body()"
```

---

## Task 9 — michi-core/response: `as_error`/`render_for`/`has_human_content`/`render_hints_only` tests + schema alignment `[sonnet]`

**Findings resolved:** S3 (AgentResponse method coverage), integration schema alignment

**Files:**
- Modify: `crates/michi-core/src/response.rs`
- Modify: `tests/snapshot_tests.rs`

**Context:** `as_error()`, `render_for()`, `has_human_content()`, `render_hints_only()` have no Rust-level tests — only JS coverage. The schema alignment integration test asserts that `AgentResponse::to_call_tool_result()` and `DomainError::to_call_tool_result()` produce structurally compatible `structured_content` JSON (same top-level keys).

- [ ] **Step 1: Write failing tests for AgentResponse methods**

```rust
#[test]
fn as_error_sets_is_error_flag() {
    let r = AgentResponse::new().body("fail").as_error();
    assert!(r.render_json().contains("\"isError\":true"));
}

#[test]
fn has_human_content_false_when_no_user_content() {
    let r = AgentResponse::new().body("agent only");
    assert!(!r.has_human_content());
}

#[test]
fn render_hints_only_produces_help_block() {
    let r = AgentResponse::new().hint("Try again with fewer params");
    let hints_text = r.render_hints_only();
    assert!(hints_text.contains("help[1]:"), "got: {hints_text}");
    assert!(hints_text.contains("Try again"), "got: {hints_text}");
}

#[test]
fn render_for_assistant_returns_toon_or_kv_content() {
    use crate::audience::Audience;
    let r = AgentResponse::new().body("agent body");
    let out = r.render_for(Audience::Assistant);
    assert!(!out.is_empty());
}
```

- [ ] **Step 2: Add schema alignment integration test**

Add to `tests/snapshot_tests.rs` or a new `tests/schema_alignment.rs`:

```rust
/// Asserts that AgentResponse and DomainError produce structurally compatible
/// structured_content JSON — same top-level keys — so a client does not need
/// two schemas depending on which code path produced the error.
#[test]
fn error_structured_content_schemas_are_compatible() {
    use michi::error::{DomainError, ErrorCode};
    use michi::AgentResponse;

    let domain_json = DomainError::new(ErrorCode::NotFound, "missing").render_json();
    let response_json = AgentResponse::new().body("fail").as_error().render_json();

    // Parse both to check top-level keys
    let domain: serde_json::Value = serde_json::from_str(&domain_json)
        .expect("DomainError::render_json must produce valid JSON");
    let response: serde_json::Value = serde_json::from_str(&response_json)
        .expect("AgentResponse::render_json must produce valid JSON");

    let domain_keys: std::collections::BTreeSet<_> =
        domain.as_object().unwrap().keys().collect();
    let response_keys: std::collections::BTreeSet<_> =
        response.as_object().unwrap().keys().collect();

    // DomainError keys must be a subset of AgentResponse keys (DomainError adds
    // "error" and "retryable"; AgentResponse adds its own fields)
    // At minimum both must have: isError, hints
    assert!(domain_keys.contains("isError"), "DomainError JSON must have isError");
    assert!(domain_keys.contains("hints"), "DomainError JSON must have hints");
    assert!(response_keys.contains("isError"), "AgentResponse JSON must have isError");
    assert!(response_keys.contains("hints"), "AgentResponse JSON must have hints");
}
```

Note: this test requires the `serde` feature. Run with `cargo nextest run --features serde`.

- [ ] **Step 3: Run all tests**

Run: `cargo nextest run -p michi-core`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/michi-core/src/response.rs tests/
git commit -m "test(response): Rust tests for as_error/render_for/has_human_content/render_hints_only + schema alignment"
```

---

## Task 10 — michi-resilience: normalizing `new()` + `try_new()` + `RetryConfigError` + napi.rs `[sonnet]`

**Findings resolved:** F, M6 (max_delay=0 drops retry_after), L3 (jitter_factor unenforced)

**Files:**
- Modify: `crates/michi-resilience/src/lib.rs`
- Modify: `src/napi.rs`

**Context:** `RetryConfig::new()` is currently a dumb struct literal — no normalization, no validation. `max_delay=0` silently drops `retry_after`. `jitter_factor > 1.0` silently exceeds the intended range. Decision F2: `new()` stays infallible but normalizes inputs; `try_new()` is an additive strict constructor. `napi.rs:274` calls `new()` — it stays as-is (already uses the normalizing variant).

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn new_clamps_jitter_factor_above_one() {
    let c = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), 2.5);
    assert!(c.jitter_factor <= 1.0, "jitter_factor must be clamped to 1.0, got {}", c.jitter_factor);
}

#[test]
fn new_clamps_negative_jitter_to_zero() {
    let c = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), -0.5);
    assert!(c.jitter_factor >= 0.0, "jitter_factor must be clamped to 0.0, got {}", c.jitter_factor);
}

#[test]
fn new_clamps_base_delay_exceeding_max() {
    let c = RetryConfig::new(3, Duration::from_secs(60), Duration::from_secs(10), 0.2);
    assert!(c.base_delay <= c.max_delay, "base_delay must not exceed max_delay after normalization");
}

#[test]
fn new_floors_zero_max_delay_to_one_ms() {
    let c = RetryConfig::new(3, Duration::ZERO, Duration::ZERO, 0.0);
    assert!(c.max_delay >= Duration::from_millis(1), "max_delay=0 must be floored to 1ms");
}

#[test]
fn try_new_rejects_zero_max_delay() {
    let result = RetryConfig::try_new(3, Duration::from_millis(500), Duration::ZERO, 0.2);
    assert!(matches!(result, Err(RetryConfigError::MaxDelayIsZero)));
}

#[test]
fn try_new_rejects_base_exceeding_max() {
    let result = RetryConfig::try_new(3, Duration::from_secs(60), Duration::from_secs(10), 0.2);
    assert!(matches!(result, Err(RetryConfigError::BaseDelayExceedsMaxDelay { .. })));
}

#[test]
fn try_new_rejects_out_of_range_jitter() {
    let result = RetryConfig::try_new(3, Duration::from_millis(100), Duration::from_secs(30), 1.5);
    assert!(matches!(result, Err(RetryConfigError::JitterFactorOutOfRange { .. })));
}

#[test]
fn try_new_accepts_valid_config() {
    let result = RetryConfig::try_new(3, Duration::from_millis(500), Duration::from_secs(30), 0.2);
    assert!(result.is_ok());
}
```

Run: `cargo nextest run -p michi-resilience`
Expected: FAIL

- [ ] **Step 2: Add `RetryConfigError`**

```rust
/// Error returned by [`RetryConfig::try_new()`] when parameters are invalid.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryConfigError {
    /// `max_delay` must be non-zero (zero silently drops server `retry_after` hints).
    MaxDelayIsZero,
    /// `base_delay` must not exceed `max_delay`.
    BaseDelayExceedsMaxDelay { base: std::time::Duration, max: std::time::Duration },
    /// `jitter_factor` must be in `[0.0, 1.0]`.
    JitterFactorOutOfRange { factor: f64 },
}

impl std::fmt::Display for RetryConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxDelayIsZero => f.write_str("max_delay must be non-zero"),
            Self::BaseDelayExceedsMaxDelay { base, max } =>
                write!(f, "base_delay ({base:?}) must not exceed max_delay ({max:?})"),
            Self::JitterFactorOutOfRange { factor } =>
                write!(f, "jitter_factor {factor} is outside [0.0, 1.0]"),
        }
    }
}

impl std::error::Error for RetryConfigError {}
```

- [ ] **Step 3: Replace `RetryConfig::new()` with normalizing version, add `try_new()`**

```rust
impl RetryConfig {
    /// Normalizing constructor — clamps all inputs to valid ranges.
    ///
    /// - `jitter_factor` is clamped to `[0.0, 1.0]`
    /// - `base_delay` is clamped to `min(base_delay, max_delay)`
    /// - `max_delay` is floored to `Duration::from_millis(1)` (prevents silent
    ///   `retry_after` discard when `max_delay` is zero)
    ///
    /// Prefer [`RetryConfig::try_new()`] when you need explicit validation errors.
    #[must_use]
    pub fn new(
        max_retries: u32,
        base_delay: std::time::Duration,
        max_delay: std::time::Duration,
        jitter_factor: f64,
    ) -> Self {
        let max_delay = max_delay.max(std::time::Duration::from_millis(1));
        let base_delay = base_delay.min(max_delay);
        let jitter_factor = jitter_factor.clamp(0.0, 1.0);
        Self { max_retries, base_delay, max_delay, jitter_factor }
    }

    /// Strict constructor — returns `Err` if any parameter is out of range.
    ///
    /// For NAPI-boundary code, use the normalizing [`RetryConfig::new()`] instead.
    pub fn try_new(
        max_retries: u32,
        base_delay: std::time::Duration,
        max_delay: std::time::Duration,
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

- [ ] **Step 4: Verify `src/napi.rs` still compiles**

`napi.rs:274` calls `RetryConfig::new(...)` — it now gets normalization for free. Confirm it compiles and no behavior change is needed.

Run: `cargo build --features napi`
Expected: compiles cleanly

- [ ] **Step 5: Run all tests**

Run: `cargo nextest run -p michi-resilience`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-resilience/src/lib.rs
git commit -m "feat(resilience): normalizing RetryConfig::new(), add try_new() + RetryConfigError"
```

---

## Task 11 — michi-resilience: port correctness tests from `src/` + `PartialSuccess`/`FailedOp` `[sonnet]`

**Findings resolved:** M3, M4, L4 (test coverage), S2 (PartialSuccess/FailedOp in resilience module)

**Files:**
- Modify: `crates/michi-resilience/src/lib.rs`
- Read: `src/resilience/mod.rs` (source of tests to port)

**Context:** `src/resilience/mod.rs` has a thorough test suite that was not ported to `crates/michi-resilience`: HTTP-date correctness tests, `retry_after: Some(_)` path tests, `is_retryable_status` coverage including the 500-not-retryable documented decision. Also: `crates/michi-resilience` currently re-declares `AlreadyDone`/`IdempotencyKey` but `PartialSuccess`/`FailedOp` were not ported (they went into michi-core in Task 7 — just re-export them here for convenience, or add doc comment pointing users to michi-core).

- [ ] **Step 1: Read `src/resilience/mod.rs` test suite**

Read the full test module in `src/resilience/mod.rs`. Note every test function and what it covers. Identify which tests are already present in `crates/michi-resilience/src/lib.rs` and which are missing.

- [ ] **Step 2: Port missing HTTP-date correctness tests**

At minimum, port these test patterns (read src/ for exact values):

```rust
#[test]
fn parse_retry_after_valid_imf_fixdate() {
    // e.g. "Wed, 21 Oct 2026 07:28:00 GMT" must parse to a positive duration
    // Use parse_retry_after_at with a fixed "now" earlier than the target date
    let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_761_027_600);
    // A date 120 seconds after `now`
    let target_secs = 1_761_027_600 + 120;
    // Construct the IMF-fixdate string for target_secs (use the known format)
    // assert result == Some(Duration::from_secs(120))
}

#[test]
fn parse_retry_after_past_date_returns_zero_or_none() {
    // A date in the past must return zero duration, not panic
}

#[test]
fn parse_retry_after_integer_seconds() {
    let result = parse_retry_after("120");
    assert_eq!(result, Some(std::time::Duration::from_secs(120)));
}
```

- [ ] **Step 3: Port `retry_after: Some(_)` path tests (M3)**

```rust
#[test]
fn retry_after_raises_delay_above_jittered_backoff() {
    let config = RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(60), 0.0);
    let server_wants = Duration::from_secs(30);
    let delay = next_retry_delay(&config, 0, 0.0, Some(server_wants));
    assert!(delay.unwrap() >= server_wants, "retry_after must floor the delay");
}

#[test]
fn retry_after_capped_by_max_delay() {
    let config = RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(10), 0.0);
    let server_wants = Duration::from_secs(60); // exceeds max_delay
    let delay = next_retry_delay(&config, 0, 0.0, Some(server_wants));
    assert!(delay.unwrap() <= config.max_delay, "retry_after must be capped by max_delay");
}
```

- [ ] **Step 4: Port `is_retryable_status` tests (L4)**

```rust
#[test]
fn status_429_is_retryable() {
    assert!(is_retryable_status(429));
}

#[test]
fn status_503_is_retryable() {
    assert!(is_retryable_status(503));
}

#[test]
fn status_500_is_not_retryable() {
    // Documented design decision — 500 = server bug, retrying reproduces it
    assert!(!is_retryable_status(500), "500 must not be retryable (documented decision)");
}

#[test]
fn status_200_is_not_retryable() {
    assert!(!is_retryable_status(200));
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo nextest run -p michi-resilience`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-resilience/src/lib.rs
git commit -m "test(resilience): port correctness tests from src/ (HTTP-date, retry_after path, is_retryable_status)"
```

---

## Task 12 — michi-resilience: fix `parse_http_date` calendar validation `[opus]`

**Finding resolved:** H5 (invalid calendar dates silently produce wrong durations)

**Files:**
- Modify: `crates/michi-resilience/src/lib.rs`

**Context:** `parse_http_date` validates `day in 1..=31` but never checks day against the actual days in the parsed month, or leap-year rules. `"Wed, 30 Feb 2026 07:28:00 GMT"` does not return `None` — it silently produces a wrong date. The fix must add month-aware day validation before calling `days_from_civil`. This requires correct leap-year logic (divisible by 4, except centuries, except 400-year centuries). Use Opus because the algorithm is subtle and wrong calendar math is invisible in passing tests.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn parse_http_date_rejects_feb_30() {
    let now = std::time::UNIX_EPOCH;
    assert!(
        parse_retry_after_at("Wed, 30 Feb 2026 07:28:00 GMT", now).is_none(),
        "Feb 30 is impossible and must return None"
    );
}

#[test]
fn parse_http_date_rejects_feb_29_non_leap() {
    let now = std::time::UNIX_EPOCH;
    // 2026 is not a leap year (not divisible by 4... wait, 2026/4 = 506.5, not a leap year)
    assert!(
        parse_retry_after_at("Sun, 29 Feb 2026 00:00:00 GMT", now).is_none(),
        "Feb 29 in a non-leap year must return None"
    );
}

#[test]
fn parse_http_date_accepts_feb_29_leap_year() {
    let now = std::time::UNIX_EPOCH;
    // 2028 is a leap year (divisible by 4, not a century)
    // This should parse to Some(...), not None
    let result = parse_retry_after_at("Tue, 29 Feb 2028 00:00:00 GMT", now);
    assert!(result.is_some(), "Feb 29 2028 is valid (leap year)");
}

#[test]
fn parse_http_date_rejects_april_31() {
    let now = std::time::UNIX_EPOCH;
    assert!(
        parse_retry_after_at("Thu, 31 Apr 2026 00:00:00 GMT", now).is_none(),
        "April has 30 days; day 31 must return None"
    );
}

#[test]
fn parse_http_date_accepts_valid_31st() {
    let now = std::time::UNIX_EPOCH;
    // January has 31 days
    let result = parse_retry_after_at("Sat, 31 Jan 2026 00:00:00 GMT", now);
    assert!(result.is_some(), "Jan 31 is valid");
}
```

Run: `cargo nextest run -p michi-resilience -- parse_http_date`
Expected: FAIL (feb_30 and april_31 tests pass — they incorrectly accept impossible dates)

- [ ] **Step 2: Add `days_in_month` helper and wire it into `parse_http_date`**

```rust
/// Returns the number of days in the given month for the given year.
/// Returns `None` for month values outside 1..=12.
fn days_in_month(year: i64, month: u64) -> Option<u64> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // Leap year: divisible by 4, except centuries unless divisible by 400
            let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if is_leap { 29 } else { 28 }
        }
        _ => return None,
    })
}
```

In `parse_http_date`, after parsing `day`, `month`, and `year`, add:

```rust
let max_day = days_in_month(year, month)?;
if day == 0 || day > max_day {
    return None;
}
```

Remove the old `day == 0 || day > 31` check (replace it with the new per-month check).

- [ ] **Step 3: Run all tests**

Run: `cargo nextest run -p michi-resilience`
Expected: all pass including the new calendar tests and all previously-passing tests

- [ ] **Step 4: Commit**

```bash
git add crates/michi-resilience/src/lib.rs
git commit -m "fix(resilience): validate day-of-month against actual month length in parse_http_date"
```

---

## Task 13 — michi-truncate: recompute `signal` after hard-cap `[haiku]`

**Finding resolved:** E, M5

**Files:**
- Modify: `crates/michi-truncate/src/lib.rs`

**Context:** `TruncateResult::signal` is computed from the full suffix string before hard-capping. When `max_chars` is smaller than the suffix length, `content` is truncated mid-suffix but `signal` retains the full uncapped text. After the hard-cap, `signal` must be recomputed as the text that actually appears in `content` after the kept chars.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn signal_reflects_what_is_actually_in_content() {
    // max_chars smaller than the suffix length
    // suffix might be e.g. " [truncated, full=true]" (23 chars)
    // if max_chars=5, content can only hold 5 chars total, signal won't fit
    let result = truncate("hello world this is long content", 5, "full=true");
    match result {
        TruncateResult::Truncated(t) => {
            // signal must be None (no room) or must actually appear in content
            if let Some(sig) = &t.signal {
                assert!(t.content.contains(sig.as_str()),
                    "signal must appear in content, got content={:?} signal={:?}", t.content, sig);
            }
            // content must be exactly max_chars chars or less
            assert!(t.content.chars().count() <= 5, "content exceeded max_chars");
        }
        TruncateResult::NotTruncated(_) => panic!("expected truncation"),
    }
}
```

Run: `cargo nextest run -p michi-truncate`
Expected: FAIL

- [ ] **Step 2: Read the current truncation logic**

Read `crates/michi-truncate/src/lib.rs` fully to understand where `signal` is set and where the hard-cap happens. Identify the exact line order.

- [ ] **Step 3: Fix signal computation**

After the hard-cap is applied to `content`, recompute `signal` from what's actually in `content`:

```rust
// After hard-cap:
// content is now the final capped string
// Recompute signal as the bytes in content that follow the kept chars
let signal = if result.len() > kept_len {
    // There's suffix text actually in content
    let suffix_in_content = &result[kept_len..];
    let trimmed = suffix_in_content.trim_start();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
} else {
    None // No room for any signal text
};
```

Adjust to match the actual variable names in the file.

Update `Truncated.signal` doc comment to match the spec:

```rust
/// The truncation signal text actually embedded in `content`, starting
/// immediately after the kept characters (leading space stripped).
/// `None` when not truncated or when `max_chars` was so small that no
/// signal text fit.
pub signal: Option<String>,
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p michi-truncate`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/michi-truncate/src/lib.rs
git commit -m "fix(truncate): recompute signal from content after hard-cap"
```

---

## Task 14 — Docs: L8 pipeline step ID, L9 KV padding, L5 AlreadyDone/PartialSuccess boundary `[haiku]`

**Findings resolved:** L8, L9, L5 (doc comments only — no code changes)

**Files:**
- Modify: `crates/michi-core/src/pipeline/mod.rs`
- Modify: `crates/michi-core/src/kv/mod.rs`
- Modify: `crates/michi-resilience/src/lib.rs` (AlreadyDone doc)

**Context:** Four doc comment gaps where silent behavior surprised the audit and needs to be explicitly documented as a deliberate contract.

- [ ] **Step 1: Add pipeline step ID doc (L8)**

Find the `PipelineStep` struct in `crates/michi-core/src/pipeline/mod.rs`. Add to its `id` field doc comment:

```rust
/// Step identifier. michi does not validate uniqueness — callers are
/// responsible for ensuring step IDs are distinct within a pipeline if they
/// intend to reference steps by ID. Duplicate IDs render without error but
/// produce ambiguous output.
pub id: String,
```

- [ ] **Step 2: Add KV key padding Unicode doc (L9)**

Find `render_kv()` in `crates/michi-core/src/kv/mod.rs`. Add to or update its doc comment:

```rust
/// Key padding uses `chars().count()`, not display width. Keys containing
/// CJK or other wide Unicode characters will be misaligned in monospace
/// terminals. This is acceptable because michi targets agent readability
/// primarily, not human terminal rendering.
```

- [ ] **Step 3: Add u64-near-MAX clamp doc (L1)**

Find the `From<u64> for Value` impl in `crates/michi-toon/src/render.rs`:

```rust
impl From<u64> for Value {
    fn from(n: u64) -> Self {
        // Values > i64::MAX are clamped to i64::MAX — u64 cannot be represented
        // losslessly in TOON's Int type. Callers with hashes or large counters
        // should convert to string before passing if exact values matter.
        #[allow(clippy::cast_possible_wrap)]
        Self::Int(n.try_into().unwrap_or(i64::MAX))
    }
}
```

- [ ] **Step 4: Add `AlreadyDone`/`PartialSuccess` boundary doc (L5)**

Find `AlreadyDone` in `crates/michi-resilience/src/lib.rs`. Add:

```rust
/// # Caller responsibility
///
/// Use [`AlreadyDone`] when an operation fully completed in a prior call.
/// For operations that partially completed (some steps succeeded, some failed),
/// use [`michi_core::idempotency::PartialSuccess`] instead. michi does not
/// enforce this boundary — the distinction is the caller's responsibility.
```

- [ ] **Step 5: Verify no test regressions**

Run: `cargo nextest run`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/michi-core/src/pipeline/mod.rs crates/michi-core/src/kv/mod.rs crates/michi-resilience/src/lib.rs crates/michi-toon/src/render.rs
git commit -m "docs: explicit contracts for u64 clamp, pipeline step ID, KV padding, AlreadyDone/PartialSuccess boundary"
```

---

## Final verification

After all tasks complete:

- [ ] Run full test suite: `just test`
- [ ] Run lints: `just check`
- [ ] Review insta snapshots: `just snapshots` (if any new ones pending)
- [ ] Confirm no regressions in Node tests: `just test` (runs both Rust + Node)

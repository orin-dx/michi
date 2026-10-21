//! NAPI export surface for the `michi` npm package (`packages/michi-node`).
//!
//! `packages/michi-node/src/lib.rs` re-exports everything from this module
//! (`pub use michi_core::napi::*;`) rather than defining `#[napi]` items directly,
//! so the export logic can be unit-tested here alongside the rest of the
//! crate. `packages/michi-node` remains a separate Cargo crate only because
//! `crate-type = ["cdylib"]` cannot coexist with a regular `[lib]` in one
//! `Cargo.toml` — see `docs/superpowers/specs/2026-07-03-michi-design.md` §1.
//!
//! Every export uses `#[napi(catch_unwind)]` so a Rust panic cannot crash the
//! host Node.js process, and every export that accepts caller-controlled
//! collection sizes validates them against hard limits before doing any
//! work, since a single crafted call from untrusted JS is otherwise able to
//! force unbounded synchronous allocation on Node's single-threaded event
//! loop.
//!
//! This is the one module in the crate exempted from `#![deny(unsafe_code)]`
//! (src/lib.rs): `#[napi(catch_unwind)]` expands to macro-generated FFI glue
//! that needs `unsafe` to call into the N-API C ABI. No unsafe is hand-written
//! here — only the module-level allow below permits the macro expansion.
#![allow(unsafe_code)]
#![deny(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use self::num::{JsCount, JsDecimals, JsDelayMillis, JsFloat, JsHttpStatus, JsInt, JsRetryCount, JsUnitInterval};
use napi_derive::napi;

/// Validating newtypes for the NAPI numeric boundary (shared kernel for
/// both the rendering and resilience domains).
pub mod num;

/// Maximum row count accepted for a single call's `rows`: [`render_toon`]'s
/// `opts.rows` and [`JsAgentResponse::items`]'s `rows`.
const MAX_ROWS: usize = 100_000;
/// Maximum count accepted per call for anything shaped like "one entry per
/// field": [`render_toon`]'s `opts.fields` and each row's value count,
/// [`JsAgentResponse::items`]'s `fields` and each row's value count, and
/// [`JsAgentResponse::kv_items`]'s `items` (one KV entry is one field).
const MAX_FIELDS: usize = 1_000;
/// Maximum hint count accepted per call: [`render_hints`]'s `hints`,
/// [`append_hints`]'s `hints`, [`render_recovery`]'s `hints`, and the
/// cumulative count enforced across calls by [`JsAgentResponse::hint`] and
/// [`JsAgentResponse::recovery_hint`].
const MAX_HINTS: usize = 10_000;

/// Value type for a TOON row cell (JavaScript-friendly).
///
/// Use the `type` field to discriminate: `"str"`, `"int"`, `"float"`, `"bool"`, `"null"`.
#[napi(object)]
#[derive(Default)]
pub struct JsToonValue {
    /// Discriminant: `"str"`, `"int"`, `"float"`, `"bool"`, or `"null"`.
    #[napi(js_name = "type")]
    pub r#type: String,
    /// The value when `type` is `"str"`.
    #[napi(js_name = "strVal")]
    pub str_val: Option<String>,
    /// The value when `type` is `"int"`.
    #[napi(js_name = "intVal", ts_type = "number")]
    pub int_val: Option<JsInt>,
    /// The value when `type` is `"float"`.
    #[napi(js_name = "floatVal", ts_type = "number")]
    pub float_val: Option<JsFloat>,
    /// The value when `type` is `"bool"`.
    #[napi(js_name = "boolVal")]
    pub bool_val: Option<bool>,
    /// Decimal places when `type` is `"float"` (KV render only). Rejected if outside [0, 20]. Defaults to 6.
    #[napi(js_name = "decimalsVal", ts_type = "number")]
    pub decimals_val: Option<JsDecimals>,
}

/// Options for rendering a TOON document (JavaScript-friendly).
#[napi(object)]
pub struct JsToonOptions {
    /// Snake_case type name, e.g. `"issue"`, `"component"`.
    #[napi(js_name = "typeName")]
    pub type_name: String,
    /// Ordered field names for the header.
    pub fields: Vec<String>,
    /// Rows, each a Vec of values parallel to `fields`. Capped at `MAX_ROWS`
    /// rows and `MAX_FIELDS` values per row.
    pub rows: Vec<Vec<JsToonValue>>,
    /// Total available count (may exceed `rows.len()` when paginated).
    #[napi(js_name = "totalCount", ts_type = "number")]
    pub total_count: Option<JsCount>,
    /// Agent-facing usage hints. Capped at `MAX_HINTS` entries.
    pub hints: Vec<String>,
}

fn js_value_to_rust(v: JsToonValue) -> napi::Result<crate::toon::Value> {
    match v.r#type.as_str() {
        "str" => v
            .str_val
            .map(|s| crate::toon::Value::Str(s.into()))
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"str\" but strVal is missing")),
        "int" => v
            .int_val
            .map(|n| crate::toon::Value::Int(n.get()))
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"int\" but intVal is missing")),
        "float" => v
            .float_val
            .map(|n| crate::toon::Value::Float(n.get()))
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"float\" but floatVal is missing")),
        "bool" => v
            .bool_val
            .map(crate::toon::Value::Bool)
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"bool\" but boolVal is missing")),
        "null" => Ok(crate::toon::Value::Null),
        other => Err(napi::Error::from_reason(format!(
            "JsToonValue: unknown type {other:?}: expected \"str\", \"int\", \"float\", \"bool\", or \"null\""
        ))),
    }
}

/// Render a TOON list document from options.
///
/// # Errors
///
/// Returns an error if `rows`, `fields`, `hints`, or any row's value count
/// exceeds this module's hard size limits (`MAX_ROWS`, `MAX_FIELDS`,
/// `MAX_HINTS`) — an unbounded caller-supplied collection here would let a
/// single JS call force unbounded synchronous allocation on Node's event
/// loop, which `#[napi(catch_unwind)]` alone does not guard against. Also
/// returns an error if `ToonOptions::validate()` rejects the input — a row
/// whose value count differs from `fields.len()`, or a structural character
/// (`[`, `]`, `{`, `}`, `,`, `\n`, `\r`) in `type_name` or any field name.
/// The error `reason` is the `ToonError` Display text verbatim.
#[napi(catch_unwind)]
pub fn render_toon(opts: JsToonOptions) -> napi::Result<String> {
    if opts.rows.len() > MAX_ROWS {
        return Err(napi::Error::from_reason(format!("rows length {} exceeds maximum of {MAX_ROWS}", opts.rows.len())));
    }
    if opts.fields.len() > MAX_FIELDS {
        return Err(napi::Error::from_reason(format!(
            "fields length {} exceeds maximum of {MAX_FIELDS}",
            opts.fields.len()
        )));
    }
    if opts.hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!(
            "hints length {} exceeds maximum of {MAX_HINTS}",
            opts.hints.len()
        )));
    }

    let mut rows: Vec<Vec<crate::toon::Value>> = Vec::with_capacity(opts.rows.len());
    for row in opts.rows {
        if row.len() > MAX_FIELDS {
            return Err(napi::Error::from_reason(format!(
                "row length {} exceeds maximum fields per row of {MAX_FIELDS}",
                row.len()
            )));
        }
        rows.push(row.into_iter().map(js_value_to_rust).collect::<napi::Result<Vec<_>>>()?);
    }

    let toon_opts = crate::toon::ToonOptions::new(opts.type_name, opts.fields, rows)
        .total_count(opts.total_count.map(JsCount::get))
        .hints(opts.hints);

    crate::toon::render_toon(&toon_opts).map_err(|e| napi::Error::from_reason(format!("toon_validation_failed: {e}")))
}

/// Render an explicit empty-state response.
#[napi(catch_unwind)]
pub fn empty_state(type_name: String) -> String {
    crate::empty::empty_state(&type_name)
}

/// Render a `help[N]:` hint block.
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries.
#[napi(catch_unwind)]
pub fn render_hints(hints: Vec<String>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    Ok(crate::hints::render_hints(&h))
}

/// Append a `help[N]:` block to an existing body string.
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries.
#[napi(catch_unwind)]
#[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
pub fn append_hints(body: String, hints: Vec<String>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    let mut out = body;
    crate::hints::append_hints(&mut out, &h);
    Ok(out)
}

/// A recovery hint for [`render_recovery`] (JavaScript-friendly — no
/// structured params over this boundary; use `AgentResponse.recoveryHint`
/// for the common case, or the Rust API directly for typed params).
#[napi(object)]
pub struct JsRecoveryHint {
    /// The tool/operation name the agent should call to recover.
    pub tool: String,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Render recovery hints as a `recovery[N]:` block.
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries.
#[napi(catch_unwind)]
pub fn render_recovery(hints: Vec<JsRecoveryHint>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let converted: Vec<crate::recovery::RecoveryHint> = hints
        .into_iter()
        .map(|h| {
            let mut hint = crate::recovery::RecoveryHint::new(h.tool);
            if let Some(reason) = h.reason {
                hint = hint.reason(reason);
            }
            hint
        })
        .collect();
    Ok(crate::recovery::render_recovery(&converted))
}

/// Truncate content to `max_chars` Unicode scalar values with an agent-readable suffix.
#[napi(catch_unwind)]
#[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
pub fn truncate(content: String, #[napi(ts_arg_type = "number")] max_chars: JsCount, hint: String) -> String {
    crate::truncate::truncate_inline(&content, max_chars.get(), &hint)
}

/// A key-value item for [`JsAgentResponse::kv_items`] (JavaScript-friendly).
#[napi(object)]
pub struct JsKvItem {
    /// The field name.
    pub key: String,
    /// The field value.
    pub value: JsToonValue,
}

fn js_kv_value_to_rust(v: JsToonValue) -> napi::Result<crate::kv::KvValue> {
    match v.r#type.as_str() {
        "str" => v
            .str_val
            .map(crate::kv::KvValue::Text)
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"str\" but strVal is missing")),
        "int" => v
            .int_val
            .map(|n| crate::kv::KvValue::Int(n.get()))
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"int\" but intVal is missing")),
        "float" => {
            let decimals = v.decimals_val.map_or(6, JsDecimals::get_u8);
            v.float_val
                .map(|n| crate::kv::KvValue::Float(n.get(), decimals))
                .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"float\" but floatVal is missing"))
        }
        "bool" => v
            .bool_val
            .map(crate::kv::KvValue::Bool)
            .ok_or_else(|| napi::Error::from_reason("JsToonValue: type is \"bool\" but boolVal is missing")),
        "null" => Ok(crate::kv::KvValue::Missing),
        other => Err(napi::Error::from_reason(format!(
            "JsToonValue: unknown type {other:?}: expected \"str\", \"int\", \"float\", \"bool\", or \"null\""
        ))),
    }
}

/// Render key-value pairs with aligned columns.
#[napi(catch_unwind)]
pub fn render_kv(
    items: Vec<JsKvItem>,
    #[napi(ts_arg_type = "number | undefined | null")] total_count: Option<JsCount>,
    hints: Vec<String>,
) -> napi::Result<String> {
    if items.len() > MAX_FIELDS {
        return Err(napi::Error::from_reason(format!("items length {} exceeds maximum of {MAX_FIELDS}", items.len())));
    }
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let converted: Vec<crate::kv::KvItem> = items
        .into_iter()
        .map(|i| Ok(crate::kv::KvItem { key: i.key, value: js_kv_value_to_rust(i.value)? }))
        .collect::<napi::Result<Vec<_>>>()?;
    let hint_objs: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    Ok(crate::kv::render_kv(&converted, total_count.map(JsCount::get), &hint_objs))
}

/// Render an explicit `already_done` status block.
#[napi(catch_unwind)]
pub fn render_already_done(operation: String, summary: String, hints: Vec<String>) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    Ok(crate::resilience::render_already_done(&operation, &summary, &hints))
}

/// Parse an RFC 7231 `Retry-After` header value into seconds.
#[napi(catch_unwind)]
pub fn parse_retry_after(header_value: String) -> Option<f64> {
    crate::resilience::parse_retry_after(&header_value).map(|d| d.as_secs_f64())
}

/// Calculate the next retry delay in milliseconds.
///
/// `jitter_seed` must be in `[0.0, 1.0]`; pass a per-call random value from
/// your preferred RNG to avoid thundering-herd retries across concurrent callers.
/// Delay inputs must be finite and non-negative; `retry_after_ms` may be `null`.
#[napi(catch_unwind)]
#[allow(clippy::too_many_arguments)] // NAPI boundary: all params are scalar and directly map to the JS API
pub fn next_retry_delay(
    #[napi(ts_arg_type = "number")] max_retries: JsRetryCount,
    #[napi(ts_arg_type = "number")] base_delay_ms: JsDelayMillis,
    #[napi(ts_arg_type = "number")] max_delay_ms: JsDelayMillis,
    #[napi(ts_arg_type = "number")] jitter_factor: JsUnitInterval,
    #[napi(ts_arg_type = "number")] jitter_seed: JsUnitInterval,
    #[napi(ts_arg_type = "number")] attempt: JsRetryCount,
    #[napi(ts_arg_type = "number | undefined | null")] retry_after_ms: Option<JsDelayMillis>,
) -> napi::Result<Option<f64>> {
    let config = crate::resilience::RetryConfig::new(
        max_retries.get_u32(),
        base_delay_ms.as_duration(),
        max_delay_ms.as_duration(),
        jitter_factor.get(),
    );
    let retry_after = retry_after_ms.map(JsDelayMillis::as_duration);
    Ok(crate::resilience::next_retry_delay(&config, attempt.get_u32(), jitter_seed.get(), retry_after)
        .map(|d| d.as_secs_f64() * 1000.0))
}

/// Return `true` if the HTTP status code is conventionally retryable (429, 502, 503, 504).
#[napi(catch_unwind)]
pub fn is_retryable_status(#[napi(ts_arg_type = "number")] status: JsHttpStatus) -> bool {
    crate::resilience::is_retryable_status(status.get_u16())
}

/// Render a classified `DomainError` card or GitHub annotation.
#[napi(catch_unwind)]
pub fn render_domain_error(
    code: String,
    message: String,
    hints: Vec<String>,
    github_annotation: Option<bool>,
) -> napi::Result<String> {
    if hints.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!("hints length {} exceeds maximum of {MAX_HINTS}", hints.len())));
    }
    let error_code = match code.as_str() {
        "invalid_input" => crate::error::ErrorCode::InvalidInput,
        "unauthorized" => crate::error::ErrorCode::Unauthorized,
        "forbidden" => crate::error::ErrorCode::Forbidden,
        "conflict" => crate::error::ErrorCode::Conflict,
        "rate_limited" => crate::error::ErrorCode::RateLimited,
        "unavailable" => crate::error::ErrorCode::Unavailable,
        "timeout" => crate::error::ErrorCode::Timeout,
        "external_failure" => crate::error::ErrorCode::ExternalFailure,
        "not_found" => crate::error::ErrorCode::NotFound,
        other => {
            return Err(napi::Error::from_reason(format!(
                "unknown error code {other:?}; expected one of: invalid_input, not_found, \
                 unauthorized, forbidden, conflict, rate_limited, unavailable, timeout, external_failure"
            )));
        }
    };
    let mut err = crate::error::DomainError::new(error_code, message);
    for h in hints {
        err = err.hint(h);
    }
    if github_annotation.unwrap_or(false) {
        Ok(err.render_github_annotation())
    } else {
        Ok(err.render())
    }
}

/// A component status item for [`render_status`].
#[napi(object)]
pub struct JsStatusItem {
    /// Component key.
    pub key: String,
    /// Component value.
    pub value: JsToonValue,
    /// Health state: `"ok"`, `"degraded: <reason>"`, or `"error: <reason>"`.
    pub health: Option<String>,
}

/// Render a P8 content-first orientation response.
#[napi(catch_unwind)]
pub fn render_status(
    tool_name: String,
    description: String,
    items: Vec<JsStatusItem>,
    hints: Option<Vec<String>>,
) -> napi::Result<String> {
    let hints_vec = hints.unwrap_or_default();
    if items.len() > MAX_FIELDS {
        return Err(napi::Error::from_reason(format!("items length {} exceeds maximum of {MAX_FIELDS}", items.len())));
    }
    if hints_vec.len() > MAX_HINTS {
        return Err(napi::Error::from_reason(format!(
            "hints length {} exceeds maximum of {MAX_HINTS}",
            hints_vec.len()
        )));
    }
    let status_items: Vec<crate::status::StatusItem> = items
        .into_iter()
        .map(|i| {
            let health = match i.health.as_deref() {
                Some("ok") | None => Some(crate::status::Health::Ok),
                Some(h) if h.starts_with("degraded:") => {
                    Some(crate::status::Health::Degraded(h.trim_start_matches("degraded:").trim().to_string()))
                }
                Some(h) if h.starts_with("error:") => {
                    Some(crate::status::Health::Error(h.trim_start_matches("error:").trim().to_string()))
                }
                Some(other) => {
                    return Err(napi::Error::from_reason(format!(
                        "unknown health value {other:?}: expected \"ok\", \"degraded: <reason>\", or \"error: <reason>\""
                    )));
                }
            };
            Ok(crate::status::StatusItem { key: i.key, value: js_kv_value_to_rust(i.value)?, health })
        })
        .collect::<napi::Result<Vec<_>>>()?;

    let hint_objs: Vec<crate::hints::Hint> = hints_vec.into_iter().map(Into::into).collect();
    let resp = crate::status::StatusResponse::new(tool_name, description, status_items).with_hints(hint_objs);
    Ok(resp.render())
}

/// MCP content-block annotations (JavaScript-friendly). Currently carries
/// only `audience` — michi has no concept of MCP's optional `priority`.
#[napi(object)]
pub struct JsAnnotations {
    /// `["assistant"]` or `["user"]` — which surface(s) this block targets.
    pub audience: Vec<String>,
}

/// One MCP content block (JavaScript-friendly). Wire-conformant with MCP's
/// text content shape: `{type: "text", text, annotations: {audience: [...]}}`.
#[napi(object)]
pub struct JsContentBlock {
    /// Always `"text"` — michi only ever produces text content blocks.
    #[napi(js_name = "type")]
    pub content_type: String,
    /// The block's text content.
    pub text: String,
    /// Which surface(s) this block is meant for.
    pub annotations: JsAnnotations,
}

/// The MCP `CallToolResult` shape, returned by [`JsAgentResponse::to_call_tool_result`].
#[napi(object)]
pub struct JsCallToolResult {
    /// Text content blocks.
    pub content: Vec<JsContentBlock>,
    /// Whether this is a tool execution error.
    #[napi(js_name = "isError")]
    pub is_error: bool,
    /// The same data as `content[0]`, as a real parsed JSON value — not a
    /// string the caller has to `JSON.parse()` themselves.
    #[napi(js_name = "structuredContent")]
    pub structured_content: serde_json::Value,
}

/// NAPI wrapper around [`crate::response::AgentResponse`].
#[napi(js_name = "AgentResponse")]
#[derive(Debug)]
pub struct JsAgentResponse {
    inner: Option<crate::response::AgentResponse>,
    /// Cumulative count of [`Self::hint`] calls, capped at [`MAX_HINTS`].
    hint_count: usize,
    /// Cumulative count of [`Self::recovery_hint`] calls, capped at [`MAX_HINTS`].
    recovery_count: usize,
}

#[napi]
impl JsAgentResponse {
    /// Create a new response builder for the given type name.
    #[napi(constructor, catch_unwind)]
    #[must_use]
    pub fn new(type_name: String) -> Self {
        Self { inner: Some(crate::response::AgentResponse::new(type_name)), hint_count: 0, recovery_count: 0 }
    }

    fn take(&mut self) -> napi::Result<crate::response::AgentResponse> {
        self.inner.take().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
    }

    /// Populate the TOON list path.
    #[napi(catch_unwind)]
    #[allow(clippy::needless_pass_by_value)] // napi-derive requires owned Vec<String> for JS array params
    pub fn items(&mut self, rows: Vec<Vec<JsToonValue>>, fields: Vec<String>) -> napi::Result<()> {
        if rows.len() > MAX_ROWS {
            return Err(napi::Error::from_reason(format!("rows length {} exceeds maximum of {MAX_ROWS}", rows.len())));
        }
        if fields.len() > MAX_FIELDS {
            return Err(napi::Error::from_reason(format!(
                "fields length {} exceeds maximum of {MAX_FIELDS}",
                fields.len()
            )));
        }
        for row in &rows {
            if row.len() > MAX_FIELDS {
                return Err(napi::Error::from_reason(format!(
                    "row length {} exceeds maximum of {MAX_FIELDS}",
                    row.len()
                )));
            }
        }
        let field_refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let converted: Vec<Vec<crate::toon::Value>> = rows
            .into_iter()
            .map(|row| row.into_iter().map(js_value_to_rust).collect::<napi::Result<Vec<_>>>())
            .collect::<napi::Result<Vec<_>>>()?;
        let b = self.take()?;
        self.inner = Some(b.items(converted, &field_refs));
        Ok(())
    }

    /// Set the total available count (TOON path only).
    #[napi(catch_unwind)]
    pub fn total_count(&mut self, #[napi(ts_arg_type = "number")] n: JsCount) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.total_count(n.get()));
        Ok(())
    }

    /// Populate the KV single-item path.
    #[napi(catch_unwind)]
    pub fn kv_items(&mut self, items: Vec<JsKvItem>) -> napi::Result<()> {
        if items.len() > MAX_FIELDS {
            return Err(napi::Error::from_reason(format!(
                "items length {} exceeds maximum of {MAX_FIELDS}",
                items.len()
            )));
        }
        let converted = items
            .into_iter()
            .map(|i| Ok(crate::kv::KvItem { key: i.key, value: js_kv_value_to_rust(i.value)? }))
            .collect::<napi::Result<Vec<_>>>()?;
        let b = self.take()?;
        self.inner = Some(b.kv_items(converted));
        Ok(())
    }

    /// Append a contextual hint.
    #[napi(catch_unwind)]
    pub fn hint(&mut self, hint: String) -> napi::Result<()> {
        if self.hint_count >= MAX_HINTS {
            return Err(napi::Error::from_reason(format!("cumulative hint count exceeds maximum of {MAX_HINTS}")));
        }
        let b = self.take()?;
        self.inner = Some(b.hint(hint));
        self.hint_count += 1;
        Ok(())
    }

    /// Append a recovery hint naming a tool.
    #[napi(catch_unwind)]
    pub fn recovery_hint(&mut self, tool: String, reason: Option<String>) -> napi::Result<()> {
        if self.recovery_count >= MAX_HINTS {
            return Err(napi::Error::from_reason(format!(
                "cumulative recovery hint count exceeds maximum of {MAX_HINTS}"
            )));
        }
        let b = self.take()?;
        let mut hint = crate::recovery::RecoveryHint::new(tool);
        if let Some(reason) = reason {
            hint = hint.reason(reason);
        }
        self.inner = Some(b.recovery_hint(hint));
        self.recovery_count += 1;
        Ok(())
    }

    /// Mark this response as an error state.
    #[napi(catch_unwind)]
    pub fn as_error(&mut self) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.as_error());
        Ok(())
    }

    /// Attach a human-facing companion block (`audience: user`) for MCP callers.
    #[napi(catch_unwind)]
    pub fn human_content(&mut self, text: String) -> napi::Result<()> {
        let b = self.take()?;
        self.inner = Some(b.human_content(text));
        Ok(())
    }

    /// Render via the TOON path (`items`/`fields`/`totalCount`), reading only
    /// that slot regardless of which of `.items()`/`.kvItems()` was called
    /// last — see [`crate::response::AgentResponse::render_toon`].
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_toon(&self) -> napi::Result<String> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(crate::response::AgentResponse::render_toon)
    }

    /// Render via the KV path (`kvItems`/`totalCount`), reading only that
    /// slot regardless of which of `.items()`/`.kvItems()` was called last —
    /// see [`crate::response::AgentResponse::render_kv`].
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_kv(&self) -> napi::Result<String> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(crate::response::AgentResponse::render_kv)
    }

    /// Render as a compact JSON string (`{"body":...,"hints":[...],"recovery":[...],"isError":bool}`).
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_json(&self) -> napi::Result<String> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(|b| b.render(crate::response::OutputFormat::Json))
    }

    /// Render just the `help[N]:` block — the three-surface seam for MCP
    /// frameworks assembling their own display body separately.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn render_hints_only(&self) -> napi::Result<String> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(crate::response::AgentResponse::render_hints_only)
    }

    /// Render for the given audience — `"assistant"` or `"user"`. `"user"`
    /// returns the `humanContent()` block if one was set, falling back to
    /// the same agent-oriented rendering `"assistant"` would produce
    /// otherwise — never an empty string. See [`Self::has_human_content`]
    /// to check for the fallback case ahead of time.
    ///
    /// # Errors
    ///
    /// Returns an error if `audience` is anything other than `"assistant"`
    /// or `"user"`, or if an internal invariant is violated (should not
    /// happen in normal use).
    #[napi(catch_unwind)]
    #[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
    pub fn render_for(&self, audience: String) -> napi::Result<String> {
        let audience = match audience.as_str() {
            "assistant" => crate::audience::Audience::Assistant,
            "user" => crate::audience::Audience::User,
            other => {
                return Err(napi::Error::from_reason(format!(
                    "unknown audience {other:?}: expected \"assistant\" or \"user\""
                )))
            }
        };
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(|b| b.render_for(audience))
    }

    /// Whether `.humanContent()` was set on this builder.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn has_human_content(&self) -> napi::Result<bool> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))
            .map(crate::response::AgentResponse::has_human_content)
    }

    /// Build the MCP `CallToolResult` for this response. Returns a real
    /// object, not a JSON string — `structuredContent` is a parsed
    /// `serde_json::Value`, so TypeScript callers don't need to
    /// `JSON.parse()` it themselves.
    ///
    /// # Errors
    ///
    /// Returns an error only if an internal invariant is violated (should not happen in normal use).
    #[napi(catch_unwind)]
    pub fn to_call_tool_result(&self) -> napi::Result<JsCallToolResult> {
        let inner = self.inner.as_ref().ok_or_else(|| napi::Error::from_reason("AgentResponse already consumed"))?;
        let result = inner.to_call_tool_result();
        let structured_content = serde_json::from_str(&result.structured_content)
            .map_err(|e| napi::Error::from_reason(format!("structured_content was not valid JSON: {e}")))?;
        Ok(JsCallToolResult {
            content: result
                .content
                .into_iter()
                .map(|c| JsContentBlock {
                    content_type: "text".to_string(),
                    text: c.text,
                    annotations: JsAnnotations {
                        audience: c
                            .audience
                            .into_iter()
                            .map(|a| match a {
                                crate::audience::Audience::Assistant => "assistant".to_string(),
                                crate::audience::Audience::User => "user".to_string(),
                            })
                            .collect(),
                    },
                })
                .collect(),
            is_error: result.is_error,
            structured_content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(t: &str) -> JsToonValue {
        JsToonValue {
            r#type: t.to_string(),
            str_val: None,
            int_val: None,
            float_val: None,
            bool_val: None,
            decimals_val: None,
        }
    }

    #[test]
    fn render_toon_basic() {
        let opts = JsToonOptions {
            type_name: "issue".to_string(),
            fields: vec!["id".to_string()],
            rows: vec![vec![JsToonValue {
                int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")),
                ..value("int")
            }]],
            total_count: Some(JsCount::try_from(1.0).expect("1 is a valid count")),
            hints: vec![],
        };
        let out = render_toon(opts).expect("valid input renders");
        assert!(out.starts_with("issue[1]{id}:"));
    }

    #[test]
    fn render_toon_rejects_too_many_rows() {
        let opts = JsToonOptions {
            type_name: "issue".to_string(),
            fields: vec!["id".to_string()],
            rows: (0..=MAX_ROWS).map(|_| vec![value("null")]).collect(),
            total_count: None,
            hints: vec![],
        };
        assert!(render_toon(opts).is_err());
    }

    #[test]
    fn render_toon_rejects_too_many_fields_in_header() {
        let opts = JsToonOptions {
            type_name: "issue".to_string(),
            fields: vec!["f".to_string(); MAX_FIELDS + 1],
            rows: vec![],
            total_count: None,
            hints: vec![],
        };
        // AC-004: exact error message, not just is_err().
        let err = render_toon(opts).expect_err("fields over MAX_FIELDS must be rejected");
        assert_eq!(err.reason, format!("fields length {} exceeds maximum of {MAX_FIELDS}", MAX_FIELDS + 1));
    }

    #[test]
    fn render_toon_rejects_too_many_hints() {
        let opts = JsToonOptions {
            type_name: "issue".to_string(),
            fields: vec![],
            rows: vec![],
            total_count: None,
            hints: vec!["h".to_string(); MAX_HINTS + 1],
        };
        // AC-005: exact error message, not just is_err().
        let err = render_toon(opts).expect_err("hints over MAX_HINTS must be rejected");
        assert_eq!(err.reason, format!("hints length {} exceeds maximum of {MAX_HINTS}", MAX_HINTS + 1));
    }

    #[test]
    fn render_toon_rejects_oversized_row() {
        let opts = JsToonOptions {
            type_name: "issue".to_string(),
            fields: vec!["a".to_string()],
            rows: vec![(0..=MAX_FIELDS).map(|_| value("null")).collect()],
            total_count: None,
            hints: vec![],
        };
        // AC-007: exact error message, not just is_err().
        let err = render_toon(opts).expect_err("oversized row must be rejected");
        assert_eq!(err.reason, format!("row length {} exceeds maximum fields per row of {MAX_FIELDS}", MAX_FIELDS + 1));
    }

    #[test]
    fn render_toon_renders_int_beyond_i32_range() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a".to_string()],
            rows: vec![vec![JsToonValue {
                int_val: Some(JsInt::try_from(1_755_000_000_000.0).expect("1_755_000_000_000 is a valid int")),
                ..value("int")
            }]],
            total_count: None,
            hints: vec![],
        };
        let out = render_toon(opts).expect("valid input renders");
        assert_eq!(out, "t[1]{a}:\n  1755000000000\n");
    }

    #[test]
    fn render_toon_rejects_row_length_mismatch() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a".to_string(), "b".to_string()],
            rows: vec![vec![JsToonValue { str_val: Some("x".to_string()), ..value("str") }]],
            total_count: None,
            hints: vec![],
        };
        let err = render_toon(opts).expect_err("mismatched row must be rejected");
        assert_eq!(err.reason, "toon_validation_failed: row 0 has 1 values but 2 fields declared");
    }

    #[test]
    fn render_toon_rejects_structural_char_in_type_name() {
        let opts = JsToonOptions {
            type_name: "a[b]".to_string(),
            fields: vec!["x".to_string()],
            rows: vec![vec![JsToonValue { str_val: Some("v".to_string()), ..value("str") }]],
            total_count: None,
            hints: vec![],
        };
        let err = render_toon(opts).expect_err("structural type_name must be rejected");
        assert_eq!(err.reason, "toon_validation_failed: type_name \"a[b]\" contains a structural character");
    }

    #[test]
    fn render_toon_rejects_structural_char_in_field_name() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a,b".to_string()],
            rows: vec![vec![JsToonValue { str_val: Some("v".to_string()), ..value("str") }]],
            total_count: None,
            hints: vec![],
        };
        let err = render_toon(opts).expect_err("structural field name must be rejected");
        assert_eq!(err.reason, "toon_validation_failed: field \"a,b\" contains a structural character");
    }

    #[test]
    fn empty_state_renders() {
        assert_eq!(empty_state("issue".to_string()), "issue[0]{}:\ntotalCount: 0\n");
    }

    #[test]
    fn render_hints_basic() {
        let out = render_hints(vec!["do this".to_string()]).expect("valid input renders");
        assert!(out.contains("help[1]:"));
    }

    #[test]
    fn render_hints_rejects_too_many() {
        assert!(render_hints(vec!["h".to_string(); MAX_HINTS + 1]).is_err());
    }

    #[test]
    fn truncate_basic() {
        assert_eq!(
            truncate(
                "hello".to_string(),
                JsCount::try_from(100.0).expect("100 is a valid count"),
                "full=true".to_string()
            ),
            "hello"
        );
    }

    #[test]
    fn js_agent_response_items_then_render_toon() {
        let mut r = JsAgentResponse::new("issues".to_string());
        r.items(
            vec![vec![JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") }]],
            vec!["id".to_string()],
        )
        .unwrap();
        let out = r.render_toon().unwrap();
        assert!(out.starts_with("issues[1]{id}:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_items_rejects_oversized_row() {
        let mut r = JsAgentResponse::new("issue".to_string());
        let err = r
            .items(vec![(0..=MAX_FIELDS).map(|_| value("null")).collect()], vec!["a".to_string()])
            .expect_err("oversized row must be rejected before consuming the builder");
        assert!(err.reason.contains("row length"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.items(vec![vec![value("null")]], vec!["a".to_string()]).expect("builder still usable after rejection");
    }

    #[test]
    fn js_agent_response_items_rejects_invalid_value_and_stays_usable() {
        let mut r = JsAgentResponse::new("issue".to_string());
        // `value("int")` has no `int_val` set, so conversion must fail.
        let err = r
            .items(vec![vec![value("int")]], vec!["a".to_string()])
            .expect_err("int value missing intVal must be rejected before consuming the builder");
        assert!(err.reason.contains("intVal is missing"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.items(vec![vec![value("null")]], vec!["a".to_string()]).expect("builder still usable after rejection");
    }

    // --- AC-010: JsAgentResponse::items rejects all three oversized paths
    // (rows, fields, per-row), and the builder remains usable after each. ---

    #[test]
    fn ac010_js_agent_response_items_rejects_too_many_rows_and_stays_usable() {
        let mut r = JsAgentResponse::new("issue".to_string());
        let rows: Vec<Vec<JsToonValue>> = (0..=MAX_ROWS).map(|_| vec![value("null")]).collect();
        let err = r
            .items(rows, vec!["a".to_string()])
            .expect_err("rows over MAX_ROWS must be rejected before consuming the builder");
        assert!(err.reason.contains("rows length"), "got: {}", err.reason);

        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }])
            .expect("builder still usable after rows rejection");
    }

    #[test]
    fn ac010_js_agent_response_items_rejects_too_many_fields_and_stays_usable() {
        let mut r = JsAgentResponse::new("issue".to_string());
        let fields: Vec<String> = vec!["f".to_string(); MAX_FIELDS + 1];
        let err =
            r.items(vec![], fields).expect_err("fields over MAX_FIELDS must be rejected before consuming the builder");
        assert!(err.reason.contains("fields length"), "got: {}", err.reason);

        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }])
            .expect("builder still usable after fields rejection");
    }

    #[test]
    fn js_agent_response_kv_items_rejects_oversized_input() {
        let mut r = JsAgentResponse::new("issue".to_string());
        let oversized: Vec<JsKvItem> =
            (0..=MAX_FIELDS).map(|i| JsKvItem { key: format!("k{i}"), value: value("null") }).collect();
        let err = r.kv_items(oversized).expect_err("oversized items must be rejected before consuming the builder");
        assert!(err.reason.contains("items length"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }])
            .expect("builder still usable after rejection");
    }

    #[test]
    fn js_agent_response_kv_items_rejects_invalid_value_and_stays_usable() {
        let mut r = JsAgentResponse::new("issue".to_string());
        // `value("int")` has no `int_val` set, so conversion must fail.
        let err = r
            .kv_items(vec![JsKvItem { key: "id".to_string(), value: value("int") }])
            .expect_err("int value missing intVal must be rejected before consuming the builder");
        assert!(err.reason.contains("intVal is missing"), "got: {}", err.reason);

        // The rejected call must not have consumed the builder via `take()`:
        // a subsequent valid call should still succeed.
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }])
            .expect("builder still usable after rejection");
    }

    #[test]
    fn js_agent_response_kv_items_then_render_kv() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem { key: "id".to_string(), value: value("null") }]).unwrap();
        let out = r.render_kv().unwrap();
        assert!(out.contains("id:"), "got: {out}");
    }

    #[test]
    fn js_agent_response_render_toon_is_slot_specific_even_after_kv_items_called_last() {
        let mut r = JsAgentResponse::new("issues".to_string());
        r.items(
            vec![vec![JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") }]],
            vec!["id".to_string()],
        )
        .unwrap();
        r.kv_items(vec![JsKvItem {
            key: "id".to_string(),
            value: JsToonValue { int_val: Some(JsInt::try_from(99.0).expect("99 is a valid int")), ..value("int") },
        }])
        .unwrap();
        let toon = r.render_toon().unwrap();
        let kv = r.render_kv().unwrap();
        assert!(toon.starts_with("issues[1]{id}:\n  1\n"), "got: {toon}");
        assert_ne!(toon, kv, "render_toon() must not follow the last-called .kvItems()");
    }

    #[test]
    fn js_agent_response_render_kv_is_slot_specific_even_after_items_called_last() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem {
            key: "id".to_string(),
            value: JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") },
        }])
        .unwrap();
        r.items(
            vec![vec![JsToonValue {
                int_val: Some(JsInt::try_from(99.0).expect("99 is a valid int")),
                ..value("int")
            }]],
            vec!["id".to_string()],
        )
        .unwrap();
        let kv = r.render_kv().unwrap();
        let toon = r.render_toon().unwrap();
        assert!(kv.contains("id: 1"), "got: {kv}");
        assert_ne!(kv, toon, "render_kv() must not follow the last-called .items()");
    }

    #[test]
    fn js_agent_response_hint_and_render_hints_only() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.hint("do this".to_string()).unwrap();
        assert_eq!(r.render_hints_only().unwrap(), "help[1]:\n  do this\n");
    }

    #[test]
    fn render_for_assistant_matches_render_toon() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem {
            key: "id".to_string(),
            value: JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") },
        }])
        .unwrap();
        assert_eq!(r.render_for("assistant".to_string()).unwrap(), r.render_kv().unwrap());
    }

    #[test]
    fn render_for_user_returns_human_content_when_set() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.human_content("hi there".to_string()).unwrap();
        assert_eq!(r.render_for("user".to_string()).unwrap(), "hi there");
    }

    #[test]
    fn render_for_user_falls_back_when_unset() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        assert_eq!(r.render_for("user".to_string()).unwrap(), r.render_kv().unwrap());
    }

    // --- AC-017: with items() populated (not kv_items()) and human_content
    // never called, render_for('user') matches render_for('assistant') and
    // is non-empty. Neither clause was previously asserted at the NAPI layer. ---

    #[test]
    fn ac017_render_for_user_with_items_populated_matches_assistant_and_is_nonempty() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.items(vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]], vec!["name".to_string()])
            .unwrap();
        let user = r.render_for("user".to_string()).unwrap();
        let assistant = r.render_for("assistant".to_string()).unwrap();
        assert_eq!(
            user, assistant,
            "with human_content unset, 'user' must fall back to the same output as 'assistant'"
        );
        assert!(!user.is_empty(), "items() populated with at least one row must yield non-empty output");
    }

    #[test]
    fn render_for_rejects_unknown_audience() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        let err = r.render_for("nonsense".to_string()).expect_err("should reject");
        assert!(err.reason.contains("nonsense"), "got: {}", err.reason);
    }

    #[test]
    fn has_human_content_reflects_whether_it_was_set() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        assert!(!r.has_human_content().unwrap());
        r.human_content("hi".to_string()).unwrap();
        assert!(r.has_human_content().unwrap());
    }

    #[test]
    fn js_agent_response_render_json_reflects_is_error() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.as_error().unwrap();
        assert!(r.render_json().unwrap().contains("\"isError\":true"));
    }

    #[test]
    fn append_hints_appends_to_existing_body() {
        let out = append_hints("body\n".to_string(), vec!["do this".to_string()]).expect("valid input");
        assert_eq!(out, "body\nhelp[1]:\n  do this\n");
    }

    #[test]
    fn append_hints_rejects_too_many() {
        assert!(append_hints("body\n".to_string(), vec!["h".to_string(); MAX_HINTS + 1]).is_err());
    }

    #[test]
    fn render_recovery_basic() {
        let hints = vec![JsRecoveryHint { tool: "retry".to_string(), reason: None }];
        let out = render_recovery(hints).expect("valid input");
        assert!(out.starts_with("recovery[1]:\n  retry"), "got: {out}");
    }

    #[test]
    fn render_recovery_includes_reason() {
        let hints = vec![JsRecoveryHint { tool: "retry".to_string(), reason: Some("rate limited".to_string()) }];
        let out = render_recovery(hints).expect("valid input");
        assert!(out.contains("rate limited"), "got: {out}");
    }

    #[test]
    fn render_recovery_rejects_too_many() {
        let hints: Vec<JsRecoveryHint> =
            (0..=MAX_HINTS).map(|_| JsRecoveryHint { tool: "retry".to_string(), reason: None }).collect();
        assert!(render_recovery(hints).is_err());
    }

    #[test]
    fn js_agent_response_hint_rejects_after_cumulative_limit_exceeded() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        for i in 0..MAX_HINTS {
            r.hint(format!("hint {i}")).expect("under limit should succeed");
        }
        let result = r.hint("one too many".to_string());
        assert!(result.is_err(), "expected error after exceeding MAX_HINTS cumulative hints");
    }

    #[test]
    fn js_agent_response_recovery_hint_rejects_after_cumulative_limit_exceeded() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        for _ in 0..MAX_HINTS {
            r.recovery_hint("retry".to_string(), None).expect("under limit should succeed");
        }
        let result = r.recovery_hint("retry".to_string(), None);
        assert!(result.is_err(), "expected error after exceeding MAX_HINTS cumulative recovery hints");
    }

    // --- AC-026: hint() and recovery_hint() are independently capped;
    // exhausting hint_count must not affect recovery_count. ---

    #[test]
    fn ac026_hint_and_recovery_hint_are_independently_capped() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        for i in 0..MAX_HINTS {
            r.hint(format!("h{i}")).expect("hint calls under MAX_HINTS must all succeed");
        }
        // hint_count is now exhausted; recovery_count must be a separate counter,
        // still at 0, so this call must succeed rather than failing on account
        // of hint()'s exhausted cap.
        r.recovery_hint("retry".to_string(), None)
            .expect("recovery_hint must succeed after hint()'s cap is exhausted — the two counters are independent");
    }

    #[test]
    fn js_agent_response_to_call_tool_result_basic() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.kv_items(vec![JsKvItem {
            key: "id".to_string(),
            value: JsToonValue { int_val: Some(JsInt::try_from(1.0).expect("1 is a valid int")), ..value("int") },
        }])
        .unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
        assert_eq!(result.content[0].annotations.audience, vec!["assistant".to_string()]);
        assert!(!result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_reflects_is_error() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.as_error().unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.is_error);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_structured_content_is_parsed_json() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert!(result.structured_content.get("isError").is_some(), "got: {:?}", result.structured_content);
    }

    #[test]
    fn js_agent_response_to_call_tool_result_includes_user_block_with_correct_annotations() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].content_type, "text");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }

    #[test]
    fn js_agent_response_human_content_adds_user_audience_block() {
        let mut r = JsAgentResponse::new("t".to_string());
        r.kv_items(vec![]).unwrap();
        r.human_content("friendly summary".to_string()).unwrap();
        let result = r.to_call_tool_result().unwrap();
        assert_eq!(result.content.len(), 2);
        assert_eq!(result.content[1].text, "friendly summary");
        assert_eq!(result.content[1].annotations.audience, vec!["user".to_string()]);
    }

    #[test]
    fn render_already_done_napi_basic() {
        let out = render_already_done("delete_item".into(), "already deleted".into(), vec![]).unwrap();
        assert!(out.contains("operation: delete_item"), "got: {out}");
        assert!(out.contains("status:    already_done"), "got: {out}");
        assert!(out.contains("summary:   already deleted"), "got: {out}");
    }

    #[test]
    fn render_already_done_napi_with_hints() {
        let out = render_already_done("sync".into(), "done".into(), vec!["use get_status to verify".into()]).unwrap();
        assert!(out.contains("help[1]:\n  use get_status to verify\n"), "got: {out}");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn render_kv_napi_float_default_decimals() {
        let item = JsKvItem {
            key: "score".into(),
            value: JsToonValue {
                float_val: Some(JsFloat::try_from(3.14159).expect("3.14159 is finite")),
                ..value("float")
            },
        };
        let out = render_kv(vec![item], None, vec![]).unwrap();
        assert!(out.contains("score:"), "got: {out}");
        assert!(out.contains("3.141590"), "expected 6 decimal places by default, got: {out}");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn render_kv_napi_float_custom_decimals() {
        let item = JsKvItem {
            key: "score".into(),
            value: JsToonValue {
                float_val: Some(JsFloat::try_from(3.14159).expect("3.14159 is finite")),
                decimals_val: Some(JsDecimals::try_from(2.0).expect("2 is a valid decimals value")),
                ..value("float")
            },
        };
        let out = render_kv(vec![item], None, vec![]).unwrap();
        assert!(out.contains("3.14"), "expected 2 decimal places, got: {out}");
        assert!(!out.contains("3.141"), "must not have more than 2 decimal places, got: {out}");
    }

    #[test]
    fn render_toon_rejects_int_with_missing_int_val() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a".to_string()],
            rows: vec![vec![value("int")]],
            total_count: None,
            hints: vec![],
        };
        let err = render_toon(opts).expect_err("missing intVal must error, not default to 0");
        assert!(err.reason.contains("intVal"), "error should name the missing field, got: {}", err.reason);
    }

    #[test]
    fn render_toon_rejects_unknown_type_even_with_a_present_payload() {
        let mut v = value("integer");
        v.int_val = Some(JsInt::try_from(42.0).expect("42 is valid"));
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a".to_string()],
            rows: vec![vec![v]],
            total_count: None,
            hints: vec![],
        };
        let err = render_toon(opts).expect_err("unrecognized type must error, not silently discard the payload");
        assert!(err.reason.contains("integer"), "error should name the offending type, got: {}", err.reason);
    }

    #[test]
    fn render_toon_accepts_explicit_null_with_no_payload() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["a".to_string()],
            rows: vec![vec![value("null")]],
            total_count: None,
            hints: vec![],
        };
        assert!(render_toon(opts).is_ok(), "type \"null\" with no payload is legitimate and must still work");
    }

    #[test]
    fn render_kv_rejects_bool_with_missing_bool_val() {
        let items = vec![JsKvItem { key: "k".to_string(), value: value("bool") }];
        let err = render_kv(items, None, vec![]).expect_err("missing boolVal must error, not default to false");
        assert!(err.reason.contains("boolVal"), "error should name the missing field, got: {}", err.reason);
    }

    #[test]
    fn render_status_napi_ok_health() {
        let items = vec![JsStatusItem {
            key: "index".into(),
            value: JsToonValue { str_val: Some("ready".into()), ..value("str") },
            health: Some("ok".into()),
        }];
        let out = render_status("my-tool".into(), "desc".into(), items, None).unwrap();
        assert!(out.contains("tool:"), "got: {out}");
        assert!(!out.contains("DEGRADED") && !out.contains("ERROR"), "ok should have no annotation, got: {out}");
    }

    #[test]
    fn render_status_napi_degraded_health() {
        let items = vec![JsStatusItem {
            key: "cache".into(),
            value: JsToonValue { str_val: Some("warm".into()), ..value("str") },
            health: Some("degraded: high latency".into()),
        }];
        let out = render_status("my-tool".into(), "desc".into(), items, None).unwrap();
        assert!(out.contains("[DEGRADED: high latency]"), "got: {out}");
    }

    #[test]
    fn render_status_napi_error_health() {
        let items = vec![JsStatusItem {
            key: "db".into(),
            value: JsToonValue { str_val: Some("down".into()), ..value("str") },
            health: Some("error: connection refused".into()),
        }];
        let out = render_status("my-tool".into(), "desc".into(), items, None).unwrap();
        assert!(out.contains("[ERROR: connection refused]"), "got: {out}");
    }

    #[test]
    fn render_status_napi_unknown_health_returns_error() {
        let items = vec![JsStatusItem {
            key: "x".into(),
            value: JsToonValue { str_val: Some("v".into()), ..value("str") },
            health: Some("degraded".into()), // missing colon and reason
        }];
        // AC-036: exact error message, not just is_err().
        let err = render_status("t".into(), "d".into(), items, None).expect_err("unrecognized health must be rejected");
        assert_eq!(
            err.reason,
            "unknown health value \"degraded\": expected \"ok\", \"degraded: <reason>\", or \"error: <reason>\""
        );
    }

    // --- AC-038: render_domain_error rejects an unrecognized code with a
    // message enumerating the valid codes. ---

    #[test]
    fn ac038_render_domain_error_rejects_unknown_code_with_enumerated_message() {
        let err = render_domain_error("bogus_code".into(), "msg".into(), vec![], None)
            .expect_err("unrecognized error code must be rejected");
        assert_eq!(
            err.reason,
            "unknown error code \"bogus_code\"; expected one of: invalid_input, not_found, unauthorized, \
             forbidden, conflict, rate_limited, unavailable, timeout, external_failure"
        );
    }

    // AC-029 previously asserted an unrecognized value type silently rendered
    // as the KV "missing" em-dash. That was the PROOF-NAPI-001 bug: an
    // unrecognized type must reject, not silently discard a possibly-present
    // payload. `"null"` remains the sole legitimate no-payload discriminant.
    #[test]
    fn kv_rejects_unrecognized_value_type_instead_of_defaulting_to_missing() {
        let item = JsKvItem { key: "id".into(), value: value("unrecognized-type") };
        let err = render_kv(vec![item], None, vec![]).expect_err("unrecognized type must error");
        assert!(err.reason.contains("unrecognized-type"), "error should name the offending type, got: {}", err.reason);
    }

    // --- AC-044: render_json() on the spec's example builder state produces
    // real, parseable, distinguishable JSON. ---

    #[test]
    fn ac044_render_json_is_valid_json_distinct_from_render_toon() {
        let mut r = JsAgentResponse::new("issue".to_string());
        r.items(vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]], vec!["name".to_string()])
            .unwrap();
        r.hint("h".to_string()).unwrap();

        let json = r.render_json().expect("render_json must succeed");
        assert!(json.starts_with('{'), "got: {json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("render_json output must parse as JSON");
        assert!(parsed.is_object(), "expected a JSON object, got: {parsed:?}");

        let toon = r.render_toon().expect("render_toon must succeed");
        assert_ne!(json, toon, "render_json output must not be byte-identical to render_toon output");
    }

    // --- AC-048: the NAPI-exported parse_retry_after returns None (not a
    // panic, not an error) for unparsable input. ---

    #[test]
    fn ac048_napi_parse_retry_after_returns_none_for_unparsable_input() {
        assert_eq!(parse_retry_after("not-a-number-or-date".to_string()), None);
    }

    // --- AC-050: a recovery_hint added via the builder appears in
    // render_toon() output, and presence vs. absence of a reason changes the
    // rendered output. ---

    #[test]
    fn ac050_builder_recovery_hint_appears_in_render_toon_output() {
        let mut r_no_reason = JsAgentResponse::new("issue".to_string());
        r_no_reason
            .items(vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]], vec!["name".to_string()])
            .unwrap();
        r_no_reason.recovery_hint("retry".to_string(), None).unwrap();
        let out_no_reason = r_no_reason.render_toon().unwrap();
        assert!(out_no_reason.contains("retry"), "got: {out_no_reason}");

        let mut r_with_reason = JsAgentResponse::new("issue".to_string());
        r_with_reason
            .items(vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]], vec!["name".to_string()])
            .unwrap();
        r_with_reason.recovery_hint("retry".to_string(), Some("r".to_string())).unwrap();
        let out_with_reason = r_with_reason.render_toon().unwrap();
        assert!(out_with_reason.contains("retry"), "got: {out_with_reason}");
        assert!(out_with_reason.contains('r'), "got: {out_with_reason}");

        assert_ne!(
            out_no_reason, out_with_reason,
            "presence of a reason must produce different render_toon() output than its absence"
        );
    }

    // --- AC-040: to_call_tool_result() succeeds for the fresh-builder and
    // items()-populated states (kv_items-populated and as_error states are
    // already covered by existing tests above). ---

    #[test]
    fn ac040_to_call_tool_result_ok_for_fresh_and_items_populated_states() {
        let r_fresh = JsAgentResponse::new("issue".to_string());
        let result_fresh = r_fresh.to_call_tool_result().expect("fresh builder must produce Ok");
        assert!(result_fresh.structured_content.is_object(), "got: {:?}", result_fresh.structured_content);

        let mut r_items = JsAgentResponse::new("issue".to_string());
        r_items
            .items(vec![vec![JsToonValue { str_val: Some("a".to_string()), ..value("str") }]], vec!["name".to_string()])
            .unwrap();
        let result_items = r_items.to_call_tool_result().expect("items-populated builder must produce Ok");
        assert!(result_items.structured_content.is_object(), "got: {:?}", result_items.structured_content);
    }

    // --- AC-002: hard-cap constants are pinned to their documented values. ---

    #[test]
    fn cap_constants_are_pinned_to_documented_values() {
        assert_eq!(MAX_ROWS, 100_000);
        assert_eq!(MAX_FIELDS, 1_000);
        assert_eq!(MAX_HINTS, 10_000);
    }

    // --- AC-006: render_toon accepts rows/fields/hints at exactly the cap. ---

    #[test]
    fn render_toon_accepts_exactly_at_cap_for_rows_fields_and_hints() {
        let opts = JsToonOptions {
            type_name: "t".to_string(),
            fields: vec!["f".to_string(); MAX_FIELDS],
            rows: (0..MAX_ROWS).map(|_| Vec::new()).collect(),
            total_count: None,
            hints: vec!["h".to_string(); MAX_HINTS],
        };
        match render_toon(opts) {
            Ok(_) => {}
            Err(e) => assert!(
                !e.reason.contains("exceeds maximum"),
                "at-cap call must not be rejected by a cap check, got: {}",
                e.reason
            ),
        }
    }

    fn kv_item(k: &str) -> JsKvItem {
        JsKvItem { key: k.to_string(), value: value("null") }
    }

    fn status_item(k: &str) -> JsStatusItem {
        JsStatusItem { key: k.to_string(), value: value("null"), health: None }
    }

    // --- AC-008d/AC-009d: render_kv items cap (MAX_FIELDS). ---

    #[test]
    fn render_kv_items_cap_boundary() {
        let too_many: Vec<JsKvItem> = (0..=MAX_FIELDS).map(|i| kv_item(&format!("k{i}"))).collect();
        let err = render_kv(too_many, None, vec![]).expect_err("items over MAX_FIELDS must be rejected");
        assert!(err.reason.contains("items length"), "got: {}", err.reason);

        let at_cap: Vec<JsKvItem> = (0..MAX_FIELDS).map(|i| kv_item(&format!("k{i}"))).collect();
        let out = render_kv(at_cap, None, vec![]).expect("items at exactly MAX_FIELDS must be accepted");
        assert!(!out.is_empty());
    }

    // --- AC-008e/AC-009e: render_kv hints cap (MAX_HINTS). ---

    #[test]
    fn render_kv_hints_cap_boundary() {
        let err = render_kv(vec![kv_item("k")], None, vec!["h".to_string(); MAX_HINTS + 1])
            .expect_err("hints over MAX_HINTS must be rejected");
        assert!(err.reason.contains("hints length"), "got: {}", err.reason);

        let out = render_kv(vec![kv_item("k")], None, vec!["h".to_string(); MAX_HINTS])
            .expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.contains("help["), "got: {out}");
    }

    // --- AC-008f/AC-009f: render_status items cap (MAX_FIELDS). ---

    #[test]
    fn render_status_items_cap_boundary() {
        let too_many: Vec<JsStatusItem> = (0..=MAX_FIELDS).map(|i| status_item(&format!("k{i}"))).collect();
        let err =
            render_status("t".into(), "d".into(), too_many, None).expect_err("items over MAX_FIELDS must be rejected");
        assert!(err.reason.contains("items length"), "got: {}", err.reason);

        let at_cap: Vec<JsStatusItem> = (0..MAX_FIELDS).map(|i| status_item(&format!("k{i}"))).collect();
        let out =
            render_status("t".into(), "d".into(), at_cap, None).expect("items at exactly MAX_FIELDS must be accepted");
        assert!(!out.is_empty());
    }

    // --- AC-008g/AC-009g: render_status hints cap (MAX_HINTS). ---

    #[test]
    fn render_status_hints_cap_boundary() {
        let err =
            render_status("t".into(), "d".into(), vec![status_item("k")], Some(vec!["h".to_string(); MAX_HINTS + 1]))
                .expect_err("hints over MAX_HINTS must be rejected");
        assert!(err.reason.contains("hints length"), "got: {}", err.reason);

        let out = render_status("t".into(), "d".into(), vec![status_item("k")], Some(vec!["h".to_string(); MAX_HINTS]))
            .expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.contains("help["), "got: {out}");
    }

    // --- AC-008h/AC-009h: render_domain_error hints cap (MAX_HINTS). ---

    #[test]
    fn render_domain_error_hints_cap_boundary() {
        let err = render_domain_error("invalid_input".into(), "bad".into(), vec!["h".to_string(); MAX_HINTS + 1], None)
            .expect_err("hints over MAX_HINTS must be rejected");
        assert!(err.reason.contains("hints length"), "got: {}", err.reason);

        let out = render_domain_error("invalid_input".into(), "bad".into(), vec!["h".to_string(); MAX_HINTS], None)
            .expect("hints at exactly MAX_HINTS must be accepted");
        assert!(!out.is_empty());
    }

    // --- AC-008i/AC-009i: render_already_done hints cap (MAX_HINTS). ---

    #[test]
    fn render_already_done_hints_cap_boundary() {
        let err = render_already_done("op".into(), "sum".into(), vec!["h".to_string(); MAX_HINTS + 1])
            .expect_err("hints over MAX_HINTS must be rejected");
        assert!(err.reason.contains("hints length"), "got: {}", err.reason);

        let out = render_already_done("op".into(), "sum".into(), vec!["h".to_string(); MAX_HINTS])
            .expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.contains("help["), "got: {out}");
    }

    // --- AC-009a: render_hints accepts hints at exactly MAX_HINTS. ---

    #[test]
    fn render_hints_accepts_exactly_at_cap() {
        let out = render_hints(vec!["h".to_string(); MAX_HINTS]).expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.contains(&format!("help[{MAX_HINTS}]")), "got starts: {}", &out[..out.len().min(40)]);
    }

    // --- AC-009b: append_hints accepts hints at exactly MAX_HINTS. ---

    #[test]
    fn append_hints_accepts_exactly_at_cap() {
        let out = append_hints("body\n".to_string(), vec!["h".to_string(); MAX_HINTS])
            .expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.starts_with("body\n"), "got: {}", &out[..out.len().min(40)]);
        assert!(out.contains(&format!("help[{MAX_HINTS}]")), "got: {}", &out[..out.len().min(60)]);
    }

    // --- AC-009c: render_recovery accepts hints at exactly MAX_HINTS. ---

    #[test]
    fn render_recovery_accepts_exactly_at_cap() {
        let hints: Vec<JsRecoveryHint> =
            (0..MAX_HINTS).map(|_| JsRecoveryHint { tool: "retry".to_string(), reason: None }).collect();
        let out = render_recovery(hints).expect("hints at exactly MAX_HINTS must be accepted");
        assert!(out.starts_with(&format!("recovery[{MAX_HINTS}]")), "got: {}", &out[..out.len().min(40)]);
    }

    // --- AC-032: packages/michi-node/src/lib.rs contains only a doc-comment
    // header and `pub use michi_core::napi::*;` — no other public items. ---

    #[test]
    fn ac032_michi_node_lib_rs_contains_only_the_reexport() {
        let src = include_str!("../../../packages/michi-node/src/lib.rs");
        let non_comment_non_blank: Vec<&str> =
            src.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with("//")).collect();
        assert_eq!(
            non_comment_non_blank,
            vec!["pub use michi_core::napi::*;"],
            "packages/michi-node/src/lib.rs must contain only a doc-comment header and \
             `pub use michi_core::napi::*;`, got non-comment/non-blank lines: {non_comment_non_blank:?}"
        );
    }

    /// Returns `true` if `line` contains the token `unsafe` used as a block
    /// or item qualifier — i.e. immediately (modulo whitespace) followed by
    /// `{`, `fn`, `impl`, or `trait` — approximating the regex
    /// `\bunsafe\s*(\{|fn|impl|trait)` from AC-033a without a regex
    /// dependency. Deliberately does not match `unsafe_code` (the attribute
    /// token used by `#![deny(unsafe_code)]` / `#![allow(unsafe_code)]`,
    /// covered separately by AC-033b) because the character immediately
    /// following `unsafe` there is `_`, which fails the word-boundary check.
    fn line_has_unsafe_qualifier(line: &str) -> bool {
        let bytes = line.as_bytes();
        let mut i = 0;
        while let Some(rel) = line[i..].find("unsafe") {
            let start = i + rel;
            let before_ok = start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            let after = start + "unsafe".len();
            let after_ok = after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_');
            if before_ok && after_ok {
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let rest = &line[j..];
                if rest.starts_with('{')
                    || rest.starts_with("fn")
                    || rest.starts_with("impl")
                    || rest.starts_with("trait")
                {
                    return true;
                }
            }
            i = after;
            if i > line.len() {
                break;
            }
        }
        false
    }

    // --- AC-033a: the only unsafe blocks/fns in the napi module tree are
    // FromNapiValue/ToNapiValue trait impl bodies in src/napi/num.rs; no
    // hand-written unsafe exists in src/lib.rs, src/napi.rs, or
    // packages/michi-node/src/lib.rs. ---

    #[test]
    fn ac033a_unsafe_confined_to_napi_num_ffi_trait_impls() {
        let lib_src = include_str!("lib.rs");
        let napi_src = include_str!("napi.rs");
        let node_src = include_str!("../../../packages/michi-node/src/lib.rs");
        let num_src = include_str!("napi/num.rs");

        for (name, src) in
            [("src/lib.rs", lib_src), ("src/napi.rs", napi_src), ("packages/michi-node/src/lib.rs", node_src)]
        {
            let hits: Vec<&str> = src.lines().filter(|l| line_has_unsafe_qualifier(l)).collect();
            assert!(hits.is_empty(), "{name} must contain no hand-written `unsafe` qualifiers, found: {hits:?}");
        }

        let lines: Vec<&str> = num_src.lines().collect();
        let impl_line_indices: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("impl") && (l.contains("FromNapiValue") || l.contains("ToNapiValue")))
            .map(|(i, _)| i)
            .collect();
        assert!(!impl_line_indices.is_empty(), "expected at least one FromNapiValue/ToNapiValue impl in num.rs");

        let mut unsafe_hit_count = 0;
        for (i, line) in lines.iter().enumerate() {
            if line_has_unsafe_qualifier(line) {
                unsafe_hit_count += 1;
                let near = impl_line_indices.iter().any(|&ii| i >= ii && i <= ii + 3);
                assert!(
                    near,
                    "num.rs:{}: `{}` is not within 3 lines of a FromNapiValue/ToNapiValue impl line",
                    i + 1,
                    line.trim()
                );
            }
        }
        assert!(unsafe_hit_count > 0, "expected num.rs to contain the sanctioned FFI-boundary unsafe occurrences");
    }

    // --- AC-033b: src/lib.rs has #![deny(unsafe_code)]; src/napi.rs has
    // #![allow(unsafe_code)] immediately preceded by an explanatory comment. ---

    #[test]
    fn ac033b_deny_and_allow_unsafe_code_attributes_present() {
        // unsafe_code = "deny" lives in [workspace.lints.rust] (Cargo.toml),
        // not a #![deny(unsafe_code)] attribute in lib.rs -- see Cargo.toml's
        // comment on why only unsafe_code is centralized this way.
        let workspace_toml = include_str!("../../../Cargo.toml");
        let cargo_toml = include_str!("../Cargo.toml");
        assert!(
            workspace_toml.contains("unsafe_code = \"deny\""),
            "Cargo.toml's [workspace.lints.rust] must set unsafe_code = \"deny\""
        );
        assert!(
            cargo_toml.contains("[lints]\nworkspace = true"),
            "Cargo.toml's [package] must opt into workspace lints via [lints]\\nworkspace = true"
        );

        let napi_src = include_str!("napi.rs");
        let idx = napi_src.find("#![allow(unsafe_code)]").expect("src/napi.rs must contain #![allow(unsafe_code)]");
        let preceding = &napi_src[..idx];
        let last_non_blank = preceding.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(
            last_non_blank.trim_start().starts_with("//"),
            "expected a comment immediately preceding #![allow(unsafe_code)], got: {last_non_blank:?}"
        );
        // Check the comment block explains the reason (catch_unwind macro expansion),
        // not just that some comment happens to be adjacent.
        let block: String = preceding
            .lines()
            .rev()
            .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            block.contains("catch_unwind"),
            "comment preceding #![allow(unsafe_code)] must explain the exemption via catch_unwind, got: {block:?}"
        );
    }

    // --- AC-034a: every #[napi(...)]-attributed free function and every
    // #[napi(...)]-attributed method in `impl JsAgentResponse` carries
    // catch_unwind. ---

    #[test]
    fn ac034a_every_napi_attributed_fn_has_catch_unwind() {
        let napi_src = include_str!("napi.rs");
        let lines: Vec<&str> = napi_src.lines().collect();
        let mut checked = 0;
        for (i, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("pub fn") {
                continue;
            }
            let mut j = i;
            let mut found_napi_attr: Option<&str> = None;
            while j > 0 {
                let prev = lines[j - 1].trim_start();
                if prev.starts_with("#[") || prev.starts_with("///") || prev.starts_with("//!") {
                    // Any `#[napi` attribute directly preceding a `pub fn` — with or
                    // without parens — must carry catch_unwind. A bare `#[napi]` with
                    // no parens is only legitimately exempt when it precedes an `impl`
                    // block, which by construction never precedes a `pub fn` line, so
                    // no extra exclusion is needed here.
                    if prev.starts_with("#[napi") {
                        found_napi_attr = Some(lines[j - 1]);
                    }
                    j -= 1;
                } else {
                    break;
                }
            }
            if let Some(attr) = found_napi_attr {
                checked += 1;
                assert!(
                    attr.contains("catch_unwind"),
                    "napi.rs:{}: `pub fn` preceded by `{}` without catch_unwind",
                    i + 1,
                    attr.trim()
                );
            }
        }
        assert!(
            checked >= 20,
            "expected to check at least 20 #[napi]-attributed fns/methods, only checked {checked} \
             (a change to how attributes are written may have broken this scan's detection)"
        );
    }
}

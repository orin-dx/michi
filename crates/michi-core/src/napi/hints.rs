//! `renderHints`, `appendHints` and `renderRecovery`: the `help[]` and `recovery[]` blocks on their own.

use napi::bindgen_prelude::Unknown;
use napi_derive::napi;

use super::input::{self, Budget, List, MAX_HINTS, MAX_STRING_BYTES};

/// A recovery hint for `renderRecovery` and `FailedOp` (JavaScript-friendly — no structured params over this
/// boundary; use `AgentResponse.recoveryHint` for the common case, or the Rust API directly for typed params).
#[napi(object, js_name = "RecoveryHint")]
#[derive(Debug, Clone)]
pub struct JsRecoveryHint {
    /// The tool/operation name the agent should call to recover.
    pub tool: String,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

impl From<JsRecoveryHint> for crate::recovery::RecoveryHint {
    fn from(h: JsRecoveryHint) -> Self {
        let hint = Self::new(h.tool);
        match h.reason {
            Some(reason) => hint.reason(reason),
            None => hint,
        }
    }
}

/// Render the TOON 4.4 inline array `help[N]: h1,h2` (no trailing newline).
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries, a hint exceeds 1 MiB, or the call's strings exceed
/// the call's byte cap (16 MiB) in all.
#[napi(catch_unwind, js_name = "renderHints", ts_args_type = "hints: Array<string>")]
pub fn render_hints(hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    Ok(hints_block(budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?))
}

fn hints_block(hints: Vec<String>) -> String {
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    crate::hints::render_hints(&h)
}

/// Append the `help[N]: …` block to a TOON document: one LF first unless `body` is empty or ends in one.
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries, a hint exceeds 1 MiB, or the call's strings (`body`
/// included) exceed the call's byte cap (16 MiB) in all.
#[napi(catch_unwind, js_name = "appendHints", ts_args_type = "body: string, hints: Array<string>")]
pub fn append_hints(body: Unknown<'_>, hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let body = budget.string(body, "body", input::MAX_INPUT_BYTES)?;
    Ok(append_block(body, budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?))
}

fn append_block(mut body: String, hints: Vec<String>) -> String {
    let h: Vec<crate::hints::Hint> = hints.into_iter().map(Into::into).collect();
    crate::hints::append_hints(&mut body, &h);
    body
}

/// Render recovery hints as the TOON 4.4 table `recovery[N]{tool,params,reason}:` (params `"{}"` here).
///
/// # Errors
///
/// Returns an error if `hints` exceeds `MAX_HINTS` entries, a `tool` exceeds 4 KiB, a `reason` exceeds 1 MiB, or the
/// call's strings exceed the call's byte cap (16 MiB) in all.
#[napi(catch_unwind, js_name = "renderRecovery", ts_args_type = "hints: Array<RecoveryHint>")]
pub fn render_recovery(hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    Ok(recovery_table(budget.recovery_hints(&hints, MAX_HINTS)?))
}

fn recovery_table(hints: Vec<JsRecoveryHint>) -> String {
    let converted: Vec<crate::recovery::RecoveryHint> = hints.into_iter().map(Into::into).collect();
    crate::recovery::render_recovery(&converted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_hints_basic() {
        assert!(hints_block(vec!["do this".to_string()]).contains("help[1]:"));
    }

    #[test]
    fn append_hints_appends_to_existing_body() {
        assert_eq!(append_block("body\n".to_string(), vec!["do this".to_string()]), "body\nhelp[1]: do this");
        // A body without a trailing newline (every michi TOON document) still gets the block on its own line.
        assert_eq!(append_block("t: []".to_string(), vec!["a, b".to_string()]), "t: []\nhelp[1]: \"a, b\"");
    }

    #[test]
    fn render_recovery_basic() {
        let hints = vec![JsRecoveryHint { tool: "retry".to_string(), reason: None }];
        assert_eq!(recovery_table(hints), "recovery[1]{tool,params,reason}:\n  retry,\"{}\",null");
    }

    #[test]
    fn render_recovery_includes_reason() {
        let hints = vec![JsRecoveryHint { tool: "retry".to_string(), reason: Some("rate limited".to_string()) }];
        let out = recovery_table(hints);
        assert!(out.contains("rate limited"), "got: {out}");
    }
}

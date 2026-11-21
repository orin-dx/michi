//! How an operation went: the error, batch-outcome and pipeline-step types that `AgentResponse` takes, and
//! `renderGithubAnnotation`.

use napi::bindgen_prelude::Unknown;
use napi_derive::napi;

use super::hints::JsRecoveryHint;
use super::input::{Budget, MAX_NAME_BYTES, MAX_STRING_BYTES};
use super::num::JsCount;

/// Render a classified error as a GitHub Actions workflow annotation: `::error title={code}::{message}`, on one line.
/// The message is encoded as workflow command data (`%`, CR and LF), and the title also encodes `:` and `,`. For the
/// agent-facing error document, its JSON or an MCP result, use `AgentResponse.error()`.
///
/// # Errors
///
/// Rejects an unknown `code`, a `code` over 4 KiB and a `message` over 1 MiB.
#[napi(
    catch_unwind,
    js_name = "renderGithubAnnotation",
    ts_args_type = "code: 'invalid_input' | 'not_found' | 'unauthorized' | 'forbidden' | 'conflict' | 'rate_limited' | \
                    'unavailable' | 'timeout' | 'external_failure', message: string"
)]
pub fn render_github_annotation(code: Unknown<'_>, message: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let code = budget.string(code, "code", MAX_NAME_BYTES)?;
    let message = budget.string(message, "message", MAX_STRING_BYTES)?;
    github_annotation(&code, message)
}

fn github_annotation(code: &str, message: String) -> napi::Result<String> {
    Ok(build_domain_error(code, message, Vec::new())?.render_github_annotation())
}

/// A `DomainError` from its code label, message and causes. Shared by `renderGithubAnnotation` and
/// `AgentResponse.error`.
pub(super) fn build_domain_error(
    code: &str,
    message: String,
    causes: Vec<String>,
) -> napi::Result<crate::error::DomainError> {
    let code: crate::error::ErrorCode =
        code.parse().map_err(|e: crate::error::UnknownErrorCode| napi::Error::from_reason(e.to_string()))?;
    let mut err = crate::error::DomainError::new(code, message);
    for c in causes {
        err = err.cause(c);
    }
    Ok(err)
}

/// One failed operation for `AgentResponse.partialSuccess` (JavaScript-friendly).
#[napi(object, js_name = "FailedOp")]
#[derive(Debug, Clone)]
pub struct JsFailedOp {
    /// Position in the caller's op slice.
    #[napi(ts_type = "number")]
    pub index: JsCount,
    /// Identifier of the operation that failed.
    pub operation: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Optional recovery hint for this failure (no structured params over this boundary).
    #[napi(ts_type = "RecoveryHint | null")]
    pub recovery: Option<JsRecoveryHint>,
}

/// One completed operation for `AgentResponse.partialSuccess` (JavaScript-friendly). It takes no `reason`.
#[napi(object, js_name = "CompletedOp")]
#[derive(Debug, Clone)]
pub struct JsCompletedOp {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    #[napi(ts_type = "number")]
    pub index: JsCount,
    /// Display name for the operation.
    pub operation: String,
}

/// One skipped operation for `AgentResponse.partialSuccess` (JavaScript-friendly).
#[napi(object, js_name = "SkippedOp")]
#[derive(Debug, Clone)]
pub struct JsSkippedOp {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    #[napi(ts_type = "number")]
    pub index: JsCount,
    /// Display name for the operation.
    pub operation: String,
    /// A stable machine token such as `budget_exhausted`, never prose; `null` when absent.
    #[napi(ts_type = "string | null")]
    pub reason: Option<String>,
}

/// A `PartialSuccess` from its three lists, for `AgentResponse.partialSuccess`; an index with more than one outcome is
/// an error.
pub(super) fn build_partial_success(
    completed: Vec<JsCompletedOp>,
    failed: Vec<JsFailedOp>,
    skipped: Vec<JsSkippedOp>,
) -> napi::Result<crate::idempotency::PartialSuccess> {
    let failed = failed
        .into_iter()
        .map(|f| {
            let op = crate::idempotency::FailedOp::new(f.index.get(), f.operation, f.reason);
            match f.recovery {
                Some(h) => op.recovery(h.into()),
                None => op,
            }
        })
        .collect();
    let completed =
        completed.into_iter().map(|o| crate::idempotency::CompletedOp::new(o.index.get(), o.operation)).collect();
    let skipped = skipped
        .into_iter()
        .map(|o| {
            let op = crate::idempotency::SkippedOp::new(o.index.get(), o.operation);
            match o.reason {
                Some(r) => op.reason(r),
                None => op,
            }
        })
        .collect();
    crate::idempotency::PartialSuccess::try_new(completed, failed, skipped)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// One pipeline step for `AgentResponse.pipeline` (JavaScript-friendly).
#[napi(object, js_name = "PipelineStep")]
#[derive(Debug, Clone)]
pub struct JsPipelineStep {
    /// Step identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Status label, as `StepStatus::label` writes it.
    #[napi(
        ts_type = "'pending' | 'running' | 'completed' | 'completed_earlier' | 'failed' | 'outcome_unknown' | 'skipped' | 'not_run' | 'panicked'"
    )]
    pub status: String,
    /// Reason label: required for `outcome_unknown` (`timeout`, `gateway`, `cancelled`, `interrupted`,
    /// `step_failed`), `skipped` (`prerequisite_failed`, `cancelled`) and `not_run` (`circuit_open`, `unverified`,
    /// `start_not_recorded`); absent or `null` for every other status.
    #[napi(
        ts_type = "'timeout' | 'gateway' | 'cancelled' | 'interrupted' | 'step_failed' | 'prerequisite_failed' | 'circuit_open' | 'unverified' | 'start_not_recorded' | null"
    )]
    pub reason: Option<String>,
}

/// A `Pipeline` from its steps, for `AgentResponse.pipeline`, each status and reason parsed by the core label table.
pub(super) fn build_pipeline(steps: Vec<JsPipelineStep>) -> napi::Result<crate::pipeline::Pipeline> {
    let steps = steps
        .into_iter()
        .map(|s| {
            let status = crate::pipeline::StepStatus::from_labels(&s.status, s.reason.as_deref())
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(crate::pipeline::PipelineStep::new(s.id, s.name, status))
        })
        .collect::<napi::Result<Vec<_>>>()?;
    Ok(crate::pipeline::Pipeline::new(steps))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The TypeScript type of renderGithubAnnotation's `code` lists every ErrorCode label. The union is hand-written
    // inside the attribute (napi-derive cannot read it from the enum), so this checks it against `ErrorCode::ALL`.
    #[test]
    fn render_github_annotation_code_union_names_every_error_code() {
        let src = include_str!("outcome.rs");
        let start = src.find("js_name = \"renderGithubAnnotation\"").expect("the export is declared");
        let declaration = &src[start..start + src[start..].find(")]").expect("the attribute ends")];
        for code in crate::ErrorCode::ALL {
            assert!(declaration.contains(&format!("'{}'", code.label())), "missing {} in {declaration}", code.label());
        }
    }

    // The same for `PipelineStep`'s `status` and `reason` types, against `StepStatus::ALL`.
    #[test]
    fn pipeline_step_ts_types_name_every_status_and_reason() {
        let src = include_str!("outcome.rs");
        let start = src.find("pub struct JsPipelineStep").expect("the struct is declared");
        let declaration = &src[start..start + src[start..].find("\n}").expect("the struct ends")];
        for status in crate::pipeline::StepStatus::ALL {
            let quoted = |label: &str| format!("'{label}'");
            assert!(declaration.contains(&quoted(status.label())), "missing {} in {declaration}", status.label());
            if let Some(reason) = status.reason() {
                assert!(declaration.contains(&quoted(reason)), "missing {reason} in {declaration}");
            }
        }
    }

    fn count(n: usize) -> JsCount {
        JsCount::try_from(f64::from(u32::try_from(n).expect("small"))).expect("a valid count")
    }

    #[test]
    fn partial_success_rejects_two_outcomes_for_one_index() {
        let completed = vec![JsCompletedOp { index: count(0), operation: "a".into() }];
        let failed = vec![JsFailedOp { index: count(0), operation: "a".into(), reason: "boom".into(), recovery: None }];
        let err = build_partial_success(completed.clone(), failed, vec![]).expect_err("index 0 twice");
        assert_eq!(err.reason, "operation index 0 has more than one outcome");
        let skipped = vec![JsSkippedOp { index: count(1), operation: "b".into(), reason: None }];
        let out = build_partial_success(completed, vec![], skipped).expect("distinct indices").render();
        assert_eq!(out, "partial_success[2]{index,operation,status,reason}:\n  0,a,completed,null\n  1,b,skipped,null");
    }

    #[test]
    fn github_annotation_rejects_unknown_code_with_enumerated_message() {
        let err = github_annotation("bogus_code", "msg".into()).expect_err("unknown code");
        assert_eq!(
            err.reason,
            "unknown error code \"bogus_code\"; expected one of: invalid_input, not_found, unauthorized, \
             forbidden, conflict, rate_limited, unavailable, timeout, external_failure"
        );
    }

    #[test]
    fn github_annotation_is_one_encoded_line() {
        assert_eq!(github_annotation("not_found", "gone".into()).expect("valid"), "::error title=not_found::gone");
        let out = github_annotation("external_failure", "50% done\nthen failed".into()).expect("valid");
        assert_eq!(out, "::error title=external_failure::50%25 done%0Athen failed");
    }

    #[test]
    fn pipeline_parses_every_status_through_the_core_table() {
        for status in crate::pipeline::StepStatus::ALL {
            let step = JsPipelineStep {
                id: "a".into(),
                name: "n".into(),
                status: status.label().into(),
                reason: status.reason().map(Into::into),
            };
            let out = build_pipeline(vec![step]).expect("every label pair parses");
            let want = crate::pipeline::Pipeline::new(vec![crate::pipeline::PipelineStep::new("a", "n", status)]);
            assert_eq!(out.render(), want.render());
        }
        let bad = JsPipelineStep { id: "a".into(), name: "n".into(), status: "skipped".into(), reason: None };
        let err = build_pipeline(vec![bad]).expect_err("skipped needs a reason");
        assert_eq!(err.reason, "step status \"skipped\" needs a reason: prerequisite_failed or cancelled");
    }
}

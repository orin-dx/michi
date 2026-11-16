//! Renders of how an operation went: `renderDomainError`, `renderAlreadyDone`, `alreadyDone`,
//! `renderPartialSuccess` and `renderPipeline`.

use napi::bindgen_prelude::Unknown;
use napi_derive::napi;

use super::hints::JsRecoveryHint;
use super::input::{self, Budget, List, MAX_HINTS, MAX_NAME_BYTES, MAX_STEPS, MAX_STRING_BYTES};
use super::num::JsCount;

/// Render the TOON 4.4 `already_done` document: `operation`, `status: already_done`, `summary`, `help`.
///
/// # Errors
///
/// Rejects more than 10,000 hints, an `operation` over 4 KiB, a `summary` or hint over 1 MiB, and more than 16 MiB
/// of strings per call.
#[napi(
    catch_unwind,
    js_name = "renderAlreadyDone",
    ts_args_type = "operation: string, summary: string, hints: Array<string>"
)]
pub fn render_already_done(operation: Unknown<'_>, summary: Unknown<'_>, hints: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let operation = budget.string(operation, "operation", MAX_NAME_BYTES)?;
    let summary = budget.string(summary, "summary", MAX_STRING_BYTES)?;
    let hints: Vec<crate::hints::Hint> =
        budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?.into_iter().map(Into::into).collect();
    Ok(crate::resilience::render_already_done(&operation, &summary, &hints))
}

/// Render a classified `DomainError` as a TOON 4.4 document (`error`, `message`, `causes`, `exit_code`, `help`), or
/// as a GitHub Actions annotation (`::error title={code}::{message}`, without hints or causes). For its JSON or an MCP
/// result, or to report it next to data, use `AgentResponse.error()`: the same text, one JSON shape.
///
/// `causes` (optional) are why the error happened, outermost first, rendered as their own `causes[N]: …` key after the
/// message; `hints` are what to do next.
///
/// # Errors
///
/// Rejects an unknown `code`, more than 10,000 hints or causes, a `code` over 4 KiB, a `message`, hint or cause over
/// 1 MiB, and more than 16 MiB of strings per call.
#[napi(
    catch_unwind,
    js_name = "renderDomainError",
    ts_args_type = "code: 'invalid_input' | 'not_found' | 'unauthorized' | 'forbidden' | 'conflict' | 'rate_limited' | \
                    'unavailable' | 'timeout' | 'external_failure', message: string, hints: Array<string>, \
                    githubAnnotation?: boolean | undefined | null, causes?: Array<string> | undefined | null"
)]
pub fn render_domain_error(
    code: Unknown<'_>,
    message: Unknown<'_>,
    hints: Unknown<'_>,
    github_annotation: Option<bool>,
    causes: Option<Unknown<'_>>,
) -> napi::Result<String> {
    let mut budget = Budget::new();
    let hints = List::new(hints, "hints")?;
    input::check_len("hints", hints.len(), MAX_HINTS)?;
    let causes = List::optional(causes, "causes")?;
    if let Some(causes) = &causes {
        input::check_len("causes", causes.len(), MAX_HINTS)?;
    }
    let code = budget.string(code, "code", MAX_NAME_BYTES)?;
    let message = budget.string(message, "message", MAX_STRING_BYTES)?;
    let hints = budget.strings(&hints, "hints", MAX_HINTS, "hint", MAX_STRING_BYTES)?;
    let causes = match &causes {
        Some(causes) => budget.strings(causes, "causes", MAX_HINTS, "cause", MAX_STRING_BYTES)?,
        None => Vec::new(),
    };
    domain_error(&code, message, hints, github_annotation.unwrap_or(false), causes)
}

/// A `DomainError` from its code label, message and causes. Shared by `renderDomainError` and `AgentResponse.error`.
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

fn domain_error(
    code: &str,
    message: String,
    hints: Vec<String>,
    github_annotation: bool,
    causes: Vec<String>,
) -> napi::Result<String> {
    let mut err = build_domain_error(code, message, causes)?;
    for h in hints {
        err = err.hint(h);
    }
    Ok(if github_annotation { err.render_github_annotation() } else { err.render(crate::response::OutputFormat::Text) })
}

/// One failed operation for `renderPartialSuccess` (JavaScript-friendly).
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

/// One completed operation for `renderPartialSuccess` (JavaScript-friendly). It takes no `reason`.
#[napi(object, js_name = "CompletedOp")]
#[derive(Debug, Clone)]
pub struct JsCompletedOp {
    /// Position in the caller's op slice. `results[index]` answers `ops[index]`.
    #[napi(ts_type = "number")]
    pub index: JsCount,
    /// Display name for the operation.
    pub operation: String,
}

/// One skipped operation for `renderPartialSuccess` (JavaScript-friendly).
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

/// Render a partial-success report: one `partial_success[N]{index,operation,status,reason}:` table, a row per
/// operation sorted by `index` (`status` is `completed`, `failed` or `skipped`; `reason` is the failure reason, the
/// skip token or `null`), then the failed operations' recovery hints as one `recovery[N]{tool,params,reason}:` table.
/// No operations render `partial_success: []` and `totalCount: 0`. To put the table next to a batch's results, use
/// `AgentResponse.partialSuccess()`.
///
/// # Errors
///
/// Rejects an `index` that appears more than once across the three lists (each operation has one outcome), a
/// completed operation with a `reason`, more than 10,000 entries in `completed`, `failed` or `skipped`, an
/// `operation`, a skipped `reason` or a recovery `tool` over 4 KiB, a failed `reason` over 1 MiB, an `index` that is
/// not an integer in `0..=2^53-1`, and more than 16 MiB of strings per call.
#[napi(
    catch_unwind,
    js_name = "renderPartialSuccess",
    ts_args_type = "completed: Array<CompletedOp>, failed: Array<FailedOp>, skipped: Array<SkippedOp>"
)]
pub fn render_partial_success(
    completed: Unknown<'_>,
    failed: Unknown<'_>,
    skipped: Unknown<'_>,
) -> napi::Result<String> {
    let mut budget = Budget::new();
    let completed = List::new(completed, "completed")?;
    let failed = List::new(failed, "failed")?;
    let skipped = List::new(skipped, "skipped")?;
    input::check_len("completed", completed.len(), MAX_STEPS)?;
    input::check_len("failed", failed.len(), MAX_STEPS)?;
    input::check_len("skipped", skipped.len(), MAX_STEPS)?;
    let completed = budget.completed_ops(&completed, MAX_STEPS)?;
    let failed = budget.failed_ops(&failed, MAX_STEPS)?;
    let skipped = budget.skipped_ops(&skipped, MAX_STEPS)?;
    partial_success(completed, failed, skipped)
}

fn partial_success(
    completed: Vec<JsCompletedOp>,
    failed: Vec<JsFailedOp>,
    skipped: Vec<JsSkippedOp>,
) -> napi::Result<String> {
    Ok(build_partial_success(completed, failed, skipped)?.render())
}

/// A `PartialSuccess` from its three lists; an index with more than one outcome is an error. Shared by
/// `renderPartialSuccess` and `AgentResponse.partialSuccess`.
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

/// One pipeline step for `renderPipeline` (JavaScript-friendly).
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

/// Render pipeline state as the TOON 4.4 table `step[N]{id,name,status,reason}:`, or `step: []` and `totalCount: 0`
/// with no steps. To put it next to other sections, use `AgentResponse.pipeline()`.
///
/// # Errors
///
/// Rejects an unknown `status`, a missing or unknown `reason` for `outcome_unknown`, `skipped` or `not_run`, a
/// `reason` on any other status, more than 10,000 steps, an `id`, `status` or `reason` over 4 KiB, a `name` over
/// 1 MiB, and more than 16 MiB of strings per call.
#[napi(catch_unwind, js_name = "renderPipeline", ts_args_type = "steps: Array<PipelineStep>")]
pub fn render_pipeline(steps: Unknown<'_>) -> napi::Result<String> {
    let mut budget = Budget::new();
    let steps = List::new(steps, "steps")?;
    input::check_len("steps", steps.len(), MAX_STEPS)?;
    pipeline_table(budget.pipeline_steps(&steps, MAX_STEPS)?)
}

fn pipeline_table(steps: Vec<JsPipelineStep>) -> napi::Result<String> {
    Ok(build_pipeline(steps)?.render())
}

/// A `Pipeline` from its steps, each status and reason parsed by the core label table. Shared by `renderPipeline` and
/// `AgentResponse.pipeline`.
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

/// The result of `alreadyDone` (JavaScript-friendly).
#[napi(object, js_name = "AlreadyDone")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsAlreadyDone {
    /// Whether the operation completed in a previous call.
    pub done: bool,
    /// The previously stored result when `done`; absent otherwise.
    pub result: Option<String>,
}

/// Classify an idempotency lookup: a stored result (any string, even empty) means the operation already completed;
/// `null` or `undefined` means it has not been seen.
///
/// # Errors
///
/// Rejects a `stored` value that is not a string, `null` or `undefined`, and one over 16 MiB.
#[napi(
    catch_unwind,
    js_name = "alreadyDone",
    ts_args_type = "stored?: string | undefined | null",
    ts_return_type = "AlreadyDone"
)]
pub fn already_done(stored: Option<Unknown<'_>>) -> napi::Result<JsAlreadyDone> {
    let mut budget = Budget::new();
    let stored = match stored {
        Some(v) if !input::is_nullish(v)? => Some(budget.string(v, "stored", input::MAX_INPUT_BYTES)?),
        _ => None,
    };
    Ok(done_or_not(stored))
}

fn done_or_not(stored: Option<String>) -> JsAlreadyDone {
    // Every variant is named: a new one must fail to compile here rather than read as "not done" and re-run work.
    match crate::resilience::already_done(stored) {
        crate::resilience::AlreadyDone::Yes { result } => JsAlreadyDone { done: true, result: Some(result) },
        crate::resilience::AlreadyDone::No => JsAlreadyDone { done: false, result: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The TypeScript type of renderDomainError's `code` lists every ErrorCode label. The union is hand-written inside
    // the attribute (napi-derive cannot read it from the enum), so this checks it against `ErrorCode::ALL`.
    #[test]
    fn render_domain_error_code_union_names_every_error_code() {
        let src = include_str!("outcome.rs");
        let start = src.find("js_name = \"renderDomainError\"").expect("the export is declared");
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

    #[test]
    fn render_already_done_basic() {
        let out = crate::resilience::render_already_done("delete_item", "already deleted", &[]);
        assert!(out.contains("operation: delete_item"), "got: {out}");
        assert!(out.contains("status: already_done"), "got: {out}");
        assert!(out.contains("summary: already deleted"), "got: {out}");
    }

    fn count(n: usize) -> JsCount {
        JsCount::try_from(f64::from(u32::try_from(n).expect("small"))).expect("a valid count")
    }

    #[test]
    fn partial_success_rejects_two_outcomes_for_one_index() {
        let completed = vec![JsCompletedOp { index: count(0), operation: "a".into() }];
        let failed = vec![JsFailedOp { index: count(0), operation: "a".into(), reason: "boom".into(), recovery: None }];
        let err = partial_success(completed.clone(), failed, vec![]).expect_err("index 0 twice");
        assert_eq!(err.reason, "operation index 0 has more than one outcome");
        let skipped = vec![JsSkippedOp { index: count(1), operation: "b".into(), reason: None }];
        let out = partial_success(completed, vec![], skipped).expect("distinct indices");
        assert_eq!(out, "partial_success[2]{index,operation,status,reason}:\n  0,a,completed,null\n  1,b,skipped,null");
    }

    #[test]
    fn domain_error_rejects_unknown_code_with_enumerated_message() {
        let err = domain_error("bogus_code", "msg".into(), vec![], false, vec![]).expect_err("unknown code");
        assert_eq!(
            err.reason,
            "unknown error code \"bogus_code\"; expected one of: invalid_input, not_found, unauthorized, \
             forbidden, conflict, rate_limited, unavailable, timeout, external_failure"
        );
    }

    #[test]
    fn domain_error_renders_causes_and_hints_and_the_annotation() {
        let out = domain_error(
            "external_failure",
            "walk failed".into(),
            vec!["check permissions".into()],
            false,
            vec!["io error".into(), "a\nb".into()],
        )
        .expect("valid causes");
        assert_eq!(
            out,
            "error: external_failure\nmessage: walk failed\ncauses[2]: io error,\"a\\nb\"\nexit_code: 1\n\
             help[1]: check permissions"
        );
        let out = domain_error("not_found", "gone".into(), vec![], true, vec![]).expect("valid");
        assert_eq!(out, "::error title=not_found::gone");
    }

    #[test]
    fn already_done_maps_every_variant_explicitly() {
        // A wildcard arm would report any future `AlreadyDone` variant as not done, telling JS to run it again.
        let src = include_str!("outcome.rs");
        let start = src.find("fn done_or_not(").expect("the mapping is defined");
        let body = &src[start..];
        let body = &body[..body.find("\n}\n").expect("function ends")];
        assert!(body.contains("AlreadyDone::No =>"), "{body}");
        assert!(!body.contains("_ =>"), "no wildcard arm:\n{body}");
        assert_eq!(done_or_not(Some(String::new())), JsAlreadyDone { done: true, result: Some(String::new()) });
        assert_eq!(done_or_not(None), JsAlreadyDone { done: false, result: None });
    }

    #[test]
    fn pipeline_table_parses_every_status_through_the_core_table() {
        for status in crate::pipeline::StepStatus::ALL {
            let step = JsPipelineStep {
                id: "a".into(),
                name: "n".into(),
                status: status.label().into(),
                reason: status.reason().map(Into::into),
            };
            let out = pipeline_table(vec![step]).expect("every label pair parses");
            let want = crate::pipeline::Pipeline::new(vec![crate::pipeline::PipelineStep::new("a", "n", status)]);
            assert_eq!(out, want.render());
        }
        let bad = JsPipelineStep { id: "a".into(), name: "n".into(), status: "skipped".into(), reason: None };
        let err = pipeline_table(vec![bad]).expect_err("skipped needs a reason");
        assert_eq!(err.reason, "step status \"skipped\" needs a reason: prerequisite_failed or cancelled");
    }
}

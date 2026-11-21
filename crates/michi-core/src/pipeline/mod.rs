/// Status of an individual pipeline step.
///
/// Each terminal status maps to one recovery action an agent can take; see "Status model" in
/// `docs/spec/08-pipeline-execution.md`.
/// Only [`StepStatus::Completed`] and [`StepStatus::CompletedEarlier`] count as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum StepStatus {
    /// Not started yet.
    Pending,
    /// In flight. A finished run never reports it; it appears in records persisted while a run was in progress.
    Running,
    /// Ran and succeeded in this run.
    Completed,
    /// Its effect was already in place, so it was not run: it succeeded earlier in the same run (carried over by
    /// resume), the caller verified it as applied, or its precheck found the effect already applied.
    CompletedEarlier,
    /// Failed definitively, or with an error that is safe to replay. Also covers a step that was never started
    /// because its deadline group's window had closed (michi-pipeline's `DeadlinePassed`).
    Failed,
    /// Ran, but whether it took effect is unknown. Verify before running it again.
    OutcomeUnknown(UnknownCause),
    /// Never took effect, for the given reason.
    Skipped(SkipReason),
    /// Never invoked, for the given reason.
    NotRun(NotRunReason),
    /// The step's own code panicked: a harness failure, distinct from an error the step returned.
    Panicked,
}

/// Why a step was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum SkipReason {
    /// A prerequisite did not complete.
    PrerequisiteFailed,
    /// The run was cancelled before this step took effect.
    Cancelled,
}

/// Why a step was never invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum NotRunReason {
    /// Its circuit breaker was open.
    CircuitOpen,
    /// Its precheck could not tell whether its effect was already in place, so it was not run. Rendered
    /// `unverified`.
    Unverified,
    /// Its start could not be recorded durably, so it was not run. Rendered `start_not_recorded`.
    StartNotRecorded,
}

/// Why a step's outcome is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum UnknownCause {
    /// An attempt timed out.
    Timeout,
    /// A gateway reported HTTP 502 or 504.
    Gateway,
    /// The run was cancelled while the step was in flight and the attempt was dropped.
    Cancelled,
    /// The step was in flight when the process running it stopped.
    Interrupted,
    /// The step failed with an error that does not say whether its effect landed (an HTTP 500, say). Rendered
    /// `step_failed`.
    StepFailed,
}

impl StepStatus {
    /// The snake_case status label used in rendered output, without its reason.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedEarlier => "completed_earlier",
            Self::Failed => "failed",
            Self::OutcomeUnknown(_) => "outcome_unknown",
            Self::Skipped(_) => "skipped",
            Self::NotRun(_) => "not_run",
            Self::Panicked => "panicked",
        }
    }

    /// The snake_case reason label for statuses that carry one, `None` otherwise.
    #[must_use]
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Skipped(SkipReason::PrerequisiteFailed) => Some("prerequisite_failed"),
            Self::Skipped(SkipReason::Cancelled) | Self::OutcomeUnknown(UnknownCause::Cancelled) => Some("cancelled"),
            Self::NotRun(NotRunReason::CircuitOpen) => Some("circuit_open"),
            Self::OutcomeUnknown(UnknownCause::Timeout) => Some("timeout"),
            Self::OutcomeUnknown(UnknownCause::Gateway) => Some("gateway"),
            Self::OutcomeUnknown(UnknownCause::Interrupted) => Some("interrupted"),
            Self::OutcomeUnknown(UnknownCause::StepFailed) => Some("step_failed"),
            Self::NotRun(NotRunReason::Unverified) => Some("unverified"),
            Self::NotRun(NotRunReason::StartNotRecorded) => Some("start_not_recorded"),
            Self::Pending
            | Self::Running
            | Self::Completed
            | Self::CompletedEarlier
            | Self::Failed
            | Self::Panicked => None,
        }
    }

    /// Whether the step succeeded: [`StepStatus::Completed`] or [`StepStatus::CompletedEarlier`].
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed | Self::CompletedEarlier)
    }

    /// Every status with every reason, in declaration order. [`StepStatus::from_labels`] searches this list, so a
    /// label is parsed by the same table that [`label`](Self::label) and [`reason`](Self::reason) write; a unit test
    /// names every variant without a wildcard, so a new one fails to compile there until it is listed here.
    pub const ALL: [Self; 16] = [
        Self::Pending,
        Self::Running,
        Self::Completed,
        Self::CompletedEarlier,
        Self::Failed,
        Self::OutcomeUnknown(UnknownCause::Timeout),
        Self::OutcomeUnknown(UnknownCause::Gateway),
        Self::OutcomeUnknown(UnknownCause::Cancelled),
        Self::OutcomeUnknown(UnknownCause::Interrupted),
        Self::OutcomeUnknown(UnknownCause::StepFailed),
        Self::Skipped(SkipReason::PrerequisiteFailed),
        Self::Skipped(SkipReason::Cancelled),
        Self::NotRun(NotRunReason::CircuitOpen),
        Self::NotRun(NotRunReason::Unverified),
        Self::NotRun(NotRunReason::StartNotRecorded),
        Self::Panicked,
    ];

    /// The status a [`label`](Self::label) and [`reason`](Self::reason) pair name: the inverse of those two methods.
    ///
    /// # Errors
    ///
    /// [`StepLabelError`] for an unknown status, a status that needs a reason and has none, an unknown reason, or a
    /// reason on a status that takes none.
    pub fn from_labels(status: &str, reason: Option<&str>) -> Result<Self, StepLabelError> {
        let mut known = false;
        for candidate in Self::ALL {
            if candidate.label() != status {
                continue;
            }
            known = true;
            if candidate.reason() == reason {
                return Ok(candidate);
            }
        }
        let own = || status.to_string();
        if !known {
            return Err(StepLabelError::UnknownStatus { status: own() });
        }
        // The status exists but this reason does not fit: it either needs one or must not have one.
        match reason {
            None => Err(StepLabelError::MissingReason { status: own() }),
            Some(r) if Self::reasons_of(status).next().is_none() => {
                Err(StepLabelError::UnexpectedReason { status: own(), reason: r.to_string() })
            }
            Some(r) => Err(StepLabelError::UnknownReason { status: own(), reason: r.to_string() }),
        }
    }

    /// The reason labels `status` takes, in declaration order; none for a status without a reason.
    fn reasons_of(status: &str) -> impl Iterator<Item = &'static str> + '_ {
        Self::ALL.into_iter().filter(move |s| s.label() == status).filter_map(|s| s.reason())
    }
}

/// Why [`StepStatus::from_labels`] rejected a status and reason pair.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StepLabelError {
    /// The status label names no status.
    UnknownStatus {
        /// The label that was given.
        status: String,
    },
    /// The status needs a reason and none was given.
    MissingReason {
        /// The status label.
        status: String,
    },
    /// The status takes no reason and one was given.
    UnexpectedReason {
        /// The status label.
        status: String,
        /// The reason that was given.
        reason: String,
    },
    /// The status takes a reason, but not this one.
    UnknownReason {
        /// The status label.
        status: String,
        /// The reason that was given.
        reason: String,
    },
}

/// `a, b, c` for the labels in `labels`, or `a, b or c` when `last_or` is set.
fn join_labels(labels: impl Iterator<Item = &'static str>, last_or: bool) -> String {
    let labels: Vec<&str> = labels.collect();
    match (labels.split_last(), last_or) {
        (Some((last, rest)), true) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => labels.join(", "),
    }
}

impl std::fmt::Display for StepLabelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownStatus { status } => {
                // Each status once, in declaration order (a status with several reasons appears several times).
                let mut seen: Vec<&'static str> = Vec::new();
                for s in StepStatus::ALL {
                    if !seen.contains(&s.label()) {
                        seen.push(s.label());
                    }
                }
                write!(f, "unknown step status {status:?}: expected one of {}", seen.join(", "))
            }
            Self::MissingReason { status } => {
                let expected = join_labels(StepStatus::reasons_of(status), true);
                write!(f, "step status {status:?} needs a reason: {expected}")
            }
            Self::UnexpectedReason { status, reason } => {
                write!(f, "step status {status:?} takes no reason, got {reason:?}")
            }
            Self::UnknownReason { status, reason } => {
                let expected = join_labels(StepStatus::reasons_of(status), true);
                write!(f, "unknown reason {reason:?} for step status {status:?}: expected {expected}")
            }
        }
    }
}

impl std::error::Error for StepLabelError {}

/// A pipeline step definition.
///
/// `#[non_exhaustive]`: build it with [`PipelineStep::new`]; the fields stay public.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PipelineStep {
    /// Step identifier.
    ///
    /// michi does not validate uniqueness — callers are responsible for ensuring
    /// step IDs are distinct within a pipeline if they intend to reference steps
    /// by ID. Duplicate IDs render without error but produce ambiguous output.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Current status.
    pub status: StepStatus,
}

impl PipelineStep {
    /// A step with its id, display name and status.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, status: StepStatus) -> Self {
        Self { id: id.into(), name: name.into(), status }
    }
}

/// A pipeline run state: its steps and their statuses.
///
/// It has no run id: nothing renders one, so a field for it would carry a value no reader ever sees. Put the id in the
/// surrounding response if an agent needs it.
///
/// `#[non_exhaustive]`: build it with [`Pipeline::new`] or [`Pipeline::default`]; the fields stay public.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pipeline {
    /// All steps in declaration order.
    pub steps: Vec<PipelineStep>,
}

impl Pipeline {
    /// A pipeline with its steps in declaration order.
    #[must_use]
    pub fn new(steps: Vec<PipelineStep>) -> Self {
        Self { steps }
    }

    /// Render the pipeline state as a TOON 4.4 document, with no trailing newline: the response
    /// `AgentResponse::new("step").pipeline("step", self)` with no cell cap, so it is the same writer a composed
    /// response uses (see [`crate::AgentResponse::pipeline`]). That is `step[N]{id,name,status,reason}:` with one row
    /// per step, or `step: []` and `totalCount: 0` when there are no steps. `status` is [`StepStatus::label`] and
    /// `reason` is [`StepStatus::reason`] or `null`. `id` and `name` are quoted when TOON's string rules (§7.2) require
    /// it.
    #[must_use]
    pub fn render(&self) -> String {
        crate::AgentResponse::new("step")
            .truncate_cells_at(usize::MAX)
            .pipeline("step", self.clone())
            .render(crate::OutputFormat::Text)
    }

    /// The table a composed response renders: one row per step, in declaration order.
    pub(crate) fn rows(&self) -> Vec<Vec<crate::toon::Value>> {
        use crate::toon::Value;
        self.steps
            .iter()
            .map(|step| {
                let reason = step.status.reason().map_or(Value::Null, Value::from);
                vec![step.id.as_str().into(), step.name.as_str().into(), step.status.label().into(), reason]
            })
            .collect()
    }
}

/// The columns of the table a [`Pipeline`] renders as.
pub(crate) const FIELDS: [&str; 4] = ["id", "name", "status", "reason"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pipeline_steps() {
        let p = Pipeline {
            steps: vec![
                PipelineStep { id: "fetch".into(), name: "Fetch Data".into(), status: StepStatus::Completed },
                PipelineStep { id: "upload".into(), name: "Upload".into(), status: StepStatus::Pending },
            ],
        };
        let out = p.render();
        assert_eq!(
            out,
            "step[2]{id,name,status,reason}:\n  fetch,Fetch Data,completed,null\n  upload,Upload,pending,null"
        );
    }

    // No wildcard arm: a new `StepStatus` variant fails to compile here until it is placed in `StepStatus::ALL`.
    fn all_index(s: StepStatus) -> usize {
        use {NotRunReason as N, SkipReason as S, StepStatus as T, UnknownCause as U};
        match s {
            T::Pending => 0,
            T::Running => 1,
            T::Completed => 2,
            T::CompletedEarlier => 3,
            T::Failed => 4,
            T::OutcomeUnknown(U::Timeout) => 5,
            T::OutcomeUnknown(U::Gateway) => 6,
            T::OutcomeUnknown(U::Cancelled) => 7,
            T::OutcomeUnknown(U::Interrupted) => 8,
            T::OutcomeUnknown(U::StepFailed) => 9,
            T::Skipped(S::PrerequisiteFailed) => 10,
            T::Skipped(S::Cancelled) => 11,
            T::NotRun(N::CircuitOpen) => 12,
            T::NotRun(N::Unverified) => 13,
            T::NotRun(N::StartNotRecorded) => 14,
            T::Panicked => 15,
        }
    }

    #[test]
    fn step_status_all_lists_every_variant_once_in_order() {
        for (i, s) in StepStatus::ALL.into_iter().enumerate() {
            assert_eq!(all_index(s), i, "{s:?} is out of place in StepStatus::ALL");
        }
    }

    #[test]
    fn from_labels_inverts_label_and_reason_for_every_status() {
        for status in StepStatus::ALL {
            assert_eq!(StepStatus::from_labels(status.label(), status.reason()), Ok(status));
        }
    }

    #[test]
    fn from_labels_errors_say_what_is_wrong() {
        let err = |s: &str, r: Option<&str>| StepStatus::from_labels(s, r).expect_err("rejected").to_string();
        assert_eq!(
            err("bogus", None),
            "unknown step status \"bogus\": expected one of pending, running, completed, completed_earlier, failed, \
             outcome_unknown, skipped, not_run, panicked"
        );
        assert_eq!(
            err("outcome_unknown", None),
            "step status \"outcome_unknown\" needs a reason: timeout, gateway, cancelled, interrupted or step_failed"
        );
        assert_eq!(
            err("skipped", Some("nope")),
            "unknown reason \"nope\" for step status \"skipped\": expected prerequisite_failed or cancelled"
        );
        assert_eq!(err("failed", Some("x")), "step status \"failed\" takes no reason, got \"x\"");
        // A reason of another status is not accepted either.
        assert!(StepStatus::from_labels("not_run", Some("timeout")).is_err());
    }

    #[test]
    fn empty_pipeline_renders_zero_steps() {
        let p = Pipeline { steps: vec![] };
        let out = p.render();
        assert_eq!(out, "step: []\ntotalCount: 0");
    }

    #[test]
    fn failed_step_label() {
        let p = Pipeline {
            steps: vec![PipelineStep { id: "build".into(), name: "Build".into(), status: StepStatus::Failed }],
        };
        assert!(p.render().ends_with(",failed,null"), "got: {}", p.render());
    }

    #[test]
    fn skipped_step_label() {
        let p = Pipeline {
            steps: vec![PipelineStep {
                id: "lint".into(),
                name: "Lint".into(),
                status: StepStatus::Skipped(SkipReason::PrerequisiteFailed),
            }],
        };
        assert!(p.render().ends_with(",skipped,prerequisite_failed"), "got: {}", p.render());
    }

    #[test]
    fn effect_model_reasons_render_snake_case_labels() {
        let p = Pipeline::new(vec![
            PipelineStep::new("a", "A", StepStatus::OutcomeUnknown(UnknownCause::StepFailed)),
            PipelineStep::new("b", "B", StepStatus::NotRun(NotRunReason::Unverified)),
            PipelineStep::new("c", "C", StepStatus::NotRun(NotRunReason::StartNotRecorded)),
        ]);
        assert_eq!(
            p.render(),
            "step[3]{id,name,status,reason}:\n  a,A,outcome_unknown,step_failed\n  b,B,not_run,unverified\n  \
             c,C,not_run,start_not_recorded"
        );
    }

    #[test]
    fn step_id_with_comma_is_toon_escaped() {
        let p = Pipeline {
            steps: vec![PipelineStep { id: "a,b".into(), name: "A,B Step".into(), status: StepStatus::Pending }],
        };
        let out = p.render();
        // Comma in id/name must be escaped so TOON field separation is preserved
        assert!(!out.contains("a,b,A,B"), "raw commas must not appear unescaped, got: {out}");
    }

    // AC-001: an exhaustive match with exactly these 9 arms and no wildcard
    // compiles today; adding a variant would break this (non_exhaustive
    // only blocks exhaustive matching from OUTSIDE the crate — see AC-003's
    // trybuild fixture for that half).
    #[test]
    fn ac001_step_status_has_exactly_nine_variants() {
        fn all_variants(s: StepStatus) -> &'static str {
            match s {
                StepStatus::Pending => "pending",
                StepStatus::Running => "running",
                StepStatus::Completed => "completed",
                StepStatus::CompletedEarlier => "completed_earlier",
                StepStatus::Failed => "failed",
                StepStatus::OutcomeUnknown(_) => "outcome_unknown",
                StepStatus::Skipped(_) => "skipped",
                StepStatus::NotRun(_) => "not_run",
                StepStatus::Panicked => "panicked",
            }
        }
        assert_eq!(all_variants(StepStatus::Completed), "completed");
        assert_eq!(all_variants(StepStatus::Panicked), StepStatus::Panicked.label());
    }

    #[test]
    fn ac002_step_status_is_copy_and_comparable() {
        let a = StepStatus::Completed;
        let b = a;
        assert_eq!(a, b, "must still be usable after copy");
        assert_eq!(StepStatus::Completed, StepStatus::Completed);
        assert_ne!(StepStatus::Completed, StepStatus::Failed);
    }

    #[test]
    fn ac004_completed_label_is_exact() {
        assert_eq!(StepStatus::Completed.label(), "completed");
    }

    #[test]
    fn ac005_skipped_label_is_exact() {
        assert_eq!(StepStatus::Skipped(SkipReason::Cancelled).label(), "skipped");
    }

    #[test]
    fn ac006_failed_label_is_exact() {
        assert_eq!(StepStatus::Failed.label(), "failed");
    }

    #[test]
    fn ac007_pending_label_is_exact() {
        assert_eq!(StepStatus::Pending.label(), "pending");
    }

    #[test]
    fn ac010_pipeline_default_has_no_steps() {
        let p = Pipeline::default();
        assert_eq!(p.steps.len(), 0);
    }

    // AC-014: the existing step_id_with_comma_is_toon_escaped test above only
    // checks a negative substring, not the exact quoted output.
    #[test]
    fn ac014_comma_in_id_and_name_renders_exact_quoted_literal() {
        let p = Pipeline {
            steps: vec![PipelineStep { id: "a,b".into(), name: "A,B Step".into(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  \"a,b\",\"A,B Step\",pending,null");
    }

    #[test]
    fn ac017_render_is_idempotent_and_does_not_mutate() {
        let p =
            Pipeline { steps: vec![PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Completed }] };
        let first = p.render();
        let second = p.render();
        assert_eq!(first, second);
        assert_eq!(p.steps[0].id, "s");
        assert_eq!(p.steps[0].name, "n");
        assert_eq!(p.steps[0].status, StepStatus::Completed);
    }

    #[test]
    fn ac018_step_order_is_preserved_not_reordered() {
        let p = Pipeline {
            steps: vec![
                PipelineStep { id: "C".into(), name: "c".into(), status: StepStatus::Completed },
                PipelineStep { id: "A".into(), name: "a".into(), status: StepStatus::Completed },
                PipelineStep { id: "B".into(), name: "b".into(), status: StepStatus::Completed },
            ],
        };
        assert_eq!(
            p.render(),
            "step[3]{id,name,status,reason}:\n  C,c,completed,null\n  A,a,completed,null\n  B,b,completed,null"
        );
    }

    #[test]
    fn ac019_duplicate_ids_render_without_error_or_dedup() {
        let p = Pipeline {
            steps: vec![
                PipelineStep { id: "dup".into(), name: "first".into(), status: StepStatus::Completed },
                PipelineStep { id: "dup".into(), name: "second".into(), status: StepStatus::Pending },
            ],
        };
        assert_eq!(
            p.render(),
            "step[2]{id,name,status,reason}:\n  dup,first,completed,null\n  dup,second,pending,null"
        );
    }

    #[test]
    fn ac020_empty_id_and_name_render_without_panic() {
        let p = Pipeline {
            steps: vec![PipelineStep { id: String::new(), name: String::new(), status: StepStatus::Pending }],
        };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  \"\",\"\",pending,null");
    }

    #[test]
    fn ac025_embedded_newline_in_id_is_quoted_and_escaped() {
        let id: String = ['l', 'i', 'n', 'e', '1', '\n', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline { steps: vec![PipelineStep { id, name: "ok".into(), status: StepStatus::Pending }] };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  \"line1\\nline2\",ok,pending,null");
    }

    // AC-025's own text says "id or name" interchangeably, but every fixture
    // in this spec's evidence only ever put a newline/CR/quote into id --
    // name only ever appears with a comma (AC-014). escape_value() is called
    // identically on both fields in render(), so this closes that
    // family-sibling gap without a new criterion ID.
    #[test]
    fn ac025_embedded_newline_in_name_is_also_quoted_and_escaped() {
        let name: String = ['l', 'i', 'n', 'e', '1', '\n', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline { steps: vec![PipelineStep { id: "ok".into(), name, status: StepStatus::Pending }] };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  ok,\"line1\\nline2\",pending,null");
    }

    #[test]
    fn ac026_embedded_quote_in_id_is_escaped_within_quoted_field() {
        let id: String = ['a', '"', 'b'].into_iter().collect();
        let p = Pipeline { steps: vec![PipelineStep { id, name: "c".into(), status: StepStatus::Pending }] };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  \"a\\\"b\",c,pending,null");
    }

    #[test]
    fn ac027_clone_is_deep_and_independent() {
        let p =
            Pipeline { steps: vec![PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Completed }] };
        let mut q = p.clone();
        assert_eq!(q.steps.len(), 1);
        assert_eq!(q.steps[0].id, "s");
        assert_eq!(q.steps[0].name, "n");
        assert_eq!(q.steps[0].status, StepStatus::Completed);

        q.steps[0].name = "changed".to_string();
        assert_eq!(p.steps[0].name, "n", "mutating the clone must not affect the original");
    }

    #[test]
    fn ac028_debug_output_is_exact() {
        assert_eq!(format!("{:?}", StepStatus::Completed), "Completed");
        assert_eq!(
            format!("{:?}", PipelineStep { id: "s".into(), name: "n".into(), status: StepStatus::Failed }),
            "PipelineStep { id: \"s\", name: \"n\", status: Failed }"
        );
        assert_eq!(format!("{:?}", Pipeline::default()), "Pipeline { steps: [] }");
    }

    #[test]
    fn ac029_step_status_satisfies_eq_bound() {
        fn requires_eq<T: Eq>() {}
        requires_eq::<StepStatus>();
    }

    #[test]
    fn ac030_embedded_carriage_return_in_id_is_quoted_and_escaped() {
        let id: String = ['l', 'i', 'n', 'e', '1', '\r', 'l', 'i', 'n', 'e', '2'].into_iter().collect();
        let p = Pipeline { steps: vec![PipelineStep { id, name: "ok".into(), status: StepStatus::Pending }] };
        assert_eq!(p.render(), "step[1]{id,name,status,reason}:\n  \"line1\\rline2\",ok,pending,null");
    }
}

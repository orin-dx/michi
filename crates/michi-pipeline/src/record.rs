//! The per-run record: what happened to every step, and the overall outcome.

use std::time::Duration;

use michi_core::pipeline::{Pipeline, PipelineStep, StepStatus};

use crate::{ExecutionError, PlanFingerprint, ReplayClass, RunId, StepId};

/// One attempt of one step.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttemptRecord {
    pub(crate) attempt: u32,
    pub(crate) class: Option<ReplayClass>,
    pub(crate) error: Option<String>,
    pub(crate) elapsed: Duration,
}

impl AttemptRecord {
    /// 0-based attempt number.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// The replay class of the attempt's failure, or `None` if it succeeded.
    #[must_use]
    pub fn class(&self) -> Option<ReplayClass> {
        self.class
    }

    /// The attempt's error message, or `None` if it succeeded.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// How long the attempt ran.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

/// What happened to one step in one run.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StepRecord {
    pub(crate) step_id: StepId,
    pub(crate) name: String,
    pub(crate) status: StepStatus,
    pub(crate) reason: Option<String>,
    pub(crate) error: Option<ExecutionError>,
    pub(crate) attempts: Vec<AttemptRecord>,
    pub(crate) started_at: Option<Duration>,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) caused_by: Option<StepId>,
}

impl StepRecord {
    pub(crate) fn new(step_id: StepId, name: String, status: StepStatus) -> Self {
        Self {
            step_id,
            name,
            status,
            reason: None,
            error: None,
            attempts: Vec::new(),
            started_at: None,
            elapsed: None,
            caused_by: None,
        }
    }

    /// A `Running` record for a step that has started. Persist one from
    /// [`crate::StepObserver::on_step_started`] so that, after a crash, [`RunRecord::from_parts`] and resume treat
    /// the step as possibly applied instead of re-running it blindly.
    #[must_use]
    pub fn in_flight(step_id: StepId, name: impl Into<String>) -> Self {
        let mut record = Self::new(step_id, name.into(), StepStatus::Running);
        record.reason = Some("in flight when the record was saved".to_string());
        record
    }

    /// The step's id.
    #[must_use]
    pub fn step_id(&self) -> &StepId {
        &self.step_id
    }

    /// The step's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The step's terminal status (or `Pending`/`Running` in a partial record).
    #[must_use]
    pub fn status(&self) -> StepStatus {
        self.status
    }

    /// Why the step ended as it did, in words, or `None` for a plain success.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// The step's final error. An ambiguous outcome of a non-idempotent step is an
    /// [`ExecutionError::AmbiguousOutcome`].
    #[must_use]
    pub fn error(&self) -> Option<&ExecutionError> {
        self.error.as_ref()
    }

    /// Every attempt, in order.
    #[must_use]
    pub fn attempts(&self) -> &[AttemptRecord] {
        &self.attempts
    }

    /// When the step was dispatched, relative to the start of the run; `None` if it never was.
    #[must_use]
    pub fn started_at(&self) -> Option<Duration> {
        self.started_at
    }

    /// How long the step took from dispatch to its terminal status; `None` if it never ran.
    #[must_use]
    pub fn elapsed(&self) -> Option<Duration> {
        self.elapsed
    }

    /// For a step skipped because a prerequisite did not complete: the step that failed in its own right.
    #[must_use]
    pub fn caused_by(&self) -> Option<&StepId> {
        self.caused_by.as_ref()
    }

    /// Whether this step failed in its own right: `Failed`, `OutcomeUnknown`, `NotRun` or `Panicked`.
    #[must_use]
    pub fn is_root_cause(&self) -> bool {
        matches!(
            self.status,
            StepStatus::Failed | StepStatus::OutcomeUnknown(_) | StepStatus::NotRun(_) | StepStatus::Panicked
        )
    }
}

/// The overall verdict of a run, one recovery action each.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum RunOutcome {
    /// Every step is `Completed` or `CompletedEarlier`. Nothing to do.
    Succeeded,
    /// Some step's outcome is unknown. Verify it, then resume.
    OutcomeUnknown,
    /// The caller cancelled the run before it finished. Resume.
    Cancelled,
    /// Some step failed, was not run, panicked or was skipped. Fix the root cause, then resume.
    Failed,
}

impl RunOutcome {
    /// The snake_case label.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// What happened in one run: a step record per plan step, in declaration order.
///
/// Returned by [`crate::execute`] and [`crate::resume`]. A plain value: persisting it (with the `serde` feature) is
/// the caller's job.
#[must_use = "a run record says which steps failed or need verification"]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RunRecord {
    pub(crate) run_id: RunId,
    pub(crate) plan_fingerprint: PlanFingerprint,
    pub(crate) steps: Vec<StepRecord>,
    pub(crate) completion_order: Vec<StepId>,
    pub(crate) cancelled: bool,
    pub(crate) elapsed: Duration,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) observer_panics: u32,
}

impl RunRecord {
    /// Rebuilds a record from step records saved one at a time (from a [`crate::StepObserver`]), after a crash.
    /// Steps the plan has but `steps` lacks count as `Pending`; a step saved as `Running` counts as possibly applied.
    /// [`crate::resume`] checks the fingerprint and step ids against the plan.
    pub fn from_parts(run_id: RunId, plan_fingerprint: PlanFingerprint, steps: Vec<StepRecord>) -> Self {
        Self {
            run_id,
            plan_fingerprint,
            steps,
            completion_order: Vec::new(),
            cancelled: false,
            elapsed: Duration::ZERO,
            observer_panics: 0,
        }
    }

    /// The run's id.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// The fingerprint of the plan this run executed.
    pub fn plan_fingerprint(&self) -> PlanFingerprint {
        self.plan_fingerprint
    }

    /// The overall verdict: `Succeeded` only if every step is `Completed` or `CompletedEarlier`; otherwise
    /// `OutcomeUnknown` if any step's outcome is unknown, else `Cancelled` if the caller cancelled, else `Failed`.
    pub fn outcome(&self) -> RunOutcome {
        if self.steps.iter().all(|s| s.status.is_success()) {
            RunOutcome::Succeeded
        } else if self.steps.iter().any(|s| matches!(s.status, StepStatus::OutcomeUnknown(_))) {
            RunOutcome::OutcomeUnknown
        } else if self.cancelled {
            RunOutcome::Cancelled
        } else {
            RunOutcome::Failed
        }
    }

    /// Every step record, in declaration order.
    pub fn steps(&self) -> &[StepRecord] {
        &self.steps
    }

    /// The record for `id`, if the run has that step.
    pub fn step(&self, id: &StepId) -> Option<&StepRecord> {
        self.steps.iter().find(|s| s.step_id == *id)
    }

    /// Steps that failed in their own right (`Failed`, `OutcomeUnknown`, `NotRun`, `Panicked`), in declaration order.
    pub fn root_causes(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| s.is_root_cause())
    }

    /// Steps skipped as a consequence of a failure or cancellation, in declaration order.
    pub fn consequential(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| matches!(s.status, StepStatus::Skipped(_)))
    }

    /// Steps whose outcome is unknown and must be verified before resume will run them again (unless idempotent).
    pub fn needs_verification(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| matches!(s.status, StepStatus::OutcomeUnknown(_)))
    }

    /// Ids of steps that completed in this run, in the order they finished. Steps carried over by resume are not
    /// included.
    pub fn completion_order(&self) -> &[StepId] {
        &self.completion_order
    }

    /// Whether the caller's cancellation token fired during the run.
    pub fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Wall time of the run, on Tokio's clock.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// How many calls to the run's [`crate::StepObserver`] panicked. The executor catches such a panic and carries
    /// on: the steps and this record are unaffected, and the observer is still called for later events. Non-zero
    /// means the observer may have missed events, so progress it persisted may be incomplete.
    pub fn observer_panics(&self) -> u32 {
        self.observer_panics
    }

    /// The first root cause in declaration order, as [`ExecutionError::StepFailed`], for conversion into a
    /// `DomainError`. `None` if no step failed in its own right.
    pub fn error(&self) -> Option<ExecutionError> {
        self.root_causes().next().map(|s| ExecutionError::StepFailed {
            step_id: s.step_id.to_string(),
            step_name: s.name.clone(),
            source: Box::new(s.error.clone().unwrap_or_else(|| ExecutionError::Failed {
                message: s.reason.clone().unwrap_or_default(),
                retryable: false,
            })),
        })
    }

    /// The michi-core [`Pipeline`] view: the run id and each step's id, name and status.
    pub fn to_pipeline(&self) -> Pipeline {
        Pipeline {
            id: self.run_id.to_string(),
            steps: self
                .steps
                .iter()
                .map(|s| PipelineStep { id: s.step_id.to_string(), name: s.name.clone(), status: s.status })
                .collect(),
        }
    }

    /// The run as a TOON 4.4 document: [`Pipeline::render`] of [`RunRecord::to_pipeline`].
    pub fn render(&self) -> String {
        self.to_pipeline().render()
    }
}

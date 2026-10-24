//! The per-run record: what happened to every step, and the overall outcome.

use std::time::Duration;

use michi_core::pipeline::{Pipeline, PipelineStep, StepStatus};

use crate::{Effect, ExecutionError, Plan, PlanFingerprint, Replay, ReplayClass, ResumeError, RunId, StepId};

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
    /// The step's [`Replay`] when the executor or [`RunRecord::from_parts`] wrote the record; `None` in a record saved
    /// by hand (such as [`StepRecord::in_flight`]) or by an older michi-pipeline, which counts as not idempotent.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub(crate) replay: Option<Replay>,
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
            replay: None,
        }
    }

    /// A `Running` record for a step that has started. Persist one from [`crate::DurableObserver::record_start`] (with
    /// [`crate::ExecOptions::durable_starts`]) or [`crate::StepObserver::on_step_started`] so that, after a crash,
    /// [`RunRecord::from_parts`] and resume treat the step as possibly applied instead of re-running it blindly.
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

    /// The step's [`Replay`] as the run knew it, or `None` for a record saved by hand or by an older version
    /// (treated as not idempotent).
    #[must_use]
    pub fn replay(&self) -> Option<Replay> {
        self.replay
    }

    /// Whether this step failed in its own right: `Failed`, `OutcomeUnknown`, `NotRun`, `Panicked`, or `Running` (a
    /// rebuilt record's step that was in flight when the process stopped).
    #[must_use]
    pub fn is_root_cause(&self) -> bool {
        matches!(
            self.status,
            StepStatus::Failed
                | StepStatus::OutcomeUnknown(_)
                | StepStatus::NotRun(_)
                | StepStatus::Panicked
                | StepStatus::Running
        )
    }

    /// Whether resume keeps this step as it is until the caller verifies it (unless the step is idempotent or
    /// prechecked), because its effect may have happened: `OutcomeUnknown`, `Panicked`, `Running`, or `Failed` with a
    /// final error whose effect is not [`crate::Effect::NotApplied`].
    ///
    /// The executor ends a step that is not idempotent `OutcomeUnknown` rather than `Failed` when its effect is
    /// unknown, so a `Failed` step here is either idempotent (resume runs it anyway) or comes from a record saved
    /// before 0.5.
    #[must_use]
    pub fn awaits_verification(&self) -> bool {
        self.awaits_verification_under(self.replay.unwrap_or_default())
    }

    /// [`StepRecord::awaits_verification`] for a given replay class, which [`crate::resume`] takes as the stricter of
    /// the record's and the current plan's.
    pub(crate) fn awaits_verification_under(&self, replay: Replay) -> bool {
        // Resume re-runs an idempotent step whatever happened, so nothing needs verifying. A prechecked step stays
        // listed: its precheck may not be able to tell, and a caller's verdict takes precedence over it.
        if replay == Replay::Idempotent {
            return false;
        }
        match self.status {
            StepStatus::OutcomeUnknown(_) | StepStatus::Panicked | StepStatus::Running => true,
            StepStatus::Failed => self.error.as_ref().is_none_or(|e| e.effect() != Effect::NotApplied),
            _ => false,
        }
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
    /// Some step's outcome is unknown, including a step a rebuilt record shows as still `Running`. Verify it, then
    /// resume.
    OutcomeUnknown,
    /// The caller cancelled the run before it finished. Resume.
    Cancelled,
    /// Some step failed, was not run, panicked, was skipped or is still pending. Fix the root cause, then resume.
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

/// What happened in one run: exactly one step record per plan step, in declaration order.
///
/// Returned by [`crate::execute`] and [`crate::resume`], or rebuilt with [`RunRecord::from_parts`]. A plain value:
/// persisting it (with the `serde` feature) is the caller's job. The record stores how many steps its plan has
/// (`plan_steps` in the serialized form), so a record that lost steps can never read as a success: deserializing one
/// whose `steps` do not list `plan_steps` distinct steps is an error.
#[must_use = "a run record says which steps failed or need verification"]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "RunRecordWire"))]
pub struct RunRecord {
    pub(crate) run_id: RunId,
    pub(crate) plan_fingerprint: PlanFingerprint,
    pub(crate) plan_steps: usize,
    pub(crate) steps: Vec<StepRecord>,
    pub(crate) completion_order: Vec<StepId>,
    pub(crate) cancelled: bool,
    pub(crate) elapsed: Duration,
    pub(crate) observer_panics: u32,
}

/// The serialized form of a [`RunRecord`], checked before it becomes one.
#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct RunRecordWire {
    run_id: RunId,
    plan_fingerprint: PlanFingerprint,
    plan_steps: usize,
    steps: Vec<StepRecord>,
    completion_order: Vec<StepId>,
    cancelled: bool,
    elapsed: Duration,
    #[serde(default)]
    observer_panics: u32,
}

#[cfg(feature = "serde")]
impl TryFrom<RunRecordWire> for RunRecord {
    type Error = String;

    fn try_from(wire: RunRecordWire) -> Result<Self, String> {
        if wire.steps.len() != wire.plan_steps {
            return Err(format!(
                "run record lists {} steps but its plan has {}; a record must list every plan step once",
                wire.steps.len(),
                wire.plan_steps
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        if let Some(dup) = wire.steps.iter().find(|s| !seen.insert(&s.step_id)) {
            return Err(format!("step {} is listed twice in the run record", dup.step_id));
        }
        Ok(Self {
            run_id: wire.run_id,
            plan_fingerprint: wire.plan_fingerprint,
            plan_steps: wire.plan_steps,
            steps: wire.steps,
            completion_order: wire.completion_order,
            cancelled: wire.cancelled,
            elapsed: wire.elapsed,
            observer_panics: wire.observer_panics,
        })
    }
}

impl RunRecord {
    /// Rebuilds a record from step records saved one at a time (from a [`crate::StepObserver`]), after a crash.
    ///
    /// `plan_fingerprint` is the fingerprint saved when the run started; `plan` is the plan about to be resumed. The
    /// result has one record per plan step, in declaration order:
    ///
    /// - When `steps` holds several records for one step, the **last** one wins. An append-only log of
    ///   [`StepRecord::in_flight`] on start and the finished record on finish therefore rebuilds each step's latest
    ///   status.
    /// - A plan step with no saved record is `Pending`, so a record missing steps never reads as a success.
    /// - A step saved as `Running` may have taken effect: it is a root cause awaiting verification.
    ///
    /// # Errors
    ///
    /// [`ResumeError::FingerprintMismatch`] if `plan_fingerprint` is not `plan`'s, and [`ResumeError::UnknownStep`]
    /// if a saved record names a step the plan does not have.
    pub fn from_parts(
        run_id: RunId,
        plan_fingerprint: PlanFingerprint,
        plan: &Plan,
        steps: Vec<StepRecord>,
    ) -> Result<Self, ResumeError> {
        if plan_fingerprint != plan.fingerprint() {
            return Err(ResumeError::FingerprintMismatch { record: plan_fingerprint, plan: plan.fingerprint() });
        }
        let mut latest: Vec<Option<StepRecord>> = vec![None; plan.len()];
        for record in steps {
            let Some(i) = plan.index_of(&record.step_id) else {
                return Err(ResumeError::UnknownStep { step_id: record.step_id });
            };
            latest[i] = Some(record);
        }
        let steps = latest
            .into_iter()
            .zip(plan.steps())
            .map(|(saved, step)| {
                let mut record = saved.unwrap_or_else(|| {
                    let mut pending = StepRecord::new(step.id().clone(), step.name().to_string(), StepStatus::Pending);
                    pending.reason = Some("no record was saved for this step".to_string());
                    pending
                });
                // The plan knows each step's replay, even for a hand-saved `in_flight` record.
                record.replay = Some(step.runner().replay());
                record
            })
            .collect();
        Ok(Self {
            run_id,
            plan_fingerprint,
            plan_steps: plan.len(),
            steps,
            completion_order: Vec::new(),
            cancelled: false,
            elapsed: Duration::ZERO,
            observer_panics: 0,
        })
    }

    /// The run's id.
    #[must_use]
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// The fingerprint of the plan this run executed.
    #[must_use]
    pub fn plan_fingerprint(&self) -> PlanFingerprint {
        self.plan_fingerprint
    }

    /// The overall verdict: `Succeeded` only if the record lists every plan step and each is `Completed` or
    /// `CompletedEarlier`; otherwise `OutcomeUnknown` if any step's outcome is unknown (`OutcomeUnknown`, or
    /// `Running` in a rebuilt record), else `Cancelled` if the caller cancelled, else `Failed`.
    #[must_use]
    pub fn outcome(&self) -> RunOutcome {
        if self.steps.len() == self.plan_steps && self.steps.iter().all(|s| s.status.is_success()) {
            RunOutcome::Succeeded
        } else if self.steps.iter().any(|s| matches!(s.status, StepStatus::OutcomeUnknown(_) | StepStatus::Running)) {
            RunOutcome::OutcomeUnknown
        } else if self.cancelled {
            RunOutcome::Cancelled
        } else {
            RunOutcome::Failed
        }
    }

    /// Every step record, in declaration order.
    #[must_use]
    pub fn steps(&self) -> &[StepRecord] {
        &self.steps
    }

    /// The record for `id`, if the run has that step.
    ///
    /// A linear scan: to look up many steps, index [`RunRecord::steps`] once instead of calling this in a loop.
    #[must_use]
    pub fn step(&self, id: &StepId) -> Option<&StepRecord> {
        self.steps.iter().find(|s| s.step_id == *id)
    }

    /// Steps that failed in their own right (`Failed`, `OutcomeUnknown`, `NotRun`, `Panicked`, `Running`), in
    /// declaration order.
    pub fn root_causes(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| s.is_root_cause())
    }

    /// Steps skipped as a consequence of a failure or cancellation, in declaration order.
    pub fn consequential(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| matches!(s.status, StepStatus::Skipped(_)))
    }

    /// Steps whose effect may have happened, which resume keeps as they are until the caller verifies them (unless
    /// the step is idempotent or prechecked): `OutcomeUnknown`, `Panicked`, `Running`, and `Failed` with an error whose
    /// effect is unknown, in declaration order. These are exactly the steps [`crate::ResumeOptions::verified`]
    /// accepts ([`StepRecord::awaits_verification`]).
    pub fn needs_verification(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps.iter().filter(|s| s.awaits_verification())
    }

    /// Ids of steps that completed in this run, in the order they finished. Steps carried over by resume are not
    /// included.
    #[must_use]
    pub fn completion_order(&self) -> &[StepId] {
        &self.completion_order
    }

    /// Whether the caller's cancellation token fired during the run.
    #[must_use]
    pub fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Wall time of the run, on Tokio's clock.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// How many calls to the run's [`crate::StepObserver`] panicked. The executor catches such a panic and carries
    /// on: the steps and this record are unaffected, and the observer is still called for later events. Non-zero
    /// means the observer may have missed events, so progress it persisted may be incomplete.
    #[must_use]
    pub fn observer_panics(&self) -> u32 {
        self.observer_panics
    }

    /// Why the run did not succeed, as [`ExecutionError::StepFailed`] for conversion into a `DomainError`: the first
    /// root cause in declaration order or, when no step failed in its own right (a cancelled run, or a rebuilt record
    /// with pending steps), the first step that did not succeed. `None` only for a [`RunOutcome::Succeeded`] run.
    #[must_use]
    pub fn error(&self) -> Option<ExecutionError> {
        let step = self.root_causes().next().or_else(|| self.steps.iter().find(|s| !s.status.is_success()))?;
        let source = step.error.clone().unwrap_or_else(|| match step.status {
            StepStatus::Skipped(michi_core::pipeline::SkipReason::Cancelled) => ExecutionError::Cancelled,
            StepStatus::Running => {
                ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::Interrupted { elapsed_ms: 0 }) }
            }
            // A pending step never ran, so running it is safe.
            StepStatus::Pending => {
                ExecutionError::rejected(step.reason.clone().unwrap_or_else(|| "the step has not run".to_string()))
            }
            // Never invoked, so nothing took effect.
            StepStatus::Skipped(_) | StepStatus::NotRun(_) => {
                ExecutionError::refused(step.reason.clone().unwrap_or_default())
            }
            // Anything else without an error came from a hand-built record: don't vouch for its effect.
            _ => ExecutionError::failed(step.reason.clone().unwrap_or_default()),
        });
        Some(ExecutionError::StepFailed {
            step_id: step.step_id.to_string(),
            step_name: step.name.clone(),
            source: Box::new(source),
        })
    }

    /// The michi-core [`Pipeline`] view: the run id and each step's id, name and status.
    #[must_use]
    pub fn to_pipeline(&self) -> Pipeline {
        Pipeline::new(
            self.run_id.to_string(),
            self.steps.iter().map(|s| PipelineStep::new(s.step_id.to_string(), s.name.clone(), s.status)).collect(),
        )
    }

    /// The run as a TOON 4.4 document: [`Pipeline::render`] of [`RunRecord::to_pipeline`].
    #[must_use]
    pub fn render(&self) -> String {
        self.to_pipeline().render()
    }
}

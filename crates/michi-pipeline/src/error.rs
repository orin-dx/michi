//! `ExecutionError`, the replay classification and the `DomainError` conversion.

use std::time::Duration;

/// An error from one step attempt, or a step's final error in a [`crate::StepRecord`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ExecutionError {
    /// HTTP-transport failure. A step reports the server's status as-is; see the crate-level replay rule for what
    /// each retryable status means. A bare 502 or 504 in a record means the step was idempotent; for a
    /// non-idempotent step the executor wraps it in [`ExecutionError::AmbiguousOutcome`].
    #[error("http {status}: {message}")]
    Http {
        /// HTTP status code returned by the failed request.
        status: u16,
        /// Human-readable failure message.
        message: String,
        /// Server-supplied retry delay, if the caller already parsed one.
        retry_after: Option<Duration>,
    },
    /// An attempt exceeded its timeout (the executor's or one the step enforced itself). Ambiguous: the attempt may
    /// already have committed.
    #[error("step timed out after {elapsed_ms}ms")]
    Timeout {
        /// Milliseconds elapsed before the attempt was abandoned.
        elapsed_ms: u64,
    },
    /// Generic step failure not tied to HTTP transport. Build it with [`ExecutionError::failed`],
    /// [`ExecutionError::refused`], [`ExecutionError::rejected`] or [`ExecutionError::transient`].
    #[error("{message}")]
    Failed {
        /// Human-readable failure message.
        message: String,
        /// Whether trying again could help. Says nothing about whether the attempt took effect: that is `effect`.
        retryable: bool,
        /// Whether the attempt's effect may have landed. Records saved before this field existed read as
        /// [`Effect::Unknown`].
        #[cfg_attr(feature = "serde", serde(default))]
        effect: Effect,
    },
    /// A circuit breaker short-circuited the call without invoking the step.
    #[error("circuit open, retry after {retry_after_ms}ms")]
    CircuitOpen {
        /// Milliseconds until the breaker may allow a probe. Precise while Open; while another caller's probe is in
        /// flight, a conservative estimate equal to the breaker's open duration.
        retry_after_ms: u64,
    },
    /// A step of a run failed, wrapping which step and its cause. Produced by [`crate::RunRecord::error`].
    #[error("step {step_id} ({step_name}) failed: {source}")]
    StepFailed {
        /// The failed step's id.
        step_id: String,
        /// The failed step's name.
        step_name: String,
        /// The step's final error.
        source: Box<ExecutionError>,
    },
    /// The step observed cancellation and stopped without taking effect. A step returns this only when nothing was
    /// committed.
    #[error("step cancelled before it took effect")]
    Cancelled,
    /// The run was cancelled while this attempt was in flight, and the executor dropped it after the cancellation
    /// grace period. Ambiguous: the attempt may already have committed.
    #[error("attempt interrupted by cancellation after {elapsed_ms}ms")]
    Interrupted {
        /// Milliseconds the attempt ran before it was dropped.
        elapsed_ms: u64,
    },
    /// A step that is not idempotent ended with an error whose effect is unknown (timeout, HTTP 500/502/504,
    /// interruption, [`ExecutionError::failed`]), so the operation may or may not have completed. Never retryable:
    /// verify whether it completed first.
    #[error("outcome unknown, the operation may have completed: {source}")]
    AmbiguousOutcome {
        /// The ambiguous error the last attempt produced.
        source: Box<ExecutionError>,
    },
    /// The step's code panicked. A harness failure, not an error the step returned.
    #[error("step panicked: {message}")]
    Panicked {
        /// The panic payload, if it was a string.
        message: String,
    },
    /// A deadline ended before the step did anything: the executor did not start it because its deadline group's
    /// window had closed. Not applied and not retryable in this run; resume runs the step again in a new window.
    #[error("deadline passed: {message}")]
    DeadlinePassed {
        /// Which deadline, and what was left undone.
        message: String,
    },
}

/// Hint attached to the [`michi_core::DomainError`] converted from an [`ExecutionError::AmbiguousOutcome`].
const VERIFY_BEFORE_RETRY_HINT: &str = "verify whether the operation completed before retrying; \
     the step is not idempotent, so a blind retry could repeat its side effect";

/// Hint attached to the [`michi_core::DomainError`] converted from an [`ExecutionError::Panicked`].
const PANIC_HINT: &str = "the step panicked, which is a bug in the step; fix it and verify whether it took effect \
     before resuming";

/// Hint attached to the [`michi_core::DomainError`] converted from an [`ExecutionError::DeadlinePassed`].
const DEADLINE_HINT: &str = "nothing was applied; resume the run to try the step again with a new deadline";

/// Whether a failed attempt's effect may have landed. The second fact about every failure, next to whether retrying
/// could help ([`ExecutionError::is_retryable`]); neither implies the other.
///
/// There is no `Applied` value: a step that knows its effect landed and still failed says so in its message and
/// reports [`Effect::Unknown`], because resume must not run it again either way.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Effect {
    /// The effect certainly did not land: the step was refused, rejected or never reached its effect.
    NotApplied,
    /// The effect may have landed. The default, so an unclassified failure is never treated as safe to repeat.
    #[default]
    Unknown,
}

/// A failed attempt's two facts in one word, kept per attempt in [`crate::AttemptRecord`]. The executor decides from
/// [`ExecutionError::is_retryable`], [`ExecutionError::effect`] and the step's [`crate::Replay`]; this is the summary.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ReplayClass {
    /// The step never ran its effect: circuit open, or it returned [`ExecutionError::Cancelled`].
    NotExecuted,
    /// Not applied and worth retrying: HTTP 429 or 503, [`ExecutionError::rejected`]. Retried for every step.
    Rejected,
    /// The effect may have landed: a timeout, HTTP 5xx other than 503, an interruption, a panic,
    /// [`ExecutionError::failed`] or [`ExecutionError::transient`]. Retried only for an idempotent or prechecked step,
    /// and only if retryable. Records saved before 0.5 call some of these `definitive`, which reads as this.
    #[cfg_attr(feature = "serde", serde(alias = "definitive"))]
    Ambiguous,
    /// Not applied and not worth retrying: HTTP 4xx other than 429, [`ExecutionError::refused`]. Not retried; resume
    /// runs the step again.
    Refused,
}

impl ExecutionError {
    /// Something went wrong and the step can't vouch for its effect: not retryable, [`Effect::Unknown`]. The default
    /// for a failure. A step that is not idempotent ends `OutcomeUnknown` and resume holds it until verified.
    #[must_use]
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed { message: message.into(), retryable: false, effect: Effect::Unknown }
    }

    /// The step rejected the work before doing anything (validation, a failed precondition): not retryable,
    /// [`Effect::NotApplied`]. The step ends `Failed` and resume runs it again.
    #[must_use]
    pub fn refused(message: impl Into<String>) -> Self {
        Self::Failed { message: message.into(), retryable: false, effect: Effect::NotApplied }
    }

    /// A dependency turned the request away and asking again later may work: retryable, [`Effect::NotApplied`].
    /// Retried for every step.
    #[must_use]
    pub fn rejected(message: impl Into<String>) -> Self {
        Self::Failed { message: message.into(), retryable: true, effect: Effect::NotApplied }
    }

    /// A transport-level hiccup mid-request: retryable, [`Effect::Unknown`]. Only idempotent or prechecked steps
    /// retry it.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self::Failed { message: message.into(), retryable: true, effect: Effect::Unknown }
    }

    /// Whether trying the failed operation again could help. Says nothing about whether it took effect; see
    /// [`ExecutionError::effect`].
    ///
    /// HTTP 429/502/503/504, `Timeout`, `Interrupted`, `CircuitOpen` and `Failed { retryable: true }` report
    /// `true`. [`ExecutionError::AmbiguousOutcome`] reports `false`: an error in a [`crate::StepRecord`] already
    /// reflects the step's [`crate::Replay`], so an ambiguous outcome of a non-idempotent step is never retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http { status, .. } => michi_core::resilience::is_retryable_status(*status),
            Self::Timeout { .. } | Self::Interrupted { .. } | Self::CircuitOpen { .. } => true,
            Self::Failed { retryable, .. } => *retryable,
            Self::StepFailed { source, .. } => source.is_retryable(),
            Self::Cancelled | Self::AmbiguousOutcome { .. } | Self::Panicked { .. } | Self::DeadlinePassed { .. } => {
                false
            }
        }
    }

    /// Whether the failed attempt's effect may have landed, looking through [`ExecutionError::StepFailed`].
    ///
    /// [`Effect::NotApplied`] only when the error says so: `CircuitOpen`, `Cancelled`, HTTP 4xx (the server refused
    /// the request; 429 says come back later) and HTTP 503, and `Failed` with `effect: NotApplied`. Everything else
    /// is [`Effect::Unknown`], including HTTP 500 and other 5xx, which say nothing about whether the request was
    /// applied (RFC 9110 §15.6).
    #[must_use]
    pub fn effect(&self) -> Effect {
        match self {
            Self::CircuitOpen { .. } | Self::Cancelled | Self::DeadlinePassed { .. } => Effect::NotApplied,
            Self::Http { status, .. } if (400..500).contains(status) || *status == 503 => Effect::NotApplied,
            Self::Failed { effect, .. } => *effect,
            Self::StepFailed { source, .. } => source.effect(),
            Self::Http { .. }
            | Self::Timeout { .. }
            | Self::Interrupted { .. }
            | Self::AmbiguousOutcome { .. }
            | Self::Panicked { .. } => Effect::Unknown,
        }
    }

    /// This error's replay class, looking through [`ExecutionError::StepFailed`]: [`ReplayClass::NotExecuted`] for a
    /// step that never ran its effect, otherwise derived from [`ExecutionError::effect`] and
    /// [`ExecutionError::is_retryable`].
    #[must_use]
    pub fn replay_class(&self) -> ReplayClass {
        match self.innermost_step_error() {
            Self::CircuitOpen { .. } | Self::Cancelled => ReplayClass::NotExecuted,
            err => match (err.effect(), err.is_retryable()) {
                (Effect::NotApplied, true) => ReplayClass::Rejected,
                (Effect::NotApplied, false) => ReplayClass::Refused,
                (Effect::Unknown, _) => ReplayClass::Ambiguous,
            },
        }
    }

    /// The error inside any [`ExecutionError::StepFailed`] wrappers.
    fn innermost_step_error(&self) -> &Self {
        match self {
            Self::StepFailed { source, .. } => source.innermost_step_error(),
            other => other,
        }
    }

    /// The innermost error, looking through `StepFailed` and `AmbiguousOutcome`.
    pub(crate) fn innermost(&self) -> &Self {
        match self {
            Self::StepFailed { source, .. } | Self::AmbiguousOutcome { source } => source.innermost(),
            other => other,
        }
    }

    /// The caller-supplied retry hint, looking through [`ExecutionError::StepFailed`].
    pub(crate) fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            Self::CircuitOpen { retry_after_ms } => Some(Duration::from_millis(*retry_after_ms)),
            Self::StepFailed { source, .. } => source.retry_after(),
            Self::Timeout { .. }
            | Self::Failed { .. }
            | Self::Cancelled
            | Self::Interrupted { .. }
            | Self::AmbiguousOutcome { .. }
            | Self::Panicked { .. }
            | Self::DeadlinePassed { .. } => None,
        }
    }
}

/// Milliseconds in `d`, saturating.
pub(crate) fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl From<ExecutionError> for michi_core::DomainError {
    fn from(err: ExecutionError) -> Self {
        use michi_core::{DomainError, ErrorCode};
        match err {
            ExecutionError::Http { status, message, retry_after } => {
                let code = match status {
                    429 => ErrorCode::RateLimited,
                    502..=504 => ErrorCode::Unavailable,
                    _ => ErrorCode::ExternalFailure,
                };
                let retryable = michi_core::resilience::is_retryable_status(status);
                let mut domain_err = DomainError::new(code, message).retryable(retryable);
                if let Some(retry_after) = retry_after {
                    domain_err = domain_err.retry_after(retry_after);
                }
                domain_err
            }
            ExecutionError::Timeout { elapsed_ms } => {
                DomainError::new(ErrorCode::Timeout, format!("step timed out after {elapsed_ms}ms")).retryable(true)
            }
            ExecutionError::Failed { message, retryable, .. } => {
                DomainError::new(ErrorCode::ExternalFailure, message).retryable(retryable)
            }
            ExecutionError::CircuitOpen { retry_after_ms } => {
                DomainError::new(ErrorCode::Unavailable, format!("circuit open, retry after {retry_after_ms}ms"))
                    .retryable(true)
                    .retry_after(Duration::from_millis(retry_after_ms))
            }
            ExecutionError::StepFailed { step_id, step_name, source } => {
                // Keeps the inner code, retryable flag, retry_after, hints and recovery; only the message gains the
                // step prefix.
                let mut inner: DomainError = (*source).into();
                inner.message = format!("step {step_id} ({step_name}) failed: {}", inner.message);
                inner
            }
            ExecutionError::AmbiguousOutcome { source } => {
                let mut inner: DomainError = (*source).into();
                inner.message = format!("outcome unknown, the operation may have completed: {}", inner.message);
                inner.retryable = false;
                inner.retry_after = None;
                inner.hint(VERIFY_BEFORE_RETRY_HINT)
            }
            ExecutionError::Cancelled => {
                DomainError::new(ErrorCode::Unavailable, "step cancelled before it took effect").retryable(false)
            }
            ExecutionError::Interrupted { elapsed_ms } => DomainError::new(
                ErrorCode::Unavailable,
                format!("attempt interrupted by cancellation after {elapsed_ms}ms"),
            )
            .retryable(true),
            ExecutionError::Panicked { message } => {
                DomainError::new(ErrorCode::ExternalFailure, format!("step panicked: {message}"))
                    .retryable(false)
                    .hint(PANIC_HINT)
            }
            ExecutionError::DeadlinePassed { message } => {
                DomainError::new(ErrorCode::Timeout, format!("deadline passed: {message}"))
                    .retryable(false)
                    .hint(DEADLINE_HINT)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16) -> ExecutionError {
        ExecutionError::Http { status, message: "x".into(), retry_after: None }
    }

    #[test]
    fn replay_class_covers_each_outcome() {
        assert_eq!(ExecutionError::CircuitOpen { retry_after_ms: 1 }.replay_class(), ReplayClass::NotExecuted);
        assert_eq!(ExecutionError::Cancelled.replay_class(), ReplayClass::NotExecuted);
        assert_eq!(http(429).replay_class(), ReplayClass::Rejected);
        assert_eq!(http(503).replay_class(), ReplayClass::Rejected);
        assert_eq!(ExecutionError::rejected("x").replay_class(), ReplayClass::Rejected);
        assert_eq!(http(502).replay_class(), ReplayClass::Ambiguous);
        assert_eq!(http(504).replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Timeout { elapsed_ms: 1 }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Interrupted { elapsed_ms: 1 }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Panicked { message: "x".into() }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(http(500).replay_class(), ReplayClass::Ambiguous, "a 500 says nothing about the effect");
        assert_eq!(ExecutionError::failed("x").replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::transient("x").replay_class(), ReplayClass::Ambiguous);
        assert_eq!(http(404).replay_class(), ReplayClass::Refused);
        assert_eq!(ExecutionError::refused("x").replay_class(), ReplayClass::Refused);
        let wrapped =
            ExecutionError::StepFailed { step_id: "a".into(), step_name: "A".into(), source: Box::new(http(504)) };
        assert_eq!(wrapped.replay_class(), ReplayClass::Ambiguous, "StepFailed classifies by its source");
        let wrapped_open = ExecutionError::StepFailed {
            step_id: "a".into(),
            step_name: "A".into(),
            source: Box::new(ExecutionError::CircuitOpen { retry_after_ms: 1 }),
        };
        assert_eq!(wrapped_open.replay_class(), ReplayClass::NotExecuted);
    }

    #[test]
    fn effect_follows_the_error_table() {
        use Effect::{NotApplied, Unknown};
        let cases: Vec<(ExecutionError, bool, Effect)> = vec![
            (ExecutionError::CircuitOpen { retry_after_ms: 1 }, true, NotApplied),
            (ExecutionError::Cancelled, false, NotApplied),
            (http(429), true, NotApplied),
            (http(503), true, NotApplied),
            (http(400), false, NotApplied),
            (http(404), false, NotApplied),
            (http(408), false, NotApplied),
            (http(409), false, NotApplied),
            (http(502), true, Unknown),
            (http(504), true, Unknown),
            (http(500), false, Unknown),
            (http(501), false, Unknown),
            (http(599), false, Unknown),
            (http(302), false, Unknown),
            (ExecutionError::Timeout { elapsed_ms: 1 }, true, Unknown),
            (ExecutionError::Interrupted { elapsed_ms: 1 }, true, Unknown),
            (ExecutionError::Panicked { message: "x".into() }, false, Unknown),
            (ExecutionError::AmbiguousOutcome { source: Box::new(http(429)) }, false, Unknown),
            (ExecutionError::failed("x"), false, Unknown),
            (ExecutionError::refused("x"), false, NotApplied),
            (ExecutionError::rejected("x"), true, NotApplied),
            (ExecutionError::transient("x"), true, Unknown),
            (ExecutionError::DeadlinePassed { message: "x".into() }, false, NotApplied),
            (step_failed(ExecutionError::refused("x")), false, NotApplied),
            (step_failed(http(500)), false, Unknown),
        ];
        for (err, retryable, effect) in cases {
            assert_eq!((err.is_retryable(), err.effect()), (retryable, effect), "{err:?}");
        }
    }

    #[test]
    fn constructors_set_message_retryable_and_effect() {
        assert_eq!(
            ExecutionError::failed("m"),
            ExecutionError::Failed { message: "m".into(), retryable: false, effect: Effect::Unknown }
        );
        assert_eq!(
            ExecutionError::refused(String::from("m")),
            ExecutionError::Failed { message: "m".into(), retryable: false, effect: Effect::NotApplied }
        );
        assert_eq!(
            ExecutionError::rejected("m"),
            ExecutionError::Failed { message: "m".into(), retryable: true, effect: Effect::NotApplied }
        );
        assert_eq!(
            ExecutionError::transient("m"),
            ExecutionError::Failed { message: "m".into(), retryable: true, effect: Effect::Unknown }
        );
        assert_eq!(Effect::default(), Effect::Unknown, "an unclassified failure is never treated as safe");
    }

    #[test]
    fn is_retryable_per_variant() {
        assert!(http(429).is_retryable() && http(502).is_retryable() && http(503).is_retryable());
        assert!(!http(500).is_retryable() && !http(404).is_retryable());
        assert!(ExecutionError::Timeout { elapsed_ms: 1 }.is_retryable());
        assert!(ExecutionError::Interrupted { elapsed_ms: 1 }.is_retryable());
        assert!(ExecutionError::CircuitOpen { retry_after_ms: 1 }.is_retryable());
        assert!(!ExecutionError::Cancelled.is_retryable());
        assert!(!ExecutionError::Panicked { message: "x".into() }.is_retryable());
        assert!(!ExecutionError::AmbiguousOutcome { source: Box::new(http(504)) }.is_retryable());
    }

    #[test]
    fn http_conversion_maps_status_groups_and_keeps_retry_after() {
        let d: michi_core::DomainError =
            ExecutionError::Http { status: 429, message: "slow".into(), retry_after: Some(Duration::from_secs(2)) }
                .into();
        assert_eq!(d.code, michi_core::ErrorCode::RateLimited);
        assert!(d.retryable);
        assert_eq!(d.retry_after, Some(Duration::from_secs(2)));
        let d: michi_core::DomainError = http(504).into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        let d: michi_core::DomainError = http(418).into();
        assert_eq!(d.code, michi_core::ErrorCode::ExternalFailure);
        assert!(!d.retryable);
    }

    #[test]
    fn step_failed_conversion_keeps_the_inner_code_retry_after_and_hints() {
        let inner = ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::Timeout { elapsed_ms: 5 }) };
        let d: michi_core::DomainError =
            ExecutionError::StepFailed { step_id: "a".into(), step_name: "Alpha".into(), source: Box::new(inner) }
                .into();
        assert_eq!(d.code, michi_core::ErrorCode::Timeout);
        assert!(d.message.starts_with("step a (Alpha) failed: outcome unknown"), "{}", d.message);
        assert!(!d.retryable);
        assert_eq!(d.retry_after, None);
        assert_eq!(d.hints.len(), 1, "the verify hint survives the StepFailed wrapper");
    }

    #[test]
    fn ambiguous_outcome_conversion_keeps_code_and_drops_retry_guidance() {
        let d: michi_core::DomainError = ExecutionError::AmbiguousOutcome {
            source: Box::new(ExecutionError::Http {
                status: 504,
                message: "gw".into(),
                retry_after: Some(Duration::from_secs(3)),
            }),
        }
        .into();
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
        assert!(!d.retryable);
        assert_eq!(d.retry_after, None);
        assert!(d.hints.iter().any(|h| h.0.contains("verify whether the operation completed")));
    }

    fn step_failed(source: ExecutionError) -> ExecutionError {
        ExecutionError::StepFailed { step_id: "upload".into(), step_name: "Upload".into(), source: Box::new(source) }
    }

    #[test]
    fn is_retryable_covers_failed_flags_504_and_looks_through_step_failed() {
        assert!(ExecutionError::rejected("x").is_retryable());
        assert!(!ExecutionError::failed("x").is_retryable());
        assert!(http(504).is_retryable());
        assert!(step_failed(ExecutionError::Timeout { elapsed_ms: 1 }).is_retryable(), "StepFailed asks its source");
        assert!(!step_failed(ExecutionError::failed("x")).is_retryable());
        assert!(
            !step_failed(ExecutionError::AmbiguousOutcome { source: Box::new(http(504)) }).is_retryable(),
            "a wrapped ambiguous outcome stays non-retryable, so the caller never replays the run"
        );
    }

    #[test]
    fn every_variant_converts_to_its_domain_code_retryability_and_retry_after() {
        use michi_core::ErrorCode::{ExternalFailure, RateLimited, Timeout, Unavailable};
        let ms = Duration::from_millis;
        let cases: Vec<(ExecutionError, michi_core::ErrorCode, bool, Option<Duration>)> = vec![
            (http(429), RateLimited, true, None),
            (http(502), Unavailable, true, None),
            (http(503), Unavailable, true, None),
            (
                ExecutionError::Http { status: 504, message: "x".into(), retry_after: Some(ms(3)) },
                Unavailable,
                true,
                Some(ms(3)),
            ),
            (http(400), ExternalFailure, false, None),
            (ExecutionError::Timeout { elapsed_ms: 250 }, Timeout, true, None),
            (ExecutionError::rejected("x"), ExternalFailure, true, None),
            (ExecutionError::failed("x"), ExternalFailure, false, None),
            (ExecutionError::CircuitOpen { retry_after_ms: 750 }, Unavailable, true, Some(ms(750))),
            (ExecutionError::Cancelled, Unavailable, false, None),
            (ExecutionError::Interrupted { elapsed_ms: 7 }, Unavailable, true, None),
            (ExecutionError::Panicked { message: "boom".into() }, ExternalFailure, false, None),
            (ExecutionError::DeadlinePassed { message: "window closed".into() }, Timeout, false, None),
            (
                ExecutionError::AmbiguousOutcome { source: Box::new(ExecutionError::Timeout { elapsed_ms: 1 }) },
                Timeout,
                false,
                None,
            ),
            (step_failed(ExecutionError::CircuitOpen { retry_after_ms: 500 }), Unavailable, true, Some(ms(500))),
            (step_failed(http(429)), RateLimited, true, None),
        ];
        for (err, code, retryable, retry_after) in cases {
            let label = err.to_string();
            let d: michi_core::DomainError = err.into();
            assert_eq!((d.code, d.retryable, d.retry_after), (code, retryable, retry_after), "{label}");
        }
        let d: michi_core::DomainError = ExecutionError::Cancelled.into();
        assert_eq!(d.message, "step cancelled before it took effect");
        let d: michi_core::DomainError = ExecutionError::Panicked { message: "boom".into() }.into();
        assert_eq!(d.hints.len(), 1, "a panic tells the agent to fix the step and verify");
        let d: michi_core::DomainError = step_failed(ExecutionError::failed("disk full")).into();
        assert_eq!(d.message, "step upload (Upload) failed: disk full");
    }

    #[test]
    fn ambiguous_outcome_has_the_exact_message_one_verify_hint_and_the_ambiguous_class() {
        let err = ExecutionError::AmbiguousOutcome {
            source: Box::new(ExecutionError::Http {
                status: 504,
                message: "gateway timeout".into(),
                retry_after: Some(Duration::from_secs(3)),
            }),
        };
        assert_eq!(err.replay_class(), ReplayClass::Ambiguous);
        let d: michi_core::DomainError = err.into();
        assert_eq!(d.message, "outcome unknown, the operation may have completed: gateway timeout");
        assert_eq!(d.hints.len(), 1);
        assert_eq!(d.hints[0].0, VERIFY_BEFORE_RETRY_HINT);
    }

    #[test]
    fn execution_error_is_a_thread_safe_std_error() {
        fn assert_error<E: std::error::Error + Send + Sync + 'static>() {}
        assert_error::<ExecutionError>();
    }

    #[test]
    fn panicked_and_interrupted_conversions() {
        let d: michi_core::DomainError = ExecutionError::Panicked { message: "boom".into() }.into();
        assert!(!d.retryable);
        assert!(d.message.contains("boom"));
        let d: michi_core::DomainError = ExecutionError::Interrupted { elapsed_ms: 7 }.into();
        assert!(d.retryable);
        assert_eq!(d.code, michi_core::ErrorCode::Unavailable);
    }
}

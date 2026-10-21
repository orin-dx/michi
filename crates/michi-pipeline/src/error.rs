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
    /// Generic step failure not tied to HTTP transport.
    #[error("{message}")]
    Failed {
        /// Human-readable failure message.
        message: String,
        /// Whether the step vouches that retrying is safe and worthwhile (the attempt had no effect, or repeating
        /// it is harmless).
        retryable: bool,
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
    /// A non-idempotent step's last attempt ended ambiguously (timeout, HTTP 502/504, interruption), so the
    /// operation may or may not have completed. Never retryable: verify whether it completed first.
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
}

/// Hint attached to the [`michi_core::DomainError`] converted from an [`ExecutionError::AmbiguousOutcome`].
const VERIFY_BEFORE_RETRY_HINT: &str = "verify whether the operation completed before retrying; \
     the step is not idempotent, so a blind retry could repeat its side effect";

/// Hint attached to the [`michi_core::DomainError`] converted from an [`ExecutionError::Panicked`].
const PANIC_HINT: &str = "the step panicked, which is a bug in the step; fix it and verify whether it took effect \
     before resuming";

/// How a failed attempt's outcome bears on replaying it (RFC 9110 §9.2.2). One classification decides whether the
/// executor retries, which terminal status the step ends in, and whether the error says replay is safe.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ReplayClass {
    /// The step never ran its effect: circuit open, or it returned [`ExecutionError::Cancelled`]. Safe to run again.
    NotExecuted,
    /// The request was not processed: HTTP 429 or 503, or `Failed { retryable: true }`. Retried for every step.
    Rejected,
    /// The attempt may have committed: a timeout, HTTP 502 or 504, an interruption or a panic. Retried only for an
    /// idempotent step.
    Ambiguous,
    /// Anything else: retrying would not change the result.
    Definitive,
}

impl ExecutionError {
    /// Whether replaying the failed operation is both worthwhile and safe.
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
            Self::Cancelled | Self::AmbiguousOutcome { .. } | Self::Panicked { .. } => false,
        }
    }

    /// This error's replay class, looking through [`ExecutionError::StepFailed`].
    #[must_use]
    pub fn replay_class(&self) -> ReplayClass {
        match self {
            Self::CircuitOpen { .. } | Self::Cancelled => ReplayClass::NotExecuted,
            Self::Http { status: 429 | 503, .. } | Self::Failed { retryable: true, .. } => ReplayClass::Rejected,
            Self::Http { status: 502 | 504, .. }
            | Self::Timeout { .. }
            | Self::Interrupted { .. }
            | Self::AmbiguousOutcome { .. }
            | Self::Panicked { .. } => ReplayClass::Ambiguous,
            Self::StepFailed { source, .. } => source.replay_class(),
            Self::Http { .. } | Self::Failed { .. } => ReplayClass::Definitive,
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
            | Self::Panicked { .. } => None,
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
            ExecutionError::Failed { message, retryable } => {
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
        assert_eq!(
            ExecutionError::Failed { message: "x".into(), retryable: true }.replay_class(),
            ReplayClass::Rejected
        );
        assert_eq!(http(502).replay_class(), ReplayClass::Ambiguous);
        assert_eq!(http(504).replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Timeout { elapsed_ms: 1 }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Interrupted { elapsed_ms: 1 }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(ExecutionError::Panicked { message: "x".into() }.replay_class(), ReplayClass::Ambiguous);
        assert_eq!(http(500).replay_class(), ReplayClass::Definitive);
        assert_eq!(http(404).replay_class(), ReplayClass::Definitive);
        assert_eq!(
            ExecutionError::Failed { message: "x".into(), retryable: false }.replay_class(),
            ReplayClass::Definitive
        );
        let wrapped =
            ExecutionError::StepFailed { step_id: "a".into(), step_name: "A".into(), source: Box::new(http(504)) };
        assert_eq!(wrapped.replay_class(), ReplayClass::Ambiguous, "StepFailed classifies by its source");
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
        assert!(ExecutionError::Failed { message: "x".into(), retryable: true }.is_retryable());
        assert!(!ExecutionError::Failed { message: "x".into(), retryable: false }.is_retryable());
        assert!(http(504).is_retryable());
        assert!(step_failed(ExecutionError::Timeout { elapsed_ms: 1 }).is_retryable(), "StepFailed asks its source");
        assert!(!step_failed(ExecutionError::Failed { message: "x".into(), retryable: false }).is_retryable());
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
            (ExecutionError::Failed { message: "x".into(), retryable: true }, ExternalFailure, true, None),
            (ExecutionError::Failed { message: "x".into(), retryable: false }, ExternalFailure, false, None),
            (ExecutionError::CircuitOpen { retry_after_ms: 750 }, Unavailable, true, Some(ms(750))),
            (ExecutionError::Cancelled, Unavailable, false, None),
            (ExecutionError::Interrupted { elapsed_ms: 7 }, Unavailable, true, None),
            (ExecutionError::Panicked { message: "boom".into() }, ExternalFailure, false, None),
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
        let d: michi_core::DomainError =
            step_failed(ExecutionError::Failed { message: "disk full".into(), retryable: false }).into();
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

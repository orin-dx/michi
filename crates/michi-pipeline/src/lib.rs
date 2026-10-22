//! # michi-pipeline
//!
//! Runs a validated graph of async steps on Tokio and returns a [`RunRecord`] saying what happened to each one.
//!
//! 1. Define steps with [`ExecStep`] (a [`StepId`], a display name, prerequisites, a [`Step`] runner) and validate
//!    them into a [`Plan`]. Validation reports every problem at once; [`Plan::execution_order`] is the dry run.
//! 2. [`execute`] the plan under a caller-supplied [`RunId`] with [`ExecOptions`]: sequential or parallel with a
//!    concurrency cap, skip-dependents or fail-fast, retry policy and budget, timeouts, keyed circuit breakers,
//!    cancellation and a progress observer.
//! 3. Read the record: every step has one status from [`michi_core::pipeline::StepStatus`] with a reason, and the
//!    run has a [`RunOutcome`] that is `Succeeded` only if every step succeeded.
//! 4. [`resume`] an interrupted run from its record under the same run id: finished steps are not re-run, and steps
//!    whose outcome is unknown are re-run only if idempotent or verified by the caller.
//!
//! The replay rule (see [`Step`]) keeps retries and resume from blindly repeating work that may already have taken
//! effect. Resume is at least once per step; pass [`StepContext::idempotency_key`] to a deduplicating downstream for
//! exactly-once effects. See `docs/spec/08-pipeline-execution.md`.
//!
//! ```
//! use michi_pipeline::{execute, step_fn, ExecOptions, ExecStep, Plan, RunId, RunOutcome, StepId};
//!
//! # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(async {
//! let fetch = StepId::try_new("fetch").unwrap();
//! let plan = Plan::new(vec![
//!     ExecStep::new(fetch.clone(), "Fetch data", step_fn(|_ctx| async { Ok(()) })),
//!     ExecStep::new(StepId::try_new("upload").unwrap(), "Upload", step_fn(|ctx| async move {
//!         let _key = ctx.idempotency_key(); // "run-1:upload" on every attempt and on resume
//!         Ok(())
//!     }))
//!     .depends_on(fetch),
//! ])
//! .unwrap();
//! let record = execute(&plan, RunId::try_new("run-1").unwrap(), &ExecOptions::new()).await;
//! assert_eq!(record.outcome(), RunOutcome::Succeeded);
//! assert_eq!(
//!     record.render(),
//!     "step[2]{id,name,status,reason}:\n  fetch,Fetch data,completed,null\n  upload,Upload,completed,null"
//! );
//! # });
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]
#![warn(clippy::must_use_candidate, clippy::missing_errors_doc, clippy::missing_panics_doc)]
#![warn(missing_debug_implementations)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod breaker;
mod cancel;
mod error;
mod exec;
mod id;
mod plan;
mod record;
mod scheduler;
mod step;

pub use breaker::{BreakerConfig, BreakerPhase, BreakerRegistry, CircuitBreaker};
pub use cancel::CancellationToken;
pub use error::{ExecutionError, ReplayClass};
pub use exec::{
    execute, resume, ExecOptions, FailurePolicy, Mode, ResumeError, ResumeOptions, StepObserver, Verification,
};
pub use id::{IdError, RunId, StepId, MAX_ID_LEN};
pub use plan::{ExecStep, ParseFingerprintError, Plan, PlanError, PlanFingerprint, PlanViolation};
pub use record::{AttemptRecord, RunOutcome, RunRecord, StepRecord};
pub use step::{step_fn, FnStep, Replay, Step, StepContext, StepFuture};

/// The michi-core status types a [`RunRecord`] reports, re-exported for convenience.
pub use michi_core::pipeline::{NotRunReason, SkipReason, StepStatus, UnknownCause};

/// The michi-core retry types [`ExecOptions`] and [`StepContext`] take and return, re-exported so configuring retries
/// doesn't need a direct michi-core dependency.
pub use michi_core::resilience::{IdempotencyKey, RetryBudget, RetryConfig, RetryConfigError};

// Runs the README example as a doctest without making it the rendered crate docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

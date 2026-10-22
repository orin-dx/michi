//! The `Step` trait, its context and the closure adapter.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::{CancellationToken, ExecutionError, RunId, StepId};

/// Whether running a step more than once has the same effect as running it once.
///
/// Decides what happens after an ambiguous attempt (timeout, HTTP 502/504, interruption): an idempotent step is
/// retried and, on resume, re-run; a non-idempotent step ends [`michi_core::pipeline::StepStatus::OutcomeUnknown`]
/// and is never replayed automatically.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Replay {
    /// Repeating the step could repeat its side effect. The default: a step must opt in to replay.
    #[default]
    NotIdempotent,
    /// Repeating the step is harmless.
    Idempotent,
}

/// What a step attempt gets from the executor.
#[derive(Debug, Clone)]
pub struct StepContext {
    run_id: RunId,
    step_id: StepId,
    attempt: u32,
    cancellation: CancellationToken,
    deadline: Option<tokio::time::Instant>,
}

impl StepContext {
    /// A context for attempt 0 with a fresh token and no deadline, for exercising a [`Step`] on its own (in tests,
    /// say). The executor builds its own.
    #[must_use]
    pub fn new(run_id: RunId, step_id: StepId) -> Self {
        Self { run_id, step_id, attempt: 0, cancellation: CancellationToken::new(), deadline: None }
    }

    pub(crate) fn for_attempt(
        run_id: RunId,
        step_id: StepId,
        attempt: u32,
        cancellation: CancellationToken,
        deadline: Option<tokio::time::Instant>,
    ) -> Self {
        Self { run_id, step_id, attempt, cancellation, deadline }
    }

    /// The run this attempt belongs to.
    #[must_use]
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// The step being run.
    #[must_use]
    pub fn step_id(&self) -> &StepId {
        &self.step_id
    }

    /// 0-based attempt number within this run of the step. For logging and backoff only; never part of the
    /// idempotency key.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// This step's cancellation token, a child of the run's. Watch it to stop cooperatively; once it fires, the
    /// executor drops the attempt after the cancellation grace period.
    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// When the step's total deadline (across all attempts) expires, if one is set.
    #[must_use]
    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    /// Time left until [`StepContext::deadline`], if one is set.
    #[must_use]
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(tokio::time::Instant::now()))
    }

    /// `run_id:step_id`: the same on every attempt and on resume, unique across runs. Pass it to a downstream that
    /// deduplicates to make the step's effect apply once.
    #[must_use]
    pub fn idempotency_key(&self) -> michi_core::resilience::IdempotencyKey {
        let mut key = String::with_capacity(self.run_id.as_str().len() + 1 + self.step_id.as_str().len());
        key.push_str(self.run_id.as_str());
        key.push(':');
        key.push_str(self.step_id.as_str());
        michi_core::resilience::IdempotencyKey::new(key)
    }
}

/// The boxed future a [`Step`] attempt returns.
pub type StepFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ExecutionError>> + Send + 'a>>;

/// What a plan step runs. Object-safe, so steps of different types share one plan without an async-trait
/// dependency.
///
/// # Replay safety
///
/// The executor may run a step more than once: on retry within a run, and on resume. Every failed attempt is
/// classified by whether it could have taken effect ([`crate::ReplayClass`]):
///
/// - **Not executed**: circuit open, or the step returned [`ExecutionError::Cancelled`]. Not retried; safe to run on
///   resume.
/// - **Rejected**: HTTP 429 or 503, or `Failed { retryable: true }`. Retried for every step.
/// - **Ambiguous**: a timeout, HTTP 502 or 504, or an interruption. Retried only if [`Step::replay`] is
///   [`Replay::Idempotent`]; otherwise the step ends `OutcomeUnknown`.
/// - **Definitive**: everything else. Not retried.
///
/// A step classifies its own outcome by the error it returns, so it must classify honestly: a step that may have
/// committed its effect must not report 429, 503, `Failed { retryable: true }` or `Cancelled`. It reports 502, 504,
/// a timeout or a non-retryable error instead.
pub trait Step: Send + Sync {
    /// Runs one attempt.
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a>;

    /// Whether the step may be replayed after an ambiguous outcome. Defaults to [`Replay::NotIdempotent`].
    fn replay(&self) -> Replay {
        Replay::NotIdempotent
    }
}

/// [`Step`] over an async closure, built by [`step_fn`].
pub struct FnStep<F> {
    f: F,
    replay: Replay,
}

// Manual so `F` (usually a closure) need not be `Debug`.
impl<F> std::fmt::Debug for FnStep<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnStep").field("replay", &self.replay).finish_non_exhaustive()
    }
}

impl<F> FnStep<F> {
    /// Declares the step idempotent ([`Replay::Idempotent`]).
    #[must_use]
    pub fn idempotent(mut self) -> Self {
        self.replay = Replay::Idempotent;
        self
    }
}

/// Adapts an async closure into a [`Step`]. The closure receives an owned clone of the attempt's [`StepContext`].
/// The step is [`Replay::NotIdempotent`] unless [`FnStep::idempotent`] is called.
pub fn step_fn<F, Fut>(f: F) -> FnStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    FnStep { f, replay: Replay::NotIdempotent }
}

impl<F, Fut> Step for FnStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), ExecutionError>> + Send + 'static,
{
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin((self.f)(ctx.clone()))
    }

    fn replay(&self) -> Replay {
        self.replay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> StepContext {
        StepContext::new(RunId::try_new("run-7").unwrap(), StepId::try_new("upload").unwrap())
    }

    #[test]
    fn idempotency_key_is_run_colon_step_and_ignores_the_attempt() {
        let first = ctx();
        let retry = StepContext::for_attempt(
            first.run_id().clone(),
            first.step_id().clone(),
            3,
            CancellationToken::new(),
            None,
        );
        assert_eq!(first.idempotency_key().as_str(), "run-7:upload");
        assert_eq!(retry.idempotency_key(), first.idempotency_key());
    }

    #[tokio::test(start_paused = true)]
    async fn step_fn_adapts_a_closure_and_defaults_to_not_idempotent() {
        let step = step_fn(|ctx: StepContext| async move {
            assert_eq!(ctx.step_id().as_str(), "upload");
            Ok(())
        });
        assert_eq!(step.replay(), Replay::NotIdempotent);
        assert_eq!(step.run(&ctx()).await, Ok(()));
        assert_eq!(step_fn(|_| async { Ok(()) }).idempotent().replay(), Replay::Idempotent);
    }

    #[test]
    fn step_is_object_safe() {
        let steps: Vec<Box<dyn Step>> = vec![Box::new(step_fn(|_| async { Ok(()) }))];
        assert_eq!(steps.len(), 1);
    }
}

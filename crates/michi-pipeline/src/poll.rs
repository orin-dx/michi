//! The poll step: one attempt that checks on a fixed schedule until something is ready.

use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::error::millis;
use crate::{AttemptBound, ExecutionError, Replay, Step, StepContext, StepFuture};

/// When a [`poll_step`] checks: at once, then after each delay in turn, then at a fixed interval. Each delay counts
/// from the end of the previous check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollSchedule {
    delays: Vec<Duration>,
    then_every: Duration,
}

impl Default for PollSchedule {
    /// 2 s, 4 s, 8 s and 16 s, then every 30 s: quick for something that lands fast, gentle on a slow registry.
    fn default() -> Self {
        Self::new([2, 4, 8, 16].map(Duration::from_secs), Duration::from_secs(30))
    }
}

impl PollSchedule {
    /// Waits `delays` in turn between the first checks, then `then_every` between every later one. A zero delay is
    /// floored to 1 ms so a poll can never spin.
    #[must_use]
    pub fn new(delays: impl IntoIterator<Item = Duration>, then_every: Duration) -> Self {
        Self { delays: delays.into_iter().map(floor).collect(), then_every: floor(then_every) }
    }

    /// The same delay between every check. Zero is floored to 1 ms.
    #[must_use]
    pub fn every(interval: Duration) -> Self {
        Self::new([], interval)
    }

    /// The wait after check `n` (0-based) before the next one.
    #[must_use]
    pub fn delay_after(&self, n: usize) -> Duration {
        self.delays.get(n).copied().unwrap_or(self.then_every)
    }
}

fn floor(d: Duration) -> Duration {
    d.max(Duration::from_millis(1))
}

/// What one check of a [`poll_step`] found.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// The awaited state is there: the step succeeds.
    Ready,
    /// Not yet, with what the check saw (`"registry serves 1.2.2, want 1.2.3"`, say). The last one goes into the error
    /// if the deadline comes first.
    NotYet(String),
}

/// [`Step`] that polls a check, built by [`poll_step`].
pub struct PollStep<F> {
    check: F,
    schedule: PollSchedule,
}

// Manual so `F` (usually a closure) need not be `Debug`.
impl<F> std::fmt::Debug for PollStep<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollStep").field("schedule", &self.schedule).finish_non_exhaustive()
    }
}

/// A step that calls `check` on `schedule` until it answers [`Readiness::Ready`] (the step succeeds), returns an
/// error that is not retryable (the step fails with it), or time runs out. For waiting on something another step
/// caused, such as a published version becoming visible in a registry.
///
/// - **One attempt.** The whole poll is a single attempt, so it never draws from the retry budget and the step's
///   retry policy never applies: the poll returns no retryable error.
/// - **Bounded by the deadline, not the attempt timeout.** The step is [`AttemptBound::Deadline`]: give it a deadline
///   ([`crate::ExecStep::deadline`]) or a deadline group. Each check runs under the attempt timeout instead. Without a
///   deadline, the attempt timeout bounds the whole poll.
/// - **Stops early, not late.** When the next check would start at or past the deadline, the poll stops at once with
///   [`ExecutionError::DeadlinePassed`], naming how many checks ran and the last thing observed. That error is not
///   applied: polling only reads. Whether the awaited state arrived later is unknown, so resume polls again.
/// - **Check errors.** A retryable error (a 503, a timed-out check) counts as not ready yet and the poll carries on;
///   any other error ends the step with it.
/// - **Cancellation** stops the poll at once, during a check or a wait, with [`ExecutionError::Cancelled`].
///
/// The step is [`Replay::Idempotent`]: a check must only read.
pub fn poll_step<F, Fut>(schedule: PollSchedule, check: F) -> PollStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Readiness, ExecutionError>> + Send + 'static,
{
    PollStep { check, schedule }
}

impl<F, Fut> Step for PollStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Readiness, ExecutionError>> + Send + 'static,
{
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin(self.poll(ctx))
    }

    fn replay(&self) -> Replay {
        Replay::Idempotent
    }

    fn attempt_bound(&self) -> AttemptBound {
        AttemptBound::Deadline
    }
}

impl<F, Fut> PollStep<F>
where
    F: Fn(StepContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Readiness, ExecutionError>> + Send + 'static,
{
    async fn poll(&self, ctx: &StepContext) -> Result<(), ExecutionError> {
        let started = Instant::now();
        let bound = ctx.attempt_deadline().or(ctx.deadline());
        let token = ctx.cancellation();
        let mut checks = 0usize;
        loop {
            let limit = match bound {
                Some(end) => ctx.attempt_timeout().min(end.saturating_duration_since(Instant::now())),
                None => ctx.attempt_timeout(),
            };
            let answer = tokio::select! {
                biased;
                () = token.cancelled() => return Err(ExecutionError::Cancelled),
                answer = tokio::time::timeout(limit, (self.check)(ctx.clone())) => answer,
            };
            checks = checks.saturating_add(1);
            let last = match answer {
                Ok(Ok(Readiness::Ready)) => return Ok(()),
                Ok(Ok(Readiness::NotYet(seen))) => seen,
                Ok(Err(err)) if err.is_retryable() => format!("the check failed: {err}"),
                Ok(Err(err)) => return Err(err),
                Err(_) => format!("the check timed out after {}ms", millis(limit)),
            };
            let wait = self.schedule.delay_after(checks - 1);
            let stop = match bound {
                Some(end) => Instant::now().checked_add(wait).is_none_or(|wake| wake >= end),
                None => false,
            };
            if stop {
                let mut message = String::with_capacity(96 + last.len());
                message.push_str("not ready after ");
                message.push_str(&checks.to_string());
                message.push_str(if checks == 1 { " check" } else { " checks" });
                message.push_str(" over ");
                message.push_str(&millis(started.elapsed()).to_string());
                message.push_str("ms, and the next check would fall past the deadline; last observed: ");
                message.push_str(&last);
                return Err(ExecutionError::DeadlinePassed { message });
            }
            tokio::select! {
                biased;
                () = token.cancelled() => return Err(ExecutionError::Cancelled),
                () = tokio::time::sleep(wait) => {}
            }
        }
    }
}

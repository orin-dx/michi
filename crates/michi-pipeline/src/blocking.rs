//! The blocking-step adapter: a synchronous closure on Tokio's blocking thread pool.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use crate::{CancellationToken, ExecutionError, Replay, Step, StepContext, StepFuture};

/// Idempotency keys (`run_id:step_id`) whose closure is still running on a blocking thread, for steps that are not
/// idempotent. Process-wide, not per step value: a closure outlives its timeout and its `BlockingStep`, and a resume on
/// a plan rebuilt from a factory has brand-new `BlockingStep`s that must still see it. An entry is removed only when
/// the closure actually returns.
static IN_FLIGHT: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// [`Step`] over a blocking closure, built by [`step_blocking`].
pub struct BlockingStep<F> {
    f: Arc<F>,
    replay: Replay,
}

// Manual so `F` (usually a closure) need not be `Debug`.
impl<F> std::fmt::Debug for BlockingStep<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingStep").field("replay", &self.replay).finish_non_exhaustive()
    }
}

impl<F> BlockingStep<F> {
    /// Declares the step idempotent ([`Replay::Idempotent`]). Its attempts may then overlap: a retry after a timeout
    /// starts a new closure while the timed-out one may still be running.
    #[must_use]
    pub fn idempotent(mut self) -> Self {
        self.replay = Replay::Idempotent;
        self
    }
}

/// Adapts a blocking closure into a [`Step`] that runs each attempt on Tokio's blocking thread pool
/// (`tokio::task::spawn_blocking`), for work that blocks a thread: shelling out to `cargo`, `npm` or `twine`, or a
/// synchronous client. The closure receives an owned [`StepContext`]. The step is [`Replay::NotIdempotent`] unless
/// [`BlockingStep::idempotent`] is called.
///
/// **A blocking closure cannot be stopped from outside.** When an attempt times out, the run is cancelled and the
/// grace period runs out, or the whole run is dropped, the executor stops waiting, but the closure keeps running on
/// its thread until it returns. A child process it started keeps running too. So:
///
/// - The closure's [`StepContext::cancellation`] token fires when the run is cancelled and also when the executor
///   stops waiting for this attempt (a timeout, the grace period ending, the run being dropped). A closure that runs a
///   child process should poll `ctx.cancellation().is_cancelled()` and kill the child when it fires. It can also
///   watch [`StepContext::attempt_deadline`].
/// - An attempt the executor stopped waiting for ends with [`ExecutionError::Timeout`] or
///   [`ExecutionError::Interrupted`], whose effect is unknown: the closure may still apply it.
/// - For a step that is not idempotent, a new attempt never starts while an earlier closure of the same run and step
///   (the same [`StepContext::idempotency_key`]) is still running in this process. Within a run the replay rules
///   already prevent it (an unknown effect is never retried). It can happen on [`crate::resume`] in the same process
///   if the caller marks the step [`crate::Verification::NotApplied`] too early. Such an attempt does not start the
///   closure; it fails with an unknown effect, so the step stays `OutcomeUnknown` until the earlier closure has
///   finished and the caller verifies again.
/// - An idempotent step's attempts may overlap.
///
/// The guard is process-wide, keyed by the idempotency key (run id and step id), and cleared only when the closure
/// actually returns. So a plan rebuilt from a factory, with brand-new `BlockingStep` values, still sees a closure that
/// outlived its timeout. Another process does not see it. Steps that share one runner through
/// [`crate::ExecStep::from_arc`] have different step ids, so they do not block each other.
///
/// Like every step, the attempt runs inside the scheduler's `JoinSet`. Dropping that set (dropping the `execute` or
/// `resume` future) aborts the async side, but Tokio never aborts a `spawn_blocking` task, so the closure finishes on
/// its own and its result is discarded. The runtime waits for blocking threads when it shuts down.
pub fn step_blocking<F>(f: F) -> BlockingStep<F>
where
    F: Fn(StepContext) -> Result<(), ExecutionError> + Send + Sync + 'static,
{
    BlockingStep { f: Arc::new(f), replay: Replay::NotIdempotent }
}

/// Removes a key from the in-flight set when the closure holding it ends, normally or by panicking.
struct InFlight {
    key: String,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.key);
    }
}

/// Cancels the closure's token when the executor stops waiting for the attempt (the future is dropped). Harmless
/// after the closure returned.
struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl<F> Step for BlockingStep<F>
where
    F: Fn(StepContext) -> Result<(), ExecutionError> + Send + Sync + 'static,
{
    fn run<'a>(&'a self, ctx: &'a StepContext) -> StepFuture<'a> {
        Box::pin(async move {
            let guard = if self.replay == Replay::Idempotent {
                None
            } else {
                let key = ctx.idempotency_key().as_str().to_string();
                let mut set = IN_FLIGHT.lock().unwrap_or_else(PoisonError::into_inner);
                if set.contains(&key) {
                    return Err(ExecutionError::failed(
                        "an earlier attempt of this step is still running on its blocking thread and its effect is \
                         unknown, so this attempt was not started; verify once it has finished",
                    ));
                }
                set.insert(key.clone());
                Some(InFlight { key })
            };
            let token = ctx.cancellation().child_token();
            let _cancel_on_drop = CancelOnDrop(token.clone());
            let attempt_ctx = ctx.clone().with_cancellation(token);
            let f = Arc::clone(&self.f);
            let handle = tokio::task::spawn_blocking(move || {
                let _guard = guard;
                f(attempt_ctx)
            });
            match handle.await {
                Ok(result) => result,
                // A panic in the closure is a panic in the step: the scheduler records it as `Panicked`.
                Err(err) if err.is_panic() => std::panic::resume_unwind(err.into_panic()),
                // The runtime is shutting down: the closure was never run or its result is lost.
                Err(_) => Err(ExecutionError::Interrupted { elapsed_ms: 0 }),
            }
        })
    }

    fn replay(&self) -> Replay {
        self.replay
    }
}

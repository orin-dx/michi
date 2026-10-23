//! `step_blocking`: closures on Tokio's blocking pool. These tests use the real clock and a multi-threaded runtime,
//! because a paused clock does not advance while a blocking task runs. Closures block on channels the test controls,
//! so no test depends on a race.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, resume, step_blocking, BlockingStep, CancellationToken, ExecOptions, ExecStep, ExecutionError, Replay,
    ResumeOptions, RunOutcome, SkipReason, Step, StepContext, StepStatus, UnknownCause, Verification,
};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Counts closures running at once and the high-water mark.
#[derive(Clone, Default)]
struct Concurrency {
    now: Arc<AtomicUsize>,
    max: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl Concurrency {
    fn enter(&self) -> usize {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(now, Ordering::SeqCst);
        n
    }
    fn exit(&self) {
        self.now.fetch_sub(1, Ordering::SeqCst);
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

/// A gate the first closure call blocks on until the test opens it, plus a signal when that call ends.
struct Gate {
    open: Mutex<Option<mpsc::Receiver<()>>>,
    done: mpsc::SyncSender<()>,
}

fn gate() -> (Arc<Gate>, mpsc::SyncSender<()>, mpsc::Receiver<()>) {
    let (open_tx, open_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(4);
    (Arc::new(Gate { open: Mutex::new(Some(open_rx)), done: done_tx }), open_tx, done_rx)
}

/// A blocking step whose first call waits at the gate; later calls return at once.
fn gated(
    gate: &Arc<Gate>,
    counter: &Concurrency,
) -> BlockingStep<impl Fn(StepContext) -> Result<(), ExecutionError> + Send + Sync> {
    let gate = Arc::clone(gate);
    let counter = counter.clone();
    step_blocking(move |_ctx| {
        let n = counter.enter();
        if n == 0 {
            let rx = gate.open.lock().unwrap().take().unwrap();
            rx.recv().unwrap();
            gate.done.send(()).unwrap();
        }
        counter.exit();
        Ok(())
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_closure_runs_on_a_blocking_thread_and_its_result_is_the_attempt_result() {
    let main_thread = std::thread::current().id();
    let seen = Arc::new(Mutex::new(None));
    let p = plan(vec![
        ExecStep::new(sid("ok"), "Ok", {
            let seen = Arc::clone(&seen);
            step_blocking(move |ctx| {
                *seen.lock().unwrap() = Some((std::thread::current().id(), ctx.idempotency_key().as_str().to_string()));
                Ok(())
            })
        }),
        ExecStep::new(sid("bad"), "Bad", step_blocking(|_ctx| Err(ExecutionError::refused("bad manifest")))),
    ]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&record, "ok"), StepStatus::Completed);
    assert_eq!(status(&record, "bad"), StepStatus::Failed);
    assert_eq!(record.step(&sid("bad")).unwrap().error(), Some(&ExecutionError::refused("bad manifest")));
    let (thread, key) = seen.lock().unwrap().clone().unwrap();
    assert_ne!(thread, main_thread);
    assert_eq!(key, "r:ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panic_in_the_closure_ends_the_step_panicked() {
    let p = plan(vec![ExecStep::new(sid("s"), "S", step_blocking(|_ctx| panic!("cargo exploded")))]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(status(&record, "s"), StepStatus::Panicked);
    assert!(record.step(&sid("s")).unwrap().reason().unwrap().contains("cargo exploded"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_timed_out_non_idempotent_closure_is_never_run_twice_at_once() {
    let counter = Concurrency::default();
    let (gate, open, done) = gate();
    let p = plan(vec![ExecStep::new(sid("publish"), "Publish", gated(&gate, &counter))]);
    let opts = ExecOptions::new().attempt_timeout(ms(50));

    // The attempt times out while the closure is still blocked: the effect is unknown.
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "publish"), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    assert_eq!(counter.calls(), 1);

    // The caller (wrongly) verifies it as not applied while the closure still runs. The new attempt does not start a
    // second closure; it ends unknown again and says why.
    let resume_opts = ResumeOptions::new(opts.clone()).verified(sid("publish"), Verification::NotApplied);
    let again = resume(&record, &p, &resume_opts).await.unwrap();
    assert_eq!(status(&again, "publish"), StepStatus::OutcomeUnknown(UnknownCause::StepFailed));
    let reason = again.step(&sid("publish")).unwrap().reason().unwrap();
    assert!(reason.contains("still running"), "{reason}");
    assert_eq!(counter.calls(), 1, "no second closure started");

    // Once the first closure has finished, a verified resume runs the step.
    open.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(5)).unwrap();
    // The in-flight marker is released just after the closure body returns, a moment after `done`.
    tokio::time::sleep(ms(100)).await;
    let last = resume(&again, &p, &resume_opts).await.unwrap();
    assert_eq!(status(&last, "publish"), StepStatus::Completed);
    assert_eq!(counter.calls(), 2);
    assert_eq!(counter.max.load(Ordering::SeqCst), 1, "never two closures at once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idempotent_closure_may_be_retried_while_the_timed_out_one_still_runs() {
    let counter = Concurrency::default();
    let (gate, open, done) = gate();
    let p =
        plan(vec![ExecStep::new(sid("upload"), "Upload", gated(&gate, &counter).idempotent()).retry(retries(1, 0))]);
    let opts = ExecOptions::new().attempt_timeout(ms(50));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "upload"), StepStatus::Completed, "the retry ran and succeeded");
    assert_eq!(counter.calls(), 2);
    assert_eq!(counter.max.load(Ordering::SeqCst), 2, "the first closure was still running during the retry");
    open.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(5)).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_closure_sees_run_cancellation_and_can_stop() {
    let p = plan(vec![ExecStep::new(
        sid("s"),
        "S",
        step_blocking(|ctx| {
            while !ctx.cancellation().is_cancelled() {
                std::thread::sleep(ms(1));
            }
            Err(ExecutionError::Cancelled)
        }),
    )]);
    let token = CancellationToken::new();
    let opts = ExecOptions::new().cancellation(token.clone());
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(ms(20)).await;
        token.cancel();
    });
    let record = execute(&p, rid("r"), &opts).await;
    canceller.await.unwrap();
    assert_eq!(status(&record, "s"), StepStatus::Skipped(SkipReason::Cancelled));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_timeout_fires_the_closures_token_so_it_can_stop_its_child() {
    let (saw_tx, saw_rx) = mpsc::sync_channel(1);
    let p = plan(vec![ExecStep::new(
        sid("s"),
        "S",
        step_blocking(move |ctx| {
            while !ctx.cancellation().is_cancelled() {
                std::thread::sleep(ms(1));
            }
            saw_tx.send(()).unwrap();
            Err(ExecutionError::failed("killed the child"))
        }),
    )]);
    let record = execute(&p, rid("r"), &ExecOptions::new().attempt_timeout(ms(30))).await;
    assert_eq!(status(&record, "s"), StepStatus::OutcomeUnknown(UnknownCause::Timeout));
    saw_rx.recv_timeout(Duration::from_secs(5)).expect("the closure saw its token fire after the timeout");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_run_does_not_stop_a_running_closure() {
    let counter = Concurrency::default();
    let (gate, open, done) = gate();
    let p = Arc::new(plan(vec![ExecStep::new(sid("s"), "S", gated(&gate, &counter))]));
    let run = tokio::spawn({
        let p = Arc::clone(&p);
        async move { execute(&p, rid("r"), &ExecOptions::new()).await }
    });
    while counter.calls() == 0 {
        tokio::time::sleep(ms(1)).await;
    }
    run.abort();
    assert!(run.await.unwrap_err().is_cancelled());
    open.send(()).unwrap();
    done.recv_timeout(Duration::from_secs(5)).expect("the closure ran to completion after its run was dropped");
}

#[test]
fn replay_defaults_to_not_idempotent() {
    let step = step_blocking(|_ctx| Ok(()));
    assert_eq!(step.replay(), Replay::NotIdempotent);
    assert_eq!(step.idempotent().replay(), Replay::Idempotent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_steps_sharing_one_blocking_runner_do_not_block_each_other() {
    let runner: Arc<dyn Step> = Arc::new(step_blocking(|_ctx| {
        std::thread::sleep(ms(20));
        Ok(())
    }));
    let p = plan(vec![
        ExecStep::from_arc(sid("a"), "A", Arc::clone(&runner)),
        ExecStep::from_arc(sid("b"), "B", Arc::clone(&runner)),
    ]);
    let opts = ExecOptions::new()
        .mode(michi_pipeline::Mode::Parallel { max_concurrency: std::num::NonZeroUsize::new(2).unwrap() });
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded, "the in-flight guard is per step, not per runner");
}

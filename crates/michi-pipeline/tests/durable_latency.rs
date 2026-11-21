//! A slow `record_start` must not serialise the starts of steps that run side by side.
//!
//! `DurableObserver::record_start` persists a marker, so in real use it is file or database I/O. Each step still waits
//! for its own marker before it runs, but the markers of N parallel steps should cost about one save, not N saves in a
//! row. These tests use a blocking sleep as the save and compare the whole run's wall time with the save time.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use michi_pipeline::{
    execute, step_fn, DurableObserver, ExecOptions, ExecStep, Mode, NotRunReason, Plan, RunId, RunOutcome, StepId,
    StepObserver, StepRecord, StepStatus,
};

const STEPS: usize = 8;
const SAVE: Duration = Duration::from_millis(100);

fn id(s: &str) -> StepId {
    StepId::try_new(s).expect("valid id")
}

fn run_id() -> RunId {
    RunId::try_new("latency").expect("valid id")
}

/// Blocks for `SAVE` on every start, like a synchronous fsync, and notes which steps' markers are saved.
#[derive(Default)]
struct SlowStore {
    saved: Mutex<Vec<String>>,
    fail: Option<&'static str>,
}

impl StepObserver for SlowStore {
    fn on_step_finished(&self, _: &RunId, _: &StepRecord) {}
}

impl DurableObserver for SlowStore {
    fn record_start(&self, _: &RunId, step_id: &StepId) -> Result<(), String> {
        std::thread::sleep(SAVE);
        if self.fail == Some(step_id.as_str()) {
            return Err("disk full".to_string());
        }
        self.saved.lock().expect("lock").push(step_id.to_string());
        Ok(())
    }
}

fn parallel_options(store: Arc<SlowStore>) -> ExecOptions {
    ExecOptions::new()
        .mode(Mode::Parallel { max_concurrency: NonZeroUsize::new(STEPS).expect("nonzero") })
        .durable_starts(store)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime")
}

#[test]
fn parallel_starts_cost_about_one_save_not_one_per_step() {
    let store = Arc::new(SlowStore::default());
    let ran_unsaved = Arc::new(AtomicBool::new(false));
    let steps: Vec<ExecStep> = (0..STEPS)
        .map(|i| {
            let (store, ran_unsaved) = (Arc::clone(&store), Arc::clone(&ran_unsaved));
            let name = format!("s{i}");
            let own = name.clone();
            ExecStep::new(
                id(&name),
                "step",
                step_fn(move |_| {
                    // The documented guarantee: this step's marker is saved before its effect runs.
                    if !store.saved.lock().expect("lock").contains(&own) {
                        ran_unsaved.store(true, Ordering::SeqCst);
                    }
                    async { Ok(()) }
                }),
            )
        })
        .collect();
    let plan = Plan::new(steps).expect("plan");
    let started = Instant::now();
    let record = runtime().block_on(execute(&plan, run_id(), &parallel_options(Arc::clone(&store))));
    let elapsed = started.elapsed();

    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert!(!ran_unsaved.load(Ordering::SeqCst), "a step ran before its start was recorded");
    assert_eq!(store.saved.lock().expect("lock").len(), STEPS);
    // Serial starts take STEPS * SAVE (800 ms). Side by side they take about one SAVE; allow three for slow runners.
    assert!(
        elapsed < SAVE * 3,
        "{STEPS} parallel starts of {SAVE:?} each took {elapsed:?}; they are being saved one after another"
    );
}

#[test]
fn a_failed_save_still_stops_only_its_step() {
    let store = Arc::new(SlowStore { fail: Some("s1"), ..SlowStore::default() });
    let steps: Vec<ExecStep> =
        (0..3).map(|i| ExecStep::new(id(&format!("s{i}")), "step", step_fn(|_| async { Ok(()) }))).collect();
    let plan = Plan::new(steps).expect("plan");
    let record = runtime().block_on(execute(&plan, run_id(), &parallel_options(store)));
    let status = |name: &str| record.steps().iter().find(|s| s.step_id().as_str() == name).map(|s| s.status());
    assert_eq!(status("s1"), Some(StepStatus::NotRun(NotRunReason::StartNotRecorded)));
    assert_eq!(status("s0"), Some(StepStatus::Completed));
    assert_eq!(status("s2"), Some(StepStatus::Completed));
}

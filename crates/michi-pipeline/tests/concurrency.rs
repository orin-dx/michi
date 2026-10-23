//! Per-key concurrency caps: `ExecStep::concurrency_key` with `ExecOptions::concurrency_limit`. All on Tokio's paused
//! clock; assertions check dispatch order and virtual time.

mod common;

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use common::*;
use michi_pipeline::{
    execute, resume, CancellationToken, DurableObserver, ExecOptions, ExecStep, FailurePolicy, Mode, NotRunReason,
    ResumeOptions, RunId, RunOutcome, SkipReason, StepId, StepObserver, StepRecord, StepStatus,
};

fn parallel(cap: usize) -> Mode {
    Mode::Parallel { max_concurrency: NonZeroUsize::new(cap).unwrap() }
}

fn limit(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

/// A scripted step in concurrency key `key`.
fn keyed(log: &Log, id: &str, deps: &[&str], key: &str, act: Act) -> ExecStep {
    common::act(log, id, deps, act).concurrency_key(key)
}

#[tokio::test(start_paused = true)]
async fn each_key_stays_under_its_limit_while_the_global_cap_has_room() {
    let npm = Log::new();
    let pypi = Log::new();
    let mut steps: Vec<ExecStep> =
        (0..6).map(|i| keyed(&npm, &format!("npm{i}"), &[], "npm", after(100, Act::Ok))).collect();
    steps.extend((0..2).map(|i| keyed(&pypi, &format!("pypi{i}"), &[], "pypi", after(100, Act::Ok))));
    let opts =
        ExecOptions::new().mode(parallel(8)).concurrency_limit("npm", limit(2)).concurrency_limit("pypi", limit(1));
    let record = execute(&plan(steps), rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(npm.max_in_flight(), 2, "at most two npm steps at once");
    assert_eq!(pypi.max_in_flight(), 1, "at most one pypi step at once");
    assert_eq!(record.elapsed().as_millis(), 300, "six npm steps in pairs of 100 ms");
}

#[tokio::test(start_paused = true)]
async fn a_key_at_its_limit_does_not_block_later_steps_of_other_keys() {
    let log = Log::new();
    let p = plan(vec![
        keyed(&log, "npm-a", &[], "npm", after(100, Act::Ok)),
        keyed(&log, "npm-b", &[], "npm", after(100, Act::Ok)),
        keyed(&log, "pypi-a", &[], "pypi", after(10, Act::Ok)),
        act(&log, "tag", &[], after(10, Act::Ok)),
    ]);
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("npm", limit(1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    let started = |id: &str| record.step(&sid(id)).unwrap().started_at().unwrap().as_millis();
    assert_eq!(started("npm-a"), 0);
    assert_eq!(started("pypi-a"), 0, "a waiting npm step does not hold back another key");
    assert_eq!(started("tag"), 0, "nor a step without a key");
    assert_eq!(started("npm-b"), 100, "npm-b waits for npm-a's slot");
}

#[tokio::test(start_paused = true)]
async fn waiting_steps_take_freed_slots_in_declaration_order_and_runs_repeat_exactly() {
    // `x` is declared first but only becomes ready when `a` finishes, while `b` holds the key and `c` waits. When `b`
    // frees the slot, the earliest-declared dispatchable step (`x`) goes first, then `c`.
    async fn once() -> (Vec<String>, Vec<(u64, String)>) {
        let log = Log::new();
        let p = plan(vec![
            keyed(&log, "x", &["a"], "k", after(10, Act::Ok)),
            keyed(&log, "b", &[], "k", after(20, Act::Ok)),
            keyed(&log, "c", &[], "k", after(30, Act::Ok)),
            act(&log, "a", &[], after(5, Act::Ok)),
        ]);
        let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("k", limit(1));
        let record = execute(&p, rid("r"), &opts).await;
        assert_eq!(record.outcome(), RunOutcome::Succeeded);
        (log.starts(), log.timed().into_iter().filter(|(_, e)| e.starts_with("start:")).collect())
    }
    let (starts, timed) = once().await;
    assert_eq!(starts, ["b", "a", "x", "c"]);
    assert_eq!(timed, [(0, "start:b".into()), (0, "start:a".into()), (20, "start:x".into()), (30, "start:c".into())]);
    assert_eq!(once().await, (starts, timed), "the same plan and options dispatch identically");
}

#[tokio::test(start_paused = true)]
async fn keys_without_a_limit_are_unlimited_and_limits_for_unused_keys_are_ignored() {
    let log = Log::new();
    let steps = (0..3).map(|i| keyed(&log, &format!("s{i}"), &[], "free", after(10, Act::Ok))).collect();
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("other", limit(1));
    let record = execute(&plan(steps), rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(log.max_in_flight(), 3);
}

#[tokio::test(start_paused = true)]
async fn the_empty_key_is_a_key_like_any_other() {
    let log = Log::new();
    let steps = (0..3).map(|i| keyed(&log, &format!("s{i}"), &[], "", after(10, Act::Ok))).collect();
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("", limit(1));
    let record = execute(&plan(steps), rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(log.max_in_flight(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_later_concurrency_key_replaces_the_earlier_one() {
    let log = Log::new();
    let steps = vec![
        keyed(&log, "a", &[], "npm", after(10, Act::Ok)),
        keyed(&log, "b", &[], "npm", after(10, Act::Ok)).concurrency_key("cargo"),
    ];
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("npm", limit(1));
    let record = execute(&plan(steps), rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(log.max_in_flight(), 2, "b is in cargo only, so it does not wait for npm");
}

#[tokio::test(start_paused = true)]
async fn sequential_mode_runs_keyed_steps_in_execution_order() {
    let log = Log::new();
    let p = plan(vec![
        keyed(&log, "a", &[], "k", after(10, Act::Ok)),
        keyed(&log, "b", &["a"], "k", after(10, Act::Ok)),
        act(&log, "c", &[], after(10, Act::Ok)),
    ]);
    let opts = ExecOptions::new().concurrency_limit("k", limit(1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    let order: Vec<String> = p.execution_order().into_iter().map(ToString::to_string).collect();
    assert_eq!(log.starts(), order);
    assert_eq!(log.max_in_flight(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_failed_or_panicked_step_frees_its_slot() {
    let log = Log::new();
    let p = plan(vec![
        keyed(&log, "a", &[], "k", after(10, Act::Err(fatal("bad input")))),
        keyed(&log, "b", &[], "k", after(10, Act::Panic)),
        keyed(&log, "c", &[], "k", after(10, Act::Ok)),
    ]);
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("k", limit(1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::Failed);
    assert_eq!(status(&record, "b"), StepStatus::Panicked);
    assert_eq!(status(&record, "c"), StepStatus::Completed);
    let started = |id: &str| record.step(&sid(id)).unwrap().started_at().unwrap().as_millis();
    assert_eq!((started("a"), started("b"), started("c")), (0, 10, 20));
}

#[derive(Default)]
struct Starts(Mutex<Vec<String>>);

impl StepObserver for Starts {
    fn on_step_started(&self, _run_id: &RunId, step_id: &StepId) {
        self.0.lock().unwrap().push(step_id.to_string());
    }
    fn on_step_finished(&self, _run_id: &RunId, _record: &StepRecord) {}
}

#[tokio::test(start_paused = true)]
async fn cancellation_while_waiting_for_a_slot_skips_the_step_without_starting_it() {
    let log = Log::new();
    let p = plan(vec![keyed(&log, "a", &[], "k", Act::Cooperative), keyed(&log, "b", &[], "k", after(10, Act::Ok))]);
    let token = CancellationToken::new();
    let observer = Arc::new(Starts::default());
    let opts = ExecOptions::new()
        .mode(parallel(4))
        .concurrency_limit("k", limit(1))
        .cancellation(token.clone())
        .observer(observer.clone());
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        token.cancel();
    });
    let record = execute(&p, rid("r"), &opts).await;
    canceller.await.unwrap();
    assert_eq!(status(&record, "a"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(status(&record, "b"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(log.starts(), ["a"], "b never started");
    assert_eq!(*observer.0.lock().unwrap(), ["a"], "no start event for a step still waiting for its key");

    // Resume runs the step that was waiting, still under the limit.
    let resumed =
        resume(&record, &p, &ResumeOptions::new(ExecOptions::new().mode(parallel(4)).concurrency_limit("k", limit(1))))
            .await
            .unwrap();
    assert_eq!(status(&resumed, "b"), StepStatus::Completed);
}

#[tokio::test(start_paused = true)]
async fn fail_fast_skips_steps_waiting_for_a_slot() {
    let log = Log::new();
    let p = plan(vec![
        keyed(&log, "a", &[], "k", after(10, Act::Err(fatal("bad input")))),
        keyed(&log, "b", &[], "k", after(10, Act::Ok)),
    ]);
    let opts =
        ExecOptions::new().mode(parallel(4)).concurrency_limit("k", limit(1)).failure_policy(FailurePolicy::FailFast);
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::Failed);
    assert_eq!(status(&record, "b"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(log.starts(), ["a"]);
}

/// Refuses to record `a`'s start, so `a` never runs.
struct RefuseA;

impl StepObserver for RefuseA {
    fn on_step_finished(&self, _run_id: &RunId, _record: &StepRecord) {}
}

impl DurableObserver for RefuseA {
    fn record_start(&self, _run_id: &RunId, step_id: &StepId) -> Result<(), String> {
        if step_id.as_str() == "a" {
            Err("store offline".to_string())
        } else {
            Ok(())
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_step_whose_start_was_not_recorded_takes_no_slot() {
    let log = Log::new();
    let p = plan(vec![keyed(&log, "a", &[], "k", after(10, Act::Ok)), keyed(&log, "b", &[], "k", after(10, Act::Ok))]);
    let opts = ExecOptions::new().mode(parallel(4)).concurrency_limit("k", limit(1)).durable_starts(Arc::new(RefuseA));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(status(&record, "a"), StepStatus::NotRun(NotRunReason::StartNotRecorded));
    assert_eq!(status(&record, "b"), StepStatus::Completed);
    assert_eq!(record.step(&sid("b")).unwrap().started_at().unwrap().as_millis(), 0, "b did not wait for a");
}

#[test]
fn options_debug_lists_the_limits() {
    let opts = ExecOptions::new().concurrency_limit("npm", limit(2));
    assert!(format!("{opts:?}").contains("concurrency_limits: {\"npm\": 2}"), "{opts:?}");
}

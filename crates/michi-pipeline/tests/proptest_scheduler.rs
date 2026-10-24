//! Model-based property tests for the scheduler: random DAGs (declared in random order) of scripted steps, run under
//! random modes, failure policies and concurrency limits on Tokio's paused clock. Each property is checked against an
//! oracle written here from the documented rules, not against the scheduler's own bookkeeping.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use michi_pipeline::{
    execute, resume, ExecOptions, ExecStep, ExecutionError, FailurePolicy, Mode, Plan, Replay, ResumeOptions, RunId,
    RunOutcome, RunRecord, Step, StepContext, StepFuture, StepId, StepStatus,
};
use proptest::prelude::*;

fn sid(s: &str) -> StepId {
    StepId::try_new(s).expect("valid step id")
}

fn rid(s: &str) -> RunId {
    RunId::try_new(s).expect("valid run id")
}

/// What a step does in the first run. In the run after it, every step succeeds, so resume has something to recover.
#[derive(Clone, Copy, Debug)]
enum Fate {
    Ok,
    /// Fails with a definitive "not applied" error.
    Refused,
    /// Fails with an error whose effect is unknown.
    Unknown,
    Panic,
}

#[derive(Clone, Debug)]
struct Spec {
    /// Indices of earlier steps this one needs.
    deps: Vec<usize>,
    fate: Fate,
    idempotent: bool,
    /// 0: no concurrency key, 1 and 2: keys `k1` and `k2`.
    key: u8,
    delay_ms: u64,
}

#[derive(Clone, Debug)]
struct Case {
    specs: Vec<Spec>,
    /// Declaration order: a permutation of `0..specs.len()`.
    order: Vec<usize>,
    parallel: Option<usize>,
    fail_fast: bool,
    k1_limit: usize,
}

fn fate() -> impl Strategy<Value = Fate> {
    prop_oneof![6 => Just(Fate::Ok), 2 => Just(Fate::Refused), 2 => Just(Fate::Unknown), 1 => Just(Fate::Panic)]
}

/// Mostly key `k1`, so its limit is what the scheduler has to hold.
fn key() -> impl Strategy<Value = u8> {
    prop_oneof![2 => Just(0u8), 4 => Just(1u8), 1 => Just(2u8)]
}

fn case() -> impl Strategy<Value = Case> {
    (1usize..=8).prop_flat_map(|n| {
        let specs = prop::collection::vec((any::<u8>(), any::<u8>(), fate(), any::<bool>(), key(), 1u64..40), n)
            .prop_map(|raw| {
                raw.into_iter()
                    .enumerate()
                    .map(|(i, (a, b, fate, idempotent, key, delay_ms))| Spec {
                        // Two masks combined with a bitwise AND: dependencies are sparse, so many steps are independent.
                        deps: (0..i).filter(|j| (a & b) >> j & 1 == 1).collect(),
                        fate,
                        idempotent,
                        key,
                        delay_ms,
                    })
                    .collect::<Vec<_>>()
            });
        (
            specs,
            Just((0..n).collect::<Vec<_>>()).prop_shuffle(),
            prop::option::weighted(0.85, 2usize..=5),
            any::<bool>(),
            1usize..=3,
        )
            .prop_map(|(specs, order, parallel, fail_fast, k1_limit)| Case {
                specs,
                order,
                parallel,
                fail_fast,
                k1_limit,
            })
    })
}

fn name(i: usize) -> String {
    format!("s{i}")
}

const KEYS: usize = 3;

/// State shared by every step of one case: the event order and in-flight counts overall and per concurrency key.
#[derive(Default)]
struct Shared {
    /// 0 for the first run, 1 for the resumed one.
    phase: AtomicUsize,
    events: Mutex<Vec<String>>,
    live: AtomicUsize,
    max_live: AtomicUsize,
    key_live: [AtomicUsize; KEYS],
    key_max: [AtomicUsize; KEYS],
}

impl Shared {
    fn enter(&self, key: usize) {
        let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_live.fetch_max(now, Ordering::SeqCst);
        let now = self.key_live[key].fetch_add(1, Ordering::SeqCst) + 1;
        self.key_max[key].fetch_max(now, Ordering::SeqCst);
    }

    fn exit(&self, key: usize) {
        self.live.fetch_sub(1, Ordering::SeqCst);
        self.key_live[key].fetch_sub(1, Ordering::SeqCst);
    }

    fn push(&self, event: String) {
        self.events.lock().expect("event log").push(event);
    }
}

/// Releases a step's in-flight slot when its attempt ends, including by panic.
struct Slot(Arc<Shared>, usize);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.exit(self.1);
    }
}

struct Probe {
    name: String,
    fate: Fate,
    idempotent: bool,
    key: usize,
    delay: Duration,
    shared: Arc<Shared>,
    calls: Arc<AtomicU32>,
}

impl Step for Probe {
    fn run<'a>(&'a self, _ctx: &'a StepContext) -> StepFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let shared = self.shared.clone();
        let fate = if shared.phase.load(Ordering::SeqCst) == 0 { self.fate } else { Fate::Ok };
        let (name, key, delay) = (self.name.clone(), self.key, self.delay);
        Box::pin(async move {
            shared.push(format!("start:{name}"));
            shared.enter(key);
            let _slot = Slot(shared.clone(), key);
            tokio::time::sleep(delay).await;
            let result = match fate {
                Fate::Ok => Ok(()),
                Fate::Refused => Err(ExecutionError::refused("refused")),
                Fate::Unknown => Err(ExecutionError::failed("unknown")),
                Fate::Panic => panic!("scripted panic"),
            };
            shared.push(format!("end:{name}"));
            result
        })
    }

    fn replay(&self) -> Replay {
        if self.idempotent {
            Replay::Idempotent
        } else {
            Replay::NotIdempotent
        }
    }
}

struct Built {
    plan: Plan,
    shared: Arc<Shared>,
    calls: Vec<Arc<AtomicU32>>,
}

fn build(case: &Case) -> Built {
    let shared = Arc::new(Shared::default());
    let calls: Vec<Arc<AtomicU32>> = case.specs.iter().map(|_| Arc::default()).collect();
    let mut steps: Vec<Option<ExecStep>> = Vec::new();
    for (i, spec) in case.specs.iter().enumerate() {
        let probe = Probe {
            name: name(i),
            fate: spec.fate,
            idempotent: spec.idempotent,
            key: usize::from(spec.key),
            delay: Duration::from_millis(spec.delay_ms),
            shared: shared.clone(),
            calls: calls[i].clone(),
        };
        let mut step =
            spec.deps.iter().fold(ExecStep::new(sid(&name(i)), name(i), probe), |s, d| s.depends_on(sid(&name(*d))));
        if spec.key != 0 {
            step = step.concurrency_key(format!("k{}", spec.key));
        }
        steps.push(Some(step));
    }
    let ordered = case.order.iter().map(|i| steps[*i].take().expect("each step once")).collect();
    Built { plan: Plan::new(ordered).expect("generated plans are valid"), shared, calls }
}

fn options(case: &Case) -> ExecOptions {
    let mode = match case.parallel {
        Some(cap) => Mode::Parallel { max_concurrency: NonZeroUsize::new(cap).expect("cap is at least 1") },
        None => Mode::Sequential,
    };
    let policy = if case.fail_fast { FailurePolicy::FailFast } else { FailurePolicy::SkipDependents };
    ExecOptions::new()
        .mode(mode)
        .failure_policy(policy)
        .concurrency_limit("k1", NonZeroUsize::new(case.k1_limit).expect("limit is at least 1"))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().expect("runtime builds")
}

fn completed(record: &RunRecord) -> BTreeSet<String> {
    record.steps().iter().filter(|s| s.status() == StepStatus::Completed).map(|s| s.step_id().to_string()).collect()
}

/// Steps that finish in `Completed` when each step's own outcome is its fate and a step runs only if every
/// prerequisite completed (the `SkipDependents` rule).
fn oracle_completed(case: &Case) -> BTreeSet<String> {
    let mut done = vec![false; case.specs.len()];
    for (i, spec) in case.specs.iter().enumerate() {
        done[i] = matches!(spec.fate, Fate::Ok) && spec.deps.iter().all(|d| done[*d]);
    }
    (0..case.specs.len()).filter(|i| done[*i]).map(name).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn a_run_follows_the_documented_rules(case in case()) {
        let rt = runtime();
        rt.block_on(async {
            let built = build(&case);
            let record = execute(&built.plan, rid("r"), &options(&case)).await;

            // One record per plan step, no more.
            let ids: BTreeSet<String> = record.steps().iter().map(|s| s.step_id().to_string()).collect();
            prop_assert_eq!(record.steps().len(), case.specs.len());
            prop_assert_eq!(ids, (0..case.specs.len()).map(name).collect::<BTreeSet<_>>());

            // `Succeeded` only if every step completed, and the other way round.
            let all_done = record.steps().iter().all(|s| s.status() == StepStatus::Completed);
            prop_assert_eq!(record.outcome() == RunOutcome::Succeeded, all_done);

            // A completed step never ran before a prerequisite finished, and all prerequisites completed.
            let done = completed(&record);
            for (i, spec) in case.specs.iter().enumerate() {
                if done.contains(&name(i)) {
                    for d in &spec.deps {
                        prop_assert!(done.contains(&name(*d)), "{} completed but {} did not", name(i), name(*d));
                    }
                }
                // A step that never ran was never started; a completed step ran.
                let ran = built.calls[i].load(Ordering::SeqCst);
                if done.contains(&name(i)) {
                    prop_assert!(ran >= 1, "{} completed without running", name(i));
                }
                if matches!(record.step(&sid(&name(i))).map(|s| s.status()), Some(StepStatus::Skipped(_) | StepStatus::NotRun(_))) {
                    prop_assert_eq!(ran, 0, "{} was not run but its step body was called", name(i));
                }
            }

            // Under SkipDependents the completed set is exactly the oracle's; under FailFast it can only be smaller.
            let expected = oracle_completed(&case);
            if case.fail_fast {
                prop_assert!(done.is_subset(&expected), "completed {:?} beyond the oracle's {:?}", done, expected);
            } else {
                prop_assert_eq!(done, expected);
            }

            // Every prerequisite's end event precedes its dependent's start event.
            let events = built.shared.events.lock().expect("event log").clone();
            let position = |e: String| events.iter().position(|x| *x == e);
            for (i, spec) in case.specs.iter().enumerate() {
                if let Some(start) = position(format!("start:{}", name(i))) {
                    for d in &spec.deps {
                        let end = position(format!("end:{}", name(*d)));
                        prop_assert!(end.is_some_and(|e| e < start), "{} started before {} ended", name(i), name(*d));
                    }
                }
            }

            // Concurrency: the overall cap and the k1 limit are never exceeded, panicking steps included.
            let cap = case.parallel.unwrap_or(1);
            let max_live = built.shared.max_live.load(Ordering::SeqCst);
            prop_assert!(max_live <= cap, "{} in flight, cap {}", max_live, cap);
            let max_k1 = built.shared.key_max[1].load(Ordering::SeqCst);
            prop_assert!(max_k1 <= case.k1_limit, "{} k1 steps at once, limit {}", max_k1, case.k1_limit);
            Ok(())
        })?;
    }

    #[test]
    fn a_sequential_run_is_deterministic(mut case in case()) {
        case.parallel = None;
        let rt = runtime();
        let (a, b) = rt.block_on(async {
            let a = execute(&build(&case).plan, rid("r"), &options(&case)).await.render();
            let b = execute(&build(&case).plan, rid("r"), &options(&case)).await.render();
            (a, b)
        });
        prop_assert_eq!(a, b);
    }

    #[test]
    fn resume_never_repeats_finished_work_or_a_step_whose_effect_is_unknown(case in case()) {
        let rt = runtime();
        rt.block_on(async {
            let built = build(&case);
            let first = execute(&built.plan, rid("r"), &options(&case)).await;
            let snapshot: BTreeMap<String, (StepStatus, u32)> = (0..case.specs.len())
                .map(|i| {
                    let id = name(i);
                    let status = first.step(&sid(&id)).expect("recorded").status();
                    (id, (status, built.calls[i].load(Ordering::SeqCst)))
                })
                .collect();

            built.shared.phase.store(1, Ordering::SeqCst);
            let second = resume(&first, &built.plan, &ResumeOptions::new(options(&case))).await.expect("same plan resumes");

            for (i, spec) in case.specs.iter().enumerate() {
                let id = name(i);
                let (before, calls_before) = snapshot[&id];
                let calls_after = built.calls[i].load(Ordering::SeqCst);
                if before == StepStatus::Completed {
                    prop_assert_eq!(calls_after, calls_before, "{} completed, then ran again on resume", id);
                    prop_assert_eq!(
                        second.step(&sid(&id)).expect("recorded").status(),
                        StepStatus::CompletedEarlier,
                        "{} should be CompletedEarlier", id
                    );
                }
                if matches!(before, StepStatus::OutcomeUnknown(_) | StepStatus::Panicked) && !spec.idempotent {
                    prop_assert_eq!(calls_after, calls_before, "{} may have taken effect and is not idempotent, yet ran again", id);
                }
            }

            // A non-idempotent step that failed with an unknown effect, or panicked, is held until the caller verifies
            // it. With none of those, the second run finishes the job.
            let blocked = case.specs.iter().any(|s| matches!(s.fate, Fate::Unknown | Fate::Panic) && !s.idempotent);
            if !blocked {
                prop_assert_eq!(second.outcome(), RunOutcome::Succeeded, "{}", second.render());
            }
            Ok(())
        })?;
    }

    /// A store that lost some records of steps that a saved, completed step depends on does not make resume run
    /// finished work again: a completed dependent proves its prerequisites completed.
    #[test]
    fn lost_records_of_a_completed_steps_prerequisites_never_re_run_them(case in case(), lost in any::<u16>()) {
        let rt = runtime();
        rt.block_on(async {
            let built = build(&case);
            let first = execute(&built.plan, rid("r"), &options(&case)).await;
            let status_of = |i: usize| first.step(&sid(&name(i))).expect("recorded").status();
            // Steps that are a prerequisite of a Completed step; each is itself Completed.
            let proven: BTreeSet<usize> = case
                .specs
                .iter()
                .enumerate()
                .filter(|(i, _)| status_of(*i) == StepStatus::Completed)
                .flat_map(|(_, spec)| spec.deps.iter().copied())
                .collect();
            let saved: Vec<_> = (0..case.specs.len())
                .filter(|i| !(proven.contains(i) && lost >> (i % 16) & 1 == 1))
                .map(|i| first.step(&sid(&name(i))).expect("recorded").clone())
                .collect();
            let calls_before: Vec<u32> = built.calls.iter().map(|c| c.load(Ordering::SeqCst)).collect();
            let rebuilt = RunRecord::from_parts(rid("r"), built.plan.fingerprint(), &built.plan, saved)
                .expect("a consistent record rebuilds");
            built.shared.phase.store(1, Ordering::SeqCst);
            resume(&rebuilt, &built.plan, &ResumeOptions::new(options(&case))).await.expect("same plan resumes");
            for i in &proven {
                prop_assert_eq!(
                    built.calls[*i].load(Ordering::SeqCst),
                    calls_before[*i],
                    "{} is a prerequisite of a completed step and ran again", name(*i)
                );
            }
            Ok(())
        })?;
    }
}

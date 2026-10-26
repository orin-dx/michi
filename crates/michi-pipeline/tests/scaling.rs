//! Complexity-class checks: planning, running and resuming must grow about linearly with the step count.
//!
//! These catch an accidental O(n²) (a linear lookup per step, a rescan per dispatch), such as the per-step scan of the
//! prior record that made resuming 50,000 steps take 1.6 s before the record was indexed. They do not track constant
//! factors: wall time on a shared runner moves by tens of percent. A tenfold plan makes linear code take about 10x as
//! long and quadratic code about 100x, so a 30x bound separates them with a threefold margin for noise. The same shape
//! of test lives in `michi-core/tests/scaling.rs` and `michi-graph/tests/scaling.rs`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use michi_pipeline::{execute, resume, step_fn, ExecOptions, ExecStep, Plan, ResumeOptions, RunId, RunOutcome, StepId};

const GROWTH: usize = 10;
const MAX_RATIO: f64 = 30.0;
/// Each size is timed this many times and the fastest run counts, which discards scheduler and cache noise.
const RUNS: usize = 5;

/// `n` no-op steps in layers of 10, each depending on two steps of the layer before.
fn steps(n: usize) -> Vec<ExecStep> {
    (0..n)
        .map(|i| {
            let id = StepId::try_new(format!("s{i}")).expect("valid id");
            let mut step = ExecStep::new(id, "noop", step_fn(|_ctx| async { Ok(()) }));
            if i >= 10 {
                let layer_start = (i / 10 - 1) * 10;
                for dep in [layer_start + i % 10, layer_start + (i + 1) % 10] {
                    step = step.depends_on(StepId::try_new(format!("s{dep}")).expect("valid id"));
                }
            }
            step
        })
        .collect()
}

fn best_of(mut f: impl FnMut()) -> Duration {
    (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// Times `work` on the input built for `n` and for `n * GROWTH`; building happens outside the timed region.
fn assert_linear<I>(label: &str, n: usize, build: impl Fn(usize) -> I, work: impl Fn(&I)) {
    let (small, large) = (build(n), build(n * GROWTH));
    let t_small = best_of(|| work(black_box(&small)));
    let t_large = best_of(|| work(black_box(&large)));
    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(1e-9);
    assert!(
        ratio < MAX_RATIO,
        "{label}: {GROWTH}x the input took {ratio:.1}x as long ({t_small:?} -> {t_large:?}); linear is ~{GROWTH}x"
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime")
}

#[test]
fn building_a_plan_is_linear_in_steps() {
    assert_linear(
        "Plan::new",
        300,
        |n| n,
        |&n| {
            black_box(Plan::new(steps(n)).expect("valid plan"));
        },
    );
}

#[test]
fn executing_a_plan_is_linear_in_steps() {
    let rt = runtime();
    assert_linear(
        "execute",
        200,
        |n| Plan::new(steps(n)).expect("valid plan"),
        |plan| {
            let record = rt.block_on(execute(plan, RunId::try_new("scale").expect("valid id"), &ExecOptions::new()));
            assert_eq!(record.outcome(), RunOutcome::Succeeded);
        },
    );
}

#[test]
fn resuming_a_finished_run_is_linear_in_steps() {
    let rt = runtime();
    assert_linear(
        "resume of a finished run",
        300,
        |n| {
            let plan = Plan::new(steps(n)).expect("valid plan");
            let record = rt.block_on(execute(&plan, RunId::try_new("scale").expect("valid id"), &ExecOptions::new()));
            (plan, record)
        },
        |(plan, record)| {
            let resumed = rt.block_on(resume(record, plan, &ResumeOptions::default())).expect("resume");
            assert_eq!(resumed.outcome(), RunOutcome::Succeeded);
        },
    );
}

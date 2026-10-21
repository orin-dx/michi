//! Scheduler overhead: no-op steps, so the time is the plan build or the dispatch loop itself.

use std::num::NonZeroUsize;

use divan::Bencher;
use michi_pipeline::{execute, step_fn, ExecOptions, ExecStep, Mode, Plan, RunId, StepId};

fn main() {
    divan::main();
}

/// `n` no-op steps in layers of 10, each depending on two steps of the layer before.
fn steps(n: usize) -> Vec<ExecStep> {
    (0..n)
        .map(|i| {
            let mut step =
                ExecStep::new(StepId::try_new(format!("s{i}")).unwrap(), "noop", step_fn(|_ctx| async { Ok(()) }));
            if i >= 10 {
                let layer_start = (i / 10 - 1) * 10;
                step = step
                    .depends_on(StepId::try_new(format!("s{}", layer_start + i % 10)).unwrap())
                    .depends_on(StepId::try_new(format!("s{}", layer_start + (i + 1) % 10)).unwrap());
            }
            step
        })
        .collect()
}

#[divan::bench(args = [500])]
fn validate_plan(b: Bencher, n: usize) {
    b.with_inputs(|| steps(n)).bench_values(|s| Plan::new(s).unwrap());
}

#[divan::bench(args = [500])]
fn execute_sequential(b: Bencher, n: usize) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let plan = Plan::new(steps(n)).unwrap();
    let opts = ExecOptions::new();
    b.bench(|| rt.block_on(execute(&plan, RunId::try_new("bench").unwrap(), &opts)));
}

#[divan::bench(args = [500])]
fn execute_parallel_cap_16(b: Bencher, n: usize) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let plan = Plan::new(steps(n)).unwrap();
    let opts = ExecOptions::new().mode(Mode::Parallel { max_concurrency: NonZeroUsize::new(16).unwrap() });
    b.bench(|| rt.block_on(execute(&plan, RunId::try_new("bench").unwrap(), &opts)));
}

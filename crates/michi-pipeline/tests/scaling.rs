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

/// Converting an error whose step text is a 1 MiB body of URLs must finish in about a second. The text is redacted
/// whole before it is cut (a cut through a secret would leave its prefix, which no longer matches a shape), so the
/// redaction itself has to be linear: a rescan per URL took minutes here. The work runs on its own thread and a timeout
/// fails the test instead of hanging it.
#[test]
fn converting_a_one_mebibyte_body_of_urls_is_bounded() {
    const LIMIT: Duration = Duration::from_secs(20);
    // Each unit ends a URL differently (whitespace, nothing, a quote), and one carries a password to find.
    for unit in ["https://u:SECRET@host/p ", "a://", "a://\"", "x://h/a://b/ ", "%3A%2F%2Fu%3ASECRET%40h "] {
        let body = unit.repeat((1 << 20) / unit.len());
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = Instant::now();
            let err = michi_pipeline::ExecutionError::Http { status: 502, message: body, retry_after: None };
            let converted: michi_core::DomainError = err.into();
            // The receiver is gone after a timeout.
            let _ = done.send((started.elapsed(), converted));
        });
        let (took, converted) =
            finished.recv_timeout(LIMIT).unwrap_or_else(|_| panic!("{unit:?}: no result within {LIMIT:?}"));
        assert!(took < LIMIT, "{unit:?}: took {took:?}");
        let cause = converted.causes().first().expect("the body is a cause");
        assert!(cause.len() <= 512, "{unit:?}: the cause is cut to 512 bytes, got {}", cause.len());
        assert!(!cause.contains("SECRET"), "{unit:?}: the password survived: {cause}");
    }
}

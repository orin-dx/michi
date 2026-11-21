//! Compile-time check that the public types stay `Send + Sync` and that the `execute`/`resume` futures are `Send`, so
//! they can run on a multi-threaded runtime.
use michi_pipeline::{
    BreakerConfig, BreakerRegistry, CancellationToken, ExecOptions, ExecStep, ExecutionError, Plan, PlanError,
    PlanFingerprint, ResumeError, ResumeOptions, RunId, RunOutcome, RunRecord, StepId, StepRecord,
};

fn is_send_sync<T: Send + Sync>() {}
fn is_send<T: Send>(_: T) {}

#[test]
fn public_types_are_send_and_sync() {
    is_send_sync::<Plan>();
    is_send_sync::<ExecStep>();
    is_send_sync::<PlanError>();
    is_send_sync::<PlanFingerprint>();
    is_send_sync::<RunRecord>();
    is_send_sync::<StepRecord>();
    is_send_sync::<RunOutcome>();
    is_send_sync::<RunId>();
    is_send_sync::<StepId>();
    is_send_sync::<ExecOptions>();
    is_send_sync::<ResumeOptions>();
    is_send_sync::<ResumeError>();
    is_send_sync::<ExecutionError>();
    is_send_sync::<BreakerRegistry>();
    is_send_sync::<BreakerConfig>();
    is_send_sync::<CancellationToken>();
}

/// Never called: it only has to compile.
#[allow(dead_code)]
fn execute_and_resume_futures_are_send(
    plan: &Plan,
    run_id: RunId,
    opts: &ExecOptions,
    ropts: &ResumeOptions,
    prior: &RunRecord,
) {
    is_send(michi_pipeline::execute(plan, run_id, opts));
    is_send(michi_pipeline::resume(prior, plan, ropts));
}

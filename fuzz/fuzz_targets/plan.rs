//! `Plan::new` never panics on any ids, dependencies or per-step limits. It accepts exactly the plans an independent
//! oracle accepts; a valid plan's `execution_order` is the topological order with declaration-order ties, and its
//! fingerprint ignores declaration order. A valid plan of instant steps also runs to success under arbitrary
//! `ExecOptions`, including durations too large for the clock.

#![no_main]

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::time::Duration;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_pipeline::{
    execute, step_fn, ExecOptions, ExecStep, Mode, Plan, PlanViolation, RetryConfig, RunId, RunOutcome, StepId,
};

#[derive(Arbitrary, Debug)]
struct Dur {
    secs: u64,
    nanos: u32,
}

impl Dur {
    fn get(&self) -> Duration {
        Duration::new(self.secs, self.nanos % 1_000_000_000)
    }
}

#[derive(Arbitrary, Debug)]
struct StepSpec {
    id: Id,
    name: String,
    deps: Vec<Id>,
    breaker_key: Option<String>,
    concurrency_key: Option<String>,
    deadline_group: Option<String>,
    attempt_timeout: Option<Dur>,
    deadline: Option<Dur>,
    retry: Option<(u32, Dur, Dur, u64)>,
}

/// Mostly a small slug (so ids collide, repeat and form cycles), sometimes any string (so invalid ids are rejected).
#[derive(Arbitrary, Debug)]
enum Id {
    Slug(u8),
    Any(String),
}

impl Id {
    fn get(&self) -> Option<StepId> {
        match self {
            Id::Slug(n) => StepId::try_new(format!("s{}", n % 10)).ok(),
            Id::Any(s) => StepId::try_new(s.as_str()).ok(),
        }
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    steps: Vec<StepSpec>,
    parallel: Option<u8>,
    attempt_timeout: Dur,
    step_deadline: Option<Dur>,
    group: String,
    group_window: Dur,
    cancel_grace: Dur,
}

fn build(spec: &StepSpec, id: StepId) -> ExecStep {
    let mut step = ExecStep::new(id, spec.name.clone(), step_fn(|_ctx| async { Ok(()) }));
    for dep in &spec.deps {
        if let Some(dep) = dep.get() {
            step = step.depends_on(dep);
        }
    }
    if let Some(k) = &spec.breaker_key {
        step = step.breaker_key(k.clone());
    }
    if let Some(k) = &spec.concurrency_key {
        step = step.concurrency_key(k.clone());
    }
    if let Some(k) = &spec.deadline_group {
        step = step.deadline_group(k.clone());
    }
    if let Some(d) = &spec.attempt_timeout {
        step = step.attempt_timeout(d.get());
    }
    if let Some(d) = &spec.deadline {
        step = step.deadline(d.get());
    }
    if let Some((n, base, max, jitter)) = &spec.retry {
        step = step.retry(RetryConfig::new(*n, base.get(), max.get(), f64::from_bits(*jitter)));
    }
    step
}

fuzz_target!(|input: Input| {
    if input.steps.len() > 40 {
        return;
    }
    // A step whose id is not a valid slug can't be declared at all; drop it and any dependency on an invalid id.
    let specs: Vec<(&StepSpec, StepId)> = input.steps.iter().filter_map(|s| s.id.get().map(|id| (s, id))).collect();

    // Oracle over ids.
    let ids: Vec<&StepId> = specs.iter().map(|(_, id)| id).collect();
    let mut first: Vec<&StepId> = Vec::new();
    let mut has_dup = false;
    for id in &ids {
        if first.contains(id) {
            has_dup = true;
        } else {
            first.push(id);
        }
    }
    let mut prereqs: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); first.len()];
    let (mut unknown, mut self_dep) = (false, false);
    let mut seen: Vec<&StepId> = Vec::new();
    for (spec, id) in &specs {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let me = first.iter().position(|x| x == &id).expect("declared");
        for dep in spec.deps.iter().filter_map(Id::get) {
            match first.iter().position(|x| **x == dep) {
                Some(t) if t == me => self_dep = true,
                Some(t) => {
                    prereqs[me].insert(t);
                }
                None => unknown = true,
            }
        }
    }
    let mut done = vec![false; first.len()];
    let mut topo: Vec<usize> = Vec::new();
    while let Some(i) = (0..first.len()).find(|&i| !done[i] && prereqs[i].iter().all(|&p| done[p])) {
        done[i] = true;
        topo.push(i);
    }
    let should_be_valid = !has_dup && !unknown && !self_dep && topo.len() == first.len();

    let steps: Vec<ExecStep> = specs.iter().map(|(s, id)| build(s, id.clone())).collect();
    let plan = match Plan::new(steps) {
        Ok(plan) => {
            assert!(should_be_valid, "accepted an invalid plan: {input:?}");
            plan
        }
        Err(err) => {
            assert!(!should_be_valid, "rejected a valid plan: {err}");
            assert!(!err.violations().is_empty());
            assert!(!err.to_string().is_empty());
            let found_dup = err.violations().iter().any(|v| matches!(v, PlanViolation::DuplicateStepId { .. }));
            assert_eq!(found_dup, has_dup);
            return;
        }
    };

    // The dry run is the topological order, ties by declaration.
    let order: Vec<&StepId> = plan.execution_order();
    let want: Vec<&StepId> = topo.iter().map(|&i| first[i]).collect();
    assert_eq!(order, want);
    assert_eq!(plan.len(), specs.len());
    assert_eq!(plan.steps().len(), specs.len());
    let pos = |id: &StepId| order.iter().position(|x| *x == id).expect("in order");
    for step in plan.steps() {
        for dep in step.prerequisites() {
            assert!(pos(dep) < pos(step.id()), "{dep} must come before {}", step.id());
        }
        assert!(plan.step(step.id()).is_some());
    }

    // The fingerprint is structural: declaring the same steps backwards gives the same one.
    let reversed: Vec<ExecStep> = specs.iter().rev().map(|(s, id)| build(s, id.clone())).collect();
    let plan_rev = Plan::new(reversed).expect("same steps, same validity");
    assert_eq!(plan.fingerprint(), plan_rev.fingerprint());
    assert_eq!(plan.fingerprint().to_string().parse(), Ok(plan.fingerprint()));

    // Running instant steps: every one succeeds in dependency order, whatever the (huge or zero) limits.
    let clean: Vec<ExecStep> = specs
        .iter()
        .map(|(s, id)| {
            let mut step = ExecStep::new(id.clone(), s.name.clone(), step_fn(|_ctx| async { Ok(()) }));
            for dep in s.deps.iter().filter_map(Id::get) {
                step = step.depends_on(dep);
            }
            step
        })
        .collect();
    let plan = Plan::new(clean).expect("same graph");
    let mut opts = ExecOptions::new()
        .retry(RetryConfig::new(0, Duration::ZERO, Duration::from_millis(1), 0.0))
        .attempt_timeout(input.attempt_timeout.get().max(Duration::from_secs(1)))
        .cancel_grace(input.cancel_grace.get())
        .group_deadline(input.group.clone(), input.group_window.get());
    if let Some(d) = &input.step_deadline {
        opts = opts.step_deadline(d.get().max(Duration::from_secs(1)));
    }
    if let Some(n) = input.parallel {
        if let Some(cap) = NonZeroUsize::new(usize::from(n)) {
            opts = opts.mode(Mode::Parallel { max_concurrency: cap });
        }
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
    let record = rt.block_on(execute(&plan, RunId::try_new("fuzz").expect("valid"), &opts));
    assert_eq!(record.steps().len(), plan.len());
    assert_eq!(record.outcome(), RunOutcome::Succeeded, "{record:?}");
    let finished = record.completion_order();
    assert_eq!(finished.len(), plan.len());
    let at = |id: &StepId| finished.iter().position(|x| x == id).expect("completed");
    for step in plan.steps() {
        for dep in step.prerequisites() {
            assert!(at(dep) < at(step.id()), "{} finished before its prerequisite {dep}", step.id());
        }
    }
});

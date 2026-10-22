//! Step definitions (`ExecStep`) and the validated, immutable `Plan`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use michi_core::resilience::RetryConfig;

use crate::{Step, StepId};

/// One step's definition: identity, display name, prerequisites, runner and optional per-step overrides.
pub struct ExecStep {
    id: StepId,
    name: String,
    depends_on: Vec<StepId>,
    runner: Arc<dyn Step>,
    breaker_key: Option<String>,
    retry: Option<RetryConfig>,
    attempt_timeout: Option<Duration>,
    deadline: Option<Duration>,
}

impl ExecStep {
    /// A step with no prerequisites. `name` is free-form display text and may repeat across steps.
    pub fn new(id: StepId, name: impl Into<String>, runner: impl Step + 'static) -> Self {
        Self::from_arc(id, name, Arc::new(runner))
    }

    /// Like [`ExecStep::new`], for a runner that is already shared.
    pub fn from_arc(id: StepId, name: impl Into<String>, runner: Arc<dyn Step>) -> Self {
        Self {
            id,
            name: name.into(),
            depends_on: Vec::new(),
            runner,
            breaker_key: None,
            retry: None,
            attempt_timeout: None,
            deadline: None,
        }
    }

    /// Adds a prerequisite: this step starts only after `id` has succeeded. Repeatable; repeats collapse.
    #[must_use]
    pub fn depends_on(mut self, id: StepId) -> Self {
        self.depends_on.push(id);
        self
    }

    /// The circuit-breaker key, typically one per downstream service. Defaults to the step id.
    #[must_use]
    pub fn breaker_key(mut self, key: impl Into<String>) -> Self {
        self.breaker_key = Some(key.into());
        self
    }

    /// Overrides the run's retry policy for this step.
    #[must_use]
    pub fn retry(mut self, retry: RetryConfig) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Overrides the run's per-attempt timeout for this step. Zero is floored to 1 ms.
    #[must_use]
    pub fn attempt_timeout(mut self, timeout: Duration) -> Self {
        self.attempt_timeout = Some(timeout);
        self
    }

    /// Bounds this step's total time across all attempts and backoff, overriding the run's step deadline.
    #[must_use]
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// The step's id.
    #[must_use]
    pub fn id(&self) -> &StepId {
        &self.id
    }

    /// The step's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The prerequisites as declared.
    #[must_use]
    pub fn prerequisites(&self) -> &[StepId] {
        &self.depends_on
    }

    /// The breaker key this step uses: the explicit one, or the step id.
    #[must_use]
    pub fn effective_breaker_key(&self) -> &str {
        self.breaker_key.as_deref().unwrap_or(self.id.as_str())
    }

    pub(crate) fn runner(&self) -> &Arc<dyn Step> {
        &self.runner
    }

    pub(crate) fn retry_override(&self) -> Option<&RetryConfig> {
        self.retry.as_ref()
    }

    pub(crate) fn attempt_timeout_override(&self) -> Option<Duration> {
        self.attempt_timeout
    }

    pub(crate) fn deadline_override(&self) -> Option<Duration> {
        self.deadline
    }
}

impl fmt::Debug for ExecStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecStep")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("depends_on", &self.depends_on)
            .field("breaker_key", &self.breaker_key)
            .finish_non_exhaustive()
    }
}

/// One problem found by [`Plan::new`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanViolation {
    /// A later step reuses an earlier step's id.
    DuplicateStepId {
        /// The repeated id.
        step_id: StepId,
    },
    /// A step depends on an id no step has.
    UnknownDependency {
        /// The step declaring the dependency.
        step_id: StepId,
        /// The missing id.
        dependency: StepId,
    },
    /// A step depends on itself.
    SelfDependency {
        /// The step.
        step_id: StepId,
    },
    /// These steps lie on one or more dependency cycles, in declaration order.
    Cycle {
        /// The steps on a cycle.
        step_ids: Vec<StepId>,
    },
}

impl fmt::Display for PlanViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateStepId { step_id } => write!(f, "duplicate step id {step_id}"),
            Self::UnknownDependency { step_id, dependency } => {
                write!(f, "step {step_id} depends on unknown step id {dependency}")
            }
            Self::SelfDependency { step_id } => write!(f, "step {step_id} depends on itself"),
            Self::Cycle { step_ids } => {
                f.write_str("dependency cycle through steps ")?;
                for (i, id) in step_ids.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(id.as_str())?;
                }
                Ok(())
            }
        }
    }
}

/// Every violation [`Plan::new`] found. Returned before anything runs, so an invalid plan is never confused with a
/// run that happened and failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    violations: Vec<PlanViolation>,
}

impl PlanError {
    /// The violations, duplicates first, then per-step dependency problems in declaration order, then cycles.
    #[must_use]
    pub fn violations(&self) -> &[PlanViolation] {
        &self.violations
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid plan: ")?;
        for (i, v) in self.violations.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{v}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PlanError {}

impl From<PlanError> for michi_core::DomainError {
    fn from(err: PlanError) -> Self {
        michi_core::DomainError::new(michi_core::ErrorCode::InvalidInput, err.to_string()).retryable(false)
    }
}

/// A stable hash of a plan's structure: each step's id and prerequisite ids, independent of declaration order.
/// Names, runners and options are not part of it. Resume refuses a record whose fingerprint differs from the plan's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
pub struct PlanFingerprint(u64);

impl fmt::Display for PlanFingerprint {
    /// 16 lowercase hex digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Why a string is not a [`PlanFingerprint`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a plan fingerprint is 16 hex digits, got {0:?}")]
pub struct ParseFingerprintError(String);

impl std::str::FromStr for PlanFingerprint {
    type Err = ParseFingerprintError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 {
            return Err(ParseFingerprintError(s.to_string()));
        }
        u64::from_str_radix(s, 16).map(Self).map_err(|_| ParseFingerprintError(s.to_string()))
    }
}

impl TryFrom<String> for PlanFingerprint {
    type Error = ParseFingerprintError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<PlanFingerprint> for String {
    fn from(fp: PlanFingerprint) -> String {
        fp.to_string()
    }
}

/// FNV-1a, 64-bit.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A validated, immutable set of steps. Build one with [`Plan::new`]; run it any number of times.
#[derive(Debug)]
pub struct Plan {
    steps: Vec<ExecStep>,
    prerequisites: Vec<Vec<usize>>,
    dependents: Vec<Vec<usize>>,
    order: Vec<usize>,
    index: BTreeMap<StepId, usize>,
    fingerprint: PlanFingerprint,
}

impl Plan {
    /// Validates `steps` and returns the plan, or every violation at once: duplicate ids, unknown dependencies,
    /// self-dependencies and cycles (found with an iterative Kahn pass, so depth is bounded by memory).
    pub fn new(steps: Vec<ExecStep>) -> Result<Self, PlanError> {
        let n = steps.len();
        let mut violations = Vec::new();
        let mut index: BTreeMap<StepId, usize> = BTreeMap::new();
        let mut duplicate = vec![false; n];
        for (i, step) in steps.iter().enumerate() {
            if index.contains_key(&step.id) {
                violations.push(PlanViolation::DuplicateStepId { step_id: step.id.clone() });
                duplicate[i] = true;
            } else {
                index.insert(step.id.clone(), i);
            }
        }

        let mut prerequisites: Vec<Vec<usize>> = vec![Vec::new(); n];
        // `added_by[j] == i + 1` once step `i` has listed `j`: an O(1) repeat check instead of scanning its
        // prerequisites so far, which made a wide fan-in O(deg²).
        let mut added_by: Vec<usize> = vec![0; n];
        for (i, step) in steps.iter().enumerate() {
            if duplicate[i] {
                continue;
            }
            let mut reported_self = false;
            for dep in &step.depends_on {
                if *dep == step.id {
                    if !reported_self {
                        violations.push(PlanViolation::SelfDependency { step_id: step.id.clone() });
                        reported_self = true;
                    }
                    continue;
                }
                match index.get(dep) {
                    Some(&j) if added_by[j] != i + 1 => {
                        added_by[j] = i + 1;
                        prerequisites[i].push(j);
                    }
                    Some(_) => {}
                    None => violations
                        .push(PlanViolation::UnknownDependency { step_id: step.id.clone(), dependency: dep.clone() }),
                }
            }
        }
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, prereqs) in prerequisites.iter().enumerate() {
            for &p in prereqs {
                dependents[p].push(i);
            }
        }

        // Kahn with the lowest declaration index first: the order sequential mode dispatches in.
        let mut indegree: Vec<usize> = prerequisites.iter().map(Vec::len).collect();
        let mut ready: BTreeSet<usize> = (0..n).filter(|&i| !duplicate[i] && indegree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = ready.pop_first() {
            order.push(i);
            for &d in &dependents[i] {
                indegree[d] -= 1;
                if indegree[d] == 0 {
                    ready.insert(d);
                }
            }
        }
        let live = n - duplicate.iter().filter(|d| **d).count();
        if order.len() < live {
            violations.push(PlanViolation::Cycle { step_ids: cycle_members(&steps, &duplicate, &order, &dependents) });
        }

        if !violations.is_empty() {
            return Err(PlanError { violations });
        }
        let fingerprint = fingerprint(&steps, &index, &prerequisites);
        Ok(Self { steps, prerequisites, dependents, order, index, fingerprint })
    }

    /// The dry run: step ids in the order sequential mode dispatches them, which is topological order with ties
    /// broken by declaration order. Nothing runs.
    #[must_use]
    pub fn execution_order(&self) -> Vec<&StepId> {
        self.order.iter().map(|&i| &self.steps[i].id).collect()
    }

    /// The plan's structural fingerprint.
    #[must_use]
    pub fn fingerprint(&self) -> PlanFingerprint {
        self.fingerprint
    }

    /// The steps, in declaration order.
    #[must_use]
    pub fn steps(&self) -> &[ExecStep] {
        &self.steps
    }

    /// The step with `id`, if any.
    #[must_use]
    pub fn step(&self, id: &StepId) -> Option<&ExecStep> {
        self.index.get(id).map(|&i| &self.steps[i])
    }

    /// Number of steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the plan has no steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub(crate) fn index_of(&self, id: &StepId) -> Option<usize> {
        self.index.get(id).copied()
    }

    pub(crate) fn prerequisites_of(&self, i: usize) -> &[usize] {
        &self.prerequisites[i]
    }

    pub(crate) fn dependents_of(&self, i: usize) -> &[usize] {
        &self.dependents[i]
    }

    pub(crate) fn topological_order(&self) -> &[usize] {
        &self.order
    }
}

/// The steps Kahn left over that lie on a cycle: peel off, in reverse, every leftover step none of whose dependents
/// is left over (it only hangs off a cycle), until none remain to peel.
fn cycle_members(steps: &[ExecStep], duplicate: &[bool], order: &[usize], dependents: &[Vec<usize>]) -> Vec<StepId> {
    let n = steps.len();
    let mut left = vec![false; n];
    for i in 0..n {
        left[i] = !duplicate[i];
    }
    for &i in order {
        left[i] = false;
    }
    let mut outdegree: Vec<usize> =
        (0..n).map(|i| if left[i] { dependents[i].iter().filter(|&&d| left[d]).count() } else { 0 }).collect();
    let mut prereqs_left: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        if left[i] {
            for &d in &dependents[i] {
                if left[d] {
                    prereqs_left[d].push(i);
                }
            }
        }
    }
    let mut peel: Vec<usize> = (0..n).filter(|&i| left[i] && outdegree[i] == 0).collect();
    while let Some(i) = peel.pop() {
        left[i] = false;
        for &p in &prereqs_left[i] {
            if left[p] {
                outdegree[p] -= 1;
                if outdegree[p] == 0 {
                    peel.push(p);
                }
            }
        }
    }
    (0..n).filter(|&i| left[i]).map(|i| steps[i].id.clone()).collect()
}

/// FNV-1a over each step's id and prerequisite ids, both sorted by id.
fn fingerprint(steps: &[ExecStep], index: &BTreeMap<StepId, usize>, prerequisites: &[Vec<usize>]) -> PlanFingerprint {
    let mut h = Fnv::new();
    for (id, &i) in index {
        h.write(id.as_str().as_bytes());
        h.write(&[0x1f]);
        let mut deps: Vec<&str> = prerequisites[i].iter().map(|&p| steps[p].id.as_str()).collect();
        deps.sort_unstable();
        for dep in deps {
            h.write(dep.as_bytes());
            h.write(&[0x1e]);
        }
        h.write(&[0x1d]);
    }
    PlanFingerprint(h.0)
}

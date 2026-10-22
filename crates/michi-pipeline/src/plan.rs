//! Step definitions (`ExecStep`) and the validated, immutable `Plan`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use michi_core::resilience::RetryConfig;
use michi_graph::{Dag, GraphBuilder, NodeIx, Order, UnknownTargets, Violation};

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

    /// Bounds this step's total time across all attempts and backoff, overriding the run's step deadline. A deadline
    /// too far away for the clock to represent (such as `Duration::MAX`) means no deadline.
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
    /// More steps than a plan can index (`u32::MAX - 1`).
    TooManySteps,
    /// A graph problem this version of michi-pipeline does not know by name, rendered by michi-graph.
    Other {
        /// The problem, as michi-graph describes it.
        message: String,
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
            Self::TooManySteps => write!(f, "more than {} steps", u32::MAX - 1),
            Self::Other { message } => f.write_str(message),
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
#[error("a plan fingerprint is 16 lowercase hex digits, got {0:?}")]
pub struct ParseFingerprintError(String);

impl std::str::FromStr for PlanFingerprint {
    type Err = ParseFingerprintError;
    /// Accepts exactly what `Display` writes: 16 lowercase hex digits. `u64::from_str_radix` alone would also take a
    /// leading `+` and uppercase digits, which do not round-trip.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
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
pub(crate) struct Fnv(u64);

impl Fnv {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    pub(crate) fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    pub(crate) fn finish(&self) -> u64 {
        self.0
    }
}

/// A validated, immutable set of steps. Build one with [`Plan::new`]; run it any number of times.
#[derive(Debug)]
pub struct Plan {
    steps: Vec<ExecStep>,
    /// Node indices are declaration positions, so they index `steps` directly.
    dag: Dag<StepId>,
    fingerprint: PlanFingerprint,
}

impl Plan {
    /// Validates `steps` and returns the plan, or every violation at once: duplicate ids, unknown dependencies,
    /// self-dependencies and cycles (found without recursion, so depth is bounded by memory).
    ///
    /// # Errors
    ///
    /// A [`PlanError`] listing every [`PlanViolation`] when the steps do not form a valid graph.
    pub fn new(steps: Vec<ExecStep>) -> Result<Self, PlanError> {
        let mut b = GraphBuilder::<StepId>::with_capacity(steps.len());
        for step in &steps {
            let node = b.node(step.id.clone());
            for dep in &step.depends_on {
                b.depends_on(node, dep.clone(), ());
            }
        }
        let dag = b
            .build(UnknownTargets::Error)
            .validate(Order::Declaration)
            .map_err(|violations| plan_error(&steps, violations))?;
        let fingerprint = fingerprint(&dag);
        Ok(Self { steps, dag, fingerprint })
    }

    /// The dry run: step ids in the order sequential mode dispatches them, which is topological order with ties
    /// broken by declaration order. Nothing runs.
    #[must_use]
    pub fn execution_order(&self) -> Vec<&StepId> {
        self.dag.order().iter().filter_map(|ix| self.steps.get(ix.index())).map(|s| &s.id).collect()
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
        self.dag.index_of(id).and_then(|ix| self.steps.get(ix.index()))
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
        self.dag.index_of(id).map(NodeIx::index)
    }

    pub(crate) fn prerequisites_of(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.node(i).map(|ix| self.dag.prerequisites(ix)).unwrap_or_default().iter().map(|p| p.index())
    }

    pub(crate) fn dependents_of(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.node(i).map(|ix| self.dag.dependents(ix)).unwrap_or_default().iter().map(|d| d.index())
    }

    pub(crate) fn topological_order(&self) -> impl Iterator<Item = usize> + '_ {
        self.dag.order().iter().map(|ix| ix.index())
    }

    fn node(&self, i: usize) -> Option<NodeIx> {
        self.dag.node_at(i)
    }
}

/// Maps michi-graph's violations onto michi's, keeping michi's shape: one `Cycle` listing every step on any cycle,
/// in declaration order.
fn plan_error(steps: &[ExecStep], violations: michi_graph::Violations<StepId>) -> PlanError {
    let mut out = Vec::new();
    let mut on_cycle: Vec<StepId> = Vec::new();
    for v in violations {
        match v {
            Violation::DuplicateNode { key } => out.push(PlanViolation::DuplicateStepId { step_id: key }),
            Violation::UnknownDependency { dependent, prerequisite } => {
                out.push(PlanViolation::UnknownDependency { step_id: dependent, dependency: prerequisite });
            }
            Violation::SelfDependency { key } => out.push(PlanViolation::SelfDependency { step_id: key }),
            Violation::Cycle(cycle) => on_cycle.extend(cycle.members().iter().cloned()),
            Violation::TooManyNodes => out.push(PlanViolation::TooManySteps),
            // michi-graph's violations are non-exhaustive: name a new one by its own message, never as another kind.
            other => out.push(PlanViolation::Other { message: other.to_string() }),
        }
    }
    if !on_cycle.is_empty() {
        let mut position: BTreeMap<&StepId, usize> = BTreeMap::new();
        for (i, step) in steps.iter().enumerate() {
            position.entry(&step.id).or_insert(i);
        }
        on_cycle.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
        out.push(PlanViolation::Cycle { step_ids: on_cycle });
    }
    PlanError { violations: out }
}

/// FNV-1a over each step's id and prerequisite ids, both sorted by id.
fn fingerprint(dag: &Dag<StepId>) -> PlanFingerprint {
    let mut h = Fnv::new();
    for (id, deps) in dag.canonical() {
        h.write(id.as_str().as_bytes());
        h.write(&[0x1f]);
        for dep in deps {
            h.write(dep.as_str().as_bytes());
            h.write(&[0x1e]);
        }
        h.write(&[0x1d]);
    }
    PlanFingerprint(h.0)
}

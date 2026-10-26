//! Keyed circuit breakers: one state machine per downstream key, shared across runs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::error::millis;
use crate::ExecutionError;

/// A breaker's current phase.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerPhase {
    /// Calls run normally.
    Closed,
    /// Calls are short-circuited without invoking the step.
    Open,
    /// One probe call may run to test recovery.
    HalfOpen,
}

/// When a breaker opens and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerConfig {
    failure_threshold: u32,
    open_duration: Duration,
}

impl BreakerConfig {
    /// Normalizing constructor: a `failure_threshold` of 0 is floored to 1 and a zero `open_duration` to 1 ms.
    #[must_use]
    pub fn new(failure_threshold: u32, open_duration: Duration) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            open_duration: if open_duration.is_zero() { Duration::from_millis(1) } else { open_duration },
        }
    }

    /// Consecutive call-level failures that open the breaker.
    #[must_use]
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }

    /// How long the breaker stays open before admitting a probe.
    #[must_use]
    pub fn open_duration(&self) -> Duration {
        self.open_duration
    }
}

impl Default for BreakerConfig {
    /// Opens after 5 consecutive failures, for 30 seconds.
    fn default() -> Self {
        Self::new(5, Duration::from_secs(30))
    }
}

/// The stored phase. HalfOpen is not stored on its own: an `Open` whose `open_duration` has elapsed is reported as
/// HalfOpen with no probe claimed yet, and `Probing` is HalfOpen with the single probe in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredPhase {
    Closed,
    Open { opened_at_ms: u64 },
    Probing { opened_at_ms: u64 },
}

/// Every piece of mutable breaker state, guarded together by one mutex so phase, probe ownership, the failure count
/// and every transition form a single state machine. `generation` increments on every phase transition; each
/// admitted call remembers the generation it was admitted in, and its outcome is applied only if the breaker is still
/// in that generation.
#[derive(Debug)]
struct BreakerState {
    phase: StoredPhase,
    consecutive_failures: u32,
    generation: u64,
}

/// How a call was admitted by [`CircuitBreaker::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// A normal call admitted while Closed, in `generation`.
    Closed { generation: u64 },
    /// The single HalfOpen probe, which moved the breaker into `generation`.
    Probe { generation: u64 },
}

/// A consecutive-failure circuit breaker. All mutable state lives behind one `std::sync::Mutex` that is never held
/// across an `.await`, so [`CircuitBreaker::phase`] is synchronous and every admission decision and transition is
/// atomic with respect to the others. An outcome from a call admitted before the breaker last changed phase is
/// ignored: it cannot reopen a recovered breaker, extend the open window or reset the failure count.
///
/// The executor admits a step once, retries it within that one admission, and reports one outcome. A HalfOpen probe
/// gets exactly one attempt. Get breakers from a [`BreakerRegistry`].
#[derive(Debug)]
pub(crate) struct CircuitBreaker {
    config: BreakerConfig,
    state: Mutex<BreakerState>,
    epoch: tokio::time::Instant,
}

impl CircuitBreaker {
    /// A closed breaker.
    #[must_use]
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            state: Mutex::new(BreakerState { phase: StoredPhase::Closed, consecutive_failures: 0, generation: 0 }),
            epoch: tokio::time::Instant::now(),
        }
    }

    /// Current phase. Once `open_duration` of Tokio time has elapsed since the breaker opened, this reports
    /// `HalfOpen` even before a caller has claimed the probe.
    #[must_use]
    pub fn phase(&self) -> BreakerPhase {
        let now_ms = self.now_ms();
        let state = self.lock_state();
        match state.phase {
            StoredPhase::Closed => BreakerPhase::Closed,
            StoredPhase::Open { opened_at_ms } => {
                if now_ms.saturating_sub(opened_at_ms) >= self.open_duration_ms() {
                    BreakerPhase::HalfOpen
                } else {
                    BreakerPhase::Open
                }
            }
            StoredPhase::Probing { .. } => BreakerPhase::HalfOpen,
        }
    }

    /// Locks the state. The lock is only ever held for a few non-panicking field updates, so a poisoned lock still
    /// holds consistent state and is recovered rather than propagated.
    fn lock_state(&self) -> MutexGuard<'_, BreakerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now_ms(&self) -> u64 {
        millis(self.epoch.elapsed())
    }

    fn open_duration_ms(&self) -> u64 {
        millis(self.config.open_duration)
    }

    /// Decides, atomically, whether a call may run and in which role.
    pub(crate) fn admit(&self) -> Result<Admission, ExecutionError> {
        let now_ms = self.now_ms();
        let open_duration_ms = self.open_duration_ms();
        let mut state = self.lock_state();
        match state.phase {
            StoredPhase::Closed => Ok(Admission::Closed { generation: state.generation }),
            StoredPhase::Open { opened_at_ms } => {
                let since_opened_ms = now_ms.saturating_sub(opened_at_ms);
                if since_opened_ms >= open_duration_ms {
                    state.phase = StoredPhase::Probing { opened_at_ms };
                    state.generation = state.generation.wrapping_add(1);
                    Ok(Admission::Probe { generation: state.generation })
                } else {
                    Err(ExecutionError::CircuitOpen { retry_after_ms: open_duration_ms - since_opened_ms })
                }
            }
            StoredPhase::Probing { .. } => Err(ExecutionError::CircuitOpen { retry_after_ms: open_duration_ms }),
        }
    }

    /// Applies an admitted call's outcome. An outcome from a generation the breaker has since left is stale and
    /// ignored.
    pub(crate) fn complete(&self, admission: Admission, succeeded: bool) {
        let now_ms = self.now_ms();
        let mut state = self.lock_state();
        match admission {
            Admission::Closed { generation } => {
                if state.generation != generation || state.phase != StoredPhase::Closed {
                    return;
                }
                if succeeded {
                    state.consecutive_failures = 0;
                } else {
                    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                    if state.consecutive_failures >= self.config.failure_threshold {
                        state.phase = StoredPhase::Open { opened_at_ms: now_ms };
                        state.generation = state.generation.wrapping_add(1);
                    }
                }
            }
            Admission::Probe { generation } => {
                if state.generation != generation || !matches!(state.phase, StoredPhase::Probing { .. }) {
                    return;
                }
                if succeeded {
                    state.phase = StoredPhase::Closed;
                    state.consecutive_failures = 0;
                } else {
                    // Reopens unconditionally, bypassing failure_threshold and leaving the counter untouched.
                    state.phase = StoredPhase::Open { opened_at_ms: now_ms };
                }
                state.generation = state.generation.wrapping_add(1);
            }
        }
    }

    /// Releases a probe that ended without an outcome (its step panicked, was cancelled or was dropped): the breaker
    /// returns to HalfOpen with no probe claimed, so the next caller may probe.
    pub(crate) fn abandon_probe(&self, generation: u64) {
        let mut state = self.lock_state();
        if state.generation != generation {
            return;
        }
        if let StoredPhase::Probing { opened_at_ms } = state.phase {
            state.phase = StoredPhase::Open { opened_at_ms };
            state.generation = state.generation.wrapping_add(1);
        }
    }
}

/// Holds an admission until its outcome is reported. Dropped without an outcome (panic, cancellation, an aborted
/// task), it releases a probe so the breaker is not stuck HalfOpen with a probe nobody will finish.
pub(crate) struct AdmissionGuard {
    breaker: Arc<CircuitBreaker>,
    admission: Admission,
    armed: bool,
}

impl AdmissionGuard {
    pub(crate) fn new(breaker: Arc<CircuitBreaker>, admission: Admission) -> Self {
        Self { breaker, admission, armed: true }
    }

    pub(crate) fn is_probe(&self) -> bool {
        matches!(self.admission, Admission::Probe { .. })
    }

    /// Reports the call's outcome.
    pub(crate) fn complete(mut self, succeeded: bool) {
        self.armed = false;
        self.breaker.complete(self.admission, succeeded);
    }
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Admission::Probe { generation } = self.admission {
                self.breaker.abandon_probe(generation);
            }
        }
    }
}

/// One breaker per key, created on first use with the registry's config. Share one `Arc<BreakerRegistry>` across runs
/// (via [`crate::ExecOptions::breakers`]) so failures accumulate per downstream rather than per run.
#[derive(Debug, Default)]
pub struct BreakerRegistry {
    config: BreakerConfig,
    breakers: Mutex<BTreeMap<String, Arc<CircuitBreaker>>>,
}

impl BreakerRegistry {
    /// An empty registry whose breakers use `config`.
    #[must_use]
    pub fn new(config: BreakerConfig) -> Self {
        Self { config, breakers: Mutex::new(BTreeMap::new()) }
    }

    /// The breaker for `key`, created closed if it does not exist yet.
    #[must_use]
    pub(crate) fn breaker(&self, key: &str) -> Arc<CircuitBreaker> {
        let mut breakers = self.breakers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(b) = breakers.get(key) {
            return Arc::clone(b);
        }
        let b = Arc::new(CircuitBreaker::new(self.config));
        breakers.insert(key.to_string(), Arc::clone(&b));
        b
    }

    /// The phase of `key`'s breaker, or `None` if no step has used that key yet.
    #[must_use]
    pub fn phase(&self, key: &str) -> Option<BreakerPhase> {
        let breakers = self.breakers.lock().unwrap_or_else(PoisonError::into_inner);
        breakers.get(key).map(|b| b.phase())
    }

    /// The keys with a breaker, sorted.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        let breakers = self.breakers.lock().unwrap_or_else(PoisonError::into_inner);
        breakers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker(threshold: u32, open_secs: u64) -> CircuitBreaker {
        CircuitBreaker::new(BreakerConfig::new(threshold, Duration::from_secs(open_secs)))
    }

    fn fail_once(b: &CircuitBreaker) {
        let a = b.admit().expect("admitted");
        b.complete(a, false);
    }

    /// Forces the breaker Open at the current instant, so advancing by `open_duration` makes it HalfOpen.
    fn force_open(b: &CircuitBreaker) {
        let now_ms = b.now_ms();
        let mut s = b.lock_state();
        s.phase = StoredPhase::Open { opened_at_ms: now_ms };
        s.generation = s.generation.wrapping_add(1);
    }

    #[test]
    fn config_normalizes_zero_threshold_and_duration() {
        let c = BreakerConfig::new(0, Duration::ZERO);
        assert_eq!(c.failure_threshold(), 1);
        assert_eq!(c.open_duration(), Duration::from_millis(1));
    }

    #[tokio::test(start_paused = true)]
    async fn threshold_consecutive_failures_open_and_success_resets_the_count() {
        let b = breaker(3, 10);
        fail_once(&b);
        fail_once(&b);
        let ok = b.admit().unwrap();
        b.complete(ok, true);
        fail_once(&b);
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Closed, "four non-consecutive failures never reach three");
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn open_breaker_short_circuits_with_the_exact_remaining_wait_then_goes_half_open() {
        let b = breaker(1, 10);
        fail_once(&b);
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(b.admit(), Err(ExecutionError::CircuitOpen { retry_after_ms: 6_000 }));
        tokio::time::advance(Duration::from_secs(6)).await;
        assert_eq!(b.phase(), BreakerPhase::HalfOpen);
    }

    #[tokio::test(start_paused = true)]
    async fn half_open_admits_one_probe_whose_success_closes_and_failure_reopens() {
        let b = breaker(5, 10);
        force_open(&b);
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = b.admit().unwrap();
        assert!(matches!(probe, Admission::Probe { .. }));
        assert!(matches!(b.admit(), Err(ExecutionError::CircuitOpen { retry_after_ms: 10_000 })), "single flight");
        assert_eq!(b.phase(), BreakerPhase::HalfOpen, "HalfOpen while the probe is claimed");
        b.complete(probe, false);
        assert_eq!(b.phase(), BreakerPhase::Open, "a failed probe reopens regardless of the threshold");
        assert_eq!(b.lock_state().consecutive_failures, 0, "and leaves the counter alone");
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = b.admit().unwrap();
        b.complete(probe, true);
        assert_eq!(b.phase(), BreakerPhase::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_a_stale_half_open_decision_cannot_reopen_a_recovered_breaker() {
        let b = breaker(100, 10);
        force_open(&b);
        tokio::time::advance(Duration::from_secs(10)).await;
        let a = b.admit().expect("first HalfOpen caller claims the probe");
        assert!(matches!(b.admit(), Err(ExecutionError::CircuitOpen { retry_after_ms: 10_000 })));
        b.complete(a, true);
        assert_eq!(b.phase(), BreakerPhase::Closed);
        let c = b.admit().expect("Closed breaker admits");
        assert!(matches!(c, Admission::Closed { .. }), "no second probe after recovery, got {c:?}");
        b.complete(c, false);
        assert_eq!(b.lock_state().consecutive_failures, 1);
        b.complete(a, false);
        assert_eq!(b.phase(), BreakerPhase::Closed, "stale probe failure must be ignored");
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_stale_closed_outcomes_neither_extend_the_open_window_nor_reset_the_count() {
        let b = breaker(2, 10);
        let c1 = b.admit().unwrap();
        let c2 = b.admit().unwrap();
        let c3 = b.admit().unwrap();
        let c4 = b.admit().unwrap();
        b.complete(c1, false);
        b.complete(c2, false);
        assert_eq!(b.phase(), BreakerPhase::Open);
        tokio::time::advance(Duration::from_secs(4)).await;
        b.complete(c3, false);
        b.complete(c4, true);
        assert_eq!(b.admit(), Err(ExecutionError::CircuitOpen { retry_after_ms: 6_000 }), "opened_at not re-stamped");
        assert_eq!(b.lock_state().consecutive_failures, 2, "a stale success must not reset the count");
    }

    #[tokio::test(start_paused = true)]
    async fn state_machine_abandoned_probe_only_releases_its_own_generation() {
        let b = breaker(1, 10);
        force_open(&b);
        tokio::time::advance(Duration::from_secs(10)).await;
        let Ok(Admission::Probe { generation: g1 }) = b.admit() else { panic!("expected a probe admission") };
        b.abandon_probe(g1);
        assert_eq!(b.phase(), BreakerPhase::HalfOpen, "an abandoned probe frees the slot");
        let p2 = b.admit().unwrap();
        b.abandon_probe(g1);
        assert!(matches!(b.admit(), Err(ExecutionError::CircuitOpen { .. })), "a stale release frees nothing");
        b.complete(p2, true);
        assert_eq!(b.phase(), BreakerPhase::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_successful_probe_resets_the_failure_count() {
        // Threshold 2, so one failure after recovery only stays Closed if the count really went back to 0.
        let b = breaker(2, 10);
        fail_once(&b);
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Open);
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = b.admit().unwrap();
        b.complete(probe, true);
        assert_eq!(b.phase(), BreakerPhase::Closed);
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Closed, "one post-recovery failure must not reach threshold 2");
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Open, "the count still counts from 0");
    }

    #[tokio::test(start_paused = true)]
    async fn phase_reports_half_open_while_the_probe_is_claimed_regardless_of_elapsed_time() {
        let b = breaker(1, 10);
        let now_ms = b.now_ms();
        b.lock_state().phase = StoredPhase::Probing { opened_at_ms: now_ms };
        assert_eq!(b.phase(), BreakerPhase::HalfOpen, "an unclaimed Open this recent would still report Open");
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_closed_failure_cannot_reopen_a_breaker_that_recovered() {
        let b = breaker(1, 10);
        let stale = b.admit().unwrap();
        fail_once(&b);
        assert_eq!(b.phase(), BreakerPhase::Open);
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = b.admit().unwrap();
        b.complete(probe, true);
        assert_eq!(b.phase(), BreakerPhase::Closed);
        b.complete(stale, false);
        assert_eq!(b.phase(), BreakerPhase::Closed, "a call admitted before the breaker opened is stale");
        assert_eq!(b.lock_state().consecutive_failures, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn dropped_guard_releases_a_probe_but_not_a_closed_admission() {
        let b = Arc::new(breaker(1, 10));
        let closed = AdmissionGuard::new(Arc::clone(&b), b.admit().unwrap());
        drop(closed);
        assert_eq!(b.phase(), BreakerPhase::Closed, "a dropped closed call reports nothing");
        force_open(&b);
        tokio::time::advance(Duration::from_secs(10)).await;
        let probe = AdmissionGuard::new(Arc::clone(&b), b.admit().unwrap());
        assert!(probe.is_probe());
        drop(probe);
        assert!(matches!(b.admit(), Ok(Admission::Probe { .. })), "the next caller may probe");
    }

    /// Bounded stress test (multi-thread runtime, about 1 s wall clock) for the HalfOpen race: a caller whose
    /// HalfOpen decision was taken before a concurrent probe succeeded must not run a second probe and reopen the
    /// recovered breaker.
    #[test]
    fn half_open_race_stress_recovered_breaker_is_never_reopened_by_a_stale_half_open_decision() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_time().build().unwrap();
        rt.block_on(async {
            let b = Arc::new(CircuitBreaker::new(BreakerConfig::new(100, Duration::from_millis(1))));
            let deadline = std::time::Instant::now() + Duration::from_millis(1000);
            let mut trial = 0u32;
            while std::time::Instant::now() < deadline {
                trial += 1;
                force_open(&b);
                std::thread::sleep(Duration::from_millis(2));
                let barrier = Arc::new(tokio::sync::Barrier::new(9));
                let mut tasks = Vec::new();
                for i in 0..9 {
                    let b = Arc::clone(&b);
                    let barrier = Arc::clone(&barrier);
                    tasks.push(tokio::spawn(async move {
                        barrier.wait().await;
                        match b.admit() {
                            Ok(a) => {
                                b.complete(a, i == 0);
                                Some(i == 0)
                            }
                            Err(_) => None,
                        }
                    }));
                }
                let mut probe_succeeded = false;
                for t in tasks {
                    if t.await.unwrap() == Some(true) {
                        probe_succeeded = true;
                    }
                }
                if probe_succeeded {
                    assert_ne!(b.phase(), BreakerPhase::Open, "trial {trial}: a stale decision reopened the breaker");
                }
            }
        });
    }

    #[tokio::test(start_paused = true)]
    async fn registry_keys_breakers_independently_and_reuses_them() {
        let reg = BreakerRegistry::new(BreakerConfig::new(1, Duration::from_secs(10)));
        assert_eq!(reg.phase("github"), None);
        fail_once(&reg.breaker("github"));
        assert_eq!(reg.phase("github"), Some(BreakerPhase::Open));
        assert_eq!(reg.breaker("slack").phase(), BreakerPhase::Closed, "one key's failures never trip another");
        assert!(Arc::ptr_eq(&reg.breaker("github"), &reg.breaker("github")));
        assert_eq!(reg.keys(), vec!["github".to_string(), "slack".to_string()]);
    }
}

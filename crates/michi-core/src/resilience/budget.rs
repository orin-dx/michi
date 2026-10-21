//! Retry budget: limits retries to a fraction of successful work.

/// Milli-retries per retry: the budget keeps integer thousandths so repeated deposits never drift.
const MILLI: u64 = 1000;

/// A retry budget modelled on tower's `TpsBudget` and the SRE book's "retry ratio below 10%": each success
/// deposits `ratio` of a retry, each retry withdraws one, and the balance never exceeds `max`.
///
/// Pure and synchronous: callers that share one across tasks wrap it in a mutex. There is no clock, so the reserve is
/// spent once rather than replenished per second; after that, only successes earn retries. First attempts are never
/// meant to be budgeted, only retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryBudget {
    deposit_milli: u64,
    max_milli: u64,
    balance_milli: u64,
}

impl RetryBudget {
    /// Normalizing constructor. `ratio` is the retries earned per success, clamped to `[0.0, 1000.0]` (NaN becomes
    /// `0.0`); `reserve` is the number of retries available before any success; `max` caps the balance and is floored
    /// to `reserve`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "ratio is clamped to [0, 1000] first")]
    pub fn new(ratio: f64, reserve: u32, max: u32) -> Self {
        let ratio = if ratio.is_finite() { ratio.clamp(0.0, 1000.0) } else { 0.0 };
        let deposit_milli = (ratio * MILLI as f64).round() as u64;
        let max_milli = u64::from(max.max(reserve)) * MILLI;
        Self { deposit_milli, max_milli, balance_milli: u64::from(reserve) * MILLI }
    }

    /// Records one success, earning `ratio` of a retry up to the cap.
    pub fn deposit(&mut self) {
        self.balance_milli = self.balance_milli.saturating_add(self.deposit_milli).min(self.max_milli);
    }

    /// Spends one retry if the balance covers it. Returns whether the retry may proceed.
    pub fn try_withdraw(&mut self) -> bool {
        if self.balance_milli >= MILLI {
            self.balance_milli -= MILLI;
            true
        } else {
            false
        }
    }

    /// Whole retries currently available.
    #[must_use]
    pub fn available(&self) -> u32 {
        u32::try_from(self.balance_milli / MILLI).unwrap_or(u32::MAX)
    }
}

impl Default for RetryBudget {
    /// 10% retry ratio, a reserve of 10 retries, capped at 100.
    fn default() -> Self {
        Self::new(0.1, 10, 100)
    }
}

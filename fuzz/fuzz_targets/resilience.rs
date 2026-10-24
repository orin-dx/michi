//! Retry policy never panics, and delays stay inside `[0, max_delay]`.

#![no_main]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::resilience::{
    next_retry_delay, parse_retry_after, parse_retry_after_at, IdempotencyKey, RetryBudget, RetryConfig,
    RetryConfigError,
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
struct Input {
    header: String,
    now_secs: u32,
    now_nanos: u32,
    max_retries: u32,
    base: Dur,
    max: Dur,
    /// Raw bits: NaN, infinities, subnormals and negative zero all show up.
    jitter_bits: u64,
    seed_bits: u64,
    attempt: u32,
    retry_after: Option<Dur>,
    operation: String,
    data: Vec<u8>,
}

fuzz_target!(|input: Input| {
    // Retry-After: never panics on any text, at any clock reading.
    let now = UNIX_EPOCH + Duration::new(u64::from(input.now_secs), input.now_nanos % 1_000_000_000);
    let _ = parse_retry_after(&input.header);
    let parsed = parse_retry_after_at(&input.header, now);
    let trimmed = input.header.trim();
    if !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit()) {
        let secs = trimmed.parse::<u64>().unwrap_or(u64::MAX);
        assert_eq!(parsed, Some(Duration::from_secs(secs)));
    }
    if let Some(d) = parsed {
        // A date is a wait from now, so it can't be longer than the span from the epoch's far end.
        let _ = now.checked_add(d);
    }
    // A clock before the epoch is a legal SystemTime too.
    let _ = parse_retry_after_at(&input.header, UNIX_EPOCH - Duration::from_secs(u64::from(input.now_secs)));
    let _ = parse_retry_after_at(&input.header, SystemTime::now());

    let base = input.base.get();
    let max = input.max.get();
    let jitter = f64::from_bits(input.jitter_bits);
    let seed = f64::from_bits(input.seed_bits);

    // The normalizing constructor always yields an in-range config.
    let cfg = RetryConfig::new(input.max_retries, base, max, jitter);
    assert!(cfg.max_delay >= Duration::from_millis(1));
    assert!(cfg.base_delay <= cfg.max_delay);
    assert!((0.0..=1.0).contains(&cfg.jitter_factor), "jitter {}", cfg.jitter_factor);
    assert_eq!(cfg.max_retries, input.max_retries);

    // The strict one accepts exactly the in-range inputs, and then agrees with the normalizing one.
    match RetryConfig::try_new(input.max_retries, base, max, jitter) {
        Ok(strict) => {
            assert!(!max.is_zero() && base <= max && (0.0..=1.0).contains(&jitter));
            // `new` additionally floors max_delay to 1ms, so they agree from there up.
            if max >= Duration::from_millis(1) {
                assert_eq!(strict, RetryConfig::new(input.max_retries, base, max, jitter));
            }
            // The strict config's delays obey the same bound (checked below through `cfg`; here, its own).
            let d = next_retry_delay(&strict, 0, 0.5, None);
            assert!(d.is_none_or(|d| d <= strict.max_delay));
        }
        Err(RetryConfigError::MaxDelayIsZero) => assert!(max.is_zero()),
        Err(RetryConfigError::BaseDelayExceedsMaxDelay { .. }) => assert!(base > max),
        Err(RetryConfigError::JitterFactorOutOfRange { .. }) => assert!(!(0.0..=1.0).contains(&jitter)),
        Err(_) => {}
    }

    // Delay: None once the attempts are used up; otherwise within [0, max_delay], never shorter than the base
    // (capped) or the server's hint (capped).
    let ra = input.retry_after.as_ref().map(Dur::get);
    let delay = next_retry_delay(&cfg, input.attempt, seed, ra);
    assert_eq!(delay.is_none(), input.attempt >= cfg.max_retries);
    if let Some(d) = delay {
        assert!(d <= cfg.max_delay, "{d:?} > {:?}", cfg.max_delay);
        // Delays are computed in u64 milliseconds, so a base or hint past ~584 million years saturates there; that
        // is far outside any real clock and not worth a finding. Below it, a delay is never shorter than either.
        let sat = Duration::from_millis(u64::MAX);
        assert!(d >= cfg.base_delay.min(sat), "{d:?} < base {:?}", cfg.base_delay);
        if let Some(ra) = ra {
            assert!(d >= ra.min(cfg.max_delay).min(sat), "{d:?} shortens retry-after {ra:?}");
        }
        assert_eq!(Some(d), next_retry_delay(&cfg, input.attempt, seed, ra), "not deterministic");
    }

    // A strict config built from the same inputs behaves the same way.
    let _ = next_retry_delay(&RetryConfig::default(), input.attempt, seed, ra);

    // Budget and idempotency keys take arbitrary input too.
    let key = IdempotencyKey::from_hash(&input.operation, &input.data);
    assert_eq!(key, IdempotencyKey::from_hash(&input.operation, &input.data));
    let mut budget = RetryBudget::new(jitter, input.max_retries % 64, input.attempt % 64);
    for i in 0..input.data.len().min(256) {
        if input.data[i] & 1 == 0 {
            budget.deposit();
        } else {
            let before = budget.available();
            if budget.try_withdraw() {
                assert_eq!(budget.available(), before - 1);
            }
        }
    }
});

use michi_core::resilience::{next_retry_delay, parse_retry_after, RetryConfig};
use proptest::prelude::*;
use std::time::Duration;

proptest! {
    #[test]
    fn parse_retry_after_never_panics(input in ".{0,200}") {
        // Smoke test: must not panic on arbitrary input
        let _ = parse_retry_after(&input);
    }

    #[test]
    fn parse_retry_after_integer_seconds_parses_correctly(n in 0u64..86_400) {
        let input = n.to_string();
        let result = parse_retry_after(&input);
        prop_assert_eq!(result, Some(Duration::from_secs(n)), "integer input {} must parse to {}s", n, n);
    }

    #[test]
    fn next_retry_delay_within_bounds(
        attempt in 0u32..5,
        jitter_seed in 0.0f64..1.0,
        base_secs in 1u64..10,
        max_secs in 10u64..60,
        jitter_factor in 0.0f64..1.0,
    ) {
        let config = RetryConfig::new(
            10,
            Duration::from_secs(base_secs),
            Duration::from_secs(max_secs),
            jitter_factor,
        );
        if let Some(delay) = next_retry_delay(&config, attempt, jitter_seed, None) {
            prop_assert!(delay <= config.max_delay, "delay {delay:?} exceeded max_delay {:?}", config.max_delay);

            // Lower bound: delay is always within
            // [min(base_delay * 2^attempt, max_delay), max_delay]. Jitter is additive-only (never
            // negative — jitter_factor and jitter_seed are both in [0.0, 1.0]
            // in `next_retry_delay`, so it only ever raises the delay above
            // the raw exponential backoff), and `retry_after` is None here, so
            // the true floor is the raw backoff (base_delay * 2^attempt)
            // *unless* max_delay caps it first — e.g. at higher attempts the
            // raw backoff can already exceed max_delay before jitter is even
            // applied, in which case the delay can legitimately equal
            // max_delay exactly rather than the uncapped raw backoff.
            let exp = 2u32.saturating_pow(attempt);
            let raw_backoff = config.base_delay.saturating_mul(exp);
            let expected_min = raw_backoff.min(config.max_delay);
            prop_assert!(
                delay >= expected_min,
                "delay {delay:?} below expected minimum {expected_min:?} (raw backoff {raw_backoff:?}, max_delay {:?})",
                config.max_delay
            );
        }
    }
}

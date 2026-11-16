//! The audit's field-bypass examples, rewritten against the API that remains. Before the fields went private these
//! tests set the field directly and failed: `max_delay = 0` after `try_new` gave a 0 ns retry against
//! `Retry-After: 60s`, `retryable = true` on a timeout kept the verify-first hint, and `code = Timeout` on a retryable
//! `RateLimited` error dropped it. The field writes themselves no longer compile; the `compile_fail,E0616` doctests on
//! `DomainError`, `RetryConfig` and `PartialSuccess` prove that.

use std::time::Duration;

use michi_core::{next_retry_delay, DomainError, ErrorClass, ErrorCode, RetryConfig, MAY_HAVE_TAKEN_EFFECT_HINT};

fn has_effect_hint(e: &DomainError) -> bool {
    e.hints().iter().any(|h| h.as_str() == MAY_HAVE_TAKEN_EFFECT_HINT)
}

// The only ways to get a zero cap are rejected or clamped, so a server's Retry-After is never cut to nothing.
#[test]
fn retry_after_survives_every_way_to_build_a_config() {
    assert!(RetryConfig::try_new(3, Duration::from_millis(100), Duration::ZERO, 0.0).is_err());
    let clamped = RetryConfig::new(3, Duration::from_millis(100), Duration::ZERO, 0.0);
    assert_eq!(clamped.max_delay(), Duration::from_millis(1));
    for cfg in [
        RetryConfig::try_new(3, Duration::from_millis(100), Duration::from_secs(120), 0.0).expect("valid"),
        RetryConfig::default(),
        clamped,
    ] {
        let delay = next_retry_delay(&cfg, 0, 0.5, Some(Duration::from_secs(60))).expect("attempt 0");
        assert_eq!(delay, Duration::from_secs(60).min(cfg.max_delay()), "{cfg:?}");
        assert!(delay > Duration::ZERO, "{cfg:?}");
    }
}

#[test]
fn marking_a_timeout_retryable_drops_the_verify_first_hint() {
    let e = DomainError::new(ErrorCode::Timeout, "POST /charge timed out");
    assert!(has_effect_hint(&e) && !e.is_retryable());
    let e = e.retryable(true);
    assert!(e.is_retryable());
    assert!(!has_effect_hint(&e), "retryable:true must not also say verify first");
    assert_eq!(e.class(), ErrorClass::Transient);
}

// The code is fixed at construction, so a retryable error cannot become a timeout without its hint.
#[test]
fn a_retryable_timeout_is_only_reachable_through_the_builder() {
    let e = DomainError::new(ErrorCode::RateLimited, "slow down");
    assert_eq!(e.code(), ErrorCode::RateLimited);
    assert!(e.is_retryable() && !has_effect_hint(&e));
    let t = DomainError::new(ErrorCode::Timeout, "slow down");
    assert!(!t.is_retryable() && has_effect_hint(&t), "a fresh timeout says verify first");
    assert_eq!(t.class(), ErrorClass::Ambiguous);
}

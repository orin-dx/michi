//! 0.4 API hygiene: option types compare by value, so callers can assert on the configuration they built.

use std::time::Duration;

use michi_core::toon::{ToonOptions, Value};
use michi_core::RetryConfig;

#[test]
fn retry_config_compares_by_value() {
    let a = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), 0.2);
    assert_eq!(a, RetryConfig::default());
    assert_ne!(a, RetryConfig::new(4, Duration::from_millis(500), Duration::from_secs(30), 0.2));
}

#[test]
fn toon_options_compare_by_value() {
    let build = || ToonOptions::new("issues", vec!["n".into()], vec![vec![Value::Int(1)]]).total_count(Some(1));
    assert_eq!(build(), build());
    assert_ne!(build(), build().total_count(Some(2)));
}

//! `resilience::RetryBudget`: deposit on success, withdraw per retry, capped, modelled on tower's `TpsBudget`.

use michi_core::resilience::RetryBudget;

#[test]
fn starts_with_the_reserve_and_refuses_once_spent() {
    let mut budget = RetryBudget::new(0.1, 2, 100);
    assert_eq!(budget.available(), 2);
    assert!(budget.try_withdraw());
    assert!(budget.try_withdraw());
    assert!(!budget.try_withdraw(), "the reserve is spent and nothing was deposited");
    assert_eq!(budget.available(), 0);
}

#[test]
fn ten_successes_at_ten_percent_earn_exactly_one_retry() {
    let mut budget = RetryBudget::new(0.1, 0, 100);
    for _ in 0..9 {
        budget.deposit();
    }
    assert!(!budget.try_withdraw(), "nine successes earn 0.9 retries");
    budget.deposit();
    assert!(budget.try_withdraw(), "the tenth success completes one retry");
    assert!(!budget.try_withdraw());
}

#[test]
fn balance_is_capped_so_a_long_quiet_period_cannot_bank_a_retry_storm() {
    let mut budget = RetryBudget::new(1.0, 0, 3);
    for _ in 0..50 {
        budget.deposit();
    }
    assert_eq!(budget.available(), 3);
}

#[test]
fn constructor_normalizes_out_of_range_inputs() {
    let nan = RetryBudget::new(f64::NAN, 1, 0);
    assert_eq!(nan.available(), 1, "max is floored to the reserve");
    let mut negative = RetryBudget::new(-5.0, 0, 10);
    negative.deposit();
    assert_eq!(negative.available(), 0, "a negative ratio earns nothing");
    assert_eq!(RetryBudget::default(), RetryBudget::new(0.1, 10, 100));
}

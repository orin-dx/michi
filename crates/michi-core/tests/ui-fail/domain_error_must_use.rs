#![deny(unused_must_use)]

fn main() {
    let e = michi_core::DomainError::new(michi_core::ErrorCode::NotFound, "m");
    e.clone().hint("h");
    e.clone().recovery(michi_core::RecoveryHint::new("t"));
    e.clone().retryable(true);
    e.clone().retry_after(std::time::Duration::from_secs(1));
    e.clone().cause("c");
    e.clone().context("x");
}

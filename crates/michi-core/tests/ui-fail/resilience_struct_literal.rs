fn main() {
    let _ = michi_core::resilience::RetryConfig {
        max_retries: 3,
        base_delay: std::time::Duration::from_millis(500),
        max_delay: std::time::Duration::from_secs(30),
        jitter_factor: 0.2,
    };
}

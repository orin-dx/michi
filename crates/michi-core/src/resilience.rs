#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # Resilience
//!
//! Retry configuration, delay calculation, RFC 7231 `Retry-After` parsing,
//! and FNV-1a idempotency primitives.

use std::time::Duration;

/// Configuration for automatic retry behaviour.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Base delay for exponential back-off.
    pub base_delay: Duration,
    /// Maximum delay cap — back-off never exceeds this.
    pub max_delay: Duration,
    /// Jitter factor in `[0.0, 1.0]`. `0.0` = no jitter, `1.0` = full jitter.
    pub jitter_factor: f64,
}

/// Error returned by [`RetryConfig::try_new()`] when parameters are out of range.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryConfigError {
    /// `max_delay` must be non-zero (zero silently drops server `retry_after` hints).
    MaxDelayIsZero,
    /// `base_delay` must not exceed `max_delay`.
    BaseDelayExceedsMaxDelay {
        /// The provided `base_delay`.
        base: std::time::Duration,
        /// The provided `max_delay`.
        max: std::time::Duration,
    },
    /// `jitter_factor` must be in `[0.0, 1.0]`.
    JitterFactorOutOfRange {
        /// The provided `jitter_factor`.
        factor: f64,
    },
}

impl std::fmt::Display for RetryConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxDelayIsZero => f.write_str("max_delay must be non-zero"),
            Self::BaseDelayExceedsMaxDelay { base, max } => {
                write!(f, "base_delay ({base:?}) must not exceed max_delay ({max:?})")
            }
            Self::JitterFactorOutOfRange { factor } => write!(f, "jitter_factor {factor} is outside [0.0, 1.0]"),
        }
    }
}

impl std::error::Error for RetryConfigError {}

impl RetryConfig {
    /// Normalizing constructor — clamps all inputs to valid ranges silently.
    ///
    /// - `jitter_factor` → clamped to `[0.0, 1.0]`
    /// - `base_delay` → clamped to `min(base_delay, max_delay)`
    /// - `max_delay` → floored to `1ms` (prevents silent `retry_after` discard
    ///   when the caller passes zero)
    ///
    /// For NAPI-boundary callers (untrusted inputs), or wherever clamping is
    /// acceptable. Use [`RetryConfig::try_new()`] when you need explicit errors.
    #[must_use]
    pub fn new(
        max_retries: u32,
        base_delay: std::time::Duration,
        max_delay: std::time::Duration,
        jitter_factor: f64,
    ) -> Self {
        let max_delay = max_delay.max(std::time::Duration::from_millis(1));
        let base_delay = base_delay.min(max_delay);
        let jitter_factor = if jitter_factor.is_finite() { jitter_factor.clamp(0.0, 1.0) } else { 0.0 };
        Self { max_retries, base_delay, max_delay, jitter_factor }
    }

    /// Strict constructor — returns `Err` if any parameter is out of range.
    ///
    /// For NAPI-boundary callers, use the normalizing [`RetryConfig::new()`] instead.
    pub fn try_new(
        max_retries: u32,
        base_delay: std::time::Duration,
        max_delay: std::time::Duration,
        jitter_factor: f64,
    ) -> Result<Self, RetryConfigError> {
        if max_delay.is_zero() {
            return Err(RetryConfigError::MaxDelayIsZero);
        }
        if base_delay > max_delay {
            return Err(RetryConfigError::BaseDelayExceedsMaxDelay { base: base_delay, max: max_delay });
        }
        if !(0.0..=1.0).contains(&jitter_factor) {
            return Err(RetryConfigError::JitterFactorOutOfRange { factor: jitter_factor });
        }
        Ok(Self { max_retries, base_delay, max_delay, jitter_factor })
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.2,
        }
    }
}

/// Compute the delay before the next retry attempt.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
pub fn next_retry_delay(
    config: &RetryConfig,
    attempt: u32,
    jitter_seed: f64,
    retry_after: Option<Duration>,
) -> Option<Duration> {
    if attempt >= config.max_retries {
        return None;
    }
    let jitter_seed = if jitter_seed.is_finite() { jitter_seed.clamp(0.0, 1.0) } else { 0.0 };
    let exp = 2u64.saturating_pow(attempt);
    let base_ms = u64::try_from(config.base_delay.as_millis()).unwrap_or(u64::MAX);
    let max_ms = u64::try_from(config.max_delay.as_millis()).unwrap_or(u64::MAX);
    let raw_ms = base_ms.saturating_mul(exp);
    let jitter_ms = (raw_ms as f64 * config.jitter_factor * jitter_seed) as u64;
    let jittered_ms = raw_ms.saturating_add(jitter_ms);
    let retry_after_ms = retry_after.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let capped_ms = jittered_ms.max(retry_after_ms).min(max_ms);
    Some(Duration::from_millis(capped_ms))
}

/// Parse the value of an HTTP `Retry-After` header as a delay in seconds.
#[must_use]
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    parse_retry_after_at(header_value, std::time::SystemTime::now())
}

/// Like [`parse_retry_after`], but takes the current time explicitly.
#[must_use]
pub fn parse_retry_after_at(header_value: &str, now: std::time::SystemTime) -> Option<Duration> {
    let trimmed = header_value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let target = parse_http_date(trimmed)?;
    Some(target.duration_since(now).unwrap_or(Duration::ZERO))
}

fn parse_http_date(s: &str) -> Option<std::time::SystemTime> {
    let s = s.strip_suffix(" GMT")?;
    let (_weekday, rest) = s.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: u64 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    if !(0..=9999).contains(&year) {
        return None;
    }
    let time = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let mut time_parts = time.split(':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;
    let max_day = days_in_month(year, month)?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 || day == 0 || day > max_day {
        return None;
    }

    let days = days_from_civil(year, month, day);
    #[allow(clippy::cast_possible_wrap)]
    let epoch_secs = days.checked_mul(86_400)?.checked_add((hour * 3600 + minute * 60 + second) as i64)?;
    let unix_secs = u64::try_from(epoch_secs).ok()?;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(unix_secs))
}

fn days_in_month(year: i64, month: u64) -> Option<u64> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if is_leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    })
}

fn month_number(name: &str) -> Option<u64> {
    Some(match name {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn days_from_civil(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Return `true` if the HTTP status code is conventionally retryable.
#[must_use]
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

/// An opaque idempotency key derived from operation inputs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    /// Create an idempotency key from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The raw key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Construct a key from an operation name and raw input bytes, hashed with FNV-1a.
    #[must_use]
    pub fn from_hash(operation: &str, data: &[u8]) -> Self {
        Self(format!("{operation}:{:016x}", fnv1a_64(data)))
    }
}

impl From<String> for IdempotencyKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for IdempotencyKey {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

/// Result of an idempotency check.
///
/// # Caller responsibility
///
/// Use [`AlreadyDone`] when an operation fully completed in a prior call.
/// For operations that only partially completed (some steps succeeded, some
/// failed), use [`crate::idempotency::PartialSuccess`] instead. michi
/// does not enforce this distinction — the choice is the caller's responsibility.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AlreadyDone {
    /// The operation completed in a previous call.
    Yes {
        /// The previously stored result.
        result: String,
    },
    /// The operation has not been seen before — proceed with execution.
    No,
}

/// Check whether an operation has already completed.
#[must_use]
pub fn already_done(stored: Option<String>) -> AlreadyDone {
    match stored {
        Some(result) => AlreadyDone::Yes { result },
        None => AlreadyDone::No,
    }
}

/// Render an already-done response.
#[must_use]
pub fn render_already_done(operation: &str, summary: &str, hints: &[String]) -> String {
    let mut out = String::with_capacity(64 + operation.len() + summary.len() + hints.len() * 50);
    out.push_str("operation: ");
    out.push_str(&crate::text::escape_line_breaks(operation));
    out.push_str("\nstatus:    already_done\nsummary:   ");
    out.push_str(&crate::text::escape_line_breaks(summary));
    out.push('\n');
    crate::hints::append_help_block(&mut out, hints.iter().map(String::as_str));
    out
}

fn fnv1a_64(data: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_retry_uses_base_delay() {
        let config = RetryConfig::default();
        let delay = next_retry_delay(&config, 0, 0.0, None).expect("first retry delay calculated");
        assert_eq!(delay, config.base_delay);
    }

    #[test]
    fn second_retry_doubles() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        let delay = next_retry_delay(&config, 1, 0.0, None).expect("attempt 1 is within max_retries");
        assert_eq!(delay, config.base_delay * 2);
    }

    #[test]
    fn delay_is_capped_at_max() {
        let config = RetryConfig {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.0,
            max_retries: 10,
        };
        let delay = next_retry_delay(&config, 5, 0.0, None).expect("attempt 5 is within max_retries=10");
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn beyond_max_retries_returns_none() {
        let config = RetryConfig::default();
        assert!(next_retry_delay(&config, 3, 0.0, None).is_none());
    }

    #[test]
    fn jitter_increases_delay() {
        let config = RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(30), 1.0);
        let with_jitter = next_retry_delay(&config, 0, 1.0, None).expect("attempt 0 within max_retries");
        assert_eq!(with_jitter, Duration::from_millis(200));
        let no_jitter = next_retry_delay(&config, 0, 0.0, None).expect("attempt 0 within max_retries");
        assert_eq!(no_jitter, Duration::from_millis(100));
    }

    #[test]
    fn non_finite_jitter_seed_treated_as_zero() {
        let config = RetryConfig::new(3, Duration::from_millis(100), Duration::from_secs(30), 1.0);
        let baseline = next_retry_delay(&config, 0, 0.0, None).expect("attempt 0 within max_retries");
        let nan = next_retry_delay(&config, 0, f64::NAN, None).expect("attempt 0 within max_retries");
        let inf = next_retry_delay(&config, 0, f64::INFINITY, None).expect("attempt 0 within max_retries");
        assert_eq!(nan, baseline, "NaN jitter_seed must be treated as no jitter");
        assert_eq!(inf, baseline, "INFINITY jitter_seed must be treated as no jitter");
    }

    #[test]
    fn jitter_never_exceeds_max_delay_when_base_already_capped() {
        let config = RetryConfig {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            jitter_factor: 1.0,
            max_retries: 10,
        };
        let delay = next_retry_delay(&config, 5, 1.0, None).expect("attempt 5 within max_retries=10");
        assert!(delay <= config.max_delay, "delay {delay:?} exceeded max_delay {:?}", config.max_delay);
    }

    #[test]
    fn extreme_duration_saturates_instead_of_wrapping() {
        let huge = Duration::from_secs(18_446_744_073_709_552);
        let config = RetryConfig { base_delay: huge, max_delay: huge, jitter_factor: 0.0, max_retries: 100 };
        // attempt=64 (not 63): 2^63 is well within u64::MAX and needs no saturation at
        // all, so it doesn't exercise saturating_pow's overflow behavior. 2^64 does
        // overflow a non-saturating u64::pow, making 64 the real boundary.
        for attempt in [0, 64] {
            let delay = next_retry_delay(&config, attempt, 0.0, None).expect("attempt within max_retries");
            assert!(
                delay.as_millis() > 1_000_000_000_000,
                "attempt {attempt}: delay {delay:?} wrapped to a tiny value"
            );
            assert!(
                delay <= config.max_delay,
                "attempt {attempt}: delay {delay:?} exceeded max_delay {:?}",
                config.max_delay
            );
        }
    }

    #[test]
    fn retry_after_sub_millisecond_truncates_toward_zero() {
        let config = RetryConfig::new(3, Duration::from_millis(1), Duration::from_secs(30), 0.0);
        let delay = next_retry_delay(&config, 0, 0.0, Some(Duration::from_nanos(1_500_000)))
            .expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_millis(1));
    }

    #[test]
    fn retry_after_exceeding_u64_millis_saturates_instead_of_truncating() {
        let config =
            RetryConfig::try_new(3, Duration::from_millis(1), Duration::from_secs(200_000_000_000_000_000), 0.0)
                .expect("valid config");
        let delay = next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(100_000_000_000_000_000)))
            .expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_millis(u64::MAX));
    }

    #[test]
    fn retry_after_wins_when_larger_than_backoff() {
        let config = RetryConfig { jitter_factor: 0.0, ..Default::default() };
        let delay =
            next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(5))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn ac016_retry_after_strictly_exceeding_backoff_wins_exactly() {
        let config = RetryConfig {
            jitter_factor: 0.0,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            max_retries: 3,
        };
        // backoff at attempt=0 is 1000ms; retry_after=2000ms is strictly greater.
        let delay =
            next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(2))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn backoff_wins_when_larger_than_retry_after() {
        let config = RetryConfig { jitter_factor: 0.0, base_delay: Duration::from_secs(10), ..Default::default() };
        let delay =
            next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(1))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(10));
    }

    #[test]
    fn retry_after_still_capped_at_max_delay() {
        let config = RetryConfig { jitter_factor: 0.0, max_delay: Duration::from_secs(5), ..Default::default() };
        let delay =
            next_retry_delay(&config, 0, 0.0, Some(Duration::from_secs(999))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(5), "retry_after must not bypass max_delay");
    }

    #[test]
    fn retry_after_wins_over_backoff_with_jitter_applied() {
        let config = RetryConfig { jitter_factor: 1.0, base_delay: Duration::from_secs(1), ..Default::default() };
        let delay =
            next_retry_delay(&config, 0, 1.0, Some(Duration::from_secs(5))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn jittered_backoff_wins_over_smaller_retry_after() {
        let config = RetryConfig { jitter_factor: 1.0, base_delay: Duration::from_secs(1), ..Default::default() };
        let delay =
            next_retry_delay(&config, 0, 1.0, Some(Duration::from_millis(500))).expect("attempt 0 within max_retries");
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn parse_retry_after_seconds() {
        assert_eq!(parse_retry_after("60"), Some(Duration::from_secs(60)));
        assert_eq!(parse_retry_after("  120  "), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parse_retry_after_at_strips_whitespace_around_http_date() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        let unpadded = parse_retry_after_at("Thu, 01 Jan 2026 00:01:00 GMT", now);
        let padded = parse_retry_after_at("  Thu, 01 Jan 2026 00:01:00 GMT  ", now);
        assert_eq!(padded, unpadded);
        assert_eq!(padded, Some(Duration::from_secs(60)));
    }

    #[test]
    fn parse_retry_after_strips_unicode_whitespace_not_just_ascii() {
        // U+2003 EM SPACE is Unicode whitespace but not ASCII whitespace; str::trim()
        // strips it, so this must succeed rather than fail to parse as an integer.
        let padded = "\u{2003}120\u{2003}";
        assert_eq!(parse_retry_after(padded), Some(Duration::from_secs(120)));
    }

    #[test]
    fn parse_retry_after_rejects_u64_overflowing_digit_string() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("99999999999999999999999", now), None);
    }

    #[test]
    fn parse_retry_after_date_in_past_clamps_to_zero() {
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), Some(Duration::ZERO));
    }

    #[test]
    fn http_date_in_future_parses_relative_to_now() {
        // now = 2026-01-01T00:00:00Z, header = 2026-01-01T00:01:00Z -> 60s
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        let delay = parse_retry_after_at("Thu, 01 Jan 2026 00:01:00 GMT", now);
        assert_eq!(delay, Some(Duration::from_secs(60)));
    }

    #[test]
    fn http_date_in_past_clamps_to_zero() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        let delay = parse_retry_after_at("Wed, 31 Dec 2025 00:00:00 GMT", now);
        assert_eq!(delay, Some(Duration::ZERO), "past dates clamp to zero, per spec's clock-skew note");
    }

    #[test]
    fn http_date_at_unix_epoch_boundary_clamps_to_zero() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        let delay = parse_retry_after_at("Thu, 01 Jan 1970 00:00:00 GMT", now);
        assert_eq!(delay, Some(Duration::ZERO), "the epoch instant itself is representable and clamps to zero");
    }

    #[test]
    fn http_date_before_unix_epoch_returns_none() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_767_225_600);
        assert_eq!(
            parse_retry_after_at("Mon, 01 Jan 1900 00:00:00 GMT", now),
            None,
            "pre-epoch dates are rejected, not clamped to zero -- epoch_secs -> u64 fails on negative seconds"
        );
        assert_eq!(parse_retry_after_at("Wed, 01 Jan 1969 23:59:59 GMT", now), None);
    }

    #[test]
    fn malformed_http_date_returns_none() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("not a date", now), None);
        assert_eq!(parse_retry_after_at("Wed, 32 Foo 2026 00:00:00 GMT", now), None);
    }

    #[test]
    fn http_date_grammar_is_more_lenient_than_strict_rfc7231_imf_fixdate() {
        let now = std::time::UNIX_EPOCH;
        let expected = Some(Duration::from_secs(1_767_225_600));
        // day-name is not validated -- any token before ", " is discarded
        assert_eq!(parse_retry_after_at("Xxx, 01 Jan 2026 00:00:00 GMT", now), expected);
        // day is parsed with str::parse, not fixed-width 2DIGIT -- unpadded accepted
        assert_eq!(parse_retry_after_at("Thu, 1 Jan 2026 00:00:00 GMT", now), expected);
        // time fields likewise accept unpadded digits
        assert_eq!(parse_retry_after_at("Thu, 01 Jan 2026 0:0:0 GMT", now), expected);
    }

    #[test]
    fn http_date_with_day_out_of_range_returns_none() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("Thu, 32 Jan 2026 00:00:00 GMT", now), None);
        assert_eq!(parse_retry_after_at("Thu, 00 Jan 2026 00:00:00 GMT", now), None);
    }

    #[test]
    fn http_date_with_time_out_of_range_returns_none() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("Thu, 01 Jan 2026 24:00:00 GMT", now), None);
        assert_eq!(parse_retry_after_at("Thu, 01 Jan 2026 00:60:00 GMT", now), None);
        assert_eq!(parse_retry_after_at("Thu, 01 Jan 2026 00:00:61 GMT", now), None);
        assert_eq!(parse_retry_after_at("Thu, 01 Jan 2026 00:00:00:00 GMT", now), None);
    }

    #[test]
    fn http_date_with_absurd_year_returns_none_instead_of_panicking() {
        let now = std::time::SystemTime::UNIX_EPOCH;
        assert_eq!(parse_retry_after_at("Wed, 21 Oct 999999999999999999 07:28:00 GMT", now), None);
    }

    #[test]
    fn parse_http_date_rejects_feb_30() {
        let now = std::time::UNIX_EPOCH;
        assert!(parse_retry_after_at("Wed, 30 Feb 2026 07:28:00 GMT", now).is_none(), "Feb 30 is impossible");
    }

    #[test]
    fn parse_http_date_rejects_feb_29_non_leap() {
        let now = std::time::UNIX_EPOCH;
        assert!(parse_retry_after_at("Sun, 29 Feb 2026 00:00:00 GMT", now).is_none(), "2026 is not a leap year");
    }

    #[test]
    fn parse_http_date_accepts_feb_29_leap_year() {
        let now = std::time::UNIX_EPOCH;
        // 2028 is a leap year (divisible by 4, not a century)
        let result = parse_retry_after_at("Tue, 29 Feb 2028 00:00:00 GMT", now);
        assert!(result.is_some(), "Feb 29 2028 is valid");
        // 2000 is a leap year (divisible by 400) -- this is the case that actually
        // distinguishes the Gregorian rule from a naive `year % 4 == 0` check.
        let result = parse_retry_after_at("Tue, 29 Feb 2000 00:00:00 GMT", now);
        assert!(result.is_some(), "Feb 29 2000 is valid (divisible by 400)");
    }

    #[test]
    fn parse_http_date_rejects_feb_29_century_non_leap() {
        let now = std::time::UNIX_EPOCH;
        // 2100 (not 1900): 1900 predates the Unix epoch, so parse_http_date's
        // epoch_secs -> u64 conversion rejects it regardless of leap-year status,
        // which would make this test pass even if the century rule were broken.
        assert!(
            parse_retry_after_at("Mon, 29 Feb 2100 00:00:00 GMT", now).is_none(),
            "2100 is divisible by 100 but not 400 -- not a leap year"
        );
    }

    #[test]
    fn parse_http_date_accepts_leap_second() {
        let now = std::time::UNIX_EPOCH;
        assert!(
            parse_retry_after_at("Thu, 01 Jan 2026 00:00:60 GMT", now).is_some(),
            "second=60 (a leap second) must be accepted"
        );
    }

    #[test]
    fn parse_http_date_rejects_april_31() {
        let now = std::time::UNIX_EPOCH;
        assert!(parse_retry_after_at("Thu, 31 Apr 2026 00:00:00 GMT", now).is_none(), "April has 30 days");
    }

    #[test]
    fn parse_http_date_accepts_jan_31() {
        let now = std::time::UNIX_EPOCH;
        let result = parse_retry_after_at("Sat, 31 Jan 2026 00:00:00 GMT", now);
        assert!(result.is_some(), "Jan 31 is valid");
    }

    #[test]
    fn public_parse_retry_after_still_handles_seconds() {
        assert_eq!(parse_retry_after("60"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn public_parse_retry_after_rejects_malformed_header() {
        assert_eq!(parse_retry_after("not a valid header"), None);
    }

    #[test]
    fn is_retryable_status_full_domain_sweep() {
        for status in 0..=u16::MAX {
            let expected = matches!(status, 429 | 502 | 503 | 504);
            assert_eq!(is_retryable_status(status), expected, "status {status}");
        }
    }

    #[test]
    fn retryable_status_codes() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));
        assert!(!is_retryable_status(404));
        assert!(!is_retryable_status(400));
    }

    #[test]
    fn non_retryable_status_codes() {
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(201));
        assert!(!is_retryable_status(301));
        assert!(!is_retryable_status(401));
    }

    #[test]
    fn http_500_is_not_retryable() {
        assert!(
            !is_retryable_status(500),
            "500 is a server bug — retrying reproduces it and risks duplicate side effects on writes"
        );
    }

    #[test]
    fn from_hash_is_deterministic() {
        let a = IdempotencyKey::from_hash("create_item", b"same input");
        let b = IdempotencyKey::from_hash("create_item", b"same input");
        assert_eq!(a, b);
    }

    #[test]
    fn from_hash_format_is_operation_colon_16_hex_chars() {
        let key = IdempotencyKey::from_hash("op", b"data");
        let s = key.as_str();
        let (prefix, hex) = s.split_once(':').expect("format is operation:hex");
        assert_eq!(prefix, "op");
        assert_eq!(hex.len(), 16, "got: {hex}");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "got: {hex}");
    }

    #[test]
    fn from_hash_different_data_yields_different_keys() {
        let a = IdempotencyKey::from_hash("op", b"a");
        let b = IdempotencyKey::from_hash("op", b"b");
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn idempotency_key_constructors() {
        assert_eq!(IdempotencyKey::new("foo").as_str(), "foo");
        assert_eq!(IdempotencyKey::from(String::from("bar")).as_str(), "bar");
        assert_eq!(IdempotencyKey::from("baz").as_str(), "baz");
    }

    // AC-048 (IdempotencyKey's field is publicly constructible/mutable) is
    // NOT tested here: this module is inside the crate, where field privacy
    // is invisible -- a regression removing `pub` would still compile and
    // pass every test in this file. See tests/public_field_access.rs for the
    // real, external-crate proof.

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    #[test]
    fn test_auto_traits() {
        assert_send_sync_static::<RetryConfig>();
        assert_send_sync_static::<IdempotencyKey>();
        assert_send_sync_static::<AlreadyDone>();
    }

    #[test]
    fn new_clamps_jitter_factor_above_one() {
        let c = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), 2.5);
        assert_eq!(c.jitter_factor, 1.0, "jitter_factor must clamp to exactly 1.0, got {}", c.jitter_factor);
    }

    #[test]
    fn new_clamps_negative_jitter_to_zero() {
        let c = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), -0.5);
        assert_eq!(c.jitter_factor, 0.0, "jitter_factor must be clamped to 0.0, got {}", c.jitter_factor);
    }

    #[test]
    fn new_clamps_non_finite_jitter_to_zero() {
        let c_nan = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), f64::NAN);
        assert_eq!(c_nan.jitter_factor, 0.0, "NaN jitter_factor must clamp to 0.0");
        let c_inf = RetryConfig::new(3, Duration::from_millis(500), Duration::from_secs(30), f64::INFINITY);
        assert_eq!(c_inf.jitter_factor, 0.0, "INFINITY jitter_factor must clamp to 0.0");
    }

    #[test]
    fn new_clamps_base_delay_exceeding_max() {
        let c = RetryConfig::new(3, Duration::from_secs(1), Duration::from_millis(500), 0.2);
        assert_eq!(c.base_delay, Duration::from_millis(500), "base_delay must clamp down to max_delay exactly");
    }

    #[test]
    fn new_floors_zero_max_delay_to_one_ms() {
        let c = RetryConfig::new(3, Duration::ZERO, Duration::ZERO, 0.0);
        assert_eq!(c.max_delay, Duration::from_millis(1), "max_delay=0 must be floored to exactly 1ms");
    }

    #[test]
    fn try_new_rejects_zero_max_delay() {
        let result = RetryConfig::try_new(3, Duration::from_millis(500), Duration::ZERO, 0.2);
        assert!(matches!(result, Err(RetryConfigError::MaxDelayIsZero)));
    }

    #[test]
    fn try_new_rejects_base_exceeding_max() {
        let result = RetryConfig::try_new(3, Duration::from_secs(2), Duration::from_secs(1), 0.2);
        match result {
            Err(RetryConfigError::BaseDelayExceedsMaxDelay { base, max }) => {
                assert_eq!(base, Duration::from_secs(2));
                assert_eq!(max, Duration::from_secs(1));
            }
            other => panic!("expected BaseDelayExceedsMaxDelay {{ base: 2s, max: 1s }}, got {other:?}"),
        }
    }

    #[test]
    fn try_new_rejects_out_of_range_jitter() {
        for factor in [-0.1, 1.1] {
            let result = RetryConfig::try_new(3, Duration::from_millis(100), Duration::from_secs(30), factor);
            match result {
                Err(RetryConfigError::JitterFactorOutOfRange { factor: got }) => assert_eq!(got, factor),
                other => panic!("expected JitterFactorOutOfRange {{ factor: {factor} }}, got {other:?}"),
            }
        }
        let result = RetryConfig::try_new(3, Duration::from_millis(100), Duration::from_secs(30), f64::NAN);
        match result {
            Err(RetryConfigError::JitterFactorOutOfRange { factor }) => assert!(factor.is_nan()),
            other => panic!("expected JitterFactorOutOfRange {{ factor: NaN }}, got {other:?}"),
        }
    }

    #[test]
    fn try_new_accepts_valid_config() {
        let result = RetryConfig::try_new(3, Duration::from_millis(500), Duration::from_secs(30), 0.2);
        let config = result.expect("valid config");
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.base_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert_eq!(config.jitter_factor, 0.2);
    }

    #[test]
    fn retry_config_error_display_strings() {
        assert_eq!(RetryConfigError::MaxDelayIsZero.to_string(), "max_delay must be non-zero");
        assert_eq!(
            RetryConfigError::JitterFactorOutOfRange { factor: 1.5 }.to_string(),
            "jitter_factor 1.5 is outside [0.0, 1.0]"
        );
        assert_eq!(
            RetryConfigError::BaseDelayExceedsMaxDelay { base: Duration::from_secs(2), max: Duration::from_secs(1) }
                .to_string(),
            "base_delay (2s) must not exceed max_delay (1s)"
        );
    }

    #[test]
    fn default_config_values() {
        let c = RetryConfig::default();
        assert_eq!(c.max_retries, 3);
        assert_eq!(c.base_delay, Duration::from_millis(500));
        assert_eq!(c.max_delay, Duration::from_secs(30));
        assert_eq!(c.jitter_factor, 0.2);
    }

    #[test]
    fn already_done_yes_when_stored() {
        assert_eq!(already_done(Some("prior result".into())), AlreadyDone::Yes { result: "prior result".into() });
    }

    #[test]
    fn already_done_no_when_none() {
        assert_eq!(already_done(None), AlreadyDone::No);
    }

    #[test]
    fn render_already_done_no_hints_format() {
        let out = render_already_done("delete_item", "item 42 was deleted in run abc", &[]);
        assert_eq!(out, "operation: delete_item\nstatus:    already_done\nsummary:   item 42 was deleted in run abc\n");
    }

    #[test]
    fn render_already_done_with_hints() {
        let out = render_already_done("my_op", "did the thing", &["hint one".to_string()]);
        assert_eq!(out, "operation: my_op\nstatus:    already_done\nsummary:   did the thing\nhelp[1]: hint one\n");
    }

    #[test]
    fn render_already_done_multiple_hints() {
        let hints = ["hint a".to_string(), "hint b".to_string()];
        let out = render_already_done("op", "done", &hints);
        assert_eq!(out, "operation: op\nstatus:    already_done\nsummary:   done\nhelp[2]: hint a,hint b\n");
    }
}

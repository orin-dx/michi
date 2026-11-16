//! Retry helpers (`parseRetryAfter`, `nextRetryDelay`, `isRetryableStatus`) and `truncate`: the exports that take
//! scalars and so need no bounded conversion.

use napi_derive::napi;

use super::num::{JsCount, JsDelayMillis, JsHttpStatus, JsRetryCount, JsUnitInterval};

/// Parse an RFC 9110 `Retry-After` header value into seconds: delay-seconds (saturating at `u64::MAX`) or an
/// HTTP-date, where a past date (before 1970 too) is `0`. Returns `null` for a malformed value.
#[napi(catch_unwind)]
#[must_use]
pub fn parse_retry_after(header_value: String) -> Option<f64> {
    crate::resilience::parse_retry_after(&header_value).map(|d| d.as_secs_f64())
}

/// Calculate the next retry delay in milliseconds.
///
/// `jitter_seed` must be in `[0.0, 1.0]`; pass a per-call random value from
/// your preferred RNG to avoid thundering-herd retries across concurrent callers.
/// Delay inputs must be finite and non-negative; `retry_after_ms` may be `null`.
///
/// # Errors
///
/// None in practice: the numeric arguments are validated by their types before the call (an out-of-domain number is
/// rejected by napi with an `InvalidArg` error) and the computation itself cannot fail.
#[napi(catch_unwind)]
#[allow(clippy::too_many_arguments)] // NAPI boundary: all params are scalar and directly map to the JS API
pub fn next_retry_delay(
    #[napi(ts_arg_type = "number")] max_retries: JsRetryCount,
    #[napi(ts_arg_type = "number")] base_delay_ms: JsDelayMillis,
    #[napi(ts_arg_type = "number")] max_delay_ms: JsDelayMillis,
    #[napi(ts_arg_type = "number")] jitter_factor: JsUnitInterval,
    #[napi(ts_arg_type = "number")] jitter_seed: JsUnitInterval,
    #[napi(ts_arg_type = "number")] attempt: JsRetryCount,
    #[napi(ts_arg_type = "number | undefined | null")] retry_after_ms: Option<JsDelayMillis>,
) -> napi::Result<Option<f64>> {
    let config = crate::resilience::RetryConfig::new(
        max_retries.get_u32(),
        base_delay_ms.as_duration(),
        max_delay_ms.as_duration(),
        jitter_factor.get(),
    );
    let retry_after = retry_after_ms.map(JsDelayMillis::as_duration);
    Ok(crate::resilience::next_retry_delay(&config, attempt.get_u32(), jitter_seed.get(), retry_after)
        .map(|d| d.as_secs_f64() * 1000.0))
}

/// Return `true` if the HTTP status code is conventionally retryable (429, 502, 503, 504). Only 429 and 503 say the
/// request was not processed; after a 502 or 504 it may have been applied, so retry those only for an idempotent
/// request.
#[napi(catch_unwind)]
#[must_use]
pub fn is_retryable_status(#[napi(ts_arg_type = "number")] status: JsHttpStatus) -> bool {
    crate::resilience::is_retryable_status(status.get_u16())
}

/// The result of `truncate` (JavaScript-friendly).
#[napi(object, js_name = "Truncated")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsTruncated {
    /// The text to show: the input when it fits, otherwise its first `maxChars - 1` chars followed by a plain `…`.
    pub content: String,
    /// Length of the input in chars.
    #[napi(js_name = "originalChars")]
    pub original_chars: u32,
    /// How many chars of the input are not in `content`; `0` when nothing was cut.
    pub removed: u32,
    /// Whether any chars were cut (`removed > 0`).
    #[napi(js_name = "wasTruncated")]
    pub was_truncated: bool,
}

/// Cut one free-text string to `maxChars` Unicode scalar values. Text that fits is returned unchanged; a longer text
/// keeps its first `maxChars - 1` chars and ends in `…` (`maxChars` 0 returns an empty string). The cut is reported
/// in `removed` and `wasTruncated`, never inside `content`, so the text cannot carry a message that data could
/// imitate. Tell the agent how to get the rest through a hint you write yourself, for example
/// `cut ${removed} of ${originalChars} chars; pass full=true`.
#[napi(catch_unwind)]
#[allow(clippy::needless_pass_by_value)] // napi-derive requires owned String for JS string params
#[must_use]
pub fn truncate(content: String, #[napi(ts_arg_type = "number")] max_chars: JsCount) -> JsTruncated {
    let t = crate::truncate::truncate(&content, max_chars.get());
    // A JS string is far below 2^32 chars, so saturating never happens in practice; it keeps the cast panic-free.
    let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    JsTruncated {
        was_truncated: t.was_truncated(),
        original_chars: to_u32(t.original_chars),
        removed: to_u32(t.removed),
        content: t.content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_basic() {
        let max = JsCount::try_from(100.0).expect("100 is a valid count");
        assert_eq!(
            truncate("hello".to_string(), max),
            JsTruncated { content: "hello".to_string(), original_chars: 5, removed: 0, was_truncated: false }
        );
    }

    #[test]
    fn truncate_reports_the_cut_outside_the_text() {
        let max = JsCount::try_from(40.0).expect("40 is a valid count");
        assert_eq!(
            truncate("a".repeat(100), max),
            JsTruncated {
                content: format!("{}…", "a".repeat(39)),
                original_chars: 100,
                removed: 61,
                was_truncated: true,
            }
        );
    }

    #[test]
    fn parse_retry_after_returns_none_for_unparsable_input() {
        assert_eq!(parse_retry_after("not-a-number-or-date".to_string()), None);
    }
}

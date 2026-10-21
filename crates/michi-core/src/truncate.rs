#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # Truncation
//!
//! UTF-8 char-boundary safe content truncation primitives for michi and AXI.

/// Result of a truncation operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Truncated {
    /// The truncated (or original) content string.
    pub content: String,
    /// Original byte length of the input.
    pub original_len: usize,
    /// Whether truncation actually occurred.
    pub was_truncated: bool,
    /// The truncation signal text that was actually embedded in `content`,
    /// starting immediately after the kept characters (leading space stripped).
    /// `None` when not truncated or when `max_chars` was so small that no
    /// signal text fit.
    pub signal: Option<String>,
}

/// Truncate `content` to at most `max_chars` Unicode scalar values, appending
/// an agent-readable suffix when truncation occurs.
///
/// The suffix pattern is `" ({n} chars truncated — use {hint})"`. Uses char
/// boundaries — never splits a Unicode scalar.
///
/// Returns the original string unchanged when `content.chars().count() <= max_chars`.
#[must_use]
pub fn truncate(content: &str, max_chars: usize, hint: &str) -> Truncated {
    // Byte length is always >= char count, so if it already fits within
    // max_chars the char count does too — skip the full UTF-8 scan.
    if content.len() <= max_chars {
        return Truncated {
            content: content.to_string(),
            original_len: content.len(),
            was_truncated: false,
            signal: None,
        };
    }

    let char_count = content.chars().count();
    if char_count <= max_chars {
        return Truncated {
            content: content.to_string(),
            original_len: content.len(),
            was_truncated: false,
            signal: None,
        };
    }

    let suffix = format!(" ({char_count} chars truncated — use {hint})");
    let suffix_chars = suffix.chars().count();
    let keep_chars = max_chars.saturating_sub(suffix_chars);

    let byte_end = content.char_indices().nth(keep_chars).map_or(content.len(), |(i, _)| i);

    let mut result = String::with_capacity(byte_end + suffix.len());
    result.push_str(&content[..byte_end]);
    result.push_str(&suffix);

    // Hard-cap to max_chars via char boundaries in case the suffix alone
    // exceeds max_chars (e.g. very small max_chars or long hint string).
    if result.chars().count() > max_chars {
        let cap_byte = result.char_indices().nth(max_chars).map_or(result.len(), |(i, _)| i);
        result.truncate(cap_byte);
    }

    // Recompute signal based on what's actually in content after hard-cap
    let signal = if result.len() > byte_end {
        let signal_text = &result[byte_end..];
        let trimmed = signal_text.trim_start();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        None
    };

    Truncated { content: result, original_len: content.len(), was_truncated: true, signal }
}

/// Truncate content for inline use (e.g. inside a TOON field).
///
/// Returns the final string directly.
#[must_use]
pub fn truncate_inline(content: &str, max_chars: usize, hint: &str) -> String {
    truncate(content, max_chars, hint).content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_not_truncated() {
        let t = truncate("hello", 100, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.content, "hello");
        assert_eq!(t.original_len, 5);
    }

    #[test]
    fn long_content_is_truncated() {
        let content = "a".repeat(200);
        let t = truncate(&content, 50, "full=true");
        assert!(t.was_truncated);
        assert!(t.content.chars().count() <= 50);
        assert!(t.content.contains("chars truncated"));
        assert!(t.content.contains("full=true"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let content = "こんにちは世界！これはテストです。";
        let t = truncate(content, 10, "full=true");
        assert!(std::str::from_utf8(t.content.as_bytes()).is_ok());
        if t.was_truncated {
            assert!(t.content.chars().count() <= 10);
        }
    }

    #[test]
    fn truncate_inline_returns_string() {
        let content = "x".repeat(200);
        let result = truncate_inline(&content, 30, "full=true");
        assert!(result.chars().count() <= 30);
    }

    #[test]
    fn exact_length_not_truncated() {
        let t = truncate("hello", 5, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.content, "hello");
    }

    #[test]
    fn suffix_contains_original_char_count() {
        let content = "a".repeat(100);
        let t = truncate(&content, 40, "full=true");
        assert!(t.was_truncated);
        assert!(t.content.contains("100 chars truncated"));
    }

    #[test]
    fn large_short_content_not_truncated() {
        let content = "a".repeat(5000);
        let t = truncate(&content, 10_000, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.content, content);
        assert_eq!(t.original_len, 5000);
    }

    #[test]
    fn signal_is_populated_when_truncated() {
        let content = "a".repeat(200);
        let t = truncate(&content, 50, "full=true");
        assert!(t.was_truncated);
        let signal = t.signal.as_deref().expect("signal present when truncated");
        assert!(signal.contains("200 chars truncated"));
        assert!(signal.contains("full=true"));
        assert_eq!(signal, "(200 chars truncated — use full=true)");
    }

    #[test]
    fn signal_is_none_when_not_truncated() {
        let t = truncate("hello", 100, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.signal, None);
    }

    #[test]
    fn signal_reflects_what_is_actually_in_content() {
        // When max_chars is very small, the suffix (signal text) may not fit in content.
        // signal must either be None (no room) or actually appear in content.
        let long_content = "hello world this is some fairly long content for testing";
        // Use a small max_chars — smaller than any reasonable suffix text
        let result = truncate(long_content, 5, "full=true");
        match result {
            Truncated { content, signal: Some(ref sig), .. } => {
                assert!(
                    content.contains(sig.as_str()),
                    "signal must appear in content\ncontent: {:?}\nsignal: {:?}",
                    content,
                    sig
                );
                assert!(
                    content.chars().count() <= 5,
                    "content exceeded max_chars=5, got {} chars: {:?}",
                    content.chars().count(),
                    content
                );
            }
            Truncated { content, signal: None, .. } => {
                // signal is None means there was no room for any signal text
                assert!(
                    content.chars().count() <= 5,
                    "content exceeded max_chars=5, got {} chars: {:?}",
                    content.chars().count(),
                    content
                );
            }
        }
    }

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    #[test]
    fn test_auto_traits() {
        assert_send_sync_static::<Truncated>();
    }

    #[test]
    fn ac017_exact_output_at_200_chars_50_max() {
        let t = truncate(&"a".repeat(200), 50, "full=true");
        assert!(t.was_truncated);
        assert_eq!(t.content.chars().count(), 50);
        assert_eq!(t.content, "aaaaaaaaaaaa (200 chars truncated — use full=true)");
        assert_eq!(t.signal, Some("(200 chars truncated — use full=true)".to_string()));
    }

    #[test]
    fn ac004_suffix_present_with_leading_space_at_exact_fit() {
        let content = "a".repeat(100);
        let t = truncate(&content, 30, "x");
        assert_eq!(t.content, " (100 chars truncated — use x)");
        assert_eq!(t.content.chars().count(), 30);
        assert_eq!(t.signal, Some("(100 chars truncated — use x)".to_string()));

        let t = truncate(&content, 40, "x");
        assert_eq!(t.content, "aaaaaaaaaa (100 chars truncated — use x)");

        let t = truncate(&content, 50, "x");
        assert_eq!(t.content, "aaaaaaaaaaaaaaaaaaaa (100 chars truncated — use x)");
    }

    #[test]
    fn ac008_truncate_inline_matches_truncate_content() {
        let contents = ["", "hello", &"a".repeat(200), "こんにちは世界！"];
        let max_chars_values = [0usize, 1, 5, 30, 50, 500];
        let hints = ["", "x", "見よ"];
        for content in contents {
            for &max_chars in &max_chars_values {
                for hint in hints {
                    assert_eq!(truncate_inline(content, max_chars, hint), truncate(content, max_chars, hint).content);
                }
            }
        }
    }

    #[test]
    fn ac009_original_len_is_byte_length_not_char_count() {
        let content = "こんにちは世界！これはテストです。";
        assert_eq!(content.chars().count(), 17);
        assert_eq!(content.len(), 51);
        let t = truncate(content, 100, "x");
        assert_eq!(t.original_len, 51);
        assert_ne!(t.original_len, content.chars().count());
    }

    #[test]
    fn ac014_empty_content_never_truncates() {
        for max_chars in [0usize, 1, 100, usize::MAX] {
            let t = truncate("", max_chars, "x");
            assert_eq!(t.content, "");
            assert_eq!(t.original_len, 0);
            assert!(!t.was_truncated);
            assert_eq!(t.signal, None);
        }
    }

    #[test]
    fn ac015_max_chars_zero_truncates_any_nonempty_content_to_empty() {
        for content in ["a", &"a".repeat(100), "こんにちは"] {
            for hint in ["", "x", "見よ"] {
                let t = truncate(content, 0, hint);
                assert_eq!(t.content, "");
                assert_eq!(t.original_len, content.len());
                assert!(t.was_truncated);
            }
        }
    }

    #[test]
    fn ac016_empty_hint_produces_suffix_with_empty_interpolation() {
        let content = "a".repeat(100);
        let t = truncate(&content, 40, "");
        assert_eq!(t.content, "aaaaaaaaaaa (100 chars truncated — use )");
        assert_eq!(t.content.chars().count(), 40);
        assert!(t.content.contains(" chars truncated — use )"));
        assert_eq!(t.signal, Some("(100 chars truncated — use )".to_string()));
    }

    #[test]
    fn ac018_multibyte_hint_chars_feed_keep_chars_arithmetic() {
        let content = "こんにちは世界！これはテストです。".repeat(3);
        let t = truncate(&content, 35, "見よ");
        assert!(t.was_truncated);
        assert_eq!(t.original_len, 153);
        assert_eq!(t.content, "こんにちは (51 chars truncated — use 見よ)");
        assert_eq!(t.content.chars().count(), 35);
        assert_eq!(t.signal, Some("(51 chars truncated — use 見よ)".to_string()));
    }

    #[test]
    fn ac019_hard_cap_region_pins_exact_prefix_and_signal() {
        let content = "a".repeat(100);

        let t = truncate(&content, 3, "x");
        assert!(t.was_truncated);
        assert_eq!(t.content, " (1");
        assert_eq!(t.signal, Some("(1".to_string()));

        let t = truncate(&content, 1, "x");
        assert!(t.was_truncated);
        assert_eq!(t.content, " ");
        assert_eq!(t.signal, None);

        let t = truncate(&content, 28, "x");
        assert!(t.was_truncated);
        assert_eq!(t.content, " (100 chars truncated — use ");
        assert_eq!(t.signal, Some("(100 chars truncated — use ".to_string()));
    }

    #[test]
    fn ac020_full_matrix_never_panics() {
        let contents = ["", "a", &"a".repeat(200), "こんにちは世界！これはテストです。", "👍🏽é̀"];
        let max_chars_values = [0usize, 1, 2, 3, 5, 29, 30, 50, usize::MAX];
        let hints = ["", "x", "見よ"];
        for content in contents {
            for &max_chars in &max_chars_values {
                for hint in hints {
                    let _ = truncate(content, max_chars, hint);
                }
            }
        }
    }

    #[test]
    fn ac021_truncated_implements_clone_partial_eq_debug() {
        let a = truncate("hello", 100, "x");
        let b = truncate("hello", 100, "x");
        assert_eq!(a, b);
        assert_eq!(a.clone(), a);
        let debug_str = format!("{a:?}");
        assert!(!debug_str.is_empty());
    }
}

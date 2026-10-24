#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # Truncation
//!
//! UTF-8 char-boundary safe content truncation primitives for michi and AXI.

/// The marker [`truncate()`] ends with when `max_chars` is too small for the full suffix (U+2026 HORIZONTAL
/// ELLIPSIS).
const ELLIPSIS: &str = "\u{2026}";

/// Result of a truncation operation.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Truncated {
    /// The truncated (or original) content string.
    pub content: String,
    /// Length of the input in UTF-8 **bytes** (`content.len()` of the argument), not in chars, whether or not
    /// truncation occurred. The truncation suffix counts chars; use `content.chars().count()` on the input for the
    /// char length.
    pub original_len: usize,
    /// Whether truncation actually occurred.
    pub was_truncated: bool,
    /// The truncation signal text embedded at the end of `content`, right after the kept characters (leading space
    /// stripped): `"({removed} more chars — use {hint})"`, `"({removed} more chars)"` when the hint is empty, or
    /// `"…"` when `max_chars` was too small for the full suffix. `None` when not truncated or when `max_chars` is 0.
    pub signal: Option<String>,
}

/// Number of decimal digits in `n`.
fn digits(mut n: usize) -> usize {
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

/// Truncate `content` to at most `max_chars` Unicode scalar values, appending an agent-readable suffix when
/// truncation occurs.
///
/// The marker is plain text inside the returned string, so data can imitate it. Use this for a single free-text
/// payload, not for a TOON cell: the TOON renderer reports its cuts out of band instead.
///
/// The suffix is `" ({removed} more chars — use {hint})"`, where `removed` is the number of chars cut (the input's
/// char count minus the chars kept, not the original length). The `— use {hint}` clause is the caller's instruction:
/// michi never invents one, and an empty `hint` drops the clause, giving `" ({removed} more chars)"`. The whole
/// result, suffix included, never exceeds `max_chars` chars, and as many chars as fit are kept.
///
/// When `max_chars` cannot hold the full suffix plus at least one kept char, the result is instead the first
/// `max_chars - 1` chars followed by `…` (just `…` for `max_chars == 1`, and the empty string for `max_chars == 0`),
/// never a fragment of the suffix.
///
/// Uses char boundaries — never splits a Unicode scalar. Returns the original string unchanged when
/// `content.chars().count() <= max_chars`.
#[must_use]
pub fn truncate(content: &str, max_chars: usize, hint: &str) -> Truncated {
    truncate_with(content, max_chars, hint, clause_chars(hint))
}

/// Chars in the `" — use {hint}"` clause, or 0 for an empty hint (no clause).
fn clause_chars(hint: &str) -> usize {
    if hint.is_empty() {
        0
    } else {
        " \u{2014} use ".chars().count() + hint.chars().count()
    }
}

/// [`truncate()`] with the hint clause's char count already known.
fn truncate_with(content: &str, max_chars: usize, hint: &str, clause_chars: usize) -> Truncated {
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

    if max_chars == 0 {
        return Truncated { content: String::new(), original_len: content.len(), was_truncated: true, signal: None };
    }

    // Suffix length without the digits of `removed`: " (" + " more chars" + [" — use " + hint] + ")".
    let fixed_chars = 2 + " more chars".len() + clause_chars + 1;
    let fits = |keep: usize| keep + fixed_chars + digits(char_count - keep) <= max_chars;

    // `removed` never has more digits than `char_count`, so this keep count fits whenever it is positive. Keeping more
    // chars shrinks `removed`, which can drop a digit and make room for one more kept char (the loop runs at most a
    // few times, once per dropped digit).
    let mut keep = max_chars.saturating_sub(fixed_chars + digits(char_count));
    while keep + 1 < char_count && fits(keep + 1) {
        keep += 1;
    }

    let (keep, suffix) = if keep >= 1 {
        let removed = (char_count - keep).to_string();
        let mut suffix = String::with_capacity(fixed_chars + removed.len() + 4 + hint.len());
        suffix.push_str(" (");
        suffix.push_str(&removed);
        suffix.push_str(" more chars");
        if !hint.is_empty() {
            suffix.push_str(" \u{2014} use ");
            suffix.push_str(hint);
        }
        suffix.push(')');
        (keep, suffix)
    } else {
        (max_chars - 1, ELLIPSIS.to_string())
    };

    let byte_end = content.char_indices().nth(keep).map_or(content.len(), |(i, _)| i);
    let mut result = String::with_capacity(byte_end + suffix.len());
    result.push_str(&content[..byte_end]);
    result.push_str(&suffix);

    let signal = Some(suffix.trim_start().to_string());
    Truncated { content: result, original_len: content.len(), was_truncated: true, signal }
}

/// Truncate content for inline use, returning just the final string.
///
/// Same rules as [`truncate()`], including an empty `hint` meaning no `— use …` clause.
///
/// The marker is part of the returned text, so it is only as trustworthy as the text around it: do not put the result
/// in a TOON cell, where the cell's own data could end in the same words. The TOON renderer cuts cells itself and
/// reports the cut out of band (see [`crate::toon::ToonOptions::max_cell_len`]).
#[must_use]
pub fn truncate_inline(content: &str, max_chars: usize, hint: &str) -> String {
    truncate(content, max_chars, hint).content
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent oracle for the suffix form: rebuilds the expected output by brute force (the largest kept count
    /// whose suffix still fits) and checks every rule, so the matrix test below does not just re-run the
    /// implementation's arithmetic.
    fn check_rules(content: &str, max_chars: usize, hint: &str) {
        let n = content.chars().count();
        let t = truncate(content, max_chars, hint);
        let out_chars = t.content.chars().count();
        assert!(out_chars <= max_chars, "{out_chars} > {max_chars}: {:?}", t.content);
        if n <= max_chars {
            assert!(!t.was_truncated);
            assert_eq!(t.content, content);
            assert_eq!(t.signal, None);
            return;
        }
        assert!(t.was_truncated);
        if max_chars == 0 {
            assert_eq!(t.content, "");
            assert_eq!(t.signal, None);
            return;
        }
        let clause = if hint.is_empty() { String::new() } else { format!(" \u{2014} use {hint}") };
        let fits = |k: usize| k + format!(" ({} more chars{clause})", n - k).chars().count() <= max_chars;
        match (1..n).rev().find(|&k| fits(k)) {
            Some(k) => {
                let kept: String = content.chars().take(k).collect();
                let suffix = format!(" ({} more chars{clause})", n - k);
                assert_eq!(t.content, format!("{kept}{suffix}"), "n={n} max={max_chars} hint={hint:?}");
                assert_eq!(t.signal.as_deref(), Some(suffix.trim_start()));
            }
            None => {
                let kept: String = content.chars().take(max_chars - 1).collect();
                assert_eq!(t.content, format!("{kept}\u{2026}"), "n={n} max={max_chars} hint={hint:?}");
                assert_eq!(t.signal.as_deref(), Some("\u{2026}"));
            }
        }
    }

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
        assert!(t.content.contains("more chars"));
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
    fn suffix_counts_removed_chars_not_original_length() {
        // 100 chars, cap 40, hint "x": 16 kept + " (84 more chars — use x)" (24) = 40, and 16 + 84 = 100.
        let content = "a".repeat(100);
        let t = truncate(&content, 40, "x");
        assert!(t.was_truncated);
        assert!(t.content.contains("84 more chars"), "got: {:?}", t.content);
        assert!(!t.content.contains("100"), "the original length must not be reported, got: {:?}", t.content);
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
        assert_eq!(t.signal.as_deref(), Some("(183 more chars — use full=true)"));
    }

    #[test]
    fn signal_is_none_when_not_truncated() {
        let t = truncate("hello", 100, "full=true");
        assert!(!t.was_truncated);
        assert_eq!(t.signal, None);
    }

    #[test]
    fn signal_reflects_what_is_actually_in_content() {
        let long_content = "hello world this is some fairly long content for testing";
        let result = truncate(long_content, 5, "full=true");
        assert_eq!(result.content, "hell…");
        let sig = result.signal.as_deref().expect("a truncated non-zero cap always carries a signal");
        assert!(result.content.ends_with(sig));
    }

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    #[test]
    fn test_auto_traits() {
        assert_send_sync_static::<Truncated>();
    }

    #[test]
    fn ac017_exact_output_at_200_chars_50_max() {
        // " (183 more chars — use full=true)" is 33 chars, so 17 are kept, and 17 + 183 = 200.
        let t = truncate(&"a".repeat(200), 50, "full=true");
        assert!(t.was_truncated);
        assert_eq!(t.content.chars().count(), 50);
        assert_eq!(t.content, format!("{} (183 more chars — use full=true)", "a".repeat(17)));
        assert_eq!(t.signal, Some("(183 more chars — use full=true)".to_string()));
    }

    #[test]
    fn ac004_suffix_present_with_leading_space_at_exact_fit() {
        let content = "a".repeat(100);
        // " (94 more chars — use x)" is 24 chars: 6 + 24 = 30.
        let t = truncate(&content, 30, "x");
        assert_eq!(t.content, "aaaaaa (94 more chars — use x)");
        assert_eq!(t.content.chars().count(), 30);
        assert_eq!(t.signal, Some("(94 more chars — use x)".to_string()));

        let t = truncate(&content, 40, "x");
        assert_eq!(t.content, format!("{} (84 more chars — use x)", "a".repeat(16)));

        let t = truncate(&content, 50, "x");
        assert_eq!(t.content, format!("{} (74 more chars — use x)", "a".repeat(26)));
    }

    #[test]
    fn removed_count_digit_width_is_accounted_for() {
        // 105 chars, cap 20, no hint: 3 kept + " (102 more chars)" (17) = 20.
        let t = truncate(&"a".repeat(105), 20, "");
        assert_eq!(t.content, "aaa (102 more chars)");
        // 101 chars, cap 20: keeping 4 drops the count to two digits, and " (97 more chars)" (16) fits.
        let t = truncate(&"a".repeat(101), 20, "");
        assert_eq!(t.content, "aaaa (97 more chars)");
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
        let t = truncate(content, 5, "x");
        assert_eq!(t.original_len, 51, "byte length of the input, also when truncated");
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
                assert_eq!(t.signal, None);
            }
        }
    }

    #[test]
    fn ac016_empty_hint_produces_no_use_clause() {
        // " (76 more chars)" is 16 chars: 24 + 16 = 40, and 24 + 76 = 100.
        let content = "a".repeat(100);
        let t = truncate(&content, 40, "");
        assert_eq!(t.content, format!("{} (76 more chars)", "a".repeat(24)));
        assert_eq!(t.content.chars().count(), 40);
        assert!(!t.content.contains("use"));
        assert!(!t.content.contains('\u{2014}'));
        assert_eq!(t.signal, Some("(76 more chars)".to_string()));
    }

    #[test]
    fn ac018_multibyte_hint_chars_feed_keep_chars_arithmetic() {
        // 51 chars, cap 35, hint "見よ": " (41 more chars — use 見よ)" is 25 chars, so 10 are kept.
        let content = "こんにちは世界！これはテストです。".repeat(3);
        let t = truncate(&content, 35, "見よ");
        assert!(t.was_truncated);
        assert_eq!(t.original_len, 153);
        assert_eq!(t.content, "こんにちは世界！これ (41 more chars — use 見よ)");
        assert_eq!(t.content.chars().count(), 35);
        assert_eq!(t.signal, Some("(41 more chars — use 見よ)".to_string()));
    }

    #[test]
    fn ac019_tiny_caps_fall_back_to_ellipsis_never_a_fragment() {
        let content = "a".repeat(100);

        let t = truncate(&content, 3, "x");
        assert!(t.was_truncated);
        assert_eq!(t.content, "aa…");
        assert_eq!(t.signal, Some("…".to_string()));

        let t = truncate(&content, 2, "x");
        assert_eq!(t.content, "a…");

        let t = truncate(&content, 1, "x");
        assert!(t.was_truncated);
        assert_eq!(t.content, "…");
        assert_eq!(t.signal, Some("…".to_string()));

        // 24 cannot hold one kept char plus " (99 more chars — use x)" (24): ellipsis fallback.
        let t = truncate(&content, 24, "x");
        assert_eq!(t.content, format!("{}…", "a".repeat(23)));
        assert_eq!(t.signal, Some("…".to_string()));

        // 25 is the smallest cap that carries the full suffix.
        let t = truncate(&content, 25, "x");
        assert_eq!(t.content, "a (99 more chars — use x)");
        assert_eq!(t.signal, Some("(99 more chars — use x)".to_string()));

        let t = truncate(&content, 28, "x");
        assert_eq!(t.content, "aaaa (96 more chars — use x)");
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
    fn rules_hold_across_lengths_caps_and_hints() {
        for n in [1usize, 2, 9, 10, 11, 99, 100, 101, 105, 999, 1000, 1001, 1200] {
            let content = "é".repeat(n);
            for max_chars in 0..70 {
                for hint in ["", "x", "full=true", "見よ"] {
                    check_rules(&content, max_chars, hint);
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

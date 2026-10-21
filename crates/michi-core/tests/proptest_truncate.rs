use michi_core::truncate::{truncate, truncate_inline};
use proptest::prelude::*;

proptest! {
    #[test]
    fn truncate_inline_never_exceeds_limit_plus_signal(
        content in ".{0,500}",
        limit in 1usize..100,
    ) {
        let hint = "full=true";
        let result = truncate_inline(&content, limit, hint);
        // truncate() sizes the kept prefix so the result, suffix or `…` included, never exceeds `limit` chars at all
        // (not limit + signal_len).
        prop_assert!(result.chars().count() <= limit, "result {} chars exceeds limit {limit}: {result:?}", result.chars().count());
    }

    #[test]
    fn truncate_inline_never_splits_utf8(content in ".{0,200}", limit in 1usize..50) {
        let result = truncate_inline(&content, limit, "full=true");
        prop_assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn suffix_reports_exactly_the_chars_cut(
        content in ".{0,500}",
        limit in 0usize..120,
        hint in prop::sample::select(vec!["", "x", "full=true"]),
    ) {
        let t = truncate(&content, limit, hint);
        let n = content.chars().count();
        prop_assert_eq!(t.was_truncated, n > limit);
        let Some(signal) = t.signal.as_deref() else { return Ok(()) };
        if signal == "\u{2026}" {
            // Ellipsis fallback: limit - 1 kept chars, then `…`.
            let kept: String = content.chars().take(limit - 1).collect();
            prop_assert_eq!(t.content, format!("{kept}\u{2026}"));
            return Ok(());
        }
        let clause = if hint.is_empty() { String::new() } else { format!(" \u{2014} use {hint}") };
        let inner = signal.strip_prefix('(').and_then(|s| s.strip_suffix(&format!(" more chars{clause})")));
        let removed: usize = inner.and_then(|d| d.parse().ok()).ok_or_else(|| TestCaseError::fail(format!("bad signal {signal:?}")))?;
        let kept_chars = t.content.chars().count() - signal.chars().count() - 1;
        prop_assert_eq!(kept_chars + removed, n, "kept + removed must equal the input char count: {:?}", t.content);
        prop_assert!(kept_chars >= 1);
        let kept: String = content.chars().take(kept_chars).collect();
        let prefix = format!("{kept} (");
        prop_assert!(t.content.starts_with(&prefix));
    }
}

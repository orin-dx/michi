use michi_core::truncate::truncate_inline;
use proptest::prelude::*;

proptest! {
    #[test]
    fn truncate_inline_never_exceeds_limit_plus_signal(
        content in ".{0,500}",
        limit in 1usize..100,
    ) {
        let hint = "full=true";
        let result = truncate_inline(&content, limit, hint);
        // The hard-cap logic in truncate() guarantees the result never exceeds
        // `limit` chars at all (not limit + signal_len) — see
        // crates/michi-truncate/src/lib.rs's final clamp.
        prop_assert!(result.chars().count() <= limit, "result {} chars exceeds limit {limit}: {result:?}", result.chars().count());
    }

    #[test]
    fn truncate_inline_never_splits_utf8(content in ".{0,200}", limit in 1usize..50) {
        let result = truncate_inline(&content, limit, "full=true");
        prop_assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }
}

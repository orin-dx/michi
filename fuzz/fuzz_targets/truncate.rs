//! Truncation never splits a char, never exceeds its bound, keeps a prefix of the input and reports the cut out of band.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::{truncate, truncate_by_count, truncate_by_size};

#[derive(Arbitrary, Debug)]
struct Input {
    content: String,
    /// Small limits hit the ellipsis edge cases; a raw `usize` hits the overflow ones.
    small_max: u8,
    big_max: usize,
    use_big: bool,
    guidance: String,
    items: Vec<(u8, u32)>,
    budget: usize,
    count_max: u8,
}

fuzz_target!(|input: Input| {
    let max = if input.use_big { input.big_max } else { usize::from(input.small_max) };
    let chars = input.content.chars().count();

    let t = truncate(&input.content, max);
    assert_eq!(t.original_chars, chars);
    assert!(t.content.chars().count() <= max, "{} > {max}: {:?}", t.content.chars().count(), t.content);

    if chars <= max {
        // Fits: returned unchanged, nothing reported.
        assert!(!t.was_truncated());
        assert_eq!(t.removed, 0);
        assert_eq!(t.content, input.content);
        assert!(t.hint(input.guidance.as_str()).is_none());
    } else {
        // Cut: the first `max - 1` chars (none for a cap of 0) plus a plain `…`, exactly `max` chars, and the
        // removed count is everything not kept. The text carries no message: only the ellipsis was added.
        assert!(t.was_truncated());
        let kept_chars = max.saturating_sub(1);
        assert_eq!(t.removed, chars - kept_chars);
        let want: String =
            if max == 0 { String::new() } else { input.content.chars().take(kept_chars).chain(['\u{2026}']).collect() };
        assert_eq!(t.content, want);
        assert_eq!(t.content.chars().count(), max);
        // The report lives in the hint, in the form michi documents.
        let hint = t.hint(input.guidance.as_str()).expect("a cut has a hint");
        let base = format!("cut {} of {chars} chars", t.removed);
        let want_hint = if input.guidance.is_empty() { base } else { format!("{base}; {}", input.guidance) };
        assert_eq!(hint.as_str(), want_hint);
    }

    // truncate_by_count: a prefix, at most `max` long, with the total recorded.
    let items: Vec<u8> = input.items.iter().map(|(b, _)| *b).collect();
    let by_count = truncate_by_count(items.clone(), usize::from(input.count_max));
    assert_eq!(by_count.total_before, items.len());
    assert_eq!(by_count.kept, items[..by_count.kept.len()]);
    assert_eq!(by_count.kept.len(), items.len().min(usize::from(input.count_max)));
    assert_eq!(by_count.omitted(), items.len() - by_count.kept.len());
    assert_eq!(by_count.truncated(), by_count.kept.len() < items.len());

    // truncate_by_size: a prefix whose summed size fits, stopping at the first item that would not.
    let sized = input.items.clone();
    let by_size = truncate_by_size(sized.clone(), input.budget, |(_, size)| *size as usize);
    assert_eq!(by_size.total_before, sized.len());
    assert_eq!(by_size.kept, sized[..by_size.kept.len()]);
    let used: u128 = by_size.kept.iter().map(|(_, s)| u128::from(*s)).sum();
    assert!(used <= input.budget as u128);
    if let Some((_, next)) = sized.get(by_size.kept.len()) {
        assert!(used + u128::from(*next) > input.budget as u128, "stopped early");
    }
});

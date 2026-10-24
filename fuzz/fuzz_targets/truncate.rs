//! Truncation never splits a char, never exceeds its bound, keeps a prefix of the input and reports the cut exactly.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::{truncate, truncate_by_count, truncate_by_size, truncate_inline};

#[derive(Arbitrary, Debug)]
struct Input {
    content: String,
    hint: String,
    /// Small limits hit the suffix/ellipsis edge cases; a raw `usize` hits the overflow ones.
    small_max: u8,
    big_max: usize,
    use_big: bool,
    items: Vec<(u8, u32)>,
    budget: usize,
    count_max: u8,
}

fuzz_target!(|input: Input| {
    let max = if input.use_big { input.big_max } else { usize::from(input.small_max) };
    let chars = input.content.chars().count();

    let t = truncate(&input.content, max, &input.hint);
    assert_eq!(t.original_len, input.content.len());
    assert!(t.content.chars().count() <= max, "{} > {max}: {:?}", t.content.chars().count(), t.content);
    assert_eq!(truncate_inline(&input.content, max, &input.hint), t.content);

    if chars <= max {
        assert!(!t.was_truncated);
        assert_eq!(t.content, input.content);
        assert_eq!(t.signal, None);
    } else if max == 0 {
        assert!(t.was_truncated);
        assert_eq!(t.content, "");
        assert_eq!(t.signal, None);
    } else {
        assert!(t.was_truncated);
        let signal = t.signal.as_deref().expect("a truncated result carries its signal");
        let kept = if signal == "\u{2026}" {
            let kept = t.content.strip_suffix('\u{2026}').expect("ends with the ellipsis");
            assert_eq!(kept.chars().count(), max - 1);
            kept
        } else {
            let kept = t.content.strip_suffix(signal).and_then(|s| s.strip_suffix(' ')).expect("ends with the signal");
            let clause = if input.hint.is_empty() { String::new() } else { format!(" \u{2014} use {}", input.hint) };
            let removed = chars - kept.chars().count();
            assert_eq!(signal, format!("({removed} more chars{clause})"));
            assert!(kept.chars().count() >= 1);
            kept
        };
        // The kept part is a whole-char prefix of the input.
        assert!(input.content.starts_with(kept));
        assert!(input.content.is_char_boundary(kept.len()));
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

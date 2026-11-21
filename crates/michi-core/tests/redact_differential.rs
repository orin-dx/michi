//! `redact_url_userinfo` is one forward pass; this pins its output to the straightforward version it replaced.
//!
//! The reference below is the earlier implementation, kept as written: easy to read, and quadratic (it searches the
//! rest of the text for every separator form, and for the URL's end and last `@`, once per URL). The linear function
//! must return the same text for every input, so the corpus is hostile on purpose: every separator form, quotes,
//! `@` and `%40`, the characters that end a URL or an authority, Unicode whitespace and multi-byte text, cut off at
//! every point.

use michi_core::redact::{redact_common_shapes, redact_url_userinfo, REDACTED};

mod reference {
    use super::REDACTED;

    pub fn redact_url_userinfo(text: &str) -> String {
        settle(text, userinfo_pass)
    }

    fn settle(text: &str, pass: fn(&str) -> String) -> String {
        const MAX_PASSES: usize = 8;
        let mut current = pass(text);
        for _ in 0..MAX_PASSES {
            let next = pass(&current);
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn userinfo_pass(text: &str) -> String {
        let upper = text.to_ascii_uppercase();
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        while let Some((i, len, encoded)) = next_separator(&upper, pos) {
            let start = i + len;
            out.push_str(&text[pos..start]);
            let tail = &text[start..];
            let end = tail.find(is_url_end).unwrap_or(tail.len());
            let found =
                last_at(&tail[..end], encoded).or_else(|| at_past_quote(tail, end, encoded)).filter(|at| *at > 0);
            match found {
                Some(at) => {
                    out.push_str(REDACTED);
                    pos = start + at;
                }
                None => pos = start,
            }
        }
        out.push_str(&text[pos..]);
        out
    }

    fn next_separator(upper: &str, from: usize) -> Option<(usize, usize, bool)> {
        const SEPARATORS: [(&str, bool); 3] = [("://", false), (":\\/\\/", false), ("%3A%2F%2F", true)];
        SEPARATORS
            .iter()
            .filter_map(|(sep, encoded)| upper[from..].find(sep).map(|i| (from + i, sep.len(), *encoded)))
            .min_by_key(|(i, _, _)| *i)
    }

    fn last_at(url: &str, encoded: bool) -> Option<usize> {
        let plain = url.rfind('@');
        if !encoded {
            return plain;
        }
        let pct = url.to_ascii_uppercase().rfind("%40");
        plain.max(pct)
    }

    fn at_past_quote(tail: &str, end: usize, encoded: bool) -> Option<usize> {
        if !tail[end..].starts_with(['"', '\'']) {
            return None;
        }
        let region_end = tail.find(|c: char| c.is_whitespace() || matches!(c, '`' | '<' | '>')).unwrap_or(tail.len());
        let region = &tail[..region_end];
        let stop = region.find(['/', '\\', '?', '#']).unwrap_or(region.len());
        let candidate = &region[..stop];
        let at = if encoded { last_at(candidate, true) } else { candidate.find('@') };
        at.filter(|at| *at >= end)
    }

    fn is_url_end(c: char) -> bool {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
    }
}

/// Pieces the corpus is built from. Each one matters to a branch of the scan.
const PIECES: [&str; 30] = [
    "://",
    ":\\/\\/",
    "%3A%2F%2F",
    "%3a%2f%2f",
    ":/",
    "//",
    "@",
    "%40",
    "%4",
    "%",
    "\"",
    "'",
    "`",
    "<",
    ">",
    " ",
    "\n",
    "\u{3000}",
    "/",
    "\\",
    "?",
    "#",
    ":",
    "a",
    "u:p",
    "http",
    "[REDACTED]",
    "é",
    "😀",
    "40",
];

/// A small deterministic generator, so a failure reproduces and nothing here needs a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() >> 33).unwrap_or(0) % n
    }
}

fn check(text: &str) {
    let want = reference::redact_url_userinfo(text);
    let got = redact_url_userinfo(text);
    assert_eq!(got, want, "redact_url_userinfo differs from the reference for {text:?}");
}

#[test]
fn every_short_sequence_of_pieces_matches_the_reference() {
    // All sequences of up to four pieces from the 14 that decide where a URL starts, ends and has its `@`: 14^4 + ...
    // is about 40,000 texts.
    let core = ["://", "%3A%2F%2F", ":\\/\\/", "@", "%40", "\"", "'", " ", "/", "?", "a", "<", "%", "40"];
    let mut stack: Vec<Vec<&str>> = vec![Vec::new()];
    while let Some(parts) = stack.pop() {
        check(&parts.concat());
        if parts.len() < 4 {
            for piece in core {
                let mut longer = parts.clone();
                longer.push(piece);
                stack.push(longer);
            }
        }
    }
}

#[test]
fn random_and_hostile_texts_match_the_reference() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..60_000 {
        let pieces = rng.below(40);
        let mut text = String::new();
        for _ in 0..pieces {
            text.push_str(PIECES[rng.below(PIECES.len())]);
        }
        check(&text);
        // Cut at every character boundary near the end: a separator or `%40` can be half there.
        for cut in text.char_indices().map(|(i, _)| i).rev().take(6) {
            check(&text[..cut]);
        }
    }
}

#[test]
fn repeated_units_match_the_reference() {
    // The shapes the scaling tests use, and the ones that make a rescan visible, at a few repeat counts.
    let units = [
        "a://b ",
        "a://",
        "a://\"",
        "a://x'",
        "%3A%2F%2F\"",
        "%3a%2f%2fb%40",
        "a:\\/\\/b ",
        "a://u:p@h ",
        "a://p@\"",
        "a://x%40\"",
        "a://\"x@y ",
        "%3A%2F%2F\"x%40y ",
        "a://x/\"y@z ",
        "http://u:p\"w@h/db ",
        "http://@",
    ];
    for unit in units {
        for n in [1, 2, 3, 5, 8] {
            check(&unit.repeat(n));
            check(&format!("{}{}", unit.repeat(n), "tail@"));
            check(&format!("lead@{}", unit.repeat(n)));
        }
    }
}

#[test]
fn common_shapes_still_agree_on_urls() {
    // `redact_common_shapes` runs the URL pass inside its own; with no credential pair or token in the text the two
    // must give one answer, and it is the reference's.
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for _ in 0..5_000 {
        let mut text = String::new();
        for _ in 0..rng.below(30) {
            text.push_str(["://", "%3A%2F%2F", ":\\/\\/", "@", "%40", "\"", " ", "a", "/", "'"][rng.below(10)]);
        }
        let want = reference::redact_url_userinfo(&text);
        // `a` and `/` and `:` never form a secret key, so only the URL pass can change the text.
        assert_eq!(redact_common_shapes(&text), want, "redact_common_shapes differs for {text:?}");
    }
}

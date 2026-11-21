//! Black-box tests for the redaction helpers.

use michi_core::redact::{redact_secrets, redact_url_userinfo, REDACTED};
use michi_core::{DomainError, ErrorCode};

#[test]
fn secrets_are_replaced_everywhere() {
    let out = redact_secrets("a sk-live-1 b sk-live-1 c", &["sk-live-1"]);
    assert_eq!(out, format!("a {REDACTED} b {REDACTED} c"));
}

#[test]
fn empty_secrets_are_ignored() {
    assert_eq!(redact_secrets("abc", &["", ""]), "abc");
}

#[test]
fn no_secrets_returns_the_text_unchanged() {
    assert_eq!(redact_secrets("abc", &[]), "abc");
}

#[test]
fn the_longest_secret_wins_when_one_contains_another() {
    // Replacing "abc" first would leave "[REDACTED]def" and leak nothing, but replacing "abcdef" last would miss.
    let out = redact_secrets("key=abcdef", &["abc", "abcdef"]);
    assert_eq!(out, format!("key={REDACTED}"));
}

#[test]
fn redaction_is_idempotent() {
    let once = redact_secrets("x tok y", &["tok"]);
    assert_eq!(redact_secrets(&once, &["tok"]), once);
}

#[test]
fn a_secret_inside_the_marker_does_not_loop_or_grow() {
    let out = redact_secrets("RED", &["RED", "REDACTED"]);
    assert_eq!(out, REDACTED);
}

#[test]
fn userinfo_is_removed_from_urls() {
    assert_eq!(
        redact_url_userinfo("fetch https://user:pw@example.com/a failed"),
        format!("fetch https://{REDACTED}@example.com/a failed")
    );
}

#[test]
fn userinfo_without_a_password_is_removed() {
    assert_eq!(redact_url_userinfo("git://token@host/r"), format!("git://{REDACTED}@host/r"));
}

#[test]
fn a_password_containing_at_and_slash_is_fully_removed() {
    let out = redact_url_userinfo("postgres://u:p@ss/word@db.internal:5432/app");
    assert_eq!(out, format!("postgres://{REDACTED}@db.internal:5432/app"));
    assert!(!out.contains("ss/word"));
}

#[test]
fn urls_without_userinfo_are_untouched() {
    let s = "see https://example.com/a?b=c and http://localhost:8080/";
    assert_eq!(redact_url_userinfo(s), s);
}

#[test]
fn several_urls_in_one_text_are_each_redacted() {
    let out = redact_url_userinfo("a https://u:p@h1/x b ssh://k@h2 c");
    assert_eq!(out, format!("a https://{REDACTED}@h1/x b ssh://{REDACTED}@h2 c"));
}

#[test]
fn quotes_and_brackets_end_a_url() {
    let out = redact_url_userinfo("(\"https://u:p@h/x\") 'a@b'");
    assert_eq!(out, format!("(\"https://{REDACTED}@h/x\") 'a@b'"));
}

#[test]
fn an_email_address_is_not_a_url() {
    assert_eq!(redact_url_userinfo("mail me@example.com"), "mail me@example.com");
}

#[test]
fn url_redaction_is_idempotent() {
    let once = redact_url_userinfo("https://u:p@h/x");
    assert_eq!(redact_url_userinfo(&once), once);
}

#[test]
fn text_with_multibyte_characters_survives() {
    let out = redact_url_userinfo("é https://ü:ß@h/é ✓");
    assert_eq!(out, format!("é https://{REDACTED}@h/é ✓"));
}

#[test]
fn from_error_redacts_url_userinfo_in_the_message() {
    #[derive(Debug, thiserror::Error)]
    #[error("clone of https://bot:ghp_secret@github.com/o/r failed")]
    struct CloneFailed;

    let e = DomainError::from_error(ErrorCode::ExternalFailure, &CloneFailed);
    assert_eq!(e.message(), format!("clone of https://{REDACTED}@github.com/o/r failed"));
    assert!(!e.render(michi_core::OutputFormat::Text).contains("ghp_secret"));
}

#[test]
fn from_error_with_causes_redacts_url_userinfo_in_every_cause() {
    #[derive(Debug, thiserror::Error)]
    #[error("fetch https://u:p@h/x: {0}")]
    struct Inner(String);

    #[derive(Debug, thiserror::Error)]
    #[error("sync failed")]
    struct Outer(#[source] Inner);

    let e = DomainError::from_error_with_causes(ErrorCode::ExternalFailure, &Outer(Inner("boom".into())));
    assert_eq!(e.causes(), [format!("fetch https://{REDACTED}@h/x: boom")]);
}

#[test]
fn redacting_again_is_a_no_op_even_when_a_secret_matches_part_of_the_marker() {
    for secrets in [&["RED"][..], &["ACTED"], &["D"], &["x", "RED"]] {
        let once = redact_secrets("a x RED b", secrets);
        assert_eq!(redact_secrets(&once, secrets), once, "{secrets:?}");
    }
    // The marker already in the text is kept, not re-redacted.
    assert_eq!(redact_secrets("[REDACTED] RED", &["RED", "ED"]), "[REDACTED] [REDACTED]");
}

// A `[REDACTED]` already in the text is copied through only where no known secret matches at that position. A secret
// that matches there starts with the marker, or with a prefix of it, and it must win, or its tail is left in the output.

#[test]
fn a_secret_that_starts_with_the_marker_is_redacted_whole() {
    let secret = "[REDACTED]private-tail";
    assert_eq!(redact_secrets(secret, &[secret]), REDACTED);
    assert_eq!(redact_secrets(&format!("pre {secret} post"), &[secret]), format!("pre {REDACTED} post"));
}

#[test]
fn a_secret_that_contains_the_marker_is_redacted_whole() {
    for secret in ["private[REDACTED]tail", "private[REDACTED]", "[REDACTED]", "a[REDACTED]b[REDACTED]c"] {
        assert_eq!(redact_secrets(&format!("x{secret}y"), &[secret]), format!("x{REDACTED}y"), "secret {secret:?}");
    }
}

#[test]
fn a_secret_that_equals_the_marker_changes_nothing() {
    assert_eq!(redact_secrets("a[REDACTED]b", &[REDACTED]), format!("a{REDACTED}b"));
}

#[test]
fn a_secret_inside_the_marker_is_left_alone_only_inside_it() {
    // These occur in the output only as part of a marker that was already there: the one place a known secret is
    // left alone.
    for secret in ["RED", "ACTED", "D]", "REDACTED", "]", "ED]"] {
        assert_eq!(redact_secrets("[REDACTED]", &[secret]), REDACTED, "secret {secret:?}");
    }
    // Outside the marker the same secrets are redacted.
    assert_eq!(redact_secrets("RED [REDACTED] RED", &["RED"]), format!("{REDACTED} {REDACTED} {REDACTED}"));
    assert_eq!(redact_secrets("a] [REDACTED]", &["]"]), format!("a{REDACTED} {REDACTED}"));
}

#[test]
fn a_secret_that_starts_with_part_of_the_marker_wins_over_the_pass_through() {
    // Documented limit: a secret that starts with `[` matches at the marker's first byte, so it wins there and the rest
    // of the marker is scanned as ordinary text.
    assert_eq!(redact_secrets("[REDACTED]", &["["]), format!("{REDACTED}REDACTED]"));
    assert_eq!(redact_secrets("[REDACTED]", &["[RED"]), format!("{REDACTED}ACTED]"));
}

#[test]
fn the_longest_secret_wins_over_a_shorter_one_and_over_the_marker() {
    let secrets = ["abc", "abc[REDACTED]", "abc[REDACTED]tail"];
    assert_eq!(redact_secrets("abc[REDACTED]tail", &secrets), REDACTED);
    assert_eq!(redact_secrets("abc[REDACTED]tai", &secrets), format!("{REDACTED}tai"));
    assert_eq!(redact_secrets("abc z", &secrets), format!("{REDACTED} z"));
    // The order of the list does not matter.
    let reversed = ["abc[REDACTED]tail", "abc[REDACTED]", "abc"];
    assert_eq!(redact_secrets("abc[REDACTED]tail", &reversed), REDACTED);
}

#[test]
fn a_secret_next_to_a_marker_is_redacted_on_both_sides() {
    assert_eq!(redact_secrets("[REDACTED]tok", &["tok"]), format!("{REDACTED}{REDACTED}"));
    assert_eq!(redact_secrets("tok[REDACTED]", &["tok"]), format!("{REDACTED}{REDACTED}"));
    assert_eq!(redact_secrets("tok[REDACTED]tok", &["tok"]), format!("{REDACTED}{REDACTED}{REDACTED}"));
}

#[test]
fn several_markers_and_secrets_in_one_text() {
    let text = "[REDACTED] a tok [REDACTED]tok[REDACTED] RED [REDACTED]";
    let want = format!("{REDACTED} a {REDACTED} {REDACTED}{REDACTED}{REDACTED} {REDACTED} {REDACTED}");
    assert_eq!(redact_secrets(text, &["tok", "RED"]), want);
}

/// A small deterministic generator, so a failure reproduces from the printed case and the corpus needs no extra crate.
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
        usize::try_from(self.next() % n as u64).unwrap_or(0)
    }

    /// Random text from marker fragments, bracket characters and a few plain letters, so markers, near-markers and
    /// secrets overlap often. Without `with_brackets` no piece contains `[`.
    fn text(&mut self, max_pieces: usize, with_brackets: bool) -> String {
        const PIECES: [&str; 12] = ["[REDACTED]", "RED", "ACTED", "[", "]", "D]", "a", "b", "ab", "tok", "[RED", "x"];
        let mut out = String::new();
        for _ in 0..self.below(max_pieces + 1) {
            let piece = PIECES[self.below(PIECES.len())];
            if with_brackets || !piece.contains('[') {
                out.push_str(piece);
            }
        }
        out
    }
}

/// Whether `needle` occurs in `hay` at a place that shares no byte with an occurrence of the marker.
fn occurs_away_from_markers(hay: &str, needle: &str) -> bool {
    // `match_indices` skips overlapping matches, and markers cannot overlap each other, so this finds every marker.
    let markers: Vec<(usize, usize)> = hay.match_indices(REDACTED).map(|(i, m)| (i, i + m.len())).collect();
    (0..hay.len())
        .filter(|i| hay.is_char_boundary(*i) && hay[*i..].starts_with(needle))
        .any(|i| !markers.iter().any(|(s, e)| i < *e && *s < i + needle.len()))
}

#[test]
fn over_a_random_corpus_a_known_secret_survives_only_where_it_touches_a_marker() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for case in 0..200_000 {
        let text = rng.text(8, true);
        let secrets: Vec<String> = (0..=rng.below(3)).map(|_| rng.text(3, true)).collect();
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let out = redact_secrets(&text, &refs);
        for s in refs.iter().filter(|s| !s.is_empty()) {
            assert!(
                !occurs_away_from_markers(&out, s),
                "case {case}: secret {s:?} survived away from a marker: text {text:?} secrets {refs:?} -> {out:?}"
            );
        }
    }
}

#[test]
fn over_a_random_corpus_a_secret_without_brackets_survives_only_inside_a_marker() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for case in 0..200_000 {
        let text = rng.text(8, true);
        let secrets: Vec<String> = (0..=rng.below(3)).map(|_| rng.text(3, false).replace(']', "")).collect();
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let out = redact_secrets(&text, &refs);
        let markers: Vec<(usize, usize)> = out.match_indices(REDACTED).map(|(i, m)| (i, i + m.len())).collect();
        for s in refs.iter().filter(|s| !s.is_empty()) {
            // Every start, not `match_indices`, which skips overlapping occurrences.
            for i in (0..out.len()).filter(|i| out.is_char_boundary(*i) && out[*i..].starts_with(s)) {
                assert!(
                    markers.iter().any(|(a, b)| *a <= i && i + s.len() <= *b),
                    "case {case}: secret {s:?} survived outside a marker: text {text:?} secrets {refs:?} -> {out:?}"
                );
            }
        }
    }
}

#[test]
fn over_a_random_corpus_redacting_twice_equals_once_unless_a_secret_contains_a_bracket() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for case in 0..200_000 {
        let text = rng.text(8, true);
        // No `[` in any secret: the documented condition. `]`, `RED`, `D]` and the like are allowed.
        let secrets: Vec<String> = (0..=rng.below(3)).map(|_| rng.text(3, false)).collect();
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let once = redact_secrets(&text, &refs);
        assert_eq!(redact_secrets(&once, &refs), once, "case {case}: text {text:?} secrets {refs:?}");
    }
}

#[test]
fn a_secret_with_an_opening_bracket_can_break_idempotence_so_the_limit_is_real() {
    // Without the condition the property is false, so the docs name it.
    let secrets = ["tok", "[REDACTED]x"];
    let first = redact_secrets("tokx", &secrets);
    assert_eq!(first, format!("{REDACTED}x"));
    assert_eq!(redact_secrets(&first, &secrets), REDACTED);
}

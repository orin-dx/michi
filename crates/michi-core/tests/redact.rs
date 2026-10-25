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
    assert_eq!(e.message, format!("clone of https://{REDACTED}@github.com/o/r failed"));
    assert!(!e.render().contains("ghp_secret"));
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
    assert_eq!(e.causes, [format!("fetch https://{REDACTED}@h/x: boom")]);
}

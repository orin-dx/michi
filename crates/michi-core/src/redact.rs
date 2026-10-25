//! Best-effort redaction of secrets from text before it reaches an agent.
//!
//! Two helpers, both pure and dependency-free:
//!
//! - [`redact_secrets`](crate::redact::redact_secrets) replaces values you already know are secret (a token you loaded, a password you were given).
//!   It is exact and reliable for those values.
//! - [`redact_url_userinfo`](crate::redact::redact_url_userinfo) removes the `user:password@` part of any URL in a text. It finds secrets you do not hold
//!   as a value, such as one inside an error from a library.
//!
//! Neither one finds secrets in general. A token that is not a known value and not in a URL passes through, so treat
//! them as a second line of defense after keeping secrets out of messages in the first place.
//! [`DomainError::from_error`](crate::DomainError::from_error) applies [`redact_url_userinfo`](crate::redact::redact_url_userinfo) to the text it takes
//! from an error.

/// What a redacted value is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// Replaces every occurrence of each secret in `text` with [`REDACTED`].
///
/// Empty secrets are ignored. When one secret contains another, the longer one wins, so no part of it is left behind.
/// The text is scanned once, so the marker itself is never searched again and the result is stable if you redact it a
/// second time.
///
/// ```
/// use michi_core::redact::redact_secrets;
///
/// assert_eq!(redact_secrets("auth failed for sk-live-1", &["sk-live-1"]), "auth failed for [REDACTED]");
/// ```
#[must_use]
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut secrets: Vec<&str> = secrets.iter().copied().filter(|s| !s.is_empty()).collect();
    if secrets.is_empty() {
        return text.to_owned();
    }
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(s) = secrets.iter().find(|s| rest.starts_with(**s)) {
            out.push_str(REDACTED);
            rest = &rest[s.len()..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Replaces the `user:password@` part of every `scheme://…` URL in `text` with [`REDACTED`], keeping the rest.
///
/// A URL runs from its `://` to the next whitespace, quote, backtick, `<` or `>`. Inside it, everything up to the
/// **last** `@` counts as userinfo, so a password that contains `@` or `/` is removed whole. The cost is that a URL
/// with no userinfo but an `@` later in its path (`https://host/pkg@1.2`) loses its host and path up to that `@`;
/// over-redacting is the safe direction here. Bare email addresses are not URLs and are left alone.
///
/// ```
/// use michi_core::redact::redact_url_userinfo;
///
/// assert_eq!(
///     redact_url_userinfo("clone of https://bot:ghp_x@github.com/o/r failed"),
///     "clone of https://[REDACTED]@github.com/o/r failed"
/// );
/// ```
#[must_use]
pub fn redact_url_userinfo(text: &str) -> String {
    const SEP: &str = "://";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(SEP) {
        let (head, tail) = rest.split_at(i + SEP.len());
        out.push_str(head);
        let end = tail.find(is_url_end).unwrap_or(tail.len());
        let (url, after) = tail.split_at(end);
        match url.rfind('@') {
            Some(at) => {
                out.push_str(REDACTED);
                out.push_str(&url[at..]);
            }
            None => out.push_str(url),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn is_url_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

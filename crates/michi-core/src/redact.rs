//! Best-effort redaction of secrets from text before it reaches an agent.
//!
//! Three helpers, all pure and dependency-free:
//!
//! - [`redact_secrets`](crate::redact::redact_secrets) replaces values you already know are secret (a token you
//!   loaded, a password you were given). It is exact and reliable for those values.
//! - [`redact_url_userinfo`](crate::redact::redact_url_userinfo) removes the `user:password@` part of any URL in a
//!   text, including JSON-escaped (`https:\/\/`) and percent-encoded (`https%3A%2F%2F`) URLs.
//! - [`redact_common_shapes`](crate::redact::redact_common_shapes) is the safety net
//!   [`DomainError::from_error`](crate::DomainError::from_error) applies: URL userinfo, the values of query-string and
//!   `key=value` pairs whose key names a secret (`token`, `api_key`, `password`, ...), and a few well-known token
//!   shapes (`ghp_`, `github_pat_`, `npm_`, `pypi-`, `Bearer`).
//!
//! **What this does not cover.** A secret that is not a known value, not in a URL, not in a `key=value` pair with a
//! telling key and not one of the listed token shapes passes through: a JSON field such as `"password": "x"`, a
//! password in a connection string without `://`, a cloud key with another prefix, an encoded or split value. These
//! helpers are a second line of defense after keeping secrets out of messages in the first place. "Redacted" here
//! means "we looked for these shapes", never "safe to show".

/// What a redacted value is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// Replaces every occurrence of each secret in `text` with [`REDACTED`].
///
/// Empty secrets are ignored. When one secret contains another, the longer one wins, so no part of it is left behind.
/// The text is scanned once, so the marker itself is never searched again and the result is stable if you redact it a
/// second time, unless a secret can match part of the marker (`RED`, `[`): then the second pass redacts the marker.
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
/// Three more shapes are covered:
///
/// - The JSON-escaped separator `:\/\/` (PHP and Java encoders emit it, and error bodies echo it).
/// - The percent-encoded separator `%3A%2F%2F` (either case), where the userinfo ends at the last `@` or `%40`.
/// - A quote inside the password (`postgres://u:p"w@host/db`). A quote ends a URL, so when the part before it holds no
///   `@`, the text after the quote is searched for one, as long as no `/`, `\`, `?` or `#` comes first. The cost is
///   that `"https://host","k":"a@b"` is over-redacted up to the `@`.
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
    settle(text, userinfo_pass)
}

/// Runs `pass` until the text stops changing, so the result is a fixed point and redacting it again changes nothing.
///
/// One pass can expose a shape another pass then matches (a URL nested in a password, a marker beside a quote). Each
/// change replaces non-marker text with a marker, so this settles fast; the cap only bounds hostile input.
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
        let found = last_at(&tail[..end], encoded).or_else(|| at_past_quote(tail, end, encoded)).filter(|at| *at > 0);
        match found {
            Some(at) => {
                out.push_str(REDACTED);
                // Resume at the `@`: the rest of the URL is kept and scanned for further URLs.
                pos = start + at;
            }
            // No userinfo here; keep scanning right after the separator, so a URL inside this one is still seen.
            None => pos = start,
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// The earliest URL separator at or after `from`: its byte offset, its length, and whether it is percent-encoded.
fn next_separator(upper: &str, from: usize) -> Option<(usize, usize, bool)> {
    const SEPARATORS: [(&str, bool); 3] = [("://", false), (":\\/\\/", false), ("%3A%2F%2F", true)];
    SEPARATORS
        .iter()
        // `upper` only changes ASCII letters, so its offsets are the text's; the separators are uppercase already.
        .filter_map(|(sep, encoded)| upper[from..].find(sep).map(|i| (from + i, sep.len(), *encoded)))
        .min_by_key(|(i, _, _)| *i)
}

/// Offset of the last `@` (or, in a percent-encoded URL, `%40`) in `url`.
fn last_at(url: &str, encoded: bool) -> Option<usize> {
    let plain = url.rfind('@');
    if !encoded {
        return plain;
    }
    let pct = url.to_ascii_uppercase().rfind("%40");
    plain.max(pct)
}

/// The first `@` after a quote that ended the URL at `end`, if nothing that ends an authority comes before it.
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

/// The safety net for text copied from an error: URL userinfo ([`redact_url_userinfo`]), the values of `key=value`
/// pairs whose key names a secret, and well-known token shapes. Used by
/// [`DomainError::from_error`](crate::DomainError::from_error) and the pipeline's `ExecutionError` conversion.
///
/// Covered:
///
/// - Everything [`redact_url_userinfo`] covers.
/// - A value after `key=` (query string, form body, log line) when the key, lowercased with `-` read as `_`, is `key`,
///   `sig`, `auth`, `pwd`, ends in `_key`, `_sig` or `_auth`, starts with `auth_`, or contains `token`, `secret`,
///   `passw`, `signature`, `apikey`, `api_key`, `credential` or `authoriz`. The value runs to whitespace, `&`, a
///   quote, a backtick, `)`, `<` or `>`. Other keys (`page`, `status_code`, `monkey`) are kept.
/// - `ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`, `npm_` and `pypi-` followed by at least 16 token
///   characters, and `Bearer ` (either case) followed by at least 16. A short lookalike (`pypi-mirror`) is kept.
///
/// Not covered: see the module docs. A match is a heuristic; this never makes text safe to show, it removes the
/// shapes that leaked in practice.
///
/// Redacting twice gives the same text as redacting once.
///
/// ```
/// use michi_core::redact::redact_common_shapes;
///
/// assert_eq!(
///     redact_common_shapes("GET https://api.x.com/v1?page=2&access_token=abc123 failed"),
///     "GET https://api.x.com/v1?page=2&access_token=[REDACTED] failed"
/// );
/// ```
#[must_use]
pub fn redact_common_shapes(text: &str) -> String {
    settle(text, redact_pass)
}

fn redact_pass(text: &str) -> String {
    // Token shapes first: a `Bearer` token inside `auth=Bearer xyz` would otherwise be left behind when the key=value
    // pass only takes the word after the `=`. Userinfo goes before the key=value pass so a `?` inside a password
    // cannot be read as a query.
    let shaped = redact_token_shapes(text);
    let userinfo = redact_url_userinfo(&shaped);
    redact_secret_pairs(&userinfo)
}

/// Prefixes of well-known token formats, redacted whole.
const TOKEN_PREFIXES: [&str; 8] = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "npm_", "pypi-"];
/// Fewest token characters after a prefix for it to count as a token and not a word.
const MIN_TOKEN_TAIL: usize = 16;

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '=' | '+' | '/' | '~')
}

/// Length of the token characters at the start of `s`.
fn token_tail_len(s: &str) -> usize {
    s.find(|c: char| !is_token_char(c)).unwrap_or(s.len())
}

fn redact_token_shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut prev_alnum = false;
    for (i, c) in text.char_indices() {
        let at_boundary = !prev_alnum;
        prev_alnum = c.is_alphanumeric();
        if i < pos || !at_boundary {
            continue;
        }
        let rest = &text[i..];
        // `(prefix kept, token start)`: Bearer keeps its word, the other shapes are redacted whole.
        let hit = if let Some(word) = ["Bearer ", "bearer "].iter().find(|w| rest.starts_with(**w)) {
            Some((i + word.len(), i + word.len()))
        } else {
            TOKEN_PREFIXES.iter().find(|p| rest.starts_with(**p)).map(|p| (i, i + p.len()))
        };
        let Some((redact_from, tail_start)) = hit else { continue };
        let tail = token_tail_len(&text[tail_start..]);
        if tail < MIN_TOKEN_TAIL {
            continue;
        }
        out.push_str(&text[pos..redact_from]);
        out.push_str(REDACTED);
        pos = tail_start + tail;
    }
    out.push_str(&text[pos..]);
    out
}

fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn is_value_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '&' | '"' | '\'' | '`' | ')' | '<' | '>')
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    const CONTAINS: [&str; 8] =
        ["token", "secret", "passw", "signature", "apikey", "api_key", "credential", "authoriz"];
    CONTAINS.iter().any(|w| key.contains(w))
        || matches!(key.as_str(), "key" | "sig" | "auth" | "pwd")
        || key.ends_with("_key")
        || key.ends_with("_sig")
        || key.ends_with("_auth")
        || key.starts_with("auth_")
}

fn redact_secret_pairs(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut search = 0;
    while let Some(i) = text[search..].find('=').map(|i| search + i) {
        search = i + 1;
        let mut key_start = i;
        while key_start > pos && is_key_char(bytes[key_start - 1]) {
            key_start -= 1;
        }
        if key_start == i || !is_secret_key(&text[key_start..i]) {
            continue;
        }
        let value = &text[i + 1..];
        let len = value.find(is_value_end).unwrap_or(value.len());
        if len == 0 {
            continue;
        }
        out.push_str(&text[pos..=i]);
        out.push_str(REDACTED);
        pos = i + 1 + len;
        search = pos;
    }
    out.push_str(&text[pos..]);
    out
}

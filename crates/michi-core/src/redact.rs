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
//!   `key=value` pairs and `Key: value` headers whose key names a secret (`token`, `api_key`, `password`,
//!   `Authorization`, `Cookie`, ...), quoted or not, and a few well-known token shapes (`ghp_`, `github_pat_`, `npm_`,
//!   `pypi-`, `Bearer`).
//!
//! **What this does not cover.** A secret that is not a known value, not in a URL, not in a pair or header with a
//! telling key and not one of the listed token shapes passes through. Known gaps: a secret with a space in it
//! (`token=a b` loses only `a`), a value on the line after its key (YAML `password:` then an indented value), a
//! command-line flag (`--password hunter2`), an encoded separator (`token%3Dvalue`), a key spelled with lookalike
//! letters or zero-width characters, a bare JWT, cloud key (`AKIA...`) or PEM private key block, a connection string
//! without `://`, a session id under a key that does not say so, and an encoded or split value. These helpers are a
//! second line of defense after keeping secrets out of messages in the first place. "Redacted" here means "we looked
//! for these shapes", never "safe to show".

/// What a redacted value is replaced with.
pub const REDACTED: &str = "[REDACTED]";

/// Replaces every occurrence of each secret in `text` with [`REDACTED`].
///
/// Empty secrets are ignored. When one secret contains another, the longer one wins, so no part of it is left behind.
/// The text is scanned once and a [`REDACTED`] already in it is copied through, so no secret is ever matched inside
/// the marker (`RED` does not turn `[REDACTED]` into `[[REDACTED]ACTED]`). Redacting the result a second time therefore
/// changes nothing, provided no secret contains `[`: a secret that starts with `[` could match across a marker's edge.
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
        if rest.starts_with(REDACTED) {
            out.push_str(REDACTED);
            rest = &rest[REDACTED.len()..];
        } else if let Some(s) = secrets.iter().find(|s| rest.starts_with(**s)) {
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

/// One forward pass over `text`: every byte is examined a bounded number of times, however many URLs there are.
///
/// Each search the pass needs (the next separator, a URL's end and its last `@`, the end of a quote's region) starts
/// where the one before it left off, or reuses what the one before it found, so no suffix is scanned twice.
fn userinfo_pass(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut scan = UrlScan::new(text);
    let mut pos = 0;
    while let Some((i, len, encoded)) = next_separator(text, pos) {
        let start = i + len;
        out.push_str(&text[pos..start]);
        // A userinfo that would be empty (the URL starts with `@`) is not redacted.
        match scan.userinfo_end(start, encoded).filter(|at| *at > start) {
            Some(at) => {
                out.push_str(REDACTED);
                // Resume at the `@`: the rest of the URL is kept and scanned for further URLs.
                pos = at;
            }
            // No userinfo here; keep scanning right after the separator, so a URL inside this one is still seen.
            None => pos = start,
        }
    }
    out.push_str(&text[pos..]);
    out
}

/// The earliest URL separator at or after `from`: its byte offset, its length, and whether it is percent-encoded.
///
/// One scan for all three forms (`://`, `:\/\/`, and `%3A%2F%2F` in either case). The next call starts after this
/// match, so a form that is absent costs one scan of the text, not one per URL.
fn next_separator(text: &str, from: usize) -> Option<(usize, usize, bool)> {
    let bytes = text.as_bytes();
    (from..bytes.len()).find_map(|i| match bytes[i] {
        b':' if bytes[i + 1..].starts_with(b"//") => Some((i, 3, false)),
        b':' if bytes[i + 1..].starts_with(b"\\/\\/") => Some((i, 5, false)),
        b'%' if bytes.get(i..i + 9).is_some_and(|s| s.eq_ignore_ascii_case(b"%3A%2F%2F")) => Some((i, 9, true)),
        _ => None,
    })
}

/// A search for the first character at or after `x` that matches, remembered: while the next query is not past the
/// match, the answer is the same and nothing is scanned.
struct Memo {
    from: usize,
    at: usize,
}

impl Memo {
    /// Holds no answer: `from` is past every `at`.
    const EMPTY: Self = Self { from: usize::MAX, at: 0 };

    /// Offset of the first match in `text` at or after `x`, or `text.len()`. Queries that never go backwards scan each
    /// byte once.
    fn find(&mut self, text: &str, x: usize, is_match: impl Fn(char) -> bool) -> usize {
        if !(self.from <= x && x <= self.at) {
            self.from = x;
            self.at = text[x..].find(is_match).map_or(text.len(), |i| x + i);
        }
        self.at
    }
}

/// What one scan of a URL found: where it ends and the last of each character that matters inside it. Positions are
/// offsets into the text. The scan began at `from`, so it answers for any start from there up to `end`.
struct Run {
    from: usize,
    end: usize,
    last_at: Option<usize>,
    last_pct: Option<usize>,
    /// The last `/`, `\`, `?` or `#`, which end an authority.
    last_stop: Option<usize>,
}

/// What one scan past a quote found: where the authority ends (`stop`) and the last `@` or `%40` before it.
struct Region {
    from: usize,
    stop: usize,
    last: Option<usize>,
}

/// The searches `userinfo_pass` makes after each separator, each remembered so that starts moving forward through the
/// text never rescan it.
struct UrlScan<'a> {
    text: &'a str,
    run: Run,
    region: Region,
    /// The first `@` at or after a position, for a quote's region in a plain URL.
    at: Memo,
}

impl<'a> UrlScan<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            run: Run { from: usize::MAX, end: 0, last_at: None, last_pct: None, last_stop: None },
            region: Region { from: usize::MAX, stop: 0, last: None },
            at: Memo::EMPTY,
        }
    }

    /// Offset of the `@` (or, in a percent-encoded URL, `%40`) that ends the userinfo of the URL whose separator ends
    /// at `start`, if it has one. For a URL with an `@`, the last one before its end. For one without, a quote that
    /// ends it can hide the `@`: see [`redact_url_userinfo`]. `start` never decreases between calls.
    fn userinfo_end(&mut self, start: usize, encoded: bool) -> Option<usize> {
        if !(self.run.from <= start && start <= self.run.end) {
            self.run = self.scan_run(start);
        }
        let run = &self.run;
        let last = if encoded { run.last_at.max(run.last_pct) } else { run.last_at };
        // A last `@` before `start` belongs to an earlier URL that this one begins inside.
        if let Some(at) = last.filter(|at| *at >= start) {
            return Some(at);
        }
        let end = run.end;
        let ended_by_quote = self.text[end..].starts_with(['"', '\'']);
        // A `/`, `\`, `?` or `#` before the quote already ended the authority.
        if !ended_by_quote || run.last_stop.is_some_and(|stop| stop >= start) {
            return None;
        }
        self.at_past_quote(end, encoded)
    }

    /// The `@` after the quote at `end` that still belongs to the URL: before the next whitespace, backtick, `<`, `>`,
    /// `/`, `\`, `?` or `#`. The first one for a plain URL, the last `@` or `%40` for a percent-encoded one.
    fn at_past_quote(&mut self, end: usize, encoded: bool) -> Option<usize> {
        if !(self.region.from <= end && end <= self.region.stop) {
            self.region = self.scan_region(end);
        }
        let stop = self.region.stop;
        if encoded {
            return self.region.last.filter(|at| *at >= end);
        }
        let at = self.at.find(self.text, end, |c| c == '@');
        (at < stop).then_some(at)
    }

    /// One pass over the URL that starts at `start`: its end, and the last `@`, `%40` and authority-ending character.
    fn scan_run(&self, start: usize) -> Run {
        let bytes = self.text.as_bytes();
        let mut run = Run { from: start, end: self.text.len(), last_at: None, last_pct: None, last_stop: None };
        for (offset, c) in self.text[start..].char_indices() {
            let i = start + offset;
            if is_url_end(c) {
                run.end = i;
                break;
            }
            match c {
                '@' => run.last_at = Some(i),
                // The two characters after `%` cannot end the URL, so a match is inside it.
                '%' if bytes[i + 1..].starts_with(b"40") => run.last_pct = Some(i),
                '/' | '\\' | '?' | '#' => run.last_stop = Some(i),
                _ => {}
            }
        }
        run
    }

    /// One pass from `from` (the quote that ended a URL) to the first character that ends the authority.
    fn scan_region(&self, from: usize) -> Region {
        let bytes = self.text.as_bytes();
        let mut region = Region { from, stop: self.text.len(), last: None };
        for (offset, c) in self.text[from..].char_indices() {
            let i = from + offset;
            if c.is_whitespace() || matches!(c, '`' | '<' | '>' | '/' | '\\' | '?' | '#') {
                region.stop = i;
                break;
            }
            if c == '@' || (c == '%' && bytes[i + 1..].starts_with(b"40")) {
                region.last = Some(i);
            }
        }
        region
    }
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
/// - A value after `key=` or `key:` (query string, form body, log line, HTTP header, JSON or Python-repr field; also
///   the fullwidth `＝` and `：`, blanks around the separator, and a quoted or JSON-escaped key) when the key,
///   lowercased with `-` read as `_`, is `key`, `sig`, `auth`, `pwd`, ends in `_key`, `_sig` or `_auth`, starts with
///   `auth_`, or contains `token`, `secret`, `passw`, `signature`, `apikey`, `api_key`, `credential`, `authoriz` or
///   `cookie`. Other keys (`page`, `status_code`, `monkey`) are kept.
/// - How far that value runs. A quoted value (`"`, `'` or the JSON-escaped `\"`) runs to its closing quote, honoring
///   `\` escapes and stopping at a newline if unclosed; the quotes stay. After a scheme word (`Basic`, `Bearer`,
///   `Token`, `Negotiate`, `NTLM`, `Bot`, `Digest`, either case) the credential is redacted and the word kept
///   (`Authorization: Basic [REDACTED]`); `Digest` takes the rest of the line, and so does a cookie key. Otherwise the
///   value runs to whitespace, `&`, a quote, a backtick, `)`, `<` or `>`. `scheme://` after a `:` is a URL, not a
///   value.
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
    const CONTAINS: [&str; 9] =
        ["token", "secret", "passw", "signature", "apikey", "api_key", "credential", "authoriz", "cookie"];
    CONTAINS.iter().any(|w| key.contains(w))
        || matches!(key.as_str(), "key" | "sig" | "auth" | "pwd")
        || key.ends_with("_key")
        || key.ends_with("_sig")
        || key.ends_with("_auth")
        || key.starts_with("auth_")
}

/// Separators between a key and its value: `=`, `:` (headers, JSON) and the fullwidth forms of both.
const PAIR_SEPARATORS: [char; 4] = ['=', ':', '\u{FF1D}', '\u{FF1A}'];

/// Authentication scheme words: the credential after one is the secret, the word is kept.
/// `digest` carries several comma-separated parameters, so its credential is the rest of the line.
const AUTH_SCHEMES: [&str; 7] = ["basic", "bearer", "token", "negotiate", "ntlm", "bot", "digest"];

/// Offsets (relative to `rest`, the text after a separator and its blanks) of the part of a value to replace, or `None`
/// when there is nothing to replace. Text before the start (an opening quote, a scheme word) and after the end (a
/// closing quote) is kept. `line_value` takes the rest of the line (a cookie, whose value holds `;` and spaces).
fn value_span(rest: &str, line_value: bool) -> Option<(usize, usize)> {
    // Each scan below stops at the value's own end (or a newline) and the caller resumes after it, so a long text of
    // `key="` pairs costs one pass, not one pass per pair.
    let is_eol = |c: char| matches!(c, '\n' | '\r');
    // An escaped quote (`\"`, JSON inside a JSON string) closes at the next `\"`; a newline ends an unclosed one.
    if let Some(inner) = rest.strip_prefix("\\\"") {
        let mut end = inner.len();
        let mut prev_backslash = false;
        for (i, c) in inner.char_indices() {
            if is_eol(c) {
                end = i;
                break;
            }
            if prev_backslash && c == '"' {
                end = i - 1;
                break;
            }
            prev_backslash = c == '\\';
        }
        return (end > 0).then_some((2, 2 + end));
    }
    if let Some(q) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let inner = &rest[1..];
        let mut end = inner.len();
        let mut chars = inner.char_indices();
        while let Some((i, c)) = chars.next() {
            if is_eol(c) || c == q {
                end = i;
                break;
            }
            if c == '\\' {
                chars.next();
            }
        }
        return (end > 0).then_some((1, 1 + end));
    }
    if line_value {
        let eol = rest.find(is_eol).unwrap_or(rest.len());
        return (eol > 0).then_some((0, eol));
    }
    for scheme in AUTH_SCHEMES {
        let Some(head) = rest.get(..scheme.len()).filter(|h| h.eq_ignore_ascii_case(scheme)) else { continue };
        let after = &rest[head.len()..];
        let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
        let credential = &after[blanks..];
        let len = if scheme == "digest" {
            credential.find(['\n', '\r']).unwrap_or(credential.len())
        } else {
            credential.find(is_value_end).unwrap_or(credential.len())
        };
        if blanks > 0 && len > 0 {
            let start = head.len() + blanks;
            return Some((start, start + len));
        }
    }
    let len = rest.find(is_value_end).unwrap_or(rest.len());
    (len > 0).then_some((0, len))
}

fn redact_secret_pairs(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    let mut search = 0;
    while let Some((i, sep)) =
        text[search..].char_indices().find(|(_, c)| PAIR_SEPARATORS.contains(c)).map(|(i, c)| (search + i, c))
    {
        search = i + sep.len_utf8();
        // The key may be quoted (`"token":` or, JSON-escaped, `\"token\":`): look before the closing quote.
        let before = text[..i].trim_end_matches([' ', '\t']);
        let key_end =
            before.len() - if before.ends_with("\\\"") { 2 } else { usize::from(before.ends_with(['"', '\''])) };
        if key_end < pos {
            continue;
        }
        let mut key_start = key_end;
        while key_start > pos && is_key_char(bytes[key_start - 1]) {
            key_start -= 1;
        }
        if key_start == key_end || !is_secret_key(&text[key_start..key_end]) {
            continue;
        }
        let after = &text[search..];
        // `scheme://` is a URL, not a header: the URL pass owns it.
        if sep == ':' && (after.starts_with("//") || after.starts_with("\\/")) {
            continue;
        }
        let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
        let line_value = text[key_start..key_end].to_ascii_lowercase().contains("cookie");
        let Some((from, to)) = value_span(&after[blanks..], line_value) else { continue };
        out.push_str(&text[pos..search + blanks + from]);
        out.push_str(REDACTED);
        pos = search + blanks + to;
        search = pos;
    }
    out.push_str(&text[pos..]);
    out
}

//! Redaction never panics, redacting twice equals redacting once, and a planted secret does not survive in any of the
//! shapes the helpers claim to cover.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use michi_core::redact::{redact_common_shapes, redact_secrets, redact_url_userinfo, REDACTED};

#[derive(Arbitrary, Debug)]
struct Input {
    text: String,
    secrets: Vec<String>,
    /// Alphanumeric seed for the planted secret.
    plant: String,
    pre: String,
    post: String,
    shape: u8,
}

/// Wraps `secret` in one of the shapes `redact_common_shapes` covers.
fn plant(shape: u8, secret: &str, pre: &str, post: &str) -> String {
    match shape % 8 {
        0 => format!("{pre}https://bot:{secret}@host/p{post}"),
        1 => format!(r"{pre}https:\/\/bot:{secret}@host\/p{post}"),
        2 => format!("{pre}https%3A%2F%2Fbot%3A{secret}%40host{post}"),
        3 => format!("{pre}https://host/p?access_token={secret}&x=1{post}"),
        4 => format!("{pre}postgres://u:\"{secret}@host/db{post}"),
        5 => format!("{pre}ghp_{secret}{post}"),
        6 => format!("{pre}Bearer {secret}{post}"),
        _ => format!("{pre}?api_key={secret}{post}"),
    }
}

fuzz_target!(|input: Input| {
    // Any text: the passes are idempotent and never panic.
    let once = redact_common_shapes(&input.text);
    assert_eq!(redact_common_shapes(&once), once, "not idempotent for {:?}", input.text);
    let once = redact_url_userinfo(&input.text);
    assert_eq!(redact_url_userinfo(&once), once, "userinfo not idempotent for {:?}", input.text);

    // Known secrets. A secret that occurs inside the marker (`RED`, `[`, `D]`) matches the marker on a second pass, so
    // such secrets are not used, and idempotence is only claimed for secrets with no bracket (one could straddle the
    // marker). Alphanumeric secrets cannot straddle the brackets, so for those "never survives" is exact.
    let secrets: Vec<&str> = input.secrets.iter().map(String::as_str).filter(|s| !REDACTED.contains(s)).collect();
    let once = redact_secrets(&input.text, &secrets);
    if secrets.iter().all(|s| !s.contains(['[', ']'])) {
        assert_eq!(redact_secrets(&once, &secrets), once, "redact_secrets not idempotent");
    }
    for s in &secrets {
        if s.chars().all(|c| c.is_ascii_alphanumeric()) {
            assert!(!once.contains(s), "known secret {s:?} survived in {once:?}");
        }
    }

    // A planted secret, in each covered shape, between arbitrary text that cannot itself continue it.
    let secret: String = input.plant.chars().filter(char::is_ascii_alphanumeric).take(40).collect();
    if secret.len() >= 16 {
        // 16 is the shortest token tail the `ghp_` and `Bearer` shapes claim.
        // A prefix or suffix of token characters would extend the secret; whitespace separates it.
        // The surrounding text has no alphanumerics, so it cannot contain or complete the secret.
        let strip = |s: &str| s.chars().filter(|c| !c.is_ascii_alphanumeric()).collect::<String>();
        let (pre, post) = (strip(&input.pre), strip(&input.post));
        let text = plant(input.shape, &secret, &format!("{pre} "), &format!(" {post}"));
        let out = redact_common_shapes(&text);
        assert!(!out.contains(&secret), "secret survived: {text:?} -> {out:?}");
    }
});

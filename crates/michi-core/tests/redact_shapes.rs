//! Reproductions of the audit's redaction probes (security #2) plus the idempotence property. Each probe leaked a
//! secret before the net was widened.

use michi_core::redact::{redact_common_shapes, redact_url_userinfo, REDACTED};
use michi_core::{DomainError, ErrorCode};
use proptest::prelude::*;

#[test]
fn a_json_escaped_url_is_redacted() {
    let out = redact_url_userinfo(r#"{"url":"https:\/\/bot:ghp_SECRET@github.com\/o\/r"}"#);
    assert!(!out.contains("ghp_SECRET"), "{out}");
    assert!(out.contains("github.com"), "the host is kept: {out}");
}

#[test]
fn a_quote_inside_the_password_does_not_hide_it() {
    let out = redact_url_userinfo(r#"postgres://u:p"w@host/db"#);
    assert_eq!(out, format!("postgres://{REDACTED}@host/db"));
}

#[test]
fn a_quote_after_a_url_without_userinfo_does_not_eat_the_next_field() {
    let s = r#"{"a":"https://host/x","b":"me@example.com"}"#;
    assert_eq!(redact_url_userinfo(s), s);
}

#[test]
fn a_percent_encoded_url_is_redacted() {
    for sep in ["https%3A%2F%2F", "https%3a%2f%2f"] {
        let out = redact_url_userinfo(&format!("{sep}bot%3Aghp_SECRET%40github.com"));
        assert!(!out.contains("ghp_SECRET"), "{out}");
        assert!(out.contains("github.com"), "{out}");
    }
}

#[test]
fn query_string_secrets_are_redacted() {
    for key in ["token", "access_token", "api_key", "apikey", "X-Api-Key", "client_secret", "password", "sig", "auth"] {
        let text = format!("error sending request for url (https://api.x.com/v1?a=1&{key}=SECRETVALUE&b=2)");
        let out = redact_common_shapes(&text);
        assert!(!out.contains("SECRETVALUE"), "{key}: {out}");
        assert!(out.contains("a=1") && out.contains("b=2"), "other params are kept: {out}");
    }
}

#[test]
fn harmless_query_keys_are_kept() {
    let s = "https://api.x.com/v1?page=2&limit=10&status_code=500&monkey=banana";
    assert_eq!(redact_common_shapes(s), s);
}

#[test]
fn well_known_token_shapes_are_redacted() {
    let tail = "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6";
    for token in [
        format!("npm_{tail}"),
        format!("ghp_{tail}"),
        format!("github_pat_{tail}"),
        format!("pypi-{tail}"),
        format!("Bearer {tail}"),
    ] {
        let out = redact_common_shapes(&format!("publish failed: {token} was refused"));
        assert!(!out.contains(tail), "{token}: {out}");
        assert!(out.contains("publish failed") && out.contains("was refused"), "{out}");
    }
}

#[test]
fn short_lookalikes_are_kept() {
    let s = "pypi-mirror is down; Bearer authentication failed; npm_config unset";
    assert_eq!(redact_common_shapes(s), s);
}

#[test]
fn from_error_redacts_a_query_token_in_the_top_level_display() {
    #[derive(Debug, thiserror::Error)]
    #[error("request failed: https://api.x.com/?token=ghp_SECRET")]
    struct Req;

    let e = DomainError::from_error(ErrorCode::ExternalFailure, &Req);
    assert!(
        !e.render(michi_core::OutputFormat::Text).contains("ghp_SECRET"),
        "{}",
        e.render(michi_core::OutputFormat::Text)
    );
}

/// Shapes that leaked in review or that a hostile corpus produced: each carries `SECRETVALUE` and none may keep it.
const HOSTILE: &[&str] = &[
    // Scheme words: the credential after the scheme is the secret.
    "authorization=Basic SECRETVALUE",
    "Authorization: Basic SECRETVALUE",
    "authorization: bearer SECRETVALUE",
    "Proxy-Authorization: Token SECRETVALUE",
    "auth=Digest SECRETVALUE",
    "Authorization:   Basic   SECRETVALUE and more",
    // Quoted values, escaped quotes inside.
    "token=\"SECRETVALUE\"",
    "token='SECRETVALUE'",
    "password=\"SEC\\\"RETVALUE\"",
    "api_key = \"SECRETVALUE\" next",
    // JSON, plain and escaped.
    r#"{"password": "SECRETVALUE"}"#,
    r#"{"a":1,"client_secret":"SECRETVALUE","b":2}"#,
    r#"{\"token\":\"SECRETVALUE\"}"#,
    r#"{"token":SECRETVALUE}"#,
    // Form bodies and query strings.
    "a=1&token=SECRETVALUE&b=2",
    "a=1&TOKEN=SECRETVALUE",
    "x=1&Access-Token=SECRETVALUE",
    // Headers.
    "Cookie: sid=SECRETVALUE; theme=dark",
    "Set-Cookie: session=SECRETVALUE; Path=/; HttpOnly",
    "X-Api-Key: SECRETVALUE",
    "x-api-key:SECRETVALUE",
    "GET / HTTP/1.1\nHost: h\nAuthorization: Bearer SECRETVALUE\nAccept: */*",
    // URLs inside quotes.
    r#"{"url":"https://bot:SECRETVALUE@host/x"}"#,
    "see 'postgres://u:SECRETVALUE@db/x' now",
    // Unicode lookalike separators.
    "token\u{FF1D}SECRETVALUE",
    "password\u{FF1A} SECRETVALUE",
    // Very long neighbours.
    "{LONG}token=SECRETVALUE{LONG}",
];

#[test]
fn the_hostile_corpus_does_not_leak_and_is_stable() {
    let long = "a=b&".repeat(5_000);
    for case in HOSTILE {
        let text = case.replace("{LONG}", &long);
        let out = redact_common_shapes(&text);
        assert!(!out.contains("SECRETVALUE"), "leaked: {case:?} -> {out:?}");
        assert_eq!(redact_common_shapes(&out), out, "not stable: {case:?}");
    }
}

#[test]
fn a_scheme_word_is_kept_and_its_credential_is_redacted() {
    assert_eq!(
        redact_common_shapes("authorization=Basic dXNlcjpwYXNz failed"),
        format!("authorization=Basic {REDACTED} failed")
    );
    assert_eq!(redact_common_shapes("Authorization: Bearer abc"), format!("Authorization: Bearer {REDACTED}"));
}

#[test]
fn a_quoted_value_keeps_its_quotes() {
    assert_eq!(redact_common_shapes("token=\"abc\" x"), format!("token=\"{REDACTED}\" x"));
    assert_eq!(redact_common_shapes("token='abc'"), format!("token='{REDACTED}'"));
    assert_eq!(redact_common_shapes("token=\"\""), "token=\"\"");
}

#[test]
fn unrelated_colons_and_pairs_are_kept() {
    for s in ["error: not found at 10:30", "status: 500", "https://host:8080/p", "name=\"bob\" id: 7", "monkey: banana"]
    {
        assert_eq!(redact_common_shapes(s), s);
    }
}

proptest! {
    /// Redacting twice equals redacting once, for any text and for texts built from the leaky shapes.
    #[test]
    fn redaction_is_idempotent(text in "(\\PC|https?://|:\\\\/\\\\/|%3A%2F%2F|%40|@|\"|\\?|&|token=|sig=|Bearer |Basic |authorization: |Cookie: |password\":\"|token=\"|'|\\\\|:|=|ghp_|[a-zA-Z0-9]{0,24}){0,12}") {
        let once = redact_common_shapes(&text);
        prop_assert_eq!(redact_common_shapes(&once), once);
    }

    /// A planted password never survives, whatever surrounds it or which shape carries it.
    #[test]
    fn a_planted_secret_never_survives(
        pre in "[a-z ]{0,8}",
        post in "[a-z ]{0,8}",
        shape in 0usize..5,
    ) {
        let secret = "Zq9xPlantedSecret7";
        let text = match shape {
            0 => format!("{pre}https://bot:{secret}@host/p{post}"),
            1 => format!("{pre}https:\\/\\/bot:{secret}@host\\/p{post}"),
            2 => format!("{pre}https%3A%2F%2Fbot%3A{secret}%40host{post}"),
            3 => format!("{pre}https://host/p?access_token={secret}&x=1{post}"),
            _ => format!("{pre}postgres://u:\"{secret}@host/db{post}"),
        };
        prop_assert!(!redact_common_shapes(&text).contains(secret), "{text}");
    }
}

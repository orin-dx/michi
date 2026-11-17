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
    assert!(!e.render().contains("ghp_SECRET"), "{}", e.render());
}

proptest! {
    /// Redacting twice equals redacting once, for any text and for texts built from the leaky shapes.
    #[test]
    fn redaction_is_idempotent(text in "(\\PC|https?://|:\\\\/\\\\/|%3A%2F%2F|%40|@|\"|\\?|&|token=|sig=|Bearer |ghp_|[a-zA-Z0-9]{0,24}){0,12}") {
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

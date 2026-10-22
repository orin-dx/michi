//! `DomainError` cause chains: diagnosis kept apart from `help[]` guidance (docs/spec/06-decisions.md, "Cause chains
//! are their own field, not hints").

use michi_core::{DomainError, Error, ErrorCode};

mod support;
use support::toon_parser::{decode, Node, Prim};

#[derive(Debug, thiserror::Error)]
#[error("permission denied")]
struct Denied;

#[derive(Debug, thiserror::Error)]
#[error("io error")]
struct Io {
    #[source]
    source: Denied,
}

#[derive(Debug, thiserror::Error)]
#[error("walk failed")]
struct Walk {
    #[source]
    source: Io,
}

fn walk() -> Walk {
    Walk { source: Io { source: Denied } }
}

#[track_caller]
fn conformant(out: &str) -> support::toon_parser::Doc {
    decode(out).unwrap_or_else(|e| panic!("not conformant TOON 4.4 encoder output: {e}\n{out}"))
}

fn causes(e: &DomainError) -> Vec<&str> {
    e.causes.iter().map(String::as_str).collect()
}

// --- from_error ------------------------------------------------------------------------------------------------------

#[test]
fn from_error_keeps_the_message_and_one_cause_per_link_outermost_first() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    assert_eq!(e.code, ErrorCode::ExternalFailure);
    assert_eq!(e.message, "walk failed");
    assert_eq!(causes(&e), ["io error", "permission denied"]);
    assert!(e.hints.is_empty(), "causes are diagnosis, never hints");
    assert!(e.retryable, "retryable still defaults from the code");
}

#[test]
fn from_error_without_a_source_has_no_causes() {
    let e = DomainError::from_error(ErrorCode::Forbidden, &Denied);
    assert_eq!(e.message, "permission denied");
    assert!(e.causes.is_empty());
    assert_eq!(e.render(), "error: forbidden\nmessage: permission denied\nexit_code: 1");
}

#[derive(Debug, thiserror::Error)]
#[error("io error: {0}")]
struct IoEmbeds(#[source] Denied);

#[derive(Debug, thiserror::Error)]
#[error("walk failed: {0}")]
struct WalkEmbeds(#[source] Io);

#[derive(Debug, thiserror::Error)]
#[error("walk failed: {0}")]
struct WalkEmbedsAll(#[source] IoEmbeds);

#[test]
fn from_error_skips_a_link_whose_text_the_previous_text_already_contains() {
    // `walk failed: io error` already says `io error`; `permission denied` is new.
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &WalkEmbeds(Io { source: Denied }));
    assert_eq!(e.message, "walk failed: io error");
    assert_eq!(causes(&e), ["permission denied"]);

    // Every link embeds its source, so the message already holds the whole chain.
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &WalkEmbedsAll(IoEmbeds(Denied)));
    assert_eq!(e.message, "walk failed: io error: permission denied");
    assert!(e.causes.is_empty(), "got {:?}", e.causes);
}

/// A link that does not embed its source keeps the next one, even when an earlier text did.
#[test]
fn from_error_compares_against_the_last_text_kept() {
    #[derive(Debug, thiserror::Error)]
    #[error("parse failed: {0}")]
    struct Parse(#[source] Read);
    #[derive(Debug, thiserror::Error)]
    #[error("read config")]
    struct Read(#[source] Denied);

    let e = DomainError::from_error(ErrorCode::InvalidInput, &Parse(Read(Denied)));
    assert_eq!(e.message, "parse failed: read config");
    assert_eq!(causes(&e), ["permission denied"]);
}

/// A message that spells out the whole chain drops every link it contains, even past a link that does not embed its
/// own source: the comparison is with the last text kept (here the message), not with the previous link.
#[test]
fn from_error_skips_links_the_message_already_spells_out() {
    #[derive(Debug, thiserror::Error)]
    #[error("walk failed: {0}: {inner}", inner = .0.source)]
    struct WalkSpellsOut(#[source] Io);

    let e = DomainError::from_error(ErrorCode::ExternalFailure, &WalkSpellsOut(Io { source: Denied }));
    assert_eq!(e.message, "walk failed: io error: permission denied");
    assert!(e.causes.is_empty(), "got {:?}", e.causes);
}

#[derive(Debug)]
struct Cycle;

impl std::fmt::Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cycle")
    }
}

static CYCLE: Cycle = Cycle;

impl std::error::Error for Cycle {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&CYCLE)
    }
}

#[test]
fn from_error_stops_on_a_cyclic_source_chain() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &Cycle);
    assert_eq!(e.message, "cycle");
    assert!(e.causes.is_empty(), "every link repeats the message");
}

#[test]
fn from_error_keeps_the_chain_as_text_only() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    assert!(std::error::Error::source(&e).is_none(), "the chain is preserved as text, not as a stored source");
}

// --- builder and render ----------------------------------------------------------------------------------------------

#[test]
fn cause_builder_appends_in_call_order() {
    let e = DomainError::new(ErrorCode::NotFound, "m").cause("a").cause(String::from("b"));
    assert_eq!(causes(&e), ["a", "b"]);
    assert!(DomainError::new(ErrorCode::NotFound, "m").causes.is_empty());
}

#[test]
fn render_puts_causes_after_the_message_as_an_inline_array() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk()).hint("check the directory permissions");
    let out = e.render();
    assert_eq!(
        out,
        "error: external_failure\nmessage: walk failed\ncauses[2]: io error,permission denied\nexit_code: 1\n\
         help[1]: check the directory permissions"
    );
    let doc = conformant(&out);
    assert_eq!(doc.get("causes"), Some(&Node::Inline(vec![Prim::str("io error"), Prim::str("permission denied")])));
    assert_eq!(doc.get("help"), Some(&Node::Inline(vec![Prim::str("check the directory permissions")])));
}

#[test]
fn a_multi_line_cause_renders_as_one_escaped_entry() {
    let e = DomainError::new(ErrorCode::ExternalFailure, "build failed").cause("line one\nline two, with: punctuation");
    let out = e.render();
    assert_eq!(
        out,
        "error: external_failure\nmessage: build failed\ncauses[1]: \"line one\\nline two, with: punctuation\"\nexit_code: 1"
    );
    let doc = conformant(&out);
    assert_eq!(doc.get("causes"), Some(&Node::Inline(vec![Prim::str("line one\nline two, with: punctuation")])));
}

#[test]
fn empty_and_quoted_causes_are_conformant() {
    let e = DomainError::new(ErrorCode::Conflict, "m").cause("").cause("\"q\"").cause("-dash").cause("42");
    let out = e.render();
    let doc = conformant(&out);
    assert_eq!(
        doc.get("causes"),
        Some(&Node::Inline(vec![Prim::str(""), Prim::str("\"q\""), Prim::str("-dash"), Prim::str("42")]))
    );
}

#[test]
fn error_domain_render_passes_causes_through() {
    let d = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    let out = Error::Domain(d.clone()).render();
    assert_eq!(out, d.render());
    assert!(out.contains("causes[2]: io error,permission denied"), "{out}");
}

#[test]
fn github_annotation_stays_message_only() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    assert_eq!(e.render_github_annotation(), "::error title=external_failure::walk failed");
}

// --- JSON ------------------------------------------------------------------------------------------------------------

#[test]
fn render_json_carries_causes_and_an_empty_array_when_there_are_none() {
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    let json = e.render_json();
    assert!(json.contains(",\"message\":\"walk failed\",\"causes\":[\"io error\",\"permission denied\"],"), "{json}");
    let none = DomainError::new(ErrorCode::NotFound, "m").render_json();
    assert!(none.contains(",\"message\":\"m\",\"causes\":[],"), "{none}");
    let escaped = DomainError::new(ErrorCode::NotFound, "m").cause("a\n\"b\"").render_json();
    assert!(escaped.contains("\"causes\":[\"a\\n\\\"b\\\"\"]"), "{escaped}");
}

#[test]
#[cfg(feature = "serde")]
fn render_json_causes_round_trip_exactly() {
    let e = DomainError::new(ErrorCode::NotFound, "m").cause("a\nb").cause("c\td");
    let parsed: serde_json::Value = serde_json::from_str(&e.render_json()).expect("render_json is JSON");
    assert_eq!(parsed["causes"], serde_json::json!(["a\nb", "c\td"]));
    assert_eq!(serde_json::to_value(&e).expect("serializes"), parsed);
}

// --- miette ----------------------------------------------------------------------------------------------------------

#[test]
#[cfg(feature = "miette")]
fn miette_help_lists_hints_then_causes() {
    use miette::Diagnostic;
    let e = DomainError::from_error(ErrorCode::ExternalFailure, &walk()).hint("check the directory permissions");
    let help = e.help().map(|h| h.to_string());
    assert_eq!(
        help.as_deref(),
        Some("check the directory permissions\ncaused by: io error\ncaused by: permission denied")
    );

    let causes_only = DomainError::from_error(ErrorCode::ExternalFailure, &walk());
    assert_eq!(
        causes_only.help().map(|h| h.to_string()).as_deref(),
        Some("caused by: io error\ncaused by: permission denied")
    );
    assert!(DomainError::new(ErrorCode::NotFound, "m").help().is_none());
}

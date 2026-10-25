#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::panic, clippy::map_err_ignore)]
#![warn(clippy::disallowed_types)]
#![warn(clippy::must_use_candidate, clippy::missing_errors_doc, clippy::missing_panics_doc)]
#![warn(missing_debug_implementations)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! # michi-core
//!
//! AXI response primitives for agent-ergonomic tools: TOON lists, key-value blocks, hints, structured errors, the
//! `AgentResponse` builder, truncation and retry policy. Async pipeline execution lives in
//! [`michi-pipeline`](https://docs.rs/michi-pipeline).
//!
//! michi turns structured data into token-efficient text an agent can read. Every agent-facing block (lists,
//! key-value items, errors, status, recovery hints) is a [TOON](https://github.com/toon-format/spec) document, so an
//! agent parses one format. The default build is synchronous and has no runtime dependencies beyond `thiserror` and
//! `compact_str`.
//!
//! ## Quick start
//!
//! ```
//! use michi_core::toon::{render_toon, ToonOptions, Value};
//!
//! let opts = ToonOptions::new(
//!     "issues",
//!     vec!["number".to_string(), "title".to_string(), "state".to_string()],
//!     vec![
//!         vec![Value::Int(51815), Value::from("[Bug]: Telegram plugin"), Value::from("open")],
//!         vec![Value::Int(51812), Value::from("dark mode request"), Value::from("open")],
//!     ],
//! )
//! .total_count(Some(8771));
//!
//! assert_eq!(
//!     render_toon(&opts)?,
//!     "issues[2]{number,title,state}:\n  51815,\"[Bug]: Telegram plugin\",open\n  51812,dark mode request,open\n\
//!      totalCount: 8771"
//! );
//! # Ok::<(), michi_core::toon::ToonError>(())
//! ```
//!
//! ## Where to start
//!
//! - [`toon`]: lists of rows, and `toon::list()` for `Serialize` types (`serde` feature).
//! - [`kv`]: one item as `key: value` lines.
//! - [`error`]: [`DomainError`] with a stable [`ErrorCode`] and agent-readable rendering.
//! - [`response`]: [`AgentResponse`], which composes the primitives into one reply.
//! - [`mcp`]: MCP `CallToolResult` assembly.
//! - [`resilience`]: retry delays, `Retry-After` parsing, retry budgets and idempotency keys.
//! - [`truncate`](mod@truncate): UTF-8 safe truncation of one string, with the cut reported outside the text.
//! - [`truncation`]: what a cap on a list cut ([`TruncationOutcome`]), however the survivors were chosen.
//!
//! ## Features
//!
//! | Feature | Adds |
//! | --- | --- |
//! | `serde` | `Serialize`/`Deserialize` on the data types, and `toon::list()` |
//! | `schemars` | `JsonSchema` on the DTO types, for MCP `outputSchema` (implies `serde`) |
//! | `miette` | `miette::Diagnostic` for [`DomainError`] |
//! | `napi` | The Node.js export surface used by `@orin-dx/michi` (needs a Node toolchain) |
//!
//! No features are on by default.

/// Audience routing (`assistant` vs `user`).
pub mod audience;
/// Definitive empty states.
pub mod empty;
/// Unified domain errors.
pub mod error;
#[cfg(feature = "serde")]
mod float_wire;
/// Contextual usage hints (`help[]`).
pub mod hints;
/// Idempotency: partial-success state for multi-step operations.
pub mod idempotency;
#[cfg(feature = "schemars")]
mod int_schema;
/// Key-value single-item rendering (`key: value`).
pub mod kv;
/// MCP `CallToolResult` mapping.
pub mod mcp;
/// NAPI export surface for the `@orin-dx/michi` npm package.
#[cfg(feature = "napi")]
#[cfg_attr(docsrs, doc(cfg(feature = "napi")))]
pub mod napi;
/// Pipeline step definitions and run state.
pub mod pipeline;
/// Structured recovery hints (`recovery[]`).
pub mod recovery;
/// Best-effort redaction of known secrets and URL userinfo.
pub mod redact;
/// Retry delay calculation, `Retry-After` parsing, retry budgets, idempotency keys and already-done detection.
pub mod resilience;
/// `AgentResponse` builder.
pub mod response;
/// Health and status response rendering.
pub mod status;
/// No-op telemetry provider.
pub mod telemetry;
/// TOON list rendering, the token-optimized list format.
pub mod toon;
/// UTF-8 char-boundary-safe truncation within one string.
pub mod truncate;
/// List truncation: what a cap on a list cut, however the survivors were chosen.
pub mod truncation;

#[cfg(test)]
pub(crate) mod test_support;

pub use audience::Audience;
pub use empty::{empty_state, empty_state_with_hints, try_empty_state, try_empty_state_with_hints};
pub use error::{DomainError, Error, ErrorClass, ErrorCode, Sensitive, MAY_HAVE_TAKEN_EFFECT_HINT};
pub use hints::{append_hints, render_hints, Hint};
pub use idempotency::{FailedOp, PartialSuccess};
pub use kv::{render_kv, KvError, KvItem, KvValue};
pub use mcp::{CallToolResult, ContentBlock, StructuredContentError};
pub use recovery::{append_recovery, render_recovery, RecoveryHint};
pub use response::{AgentResponse, OutputFormat};
pub use status::{Health, StatusItem, StatusResponse};

// Explicit lists, not globs: a glob would silently re-export every item later added to these modules at the crate
// root, and could shadow or clash with another root name in a minor release.
pub use resilience::{
    already_done, is_retryable_status, next_retry_delay, parse_retry_after, parse_retry_after_at, render_already_done,
    AlreadyDone, IdempotencyKey, RetryBudget, RetryConfig, RetryConfigError,
};
pub use truncate::{truncate, Truncated};
pub use truncation::{truncate_by_count, truncate_by_size, TruncationOutcome};

/// Crate-level `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync_static<T: Send + Sync + 'static>() {}

    #[test]
    fn ac033_crate_root_carries_deny_unsafe_and_warn_missing_docs() {
        // Scoped to the crate-attribute prologue (lines before the first `//!`
        // doc comment) -- a bare `contains` over the whole file is vacuous,
        // since include_str! also pulls in this very test module, whose own
        // assertion text contains both literal attribute strings.
        let src = include_str!("lib.rs");
        let prologue: Vec<&str> = src.lines().take_while(|l| !l.starts_with("//!")).collect();
        assert!(
            prologue.iter().any(|l| l.trim() == "#![warn(missing_docs)]"),
            "missing #![warn(missing_docs)] in prologue"
        );

        // unsafe_code = "deny" lives in the workspace root's
        // [workspace.lints.rust] (Cargo.toml), not a #![deny(unsafe_code)]
        // attribute here -- see the root Cargo.toml's comment on why only
        // unsafe_code is centralized this way.
        // Both manifests are read from the workspace: the packaged crate has no root manifest, and cargo rewrites its
        // own Cargo.toml with the workspace lints inlined.
        let Some(root_cargo_toml) = crate::test_support::workspace_file("Cargo.toml") else { return };
        assert!(
            root_cargo_toml.contains("unsafe_code = \"deny\""),
            "root Cargo.toml's [workspace.lints.rust] must set unsafe_code = \"deny\""
        );
        let Some(own_cargo_toml) = crate::test_support::workspace_file("crates/michi-core/Cargo.toml") else { return };
        assert!(
            own_cargo_toml.contains("[lints]\nworkspace = true"),
            "this crate's Cargo.toml must opt into workspace lints via [lints]\\nworkspace = true"
        );
    }

    #[test]
    #[cfg(feature = "serde")]
    fn ac036_positive_half_types_round_trip_under_serde_feature() {
        let audience = Audience::Assistant;
        let json = serde_json::to_string(&audience).expect("Audience serializes");
        assert_eq!(serde_json::from_str::<Audience>(&json).expect("Audience deserializes"), audience);

        let hint = Hint::from("x");
        let json = serde_json::to_string(&hint).expect("Hint serializes");
        assert_eq!(serde_json::from_str::<Hint>(&json).expect("Hint deserializes"), hint);

        let recovery = RecoveryHint::new("t");
        let json = serde_json::to_string(&recovery).expect("RecoveryHint serializes");
        assert_eq!(serde_json::from_str::<RecoveryHint>(&json).expect("RecoveryHint deserializes"), recovery);

        let block = ContentBlock::new("t", vec![]);
        let json = serde_json::to_string(&block).expect("ContentBlock serializes");
        assert_eq!(serde_json::from_str::<ContentBlock>(&json).expect("ContentBlock deserializes"), block);

        let result = CallToolResult::new(vec![], false, "{}");
        let json = serde_json::to_string(&result).expect("CallToolResult serializes");
        assert!(serde_json::from_str::<CallToolResult>(&json).is_ok(), "CallToolResult deserializes");
    }

    #[test]
    fn test_auto_traits() {
        assert_send_sync_static::<Audience>();
        assert_send_sync_static::<DomainError>();
        assert_send_sync_static::<ErrorClass>();
        assert_send_sync_static::<ErrorCode>();
        assert_send_sync_static::<Hint>();
        assert_send_sync_static::<KvItem>();
        assert_send_sync_static::<KvValue>();
        assert_send_sync_static::<ContentBlock>();
        assert_send_sync_static::<CallToolResult>();
        assert_send_sync_static::<RecoveryHint>();
        assert_send_sync_static::<AgentResponse>();
        assert_send_sync_static::<OutputFormat>();
        assert_send_sync_static::<Health>();
        assert_send_sync_static::<StatusItem>();
        assert_send_sync_static::<StatusResponse>();
        assert_send_sync_static::<TruncationOutcome<String>>();
    }
}

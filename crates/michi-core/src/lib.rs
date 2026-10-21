#![warn(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::disallowed_types)]

//! # michi-core
//!
//! AXI response primitives for agent-ergonomic tools: TOON lists, key-value blocks, hints, structured errors, the
//! `AgentResponse` builder, truncation and retry policy. Async pipeline execution lives in `michi-pipeline`.

/// Audience routing (`assistant` vs `user`).
pub mod audience;
/// Definitive empty states.
pub mod empty;
/// Unified domain errors.
pub mod error;
/// Contextual usage hints (`help[]`).
pub mod hints;
/// Idempotency: partial-success state for multi-step operations.
pub mod idempotency;
/// Key-value single-item rendering (`key: value`).
pub mod kv;
/// MCP `CallToolResult` mapping.
pub mod mcp;
/// NAPI export surface for the `@orin-dx/michi` npm package.
#[cfg(feature = "napi")]
pub mod napi;
/// Pipeline step definitions and run state.
pub mod pipeline;
/// Structured recovery hints (`recovery[]`).
pub mod recovery;
/// Retry delay calculation, `Retry-After` parsing and idempotency keys.
pub mod resilience;
/// `AgentResponse` builder.
pub mod response;
/// Health and status response rendering.
pub mod status;
/// No-op telemetry provider.
pub mod telemetry;
/// TOON list rendering, the token-optimized list format.
pub mod toon;
/// UTF-8 char-boundary-safe truncation.
pub mod truncate;

pub use audience::Audience;
pub use empty::{empty_state, empty_state_with_hints};
pub use error::{DomainError, Error, ErrorClass, ErrorCode, Sensitive};
pub use hints::{append_hints, render_hints, Hint};
pub use idempotency::{FailedOp, PartialSuccess};
pub use kv::{render_kv, KvError, KvItem, KvValue};
pub use mcp::{CallToolResult, ContentBlock};
pub use recovery::{append_recovery, render_recovery, RecoveryHint};
pub use response::{AgentResponse, OutputFormat};
pub use status::{Health, StatusItem, StatusResponse};

pub use resilience::*;
pub use truncate::*;

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
        let root_cargo_toml = include_str!("../../../Cargo.toml");
        assert!(
            root_cargo_toml.contains("unsafe_code = \"deny\""),
            "root Cargo.toml's [workspace.lints.rust] must set unsafe_code = \"deny\""
        );
        let own_cargo_toml = include_str!("../Cargo.toml");
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
    }
}

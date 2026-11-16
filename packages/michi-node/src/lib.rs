//! Thin cdylib shim exposing `michi-core`'s `napi` feature surface as the `@orin-dx/michi` npm package.
//!
//! The `#[napi]` export implementations live in `michi_core::napi` (`crates/michi-core/src/napi.rs` and `napi/`) so
//! they can be unit-tested alongside the rest of the library. This crate exists only because
//! `crate-type = ["cdylib"]` cannot coexist with a regular `[lib]` in the same `Cargo.toml` — see "NAPI npm package"
//! in `docs/spec/04-mcp-and-napi.md`.

pub use michi_core::napi::*;

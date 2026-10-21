//! Thin cdylib shim exposing `michi-core`'s `napi` feature surface as the `@orin-dx/michi` npm package.
//!
//! The `#[napi]` export implementations live in `michi_core::napi` (`crates/michi-core/src/napi.rs`) so they can be
//! unit-tested alongside the rest of the library. This crate exists only because `crate-type = ["cdylib"]` cannot
//! coexist with a regular `[lib]` in the same `Cargo.toml` — see `docs/superpowers/specs/2026-07-03-michi-design.md` §1.

pub use michi_core::napi::*;

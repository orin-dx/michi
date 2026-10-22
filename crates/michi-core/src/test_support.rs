//! Test-only access to files outside this crate.
//!
//! Some guard tests read workspace files (the root manifest, the Node shim, the docs). Those files are not in the
//! published `.crate`, so the tests read them at runtime and skip when built from the package, instead of using
//! `include_str!`, which would stop the packaged crate's tests from compiling.

use std::path::Path;

/// Reads `rel` (relative to the workspace root), or returns `None` when this crate is not inside the michi workspace.
///
/// Inside the workspace a missing file panics, so a moved file fails the test instead of silently skipping it.
pub(crate) fn workspace_file(rel: &str) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    if !manifest.contains("\"crates/michi-core\"") {
        return None;
    }
    match std::fs::read_to_string(root.join(rel)) {
        Ok(text) => Some(text),
        Err(err) => panic!("workspace file {rel} is unreadable: {err}"),
    }
}

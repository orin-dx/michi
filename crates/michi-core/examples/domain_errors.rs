//! Structured DomainError rendering and GitHub Actions annotation example.
//! Run with: `cargo run -p michi-core --example domain_errors`

// Examples exist to print their output — the disallowed_macros ban on
// println! is scoped to library code, not demonstration binaries.
#![allow(clippy::disallowed_macros)]

use michi_core::error::{DomainError, ErrorCode};
use michi_core::recovery::RecoveryHint;

fn main() {
    let err = DomainError::new(ErrorCode::NotFound, "Package '@orin-dx/michi' not found in workspace")
        .hint("Run `just check-all` to inspect workspace packages")
        .recovery(RecoveryHint::new("list_packages"));

    println!("=== Agent Error Block ===");
    println!("{}", err.render());

    println!("=== GitHub Workflow Annotation ===");
    println!("{}", err.render_github_annotation());
}

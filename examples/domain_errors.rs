//! Structured DomainError rendering and GitHub Actions annotation example.
//! Run with: `cargo run --example domain_errors`

// Examples exist to print their output — the disallowed_macros ban on
// println! is scoped to library code, not demonstration binaries.
#![allow(clippy::disallowed_macros)]

use michi::error::{DomainError, ErrorCode};
use michi::recovery::RecoveryHint;

fn main() {
    let err = DomainError::new(ErrorCode::NotFound, "Package '@orin-dx/michi' not found in workspace")
        .hint("Run `just check-all` to inspect workspace packages")
        .recovery(RecoveryHint::new("list_packages"));

    println!("=== Agent Error Block ===");
    println!("{}", err.render());

    println!("=== GitHub Workflow Annotation ===");
    println!("{}", err.render_github_annotation());
}

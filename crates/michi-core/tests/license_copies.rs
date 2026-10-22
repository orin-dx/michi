//! Each published package ships its own copy of the root LICENSE (crates.io and npm only package files inside the
//! package directory). The copies are plain files, not symlinks, so they work on every platform; this keeps them equal.

use std::path::Path;

#[test]
fn every_package_license_matches_the_root_license() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read =
        |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("cannot read {rel}: {err}"));
    let license = read("LICENSE");
    for copy in [
        "crates/michi-core/LICENSE",
        "crates/michi-pipeline/LICENSE",
        "packages/michi-node/LICENSE",
        "packages/michi-node/npm/darwin-arm64/LICENSE",
        "packages/michi-node/npm/linux-x64-gnu/LICENSE",
        "packages/michi-node/npm/linux-x64-musl/LICENSE",
        "packages/michi-node/npm/darwin-x64/LICENSE",
        "packages/michi-node/npm/linux-arm64-gnu/LICENSE",
        "packages/michi-node/npm/linux-arm64-musl/LICENSE",
    ] {
        assert_eq!(read(copy), license, "{copy} differs from the root LICENSE; copy it again with `cp LICENSE {copy}`");
    }
}

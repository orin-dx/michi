#!/usr/bin/env bash
# Checks michi-core and michi-pipeline's API against crates.io at the release type their pending changes call for.
#
# With pending changesets for the michi group, the release type comes from them (scripts/pending-release-type.py).
# Without any, it comes from the version bump: on Callisto's version PR the changesets are already applied and the
# manifests carry the new version, so cargo-semver-checks compares that version with crates.io's.
set -euo pipefail

version=$(grep -m1 '^version' crates/michi-core/Cargo.toml | sed -E 's/version *= *"(.*)"/\1/')
release_type=$(python3 scripts/pending-release-type.py "$version")
if [[ "$release_type" == infer ]]; then
  echo "No pending michi changesets: checking against the version bump (michi-core $version)"
  cargo semver-checks check-release -p michi-core -p michi-pipeline
else
  echo "Pending release type: $release_type"
  cargo semver-checks check-release -p michi-core -p michi-pipeline --release-type "$release_type"
fi

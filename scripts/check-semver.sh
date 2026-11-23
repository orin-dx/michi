#!/usr/bin/env bash
# Checks the API of michi-core, michi-pipeline and michi-graph against crates.io at the release type their pending
# changes call for.
#
# Callisto computes the pending bump (`callisto status`); the fixed group gives every member the same one, so
# michi-core's stands for all. cargo-semver-checks names release types literally, so before 1.0 Callisto's minor bump
# (0.4 -> 0.5) is `major` here. With nothing pending, the version bump is the guide: on Callisto's version PR the
# changesets are already applied and the manifests carry the new version.
set -euo pipefail

status=$(callisto --format json status)
severity=$(jq -r '.packages[] | select(.package == "cargo/michi-core") | .pendingSeverity // "none"' <<<"$status")
version=$(jq -r '.packages[] | select(.package == "cargo/michi-core") | .currentVersion' <<<"$status")

case "$severity" in
  none | null) release_type="infer" ;;
  major) release_type="major" ;;
  minor) if [[ "$version" == 0.* ]]; then release_type="major"; else release_type="minor"; fi ;;
  patch) release_type="patch" ;;
  *)
    echo "unexpected pending severity from callisto: $severity" >&2
    exit 1
    ;;
esac

if [[ "$release_type" == infer ]]; then
  echo "Nothing pending for michi: checking against the version bump (michi-core $version)"
  cargo semver-checks check-release -p michi-core -p michi-pipeline -p michi-graph
else
  echo "Pending release type: $release_type (Callisto: $severity, michi-core $version)"
  cargo semver-checks check-release -p michi-core -p michi-pipeline -p michi-graph --release-type "$release_type"
fi

#!/usr/bin/env bash
# Print the version a Callisto release receipt shows as published to a registry, for the post-release smoke test.
#
# Usage: scripts/released-version.sh RECEIPT_JSON
#
# Prints the version, or an empty line when the receipt is missing or holds no registry publish (the smoke test is then
# skipped). Exits 1 when the receipt covers several versions, since the smoke test checks one. `published` is a publish
# that landed in this run; `alreadySatisfied` is one a re-run adopted from a failed earlier attempt.
set -euo pipefail

receipt="${1:?usage: released-version.sh RECEIPT_JSON}"
if [[ ! -f "$receipt" ]]; then
  echo
  exit 0
fi
versions=$(jq -r '[.outcomes[] | select((.outcome.kind == "published" or .outcome.kind == "alreadySatisfied") and
  .operation.role.kind == "registryPublish") | .operation.version] | unique | join(" ")' "$receipt")
if [[ "$versions" == *" "* ]]; then
  echo "::error::the receipt covers several released versions ($versions); the smoke test checks one" >&2
  exit 1
fi
echo "$versions"

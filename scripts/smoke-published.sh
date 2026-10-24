#!/usr/bin/env bash
# Post-release smoke test: install the *published* michi release into clean projects and use it, like a user would.
#
#   scripts/smoke-published.sh <version> [npm|crates|all]
#
# Env:
#   SMOKE_RETRY_SECONDS  how long to keep retrying while a registry lags behind the publish (default 600)
#   SMOKE_KEEP=1         keep the temp directory for debugging
set -euo pipefail

version="${1:-}"
only="${2:-all}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <version> [npm|crates|all]   (e.g. $0 0.7.0)" >&2
  exit 2
fi
case "$only" in npm | crates | all) ;; *)
  echo "unknown target '$only' (npm, crates or all)" >&2
  exit 2
  ;;
esac

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
smoke="$here/smoke"
retry_seconds="${SMOKE_RETRY_SECONDS:-600}"
work="$(mktemp -d "${TMPDIR:-/tmp}/michi-smoke.XXXXXX")"
cleanup() {
  if [[ "${SMOKE_KEEP:-}" == 1 ]]; then echo "kept $work" >&2; else rm -rf "$work"; fi
}
trap cleanup EXIT

fail() {
  echo "SMOKE FAILED: $*" >&2
  exit 1
}

# retry <what> <command...>: run until it succeeds, backing off 5s, 10s, ... up to 60s between tries, for at most
# $retry_seconds. The registries lag right after a publish, so "not found" is expected for a while.
retry() {
  local what="$1" delay=5 waited=0
  shift
  until "$@"; do
    if ((waited >= retry_seconds)); then
      fail "$what still failing after ${retry_seconds}s: version $version is not installable from the registry"
    fi
    echo "  $what not ready, retrying in ${delay}s (${waited}s/${retry_seconds}s)" >&2
    sleep "$delay"
    waited=$((waited + delay))
    delay=$((delay * 2 > 60 ? 60 : delay * 2))
  done
}

# The platform package npm should have picked for this host, named like napi does.
expected_platform_package() {
  local os arch libc=""
  case "$(uname -s)" in
    Darwin) os=darwin ;;
    Linux) os=linux ;;
    *) fail "unsupported host OS $(uname -s)" ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) arch=arm64 ;;
    x86_64 | amd64) arch=x64 ;;
    *) fail "unsupported host CPU $(uname -m)" ;;
  esac
  if [[ "$os" == linux ]]; then
    if (ldd --version 2>&1 || true) | grep -qi musl; then libc=-musl; else libc=-gnu; fi
  fi
  echo "@orin-dx/michi-$os-$arch$libc"
}

smoke_npm() {
  local platform_package dir
  platform_package="$(expected_platform_package)"
  dir="$work/npm"
  mkdir -p "$dir/cache"
  echo "== npm: @orin-dx/michi@$version (expecting $platform_package)"
  (
    cd "$dir"
    printf '{"name":"michi-smoke","version":"0.0.0","private":true}\n' > package.json
    retry "npm install" npm install "@orin-dx/michi@$version" --cache "$dir/cache" --no-audit --no-fund --no-save \
      --save-exact --loglevel error
    [[ -d "node_modules/$platform_package" ]] ||
      fail "npm did not install $platform_package for this host (installed: $(ls node_modules/@orin-dx | tr '\n' ' '))"
    # Run from the project directory so require() resolves from its node_modules.
    cp "$smoke/npm/smoke.cjs" ./smoke.cjs
    node ./smoke.cjs "$version" "$platform_package" || fail "npm smoke script failed"
  )
}

smoke_crates() {
  local dir="$work/crates"
  echo "== crates.io: michi-core, michi-pipeline, michi-graph =$version"
  mkdir -p "$dir/src"
  sed "s/@VERSION@/$version/g" "$smoke/crates/Cargo.toml.in" > "$dir/Cargo.toml"
  cp "$smoke/crates/src/main.rs" "$dir/src/main.rs"
  (
    cd "$dir"
    export CARGO_TARGET_DIR="$work/cargo-target"
    # Resolving (not building) is what lags behind the publish, so retry only that.
    retry "cargo resolve" cargo generate-lockfile --quiet
    cargo run --quiet --locked || fail "crates smoke program failed"
  )
}

[[ "$only" == crates ]] || smoke_npm
[[ "$only" == npm ]] || smoke_crates
echo "SMOKE PASSED: michi $version ($only)"

# Default recipe: run full CI pipeline via moon & just
default: ci

# ── Build ──────────────────────────────────────────────────────────────────
build:
    moon run :build

build-release:
    cargo build --release --workspace

build-node:
    cd packages/michi-node && pnpm build --platform

build-node-release:
    cd packages/michi-node && pnpm build --platform --release

# ── Test ───────────────────────────────────────────────────────────────────
test:
    moon run :test

test-rust:
    cargo nextest run --workspace

test-rust-all:
    cargo nextest run --workspace --all-features

# Each published crate alone with its default features: a workspace-wide run unifies features across crates.
test-crates-default:
    cargo nextest run -p michi-core
    cargo nextest run -p michi-graph
    cargo nextest run -p michi-pipeline

# The fixed group's semver release type from pending changesets (front matter only; michi-graph excluded).
pending-release-type:
    python3 scripts/pending-release-type.py "$(grep -m1 '^version' crates/michi-core/Cargo.toml | sed -E 's/version *= *"(.*)"/\1/')"

# michi-graph is MIT: fail if a non-permissive license enters its normal or build dependencies.
check-graph-license:
    python3 scripts/test_check_permissive_deps.py
    cargo metadata --format-version 1 --all-features | python3 scripts/check-permissive-deps.py michi-graph

# Post-release check: install a *published* version from npm and crates.io into clean projects and use it.
# Needs network. `SMOKE_RETRY_SECONDS` (default 600) bounds the wait for the registries.
smoke-published version:
    scripts/smoke-published.sh {{version}}

test-rust-release:
    cargo nextest run --workspace --release

test-node: build-node
    cd packages/michi-node && pnpm test

# ── Lint & Format ───────────────────────────────────────────────────────────
check: fmt-check clippy deny typos fmt-md-check

lint:
    moon run :lint

# Affected-only lint: uses moon's dependency-graph-aware affected-project
# detection to build the crate scope, then feeds it into ONE cargo
# invocation with multiple -p flags. This is the incremental counterpart to
# `lint` (which lints the whole workspace in one shot) -- it exists so a
# pre-push hook doesn't have to choose between "lint everything every time"
# and "fan out one cargo process per crate" (the latter serializes behind
# Cargo's shared target/ lock; see .moon/tasks/rust.yml). Only lints the
# affected crates' own source, not their downstream dependents -- clippy
# warnings are about a crate's own code, not its callers.
lint-affected:
    #!/usr/bin/env bash
    set -euo pipefail
    projects=$(moon query projects --affected 2>/dev/null | jq -r '.projects[].id // empty')
    if [ -z "$projects" ]; then
        echo "No affected crates; skipping lint."
        exit 0
    fi
    # Root-level changes (manifests, toolchain, lint config) affect every crate.
    if grep -qx michi-workspace <<< "$projects"; then
        echo "Workspace root affected; linting every crate."
        cargo clippy --workspace --all-targets --all-features -- -D warnings
        exit 0
    fi
    args=()
    while IFS= read -r p; do
        args+=(-p "$p")
    done <<< "$projects"
    echo "Linting affected crates: ${args[*]}"
    cargo clippy "${args[@]}" --all-targets --all-features -- -D warnings

clippy:
    cargo clippy --workspace --all-features --all-targets -- -D warnings

fmt:
    moon run :format

format: fmt

fmt-check:
    moon run :format-check

fmt-md:
    CI=true pnpm exec oxfmt --ignore-path=.oxfmtignore --write "**/*.md"

fmt-md-check:
    CI=true pnpm exec oxfmt --ignore-path=.oxfmtignore --check "**/*.md"

typos:
    typos

deny:
    cargo deny check

audit:
    moon run :audit

# ── Documentation Checks ────────────────────────────────────────────────────
doc: doc-check

doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
    # Default features alone: intra-doc links to feature-gated items only break here.
    RUSTDOCFLAGS="-D warnings" cargo doc -p michi-core --no-deps

# API SemVer check of the published crates against their latest crates.io release (the default baseline). CI runs the
# same check.
check-api:
    bash scripts/check-semver.sh

# Unused-dependency check.
machete:
    cargo machete

# ── Examples & Benchmarks ──────────────────────────────────────────────────
examples:
    moon run :examples

example name:
    cargo run -p michi-core --example {{name}}

snapshots:
    cargo insta review

snapshots-accept:
    cargo insta test --accept

bench:
    cargo bench --workspace

bench-baseline:
    cargo bench --workspace -- --save-baseline main

# Mutation testing sweep, workspace-wide. Complements the per-task mutation
# checks the implementation workflow already runs during development
# (surviving mutants get precision tests before a task's exit gate) with a
# standing, repeatable local/CI command over the whole tree.
mutants:
    cargo mutants --workspace

# ── Continuous Development ──────────────────────────────────────────────────
watch:
    cargo watch -x "check --workspace --all-features"

# ── Coverage & Cleanup ─────────────────────────────────────────────────────
coverage:
    cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

clean:
    moon clean 2>/dev/null || true
    cargo clean
    rm -f lcov.info

# ── Git Hooks & Pre-push ───────────────────────────────────────────────────
pre-commit: fmt-check

pre-push: fmt-check lint-affected

hooks:
    @echo '#!/bin/sh\njust pre-commit' > .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo '#!/bin/sh\njust pre-push' > .git/hooks/pre-push
    @chmod +x .git/hooks/pre-push
    @echo "Git pre-commit and pre-push hooks installed successfully."

# ── CI Verification Pipeline ───────────────────────────────────────────────
check-all: ci

ci: fmt-check lint test test-rust-release audit doc-check

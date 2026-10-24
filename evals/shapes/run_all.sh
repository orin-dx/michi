#!/usr/bin/env bash
# Regenerate the corpus, every rendering, token counts and the prompts, then self-test the harness (no model calls).
# Usage: ./run_all.sh (from anywhere). Needs python3, node + npm, cargo, and PyYAML, tomli_w, tiktoken (and tomli on
# Python < 3.11) installed to $PYDEPS:
#   pip install --target "$PYDEPS" tiktoken==0.14.0 pyyaml tomli_w tomli
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="${TMPDIR:-/tmp}"
PYDEPS="${PYDEPS:-$TMP/py}"
export PYTHONPATH="$PYDEPS"
export TIKTOKEN_CACHE_DIR="$HERE/tiktoken_cache"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP/shapes-eval-target}"

(cd "$HERE" && npm install --silent)
python3 "$HERE/gen_shapes.py"
(cd "$HERE" && node toon_ref.mjs)
(cd "$HERE/michi-render" && cargo run --release -q -- "$HERE")
python3 "$HERE/render_formats.py"
python3 -W ignore "$HERE/count_tokens.py"
python3 "$HERE/build_prompts.py"
python3 "$HERE/selftest.py" > /dev/null
echo "done: results/token_counts.csv, prompts/all.jsonl; self-test passed"

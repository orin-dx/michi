#!/usr/bin/env bash
# Regenerate the corpus, all renders, token counts and retrieval-pilot prompts.
# Usage: ./run_all.sh  (from anywhere). Requires: python3, cargo, and tiktoken installed to $PYDEPS.
# michi-render builds the michi-core crate from this checkout (relative path dependency).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="${TMPDIR:-/tmp}"
PYDEPS="${PYDEPS:-$TMP/py}"            # pip install --target "$PYDEPS" tiktoken==0.14.0
export PYTHONPATH="$PYDEPS"
export TIKTOKEN_CACHE_DIR="$HERE/tiktoken_cache"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$TMP/toon-eval-target}"

python3 "$HERE/gen_corpus.py"
(cd "$HERE/michi-render" && cargo run --release -q -- "$HERE")
python3 "$HERE/render_formats.py"
python3 -W ignore "$HERE/count_tokens.py" > /dev/null
python3 -W ignore "$HERE/ablation.py" > /dev/null
python3 "$HERE/pilot/build_pilot.py"
echo "done: results/summary.md, results/token_counts.csv, results/ablation.md, pilot/prompts/"

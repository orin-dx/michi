#!/usr/bin/env bash
# Model command: reads a prompt on stdin, prints Claude's bare answer on stdout, and prints one `META {json}` line
# on stderr (cost, duration, token usage) for the runner to record. Same invocation as the TOON pilot (minimal
# system prompt, no tools) with three additions: it runs in an empty temporary directory so no CLAUDE.md is picked up,
# it asks for JSON output so cost and Claude's own input-token count come back with the answer, and it does not
# persist the session.
#
# Fails loudly: a non-zero exit from claude, an error result, or an empty answer exits 1 with the reason on stderr, so
# the runner records an error instead of a wrong answer. Usage: cmd/claude.sh claude-haiku-5-5
set -uo pipefail
model="${1:?usage: claude.sh <model>}"
dir=$(mktemp -d "${TMPDIR:-/tmp}/shapes-eval.XXXXXX") || { echo "mktemp failed" >&2; exit 1; }
trap 'rm -rf "$dir"' EXIT
cd "$dir" || exit 1
claude -p --model "$model" \
  --system-prompt "You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting." \
  --tools "" --output-format json --no-session-persistence --strict-mcp-config \
  >"$dir/out" 2>"$dir/err"
rc=$?
if [[ $rc -ne 0 ]]; then
  echo "claude exited $rc: $(tail -c 400 "$dir/err") $(head -c 400 "$dir/out")" >&2
  exit 1
fi
python3 -I - "$dir/out" <<'EOF'
import json, sys
raw = open(sys.argv[1], encoding="utf-8").read()
try:
    obj = json.loads(raw)
except ValueError:
    sys.exit("claude printed non-JSON output: " + raw[:300])
if isinstance(obj, list):
    obj = next((o for o in obj if isinstance(o, dict) and o.get("type") == "result"), {})
answer = obj.get("result")
if obj.get("is_error") or not isinstance(answer, str) or not answer.strip():
    sys.exit("claude returned an error or an empty answer: " + raw[:400])
u = obj.get("usage") or {}
meta = {"cost_usd": obj.get("total_cost_usd"), "duration_ms": obj.get("duration_ms"),
        "api_ms": obj.get("duration_api_ms"), "turns": obj.get("num_turns"),
        "input_tokens": u.get("input_tokens"), "cache_creation": u.get("cache_creation_input_tokens"),
        "cache_read": u.get("cache_read_input_tokens"), "output_tokens": u.get("output_tokens")}
print(answer)
print("META " + json.dumps(meta), file=sys.stderr)
EOF

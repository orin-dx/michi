#!/usr/bin/env bash
# Pilot model command: reads a prompt on stdin, prints agy's answer for the given model. Runs in an empty
# directory so the agent has nothing to inspect but the prompt. An empty answer (quota, auth or CLI failure)
# exits non-zero with agy's stderr, so the harness records an error instead of a wrong answer.
set -uo pipefail
model="${1:-gemini-3.8-flash-medium}"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
prompt=$(cat)
cd "$dir" || exit 1
# agy's print mode takes the prompt as an argument; it does not read stdin.
text="You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting. Do not run commands or read files.

$prompt"
answer=$(agy -p "$text" --model "$model" --disable-slash-commands --print-timeout 200s 2>"$dir/err")
if [[ -z "${answer//[[:space:]]/}" ]]; then
  tail -3 "$dir/err" >&2
  exit 1
fi
printf '%s\n' "$answer"

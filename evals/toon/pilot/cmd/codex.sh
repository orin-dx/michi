#!/usr/bin/env bash
# Pilot model command: reads a prompt on stdin, prints Codex's answer. Runs in an empty directory with a
# read-only sandbox so the agent has nothing to inspect but the prompt. An empty answer (quota, auth or CLI
# failure) exits non-zero with Codex's stderr, so the harness records an error instead of a wrong answer.
set -uo pipefail
model="${1:-gpt-6-sol}"
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
prompt=$(cat)
cd "$dir" || exit 1
answer=$({
  echo "You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting. Do not run commands or read files."
  echo
  printf '%s\n' "$prompt"
} | codex exec --skip-git-repo-check -s read-only -m "$model" - 2>"$dir/err")
if [[ -z "${answer//[[:space:]]/}" ]]; then
  tail -5 "$dir/err" >&2
  exit 1
fi
printf '%s\n' "$answer"

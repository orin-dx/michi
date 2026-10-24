# TOON benchmarks

How many tokens TOON saves over other list formats, and whether models read it as accurately. Both were measured on the same seeded datasets, rendered by michi-core. Everything here can be reproduced from [`evals/toon`](../evals/toon/).

## Summary

- **Fewer tokens.** For a 100-row list, TOON is a median **47% smaller than pretty JSON**, **23% smaller than minified JSON**, **33% smaller than Markdown-KV** and **8% smaller than a Markdown table**.
- **Same accuracy.** On four models from two providers (Claude Haiku, Sonnet and Opus 5.5, and OpenAI's GPT-6 Sol), models answered retrieval questions about TOON as accurately as about JSON and Markdown: **96.8%** for TOON against 96.7% to 97.7% for the other formats, every difference within noise.
- **So: no loss in accuracy, much more data per context window.** Against the pretty JSON agents usually get, the same token budget holds nearly twice as many rows; against minified JSON, about 30% more.
- **Where TOON saves less.** Sparse rows (JSON can leave absent fields out; TOON writes `null`), nested arrays and key-value blocks narrow the gap. A key-value block is about the size of minified JSON.

## Token counts

Nine datasets that look like real agent tool output (code search hits, GitHub issues, package search, test failures, a dependency audit, a file listing, service status, sparse contacts, and a ledger with IDs above 2^53), each at 5, 25 and 100 rows. TOON comes from michi-core; the other formats are written the conventional way. Counts use the `cl100k_base` tokenizer; `o200k_base` gives the same picture within a point.

TOON's savings, median (and range) across the datasets:

| Rows | vs pretty JSON       | vs minified JSON     | vs Markdown-KV       | vs Markdown table   |
| ---: | -------------------- | -------------------- | -------------------- | ------------------- |
|    5 | 43.9% (22.8 to 47.0) | 18.4% (0.0 to 22.3)  | 27.5% (0.0 to 29.4)  | 11.3% (8.0 to 26.7) |
|   25 | 46.4% (19.9 to 50.1) | 22.7% (-4.3 to 26.2) | 32.5% (-0.8 to 33.9) | 8.2% (1.4 to 20.5)  |
|  100 | 47.2% (20.9 to 50.4) | 23.4% (-4.3 to 26.8) | 33.2% (-0.4 to 35.0) | 7.7% (0.1 to 20.2)  |

Tokens per dataset at 100 rows:

| Dataset                    |  TOON | Pretty JSON | Minified JSON | Markdown-KV | Markdown table |
| -------------------------- | ----: | ----------: | ------------: | ----------: | -------------: |
| code search                | 3,663 |       6,969 |         4,787 |       5,481 |          4,008 |
| dependency audit           | 4,267 |       8,307 |         5,830 |       6,563 |          4,596 |
| file listing               | 3,224 |       6,028 |         4,211 |       4,852 |          3,521 |
| GitHub issues              | 3,966 |       8,002 |         5,021 |       5,821 |          3,971 |
| ledger (big IDs)           | 5,379 |       9,564 |         7,159 |       8,017 |          5,756 |
| package search             | 3,719 |       7,041 |         4,836 |       5,602 |          4,029 |
| service status (key-value) |   920 |       1,163 |           882 |         916 |          1,027 |
| sparse contacts            | 2,365 |       4,226 |         2,585 |       3,173 |          2,964 |
| test failures              | 3,822 |       7,315 |         5,210 |       5,827 |          3,980 |

Claude's tokenizer is not public, so these OpenAI tokenizers are a proxy for Claude token costs.

## Retrieval accuracy

94 questions about the 100-row datasets, with answers computed from the source data rather than from any rendering. Each was asked about the data in all five formats, three times, on four models: 5,640 prompts, with no failed calls. The question kinds are lookups by key ("size of the file at path X?"), finding an item by another field ("which issue has title X?"), counts with and without a filter, filtered lists, min, max and "which item has the largest X", the item at a position ("item 74 in the list"), missing values, and values inside an array field ("how many issues have the label bug?").

Answers were scored strictly: the model was asked for the bare answer, numbers are compared by value (integers digit for digit, so large IDs cannot round), and lists as sets.

### By model

| Format         | Claude Haiku 5.5 | Claude Sonnet 5.5 | Claude Opus 5.5 | GPT-6 Sol | All four |
| -------------- | ---------------: | ----------------: | --------------: | --------: | -------: |
| TOON           |            97.5% |             96.5% |           97.2% |     96.1% |    96.8% |
| Pretty JSON    |            97.9% |             97.2% |           98.2% |     97.2% |    97.6% |
| Minified JSON  |            98.2% |             97.2% |           97.9% |     96.8% |    97.5% |
| Markdown-KV    |           100.0% |             95.7% |           98.9% |     96.1% |    97.7% |
| Markdown table |            97.9% |             95.7% |           96.8% |     96.5% |    96.7% |

Each per-model cell is 282 answers (about ±2 points at 95% confidence); each "All four" cell is 1,128 answers (about ±1 point). No format is consistently best: Markdown-KV leads on Haiku and Opus and ties for last on Sonnet and GPT-6 Sol.

### By question kind (all four models)

| Kind | Answers per format | TOON | Pretty JSON | Minified JSON | Markdown-KV | Markdown table |
| --- | --: | --: | --: | --: | --: | --: |
| lookup by key | 348 | 99.1% | 100.0% | 98.6% | 98.6% | 99.7% |
| count | 120 | 95.8% | 98.3% | 99.2% | 99.2% | 97.5% |
| count with a filter | 96 | 99.0% | 94.8% | 89.6% | 93.8% | 94.8% |
| filtered list | 108 | 99.1% | 100.0% | 100.0% | 100.0% | 100.0% |
| largest X (argmax) | 84 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |
| max | 24 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |
| min | 96 | 87.5% | 89.6% | 90.6% | 94.8% | 87.5% |
| item at a position | 108 | 96.3% | 95.4% | 99.1% | 96.3% | 88.9% |
| find by another field | 96 | 95.8% | 100.0% | 100.0% | 96.9% | 99.0% |
| count of missing values | 24 | 87.5% | 91.7% | 100.0% | 100.0% | 87.5% |
| missing value lookup | 12 | 75.0% | 75.0% | 83.3% | 83.3% | 100.0% |
| value inside an array field | 12 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |

Small groups (under 30 answers) swing on one or two answers. The "min" misses are mostly one question ("lowest size among all files") that every format gets wrong the same way: models skip the files of size 0.

### How the accuracy was measured

- **Claude models** ran through `claude -p` with a minimal system prompt ("You answer questions about the data given in the prompt. Reply with only the answer") and no tools, so nothing but the prompt and the data reached the model.
- **GPT-6 Sol** ran through `codex exec` in an empty temporary directory with a read-only sandbox, with the same instruction at the top of the prompt and a line telling it not to run commands or read files (`evals/toon/pilot/cmd/codex.sh`).
- A first Claude run without the minimal system prompt let Claude Code's own instructions through, and Sonnet then often reasoned aloud instead of answering; strict scoring counted those as wrong and made TOON look worse than it is. That run is kept in [`docs/research/toon-retrieval-accuracy.md`](research/toon-retrieval-accuracy.md) as superseded.

## Limits

- Two providers so far: Anthropic (three sizes) and OpenAI (one model). Gemini and open-weight models are not measured yet (`evals/toon/pilot/cmd/agy.sh` runs them through agy).
- Lists of up to 100 rows. Much longer lists and deeply nested data are untested.
- Exact-answer questions. Open-ended tasks that use the data are untested.

## Reproduce

From `evals/toon/`:

```bash
./run_all.sh                     # corpus, renders, token counts, pilot prompts
python3 pilot/run_pilot.py --model-cmd 'python3 pilot/oracle.py' --label oracle    # self-test: expect 100%
python3 pilot/run_pilot.py --label <name> --repeats 3 --jobs 6 \
  --model-cmd 'claude -p --model <model> --system-prompt "You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting." --tools ""'
```

For Codex or agy, pass a wrapper instead: `--model-cmd 'pilot/cmd/codex.sh gpt-6-sol'` or `--model-cmd 'pilot/cmd/agy.sh gemini-3.8-flash-medium'`. Mind the provider's quota: 1,410 calls per model at three repeats. For Codex or agy, pass a wrapper instead: `--model-cmd 'pilot/cmd/codex.sh gpt-6-sol'` or `--model-cmd 'pilot/cmd/agy.sh gemini-3.8-flash-medium'`. Mind the provider's quota: 1,410 calls per model at three repeats. `--model-cmd` takes any command that reads a prompt on stdin and prints the answer, so other models can be measured the same way. Raw answers and per-run summaries are in `evals/toon/results/` (`pilot_r2-<model>.jsonl`, `pilot_r2-<model>_summary.md`); the token method, including how each format is written, is in [`evals/toon/results.md`](../evals/toon/results.md).

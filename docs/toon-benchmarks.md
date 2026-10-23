# TOON benchmarks

How many tokens TOON saves over other list formats, and whether models read it as accurately. Both were measured on the same seeded datasets, rendered by michi-core. Everything here can be reproduced from [`evals/toon`](../evals/toon/).

## Summary

- **Fewer tokens.** For a 100-row list, TOON is a median **47% smaller than pretty JSON**, **23% smaller than minified JSON**, **33% smaller than Markdown-KV** and **8% smaller than a Markdown table**.
- **Same accuracy.** On Claude Haiku, Sonnet and Opus 5.5, models answered retrieval questions about TOON as accurately as about JSON and Markdown: **97.0%** for TOON against 96.8% to 98.2% for the other formats, every difference within noise.
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

94 questions about the 100-row datasets, with answers computed from the source data rather than from any rendering. Each was asked about the data in all five formats, three times, on three models: 4,230 prompts, with no failed calls. The question kinds are lookups by key ("size of the file at path X?"), finding an item by another field ("which issue has title X?"), counts with and without a filter, filtered lists, min, max and "which item has the largest X", the item at a position ("item 74 in the list"), missing values, and values inside an array field ("how many issues have the label bug?").

Answers were scored strictly: the model was asked for the bare answer, numbers are compared by value (integers digit for digit, so large IDs cannot round), and lists as sets.

### By model

| Format         | Haiku 5.5 | Sonnet 5.5 | Opus 5.5 | All three |
| -------------- | --------: | ---------: | -------: | --------: |
| TOON           |     97.5% |      96.5% |    97.2% |     97.0% |
| Pretty JSON    |     97.9% |      97.2% |    98.2% |     97.8% |
| Minified JSON  |     98.2% |      97.2% |    97.9% |     97.8% |
| Markdown-KV    |    100.0% |      95.7% |    98.9% |     98.2% |
| Markdown table |     97.9% |      95.7% |    96.8% |     96.8% |

Each per-model cell is 282 answers (about ±2 points at 95% confidence); each "All three" cell is 846 answers (about ±1.2 points). No format is consistently best: Markdown-KV leads on Haiku and Opus and ties for last on Sonnet.

### By question kind (all three models)

| Kind | Answers per format | TOON | Pretty JSON | Minified JSON | Markdown-KV | Markdown table |
| --- | --: | --: | --: | --: | --: | --: |
| lookup by key | 261 | 100.0% | 100.0% | 98.9% | 98.9% | 100.0% |
| count | 90 | 96.7% | 100.0% | 100.0% | 100.0% | 97.8% |
| count with a filter | 72 | 98.6% | 95.8% | 90.3% | 98.6% | 95.8% |
| filtered list | 81 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |
| largest X (argmax) | 63 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |
| max | 18 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |
| min | 72 | 87.5% | 90.3% | 91.7% | 97.2% | 87.5% |
| item at a position | 81 | 95.1% | 93.8% | 98.8% | 95.1% | 85.2% |
| find by another field | 72 | 94.4% | 100.0% | 100.0% | 95.8% | 98.6% |
| count of missing values | 18 | 94.4% | 94.4% | 100.0% | 100.0% | 100.0% |
| missing value lookup | 9 | 66.7% | 66.7% | 77.8% | 77.8% | 100.0% |
| value inside an array field | 9 | 100.0% | 100.0% | 100.0% | 100.0% | 100.0% |

Small groups (under 20 answers) swing on one or two answers. The "min" misses are mostly one question ("lowest size among all files") that every format gets wrong the same way: models skip the files of size 0.

### How the accuracy was measured

Each call ran a Claude model through `claude -p` with a minimal system prompt ("You answer questions about the data given in the prompt. Reply with only the answer") and no tools, so nothing but the prompt and the data reached the model. A first run without that system prompt let Claude Code's own instructions through, and Sonnet then often reasoned aloud instead of answering; strict scoring counted those as wrong and made TOON look worse than it is. That run is kept in [`docs/research/toon-retrieval-accuracy.md`](research/toon-retrieval-accuracy.md) as superseded.

## Limits

- Claude models only. Other model families may read the formats differently.
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

`--model-cmd` takes any command that reads a prompt on stdin and prints the answer, so other models can be measured the same way. Raw answers and per-run summaries are in `evals/toon/results/` (`pilot_r2-<model>.jsonl`, `pilot_r2-<model>_summary.md`); the token method, including how each format is written, is in [`evals/toon/results.md`](../evals/toon/results.md).

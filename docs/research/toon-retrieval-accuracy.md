# TOON retrieval accuracy

Status: Open

## Question

For list data returned to an assistant, does TOON preserve retrieval accuracy compared with Markdown-KV, and does the result vary by model size?

## Evidence so far

Token counts are measured in [`evals/toon`](../../evals/toon/results.md) (2026-10-21): nine seeded datasets at 5, 25 and 100 rows, rendered by michi-core and compared with pretty and minified JSON, Markdown-KV and a Markdown table under the `cl100k_base` and `o200k_base` tokenizers. For flat lists at 100 rows TOON is a median 47% smaller than pretty JSON, 23% smaller than minified JSON, 33% smaller than Markdown-KV and 7.5% smaller than a Markdown table. On size the Markdown table is the real competitor, so the open question is retrieval accuracy, not tokens.

Retrieval accuracy is still unmeasured. The pilot harness is ready: 94 questions with answers computed from the source data, crossed with the five formats (470 prompts), run by `evals/toon/pilot/run_pilot.py --model-cmd '<command reading a prompt on stdin>'`.

## Method

Use the same list data and retrieval tasks in both formats. Compare answers against a predefined scoring rule across several model sizes. Record model/version, prompt, data, token count, and per-task result so the comparison can be reproduced.

## Decision this blocks

Recommend a default format for assistant-facing list responses, or leave the choice to consumers if results are mixed. Do not claim that lower token count implies better retrieval.

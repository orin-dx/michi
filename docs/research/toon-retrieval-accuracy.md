# TOON retrieval accuracy

Status: First results (2026-10-23); not conclusive

## Question

For list data returned to an assistant, does TOON preserve retrieval accuracy compared with Markdown-KV, and does the result vary by model size?

## Evidence so far

Token counts are measured in [`evals/toon`](../../evals/toon/results.md) (2026-10-21): nine seeded datasets at 5, 25 and 100 rows, rendered by michi-core and compared with pretty and minified JSON, Markdown-KV and a Markdown table under the `cl100k_base` and `o200k_base` tokenizers. For flat lists at 100 rows TOON is a median 47% smaller than pretty JSON, 23% smaller than minified JSON, 33% smaller than Markdown-KV and 7.5% smaller than a Markdown table. On size the Markdown table is the real competitor, so the open question is retrieval accuracy, not tokens.

## First pilot run (2026-10-23)

94 questions with answers computed from the 100-row source data, each asked once in all five formats (470 prompts per model), rendered by michi-core 0.5.0. Raw answers and summaries: `evals/toon/results/pilot_<model>.jsonl` and `pilot_<model>_summary.md`. The harness's oracle self-test scores 100%.

Command: `python3 pilot/run_pilot.py --model-cmd 'claude -p --model <model>' --label <model> --jobs 6`.

| Format | Haiku 5.5 | Sonnet 5.5 (strict) | Sonnet 5.5, right answer found but not given bare |
| --- | --: | --: | --: |
| TOON | 98.9% | 83.0% | 88.3% |
| JSON, pretty | 98.9% | 92.6% | 94.7% |
| JSON, minified | 98.9% | 88.3% | 93.6% |
| Markdown-KV | 98.9% | 90.4% | 94.7% |
| Markdown table | 98.9% | 91.5% | 95.7% |

- **Haiku 5.5: no difference.** The one miss is the same question in every format ("lowest size among all files": every format answered 2703, skipping the files of size 0).
- **Sonnet 5.5: TOON trails.** Strictly scored, TOON gets 78 of 94; the other formats get 83 to 87. Some misses are format non-compliance: the model reasoned aloud and reached the right answer, but the scorer reads only the first line. Counting those as right, TOON still has 11 wrong answers against 4 to 6 for each other format.
- **Where TOON loses on Sonnet:** lookups by key (79% against 83 to 97%) and "which item has the largest X" (43%, the same as the Markdown table and below JSON's 57 to 100%). Both tabular formats lose on argmax, which suggests scanning a column of a wide table is the hard part, not TOON's syntax as such.
- **Where TOON wins on Sonnet:** sparse records with many missing fields (100%, with the Markdown table, against 83% for JSON and Markdown-KV) and looking up a missing value.

## Caveats

- **One repeat, 94 questions per format.** At around 90% accuracy the standard error is about 3 points, so a difference of five or six questions is suggestive, not established.
- **`claude -p` is Claude Code in headless mode, not a bare API call.** It brings Claude Code's system prompt, which likely explains why Sonnet often reasoned aloud despite the prompt asking for the bare answer. Haiku followed the instruction every time.
- **No third model size yet** (Opus 5.5), and no non-Anthropic model.

## Next

- Re-run with `--repeats 3` on Haiku, Sonnet and Opus through a bare API call (or a minimal system prompt), so format compliance doesn't mix into accuracy.
- Look at TOON's lookup misses on Sonnet: if they cluster on rows far from the header, a row number per item (or repeating the header on long lists) may help, at a small token cost.

## Decision this blocks

Recommend a default format for assistant-facing list responses, or leave the choice to consumers if results are mixed. Do not claim that lower token count implies better retrieval. **So far:** no change to michi's default. On the small model the formats are equal; on the mid-size model TOON is a few points behind JSON and the Markdown table for lookups and ahead for sparse data, within noise of a single run.

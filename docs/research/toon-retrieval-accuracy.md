# TOON retrieval accuracy

Status: Answered for Claude models (2026-10-23): TOON keeps retrieval accuracy

## Question

For list data returned to an assistant, does TOON preserve retrieval accuracy compared with Markdown-KV, and does the result vary by model size?

## Answer

Yes, on the three Claude 5.5 sizes tested. With a clean prompt and three repeats, TOON is within noise of pretty JSON, minified JSON, Markdown-KV and a Markdown table on every model, and it costs the fewest tokens. michi keeps TOON as its default list format.

## Evidence

Token counts are measured in [`evals/toon`](../../evals/toon/results.md) (2026-10-21): nine seeded datasets at 5, 25 and 100 rows, rendered by michi-core and compared with pretty and minified JSON, Markdown-KV and a Markdown table under the `cl100k_base` and `o200k_base` tokenizers. For flat lists at 100 rows TOON is a median 47% smaller than pretty JSON, 23% smaller than minified JSON, 33% smaller than Markdown-KV and 7.5% smaller than a Markdown table.

### Round 2 (2026-10-23): the measurement to rely on

94 questions with answers computed from the 100-row source data (lookups, counts, filters, min/max/argmax, positions, missing values), each asked in all five formats, three times, on three models: 1,410 prompts per model, no command errors. Rendered by michi-core 0.6.0. Each call used a minimal system prompt ("Reply with only the answer") and no tools. Raw answers and summaries: `evals/toon/results/pilot_r2-<model>.jsonl` and `pilot_r2-<model>_summary.md`.

| Format         | Haiku 5.5 | Sonnet 5.5 | Opus 5.5 |
| -------------- | --------: | ---------: | -------: |
| TOON           |     97.5% |      96.5% |    97.2% |
| JSON, pretty   |     97.9% |      97.2% |    98.2% |
| JSON, minified |     98.2% |      97.2% |    97.9% |
| Markdown-KV    |    100.0% |      95.7% |    98.9% |
| Markdown table |     97.9% |      95.7% |    96.8% |

- Each cell is 282 answers; its 95% interval is about ±1.5 to ±2.4 points. Every format overlaps every other on every model.
- No format wins consistently: Markdown-KV is best on Haiku and Opus and joint worst on Sonnet; TOON is ahead of both Markdown formats on Sonnet.
- By question kind the differences are a few answers each and change direction between models. TOON answered all 87 lookups correctly on Sonnet, the kind it seemed weakest at in round 1.

Command: `python3 pilot/run_pilot.py --model-cmd 'claude -p --model <model> --system-prompt "You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting." --tools ""' --label r2-<model> --repeats 3 --jobs 6`.

### Round 1 (2026-10-23): superseded

One repeat on Haiku and Sonnet through plain `claude -p`, which runs Claude Code headless with its own system prompt. Haiku scored 98.9% in every format. Sonnet scored 83.0% on TOON against 88.3 to 92.6% for the others, but it often reasoned aloud instead of giving the bare answer, and the scorer reads only the first line. Round 2 removed that confound (minimal system prompt, no tools) and added repeats and Opus; the TOON gap disappeared. Round 1's files are `pilot_haiku-5-5*` and `pilot_sonnet-5-5*`.

## Limits

- Claude models only. A non-Anthropic model may read the formats differently.
- Retrieval from lists of 100 rows. Much longer lists, or deeply nested data, are untested.
- The questions ask for exact answers. Open-ended tasks that use the data are untested.

## Decision

Keep TOON as michi's default for assistant-facing lists: same accuracy, fewer tokens. Revisit if a consumer reports a model where it reads worse, using the same harness (`--model-cmd` accepts any command that reads a prompt on stdin).

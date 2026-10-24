# Shape x format x task evaluation

Which body format works best for which shape of data when an agent reads tool output? The TOON eval in [`evals/toon`](../toon/) measured one shape (uniform lists of flat records). This one covers seven shapes in up to nine formats. The write-up, with results and caveats, is [`docs/research/shape-format-eval.md`](../../docs/research/shape-format-eval.md).

| Shape      | Source data                                    |
| ---------- | ---------------------------------------------- |
| `uniform`  | 50 deployments, 9 flat fields                  |
| `single`   | one instance record, 17 scalar fields (2 null) |
| `nested`   | an issue: labels, assignees, 8 comments        |
| `tree`     | a file tree 4 levels deep, sizes and shas      |
| `events`   | 40 events of 4 types with different fields     |
| `sparse`   | 40 rows x 12 columns, about 35% null           |
| `longtext` | 24 tickets with 2 to 5 sentence summaries      |

Formats: TOON from michi (`toon`), TOON from the official reference encoder (`toon_ref`, "reference encoder, not michi"), minified JSON, pretty JSON, minified JSON with nulls omitted (sparse only), YAML, Markdown, TOML (single and nested only) and XML (nested and tree only). Each shape has 12 questions: lookup, filter and count, aggregate, multi-hop, null count where it applies, and two exact-copy tasks (return an opaque id or path character for character). Answers are computed from the source data in `gen_shapes.py`, never from a rendering.

## Setup

```bash
pip install --target "$TMPDIR/py" tiktoken==0.14.0 pyyaml tomli_w tomli    # tomli only on Python < 3.11
(cd evals/shapes && npm install)                                            # @toon-format/toon 4.4.0
```

## Build everything (no model calls)

```bash
evals/shapes/run_all.sh
```

This runs `gen_shapes.py` (corpus and `questions.jsonl`), `toon_ref.mjs` (reference TOON, with a decode round trip), `michi-render/` (michi's TOON), `render_formats.py` (the other formats, round-trip checked where a parser exists, and `renders/manifest.json` with every skipped cell and the reason), `count_tokens.py` (cl100k and o200k), `build_prompts.py` (`prompts/all.jsonl`) and `selftest.py` (fake models: an oracle must score 100%, a wrong answer 0%, and an upper-cased answer must fail every exact-copy question). `corpus/`, `renders/` and `prompts/` are gitignored.

## Run a model

`cmd/claude.sh <model>` is the same call as the TOON pilot (`claude -p`, minimal system prompt, `--tools ""`), run in an empty directory, with JSON output so cost and Claude's own input-token count are recorded. It exits non-zero on any error or empty answer.

```bash
cd evals/shapes
# a 10-call pilot first
python3 run_eval.py --model-cmd 'cmd/claude.sh claude-haiku-5-5' --label haiku-5-5 --jobs 2 \
  --ids uniform-01:json_pretty,uniform-11:toon,single-01:json_min,nested-05:toon_ref,tree-10:xml,events-12:md,sparse-04:yaml,longtext-05:json_pretty,sparse-02:md,tree-07:toon_ref
# the full grid (492 calls per model); resumes where the label left off
python3 run_eval.py --model-cmd 'cmd/claude.sh claude-haiku-5-5' --label haiku-5-5 --jobs 4
# a subset: shape:format cells
python3 run_eval.py --model-cmd 'cmd/claude.sh claude-sonnet-5-5' --label sonnet-5-5 --cells sparse:toon,sparse:yaml
python3 summarize.py            # results/summary.md, results/misses_<label>.md
```

`run_eval.py` refuses to start if the calls already recorded in `results/raw_*.jsonl` plus the planned ones pass `--max-calls` (default 900), and stops submitting after 3 consecutive command errors. Raw answers, scores, latency and cost per call are in `results/raw_<label>.jsonl`; `results/summary.md` is generated from them.

## Layout

| File | Purpose |
| --- | --- |
| `gen_shapes.py` | seeded shapes and questions with ground truth |
| `toon_ref.mjs`, `package.json` | reference TOON encoder 4.4.0 |
| `michi-render/` | michi's TOON (list and key-value) |
| `render_formats.py` | JSON, YAML, TOML, XML and Markdown renderers, `manifest.json` |
| `count_tokens.py` | tiktoken counts |
| `build_prompts.py` | prompt template; no format is explained to the model |
| `run_eval.py`, `score.py`, `cmd/` | runner, strict scorer, model wrappers |
| `summarize.py` | tables, pooled and paired comparisons |
| `results/` | token counts, raw answers per model, generated summary |

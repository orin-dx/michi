# TOON value evaluation: token counts and a retrieval pilot

Date: 2026-10-21. Location: `evals/toon/` in the michi repository. Generated files (corpus, renders, prompts) are gitignored; `./run_all.sh` regenerates them.

## The claim tested

`docs/spec/02-toon-format.md` says TOON uses "roughly 40% fewer tokens than equivalent JSON for list data". `docs/spec/06-decisions.md` and `docs/research/toon-retrieval-accuracy.md` leave open whether TOON keeps retrieval accuracy as well as Markdown-KV does.

## Verdict

**Measured:**

- **The claim holds against pretty-printed JSON.** For the 8 list datasets at 100 rows, TOON is a median **47% smaller** (range 44–50%).
- **The claim does not hold against minified JSON.** Against minified JSON TOON is a median **23% smaller** (range 8.5–27%).
- **Against Markdown-KV**, TOON is a median **33% smaller** (range 25–35%).
- **Against a Markdown table**, TOON is only a median **7.5% smaller**, and the two tie on one dataset.

The spec sentence is accurate only if "equivalent JSON" means indented JSON.

**Retrieval accuracy (measured 2026-10-23):** on Claude Haiku, Sonnet and Opus 5.5, with three repeats, TOON answered 97.0% of the 94 questions correctly against 96.8% to 98.2% for the other formats, all within noise. Results: [`docs/toon-benchmarks.md`](../../docs/toon-benchmarks.md); evidence and the superseded first run: [`docs/research/toon-retrieval-accuracy.md`](../../docs/research/toon-retrieval-accuracy.md).

## Method

### Corpus

`gen_corpus.py` builds the corpus with seeded `random.Random` (seed `20261021` plus the dataset index). The 5- and 25-row files are the first rows of the 100-row file, so the output is the same on every run. There are 9 datasets, each at 5, 25 and 100 rows, for 27 files in `corpus/`. Every string is under 200 chars, so michi's default `max_cell_len=200` never truncates. That keeps the TOON output lossless and comparable; the renderer asserts that nothing was truncated.

| dataset | michi type name | shape |
| --- | --- | --- |
| code_search | `matches` | path, line, column, language, snippet (code with `(){}:,`), score (float) |
| github_issues | `issues` | number, title (some contain `:`), state, author, **labels (array)**, comments, created_at |
| package_search | `packages` | name, version, description (with commas), downloads, license, updated |
| test_failures | `failures` | test (`mod::tests::name`), file, line, duration_ms, retried (bool), message (quotes, backticks, some with `\n`) |
| dependency_audit | `findings` | GHSA id, package, severity, cvss, installed, fixed_in (**30% null**), title |
| file_listing | `files` | path, type, size, mode (`-rw-r--r--`, which TOON must quote), modified |
| service_status | (KV) | 5/25/100 `key: value` pairs: ints, floats, bools, strings, nulls (`render_kv`) |
| sparse_contacts | `contacts` | id and name, plus 7 optional fields that are **absent** in 40–90% of rows |
| ledger_bigids | `transactions` | txn_id/account_id **above 2^53, 20% above i64::MAX**, `"$1,234.56"`, memos with `,` and `"`, `"Acme Corp, Inc."` |

### Formats

- **TOON** comes from michi-core itself, via a path dependency (`michi-render/`, features `serde`). Lists use `michi_core::toon::list(type_name, rows)`; the KV dataset uses `michi_core::kv::render_kv(items, None, &[])`. Neither `totalCount` nor `help[]` is appended, so only the data is compared.
  - michi-core is built from the surrounding checkout through a relative path dependency. The published numbers came from the 0.4 hardening branch (crate version 0.3.1 with the 0.4 changes) built with rustc 1.98.1.
- The other four formats come from `render_formats.py`:
  - **Pretty JSON:** `json.dumps(indent=2)`.
  - **Minified JSON:** `separators=(",", ":")`. Lists are encoded as `{"<type>": [rows]}`, the value the TOON document describes. KV is encoded as `{key: value}`.
  - **Markdown-KV:** a `## <type> <n>` heading, then one `key: value` line per field, with a blank line between rows.
  - **Markdown table:** a header row, a `---` row, then one row per item with `|` escaped.
- How the formats differ in content:
  - JSON and Markdown-KV omit absent sparse fields, while TOON must write `null`. That is JSON's natural advantage on sparse data.
  - The Markdown formats write arrays as `bug, docs`. michi's `list()` writes a nested array as a JSON string cell: `"[\"bug\",\"docs\"]"`.
  - The Markdown formats write line feeds as a literal `\n` so that each row stays on one line.

### Tokenizers

- **Library:** `tiktoken` 0.14.0, installed with `pip install --target $TMPDIR/py tiktoken`.
- **Encodings:** `cl100k_base` (GPT-4 / GPT-3.5) and `o200k_base` (GPT-4o family). The encoding files are cached in `tiktoken_cache/`, so later runs work offline.
- **Not covered:** no Anthropic, Llama or Gemini tokenizer was available offline. Claude's tokenizer is not public, so the counts here do not measure Claude token costs directly. They are only a proxy.

## Results: token counts

The full tables for both tokenizers are in `results/summary.md`; the raw counts (tokens, chars and bytes per file) are in `results/token_counts.csv`. The two tokenizers agree to within about 1.5 percentage points everywhere.

### Savings summary

Each cell is TOON's saving: the median, then the min..max across the 9 datasets. Positive means TOON is smaller.

| tokenizer | rows | vs pretty JSON | vs minified JSON | vs Markdown-KV | vs Markdown table |
| --- | --: | --- | --- | --- | --- |
| cl100k_base | 5 | 43.9% (22.8..47.0) | 18.4% (0.0..22.3) | 27.5% (0.0..29.4) | 11.3% (8.0..26.7) |
| cl100k_base | 25 | 46.4% (19.9..50.1) | 22.7% (-4.3..26.2) | 32.5% (-0.8..33.9) | 8.2% (1.4..20.5) |
| cl100k_base | 100 | 47.2% (20.9..50.4) | 23.4% (-4.3..26.8) | 33.2% (-0.4..35.0) | 7.7% (0.1..20.2) |
| cl100k_base | all 27 | 45.4% (19.9..50.4) | 21.7% (-4.3..26.8) | 31.1% (-0.8..35.0) | 8.6% (0.1..26.7) |
| o200k_base | 5 | 43.8% (22.8..47.3) | 18.4% (0.0..21.6) | 27.8% (0.0..28.9) | 11.0% (8.5..26.7) |
| o200k_base | 25 | 46.4% (19.9..50.0) | 22.7% (-4.3..25.7) | 32.5% (-0.8..33.9) | 8.2% (0.9..20.6) |
| o200k_base | 100 | 47.0% (20.9..50.3) | 23.4% (-4.3..26.9) | 33.1% (-0.4..35.1) | 7.7% (-0.6..19.7) |
| o200k_base | all 27 | 45.1% (19.9..50.3) | 21.7% (-4.3..26.9) | 30.7% (-0.8..35.1) | 8.8% (-0.6..26.7) |

**List datasets only (excludes the KV dataset), 100 rows:**

| tokenizer   | vs pretty JSON     | vs minified JSON  | vs Markdown-KV     | vs Markdown table |
| ----------- | ------------------ | ----------------- | ------------------ | ----------------- |
| cl100k_base | 47.3% (43.8..50.4) | 23.5% (8.5..26.8) | 33.4% (25.5..35.0) | 7.4% (0.1..20.2)  |
| o200k_base  | 47.1% (43.7..50.3) | 23.4% (8.2..26.9) | 33.3% (25.1..35.1) | 7.5% (-0.6..19.7) |

### Per-dataset token counts (cl100k_base)

| dataset | rows | toon | json_pretty | json_min | md_kv | md_table | vs pretty | vs min | vs MD-KV | vs MD table |
| --- | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |
| code_search | 5 | 199 | 359 | 244 | 274 | 226 | +44.6% | +18.4% | +27.4% | +11.9% |
| code_search | 25 | 914 | 1734 | 1187 | 1357 | 1009 | +47.3% | +23.0% | +32.6% | +9.4% |
| code_search | 100 | 3663 | 6969 | 4787 | 5481 | 4008 | +47.4% | +23.5% | +33.2% | +8.6% |
| dependency_audit | 5 | 235 | 430 | 300 | 332 | 265 | +45.3% | +21.7% | +29.2% | +11.3% |
| dependency_audit | 25 | 1096 | 2102 | 1475 | 1659 | 1192 | +47.9% | +25.7% | +33.9% | +8.1% |
| dependency_audit | 100 | 4267 | 8307 | 5830 | 6563 | 4596 | +48.6% | +26.8% | +35.0% | +7.2% |
| file_listing | 5 | 175 | 312 | 216 | 244 | 198 | +43.9% | +19.0% | +28.3% | +11.6% |
| file_listing | 25 | 813 | 1511 | 1052 | 1208 | 893 | +46.2% | +22.7% | +32.7% | +9.0% |
| file_listing | 100 | 3224 | 6028 | 4211 | 4852 | 3521 | +46.5% | +23.4% | +33.6% | +8.4% |
| github_issues | 5 | 206 | 389 | 248 | 284 | 224 | +47.0% | +16.9% | +27.5% | +8.0% |
| github_issues | 25 | 999 | 2004 | 1256 | 1450 | 1013 | +50.1% | +20.5% | +31.1% | +1.4% |
| github_issues | 100 | 3966 | 8002 | 5021 | 5821 | 3971 | +50.4% | +21.0% | +31.9% | +0.1% |
| ledger_bigids | 5 | 292 | 487 | 362 | 400 | 320 | +40.0% | +19.3% | +27.0% | +8.8% |
| ledger_bigids | 25 | 1351 | 2386 | 1781 | 1993 | 1452 | +43.4% | +24.1% | +32.2% | +7.0% |
| ledger_bigids | 100 | 5379 | 9564 | 7159 | 8017 | 5756 | +43.8% | +24.9% | +32.9% | +6.5% |
| package_search | 5 | 203 | 361 | 246 | 281 | 228 | +43.8% | +17.5% | +27.8% | +11.0% |
| package_search | 25 | 950 | 1773 | 1218 | 1408 | 1035 | +46.4% | +22.0% | +32.5% | +8.2% |
| package_search | 100 | 3719 | 7041 | 4836 | 5602 | 4029 | +47.2% | +23.1% | +33.6% | +7.7% |
| service_status (KV) | 5 | 44 | 57 | 44 | 44 | 60 | +22.8% | +0.0% | +0.0% | +26.7% |
| service_status (KV) | 25 | 241 | 301 | 231 | 239 | 275 | +19.9% | -4.3% | -0.8% | +12.4% |
| service_status (KV) | 100 | 920 | 1163 | 882 | 916 | 1027 | +20.9% | -4.3% | -0.4% | +10.4% |
| sparse_contacts | 5 | 134 | 239 | 145 | 171 | 174 | +43.9% | +7.6% | +21.6% | +23.0% |
| sparse_contacts | 25 | 618 | 1079 | 662 | 804 | 777 | +42.7% | +6.6% | +23.1% | +20.5% |
| sparse_contacts | 100 | 2365 | 4226 | 2585 | 3173 | 2964 | +44.0% | +8.5% | +25.5% | +20.2% |
| test_failures | 5 | 202 | 370 | 260 | 286 | 221 | +45.4% | +22.3% | +29.4% | +8.6% |
| test_failures | 25 | 952 | 1820 | 1290 | 1441 | 1001 | +47.7% | +26.2% | +33.9% | +4.9% |
| test_failures | 100 | 3822 | 7315 | 5210 | 5827 | 3980 | +47.8% | +26.6% | +34.4% | +4.0% |

### Ablations (100 rows, `results/ablation.md`)

`ablation.py` renders variants of three datasets through the same michi build, to isolate what costs TOON tokens.

| variant | TOON vs min JSON (cl100k) | TOON vs MD table (cl100k) |
| --- | --: | --: |
| github_issues (labels array → JSON-string cell) | +21.0% | +0.1% |
| github_issues, labels flattened to one string `"bug, docs"` | +26.0% | +7.0% |
| github_issues, labels column dropped | +25.8% | +7.8% |
| sparse_contacts (sparse, as shipped) | +8.5% | +20.2% |
| sparse_contacts, every optional field filled | +33.7% | +9.3% |
| test_failures (`a::tests::b` names, which need quotes) | +26.6% | +4.0% |
| test_failures, `a.tests.b` names (no quotes needed) | +29.7% | +6.9% |

## Findings

### Measured

1. **The 40% figure holds against pretty JSON.** The median saving is 45–47% across all 27 files and both tokenizers. The smallest saving on a list dataset is 40.0% (ledger_bigids at 5 rows). Against minified JSON the median is about 22–23%, roughly half the claim.
2. **Savings grow with N, as the spec predicts, but level off early.** Against pretty JSON the median is 43.9% at 5 rows, 46.4% at 25 and 47.2% at 100. Most of the gain is already there at 5 rows.
3. **Key-value data:** `render_kv` matches minified JSON and Markdown-KV at 5 keys. At 25 and 100 keys it is **0.4–4.3% larger** than both. It still beats pretty JSON by about 21% and a Markdown table by 10–27%. TOON KV has no header to amortize, so it is essentially Markdown-KV plus quoting.
4. **Sparse data is TOON's weakest case against JSON.** It saves only 6.6–8.5% against minified JSON, because each absent field becomes a `null` cell. JSON simply omits the key. With every field filled in, the saving rises to 33.7%. Against a Markdown table, TOON does best on sparse data (+20%), because the table's empty cells still cost ` |  |` each.
5. **Nested values erase TOON's lead over a Markdown table.** michi's `list()` encodes an array cell as escaped JSON text (`"[\"bug\",\"docs\"]"`), which is token-expensive. On github_issues, TOON and the Markdown table are **tied** at 100 rows (+0.1% cl100k, −0.6% o200k). That is the only list dataset where TOON is not the smallest format. Flattening labels to a comma-joined string brings TOON back to +7%.
6. **Quoting costs a few points.** Any cell containing `:`, `,`, `"` or a leading `-` is quoted, and `"` inside is escaped. That covers test names like `mod::tests::x`, ISO timestamps, file modes like `-rw-r--r--`, and memos and amounts with commas. Removing the colons from test names alone raised the saving against minified JSON from 26.6% to 29.7%. The quoting-heavy ledger dataset still saves 25% against minified JSON, so quoting is a cost, not a reversal.
7. **Large integers cost TOON nothing extra.** IDs above 2^53 and above i64::MAX render as bare digits, the same as in JSON. ledger_bigids behaves like the other quoting-heavy datasets.

### Inferred, not measured

- "Equivalent JSON" in the spec most likely means pretty or indented JSON, which is what most MCP servers and CLI `--json` flags emit. Against a tool that already emits compact JSON, the accurate figure is **about 20–25% for flat lists, under 10% for sparse lists, and slightly negative for KV**. The spec sentence should name its baseline; suggested wording: "~45% fewer tokens than pretty-printed JSON and ~20–25% fewer than minified JSON for flat list data (tiktoken cl100k/o200k)".
- The Markdown table is the real competitor on size: within 0–12% of TOON on dense lists. Choosing between TOON and Markdown tables is therefore more a question of retrieval accuracy, typing (null vs empty, numbers vs strings) and parseability than of tokens. That is what the open retrieval question should decide.
- Claude's tokenizer may weigh punctuation differently. These percentages should be rechecked with Anthropic's token-counting API before they are quoted for Claude consumers.

## Limitations

- **Synthetic data.** The corpus is realistic in shape, but the vocabulary is small and repetitive, which may make every format tokenize more cheaply than real data would.
- **Two OpenAI tokenizers only.** No Claude, Llama or Gemini tokenizer was used.
- **Data only.** Envelopes (`totalCount`, `help[]`, MCP wrapping) are excluded. They add a small constant to every format.
- **Markdown renderings were chosen by me.** Other choices would shift the Markdown numbers by a few percent, for example YAML-style lists for arrays, or no `##` heading per Markdown-KV record. The rules are documented in `render_formats.py`.
- **Truncation is off.** All cells are under michi's 200-char default `max_cell_len`. In real use, truncation would make michi output smaller still, but lossy. That is not a format effect.
- **Mutable source.** michi-core was built from a worktree another agent was editing. To reproduce exactly, pin the michi commit in `michi-render/Cargo.toml`.
- **No accuracy results.** The retrieval pilot was not run, and no model was called. Nothing here says anything about accuracy.

## How to run the retrieval pilot

The pilot has 94 questions over the 100-row data, 10–12 per dataset. They cover:

- exact lookups and reverse lookups
- counts and threshold counts
- max/argmax and min
- filtered lists
- positional lookups ("item number N")
- extras: counting nested labels, counting nulls/absent fields, a null lookup, and exact lookups of IDs above 2^53

The answers are computed from `corpus/*_100.json`, never from a render. Each question is paired with each of the 5 formats, giving 470 prompts, each about 1k–10k tokens of context.

**Files:**

- `pilot/questions.jsonl`: id, dataset, kind, question, answer, answer_type.
- `pilot/prompts/<format>.jsonl`: the full prompt (instructions, `<data>` context, question), plus expected answer, answer_type and context_file.
- `pilot/build_pilot.py`: regenerates both from the corpus.
- `pilot/run_pilot.py`: the harness. It pipes each prompt to a model command on stdin and scores stdout. It can resume, repeat and run in parallel, and writes per-item records and a summary.
- `pilot/oracle.py`: a self-test "model".

**Scoring:** numbers must match exactly; integers are compared as exact digits, so a rounded 2^53+ ID fails. Strings are compared after normalizing (trimmed, case-folded, surrounding quotes/backticks and a trailing period removed, first line only). Lists are compared as unordered sets. A null answer is correct if the model replies `null`, `none`, `missing` or `n/a`.

**Commands:**

```sh
cd $TMPDIR/toon-eval
# 0. (optional) regenerate everything; needs cargo + tiktoken in $TMPDIR/py
./run_all.sh
# 1. check the harness: must print 100% for every format (and 0% with --wrong)
python3 pilot/run_pilot.py --model-cmd 'python3 pilot/oracle.py' --label selftest
# 2. run a real model; any CLI that reads the prompt on stdin and prints the answer works
python3 pilot/run_pilot.py --model-cmd 'claude -p --model claude-haiku-4-5' --label haiku --jobs 4 --repeats 3
python3 pilot/run_pilot.py --model-cmd 'llm -m gpt-4o-mini' --label gpt4o-mini --jobs 4
python3 pilot/run_pilot.py --model-cmd 'ollama run llama3.1:8b' --label llama8b
#    narrow a run: --formats toon,md_kv --datasets github_issues,ledger_bigids --limit 20
# 3. re-score / re-summarize a run
python3 pilot/run_pilot.py --score-only results/pilot_haiku.jsonl
```

**Output:**

- `results/pilot_<label>.jsonl` holds the raw answer, a `correct` flag and the latency for every prompt and repeat.
- `results/pilot_<label>_summary.md` gives accuracy by format, by dataset x format, and by question kind x format.

To address the open question in 06-decisions.md, compare `toon` against `md_kv` and `md_table` across at least one small, one mid and one frontier model, with `--repeats 3`. Report the token counts from this file next to the accuracy figures.

**Status:** I ran the self-test before writing this report: the oracle scored 470/470 (100% in every format) and `--wrong` scored 0/470. Those runs were deleted afterwards, so `results/` holds no pilot output yet.

## Files

```
gen_corpus.py          seeded corpus generator          -> corpus/<dataset>_<5|25|100>.json
michi-render/          scratch cargo project (path dep on michi-core, serde) -> renders/toon/
render_formats.py      json_pretty, json_min, md_kv, md_table -> renders/<format>/
count_tokens.py        tiktoken counts                  -> results/token_counts.csv, results/summary.md
ablation.py            nested / sparse / quoting ablations -> ablation/, results/ablation.md
pilot/                 retrieval pilot (questions, prompts, harness, oracle)
run_all.sh             regenerates all of the above (CARGO_TARGET_DIR defaults to $TMPDIR/toon-eval-target)
tiktoken_cache/        cached cl100k_base / o200k_base encodings (offline re-runs)
```

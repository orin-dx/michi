# Shape x format evaluation

Status: Answered for token counts; inconclusive for accuracy (2026-10-24). Token counts are exact and repeatable. The accuracy runs are small (12 questions per shape and format, one repeat) and show no format winning on any shape.

## Question

The TOON eval ([retrieval accuracy](toon-retrieval-accuracy.md), [benchmarks](../toon-benchmarks.md)) measured one data shape: uniform lists of flat records. Which body format works best for which shape of data when an LLM agent reads tool output? The answer feeds a later per-call `Format` choice in michi.

## Answer

- **Tokens depend on the shape.** TOON is much smaller than minified JSON on flat tables (28% on a 50-row list, 40% on a sparse table, 17% on records with long text, by `cl100k`), and within 9% either way on a single record, a nested record, a file tree and a mixed event log. Pretty JSON is the largest everywhere (27% to 64% more tokens than minified), and YAML is 3% to 22% larger than minified JSON.
- **Accuracy did not separate the formats.** On Claude Haiku 5.5 across all seven shapes, the five main formats scored 85% to 88% (84 questions each). On Sonnet 5.5 and Opus 5.5 across three shapes, they scored 83% to 89% and 83% to 97% (36 questions each). No paired comparison against minified JSON was significant (smallest p is 0.22). Differences of up to 20 points per shape and format are noise at this size.
- **Identifiers are copied exactly.** In 148 exact-copy calls (16 to 24 character ids, shas and paths) no model changed a character in any format. Every miss was the wrong item chosen or a reply that was not a bare answer.
- **Nothing here supports a format that beats the others for accuracy.** It supports choosing by token cost, which is where the formats differ, and it shows two places where michi's current TOON output is a poor fit (mixed event logs, long text cells that exceed the default cap).

## Method

### Shapes

`evals/shapes/gen_shapes.py` generates all seven from one seed, with synthetic data only. Every answer is computed from the source data in the same script, never from a rendering.

| Shape | Source data | Questions |
| --- | --- | --- |
| `uniform` | 50 deployments, 9 flat fields, one 24-hex `uid` each | 12 |
| `single` | one instance record, 17 scalar fields, 2 of them null | 12 |
| `nested` | an issue with 4 labels, 2 assignees, 8 comments (id, author, timestamp, 2 sentence body) | 12 |
| `tree` | a repo tree 4 levels deep, 35 files, a size and a 20-hex sha per file | 12 |
| `events` | 40 events of 4 types (login, deploy, alert, error) with different fields | 12 |
| `sparse` | 40 rows x 12 columns, 36% of cells null | 12 |
| `longtext` | 24 tickets with 2 to 5 sentence summaries (109 to 308 characters) | 12 |

Each shape has a lookup or two, filter and count, aggregate (sum, min, max, argmax), multi-hop (for example "find the service with the most error events; what version did its latest deploy event deploy?"), a null count where the shape has nulls, and two or three exact-copy tasks ("return the uid of ... exactly as written"). Ids are 16 to 24 characters: hex, mixed-case alphanumeric tokens, shas and paths.

### Formats and the grid

| Format | Where used |
| --- | --- |
| TOON, michi (`toon`) | uniform, single (`render_kv`), events, sparse, longtext |
| TOON, reference encoder `@toon-format/toon` 4.4.0 (`toon_ref`) | nested, tree, events. **Reference encoder, not michi**: michi emits flat tables and key-value blocks only |
| JSON minified, JSON pretty (2 spaces) | every shape |
| JSON minified with nulls omitted | sparse only |
| YAML (PyYAML, no line wrapping) | every shape |
| Markdown | table for uniform, sparse, events, longtext; Markdown-KV for single; headings and bullets for nested; bullet outline for tree |
| TOML (`tomli_w`) | single and nested. TOML has no null, so null fields are omitted |
| XML | nested and tree only |

That is 41 shape-format cells, 12 questions each, 492 prompts per full run. Cells that were not run, and why, are in `evals/shapes/renders/manifest.json` (rebuilt by `run_all.sh`); the main ones:

- `toon_ref` on uniform, single, sparse and longtext: byte-identical to michi's output (checked), so a second arm adds nothing.
- `toon` on nested and tree: michi has no renderer for them.
- TOML on sparse, uniform, events, longtext and tree: out of scope by design, and a sparse table cannot be written in TOML without null.
- Markdown on nested shapes as a table, and Markdown-KV on lists: not run; the table or outline forms are the natural ones.

Each rendering is checked against the source where a parser exists (JSON, YAML, TOML, TOON reference decode; XML is checked for well-formedness only). Markdown cannot tell null from empty (the source has no empty strings), and the events table cannot tell an absent field from a null `trace_id`.

### Prompt, models and scoring

- **Prompt.** The TOON pilot's template, with the data introduced by its format name only: "TOON (Token-Oriented Object Notation)", "JSON", "YAML", "Markdown". **No format was explained to the model.** The pilot gave TOON and Markdown-KV a one-line structure description; this eval adds none for any format, so every format is cold-read.
- **Models.** `claude -p --model <m> --system-prompt "You answer questions about the data given in the prompt. Reply with only the answer: no explanation, no reasoning, no formatting." --tools ""`, as in the TOON pilot, run from an empty directory (so no `CLAUDE.md` is loaded) with JSON output to record cost and Claude's own input-token count. The wrapper (`cmd/claude.sh`) exits non-zero on any error or empty answer, and the runner stops after 3 consecutive errors and refuses to pass a 900-call cap.
- **Scoring.** As in the TOON pilot: first non-empty line only, numbers by value, lists as sets, strings case-insensitive. Exact-copy answers must match case-sensitively and character for character. A wrapper self-test (oracle 100%, wrong answer 0%, upper-cased answer fails every exact-copy question) passes.
- **Runs.** Haiku 5.5 on the full grid (492 calls, which include a 10-call pilot). Sonnet 5.5 and Opus 5.5 on `uniform`, `tree` and `sparse` in 16 cells (192 calls each): the shapes where Haiku's cells spread the most. Events was close behind but did not fit in the 900-call cap. One repeat per cell. 876 calls in total, no failed calls. The 10-call Haiku pilot took about 4 s of wall time per call (1.2 to 1.5 s of API time) and cost $0.008 to $0.034 per call as the CLI reports it, scaling with prompt size; nothing rate-limited, so the full grid ran 4 calls in parallel.
- **Noise.** One repeat and 12 questions per cell means a 95% interval of about +-20 points (Wilson) around any shape-format score. A model-format score over seven shapes (84 questions) is about +-7 points; over three shapes and three models (108 questions) about +-6. The paired tests compare each format with minified JSON on the same questions (exact sign test on the questions where exactly one was right).

## Results

### Tokens

Tokens of the rendering alone, `cl100k_base` (the `o200k_base` counts are within 3% and are in `results/token_counts.csv`). `-` means not run. `†` marks the reference TOON encoder; for events the michi union table is shown first, then the reference encoder.

| Format | uniform | single | nested | tree | events | sparse | longtext |
| --- | --: | --: | --: | --: | --: | --: | --: |
| TOON | **2,667** | 149 | **549†** | 1,134† | 2,015 / 2,377† | **1,936** | **1,786** |
| JSON minified | 3,713 | **144** | 603 | 1,130 | **1,989** | 3,223 | 2,158 |
| JSON pretty | 5,268 | 193 | 772 | 1,849 | 2,910 | 4,656 | 2,732 |
| YAML | 4,238 | 149 | 643 | 1,382 | 2,339 | 3,739 | 2,330 |
| Markdown | 2,876 | 149 | 684 | **888** | 2,667 | 2,271 | 1,855 |
| TOML | - | 147 | 659 | - | - | - | - |
| XML | - | - | 864 | 1,086 | - | - | - |
| JSON minified, nulls omitted | - | - | - | - | - | 2,467 | - |

TOON against minified JSON: uniform -28%, sparse -40%, longtext -17%, nested -9% (reference), tree 0% (reference), single +3%, events +1% (michi) or +20% (reference). Against a Markdown table: 7% smaller on uniform, 15% on sparse, 4% on longtext, 24% on events (michi), and the same on single.

Claude Haiku 5.5 reports its own input tokens for each call. Subtracting a fixed per-call overhead (about 1,700 tokens, estimated as the intercept of a fit of reported tokens against prompt size) leaves the prompt, which is the rendering plus about 100 to 130 tokens of instruction and question that are the same for every format. The ordering is the same as above, but TOON's lead on flat tables is larger in Claude's count:

| Format        | uniform | single | nested |   tree |         events | sparse | longtext |
| ------------- | ------: | -----: | -----: | -----: | -------------: | -----: | -------: |
| TOON          |   3,188 |    252 |   785† | 1,448† | 2,848 / 2,807† |  2,804 |    2,582 |
| JSON minified |   5,192 |    248 |    886 |  1,692 |          2,720 |  4,634 |    3,175 |
| JSON pretty   |   6,698 |    299 |  1,062 |  2,408 |          3,615 |  6,047 |    3,748 |
| YAML          |   5,266 |    240 |    879 |  1,646 |          2,755 |  4,690 |    3,221 |
| Markdown      |   3,600 |    239 |    931 |  1,175 |          3,042 |  3,066 |    2,677 |

By Claude's count TOON is 39% below minified JSON on uniform and sparse, 19% on longtext, 11% and 14% on nested and tree, level on single and 3% to 5% above on events. The method is an estimate (the overhead is a fitted constant), so treat differences under about 100 tokens as noise; the Claude tokenizer is not public and the cl100k and o200k counts understate how much more it spends on JSON punctuation.

Two things in michi's own output:

- **Events.** `toon::list` writes the union of the keys as one header. The four event types give a 19-column table in which 66% of the cells are `null`, so it comes out the same size as minified JSON. The reference encoder's list-of-objects form is 20% larger than minified JSON by `cl100k`.
- **Long text.** `toon::list` truncates a string cell at 200 characters by default, with a `(N more chars)` suffix. 14 of the 24 summaries are longer, so a default `list()` call would have cut them. The renderings in this eval lift the cap so the data stays whole.

### Accuracy, Claude Haiku 5.5 (full grid)

Correct out of 12 per cell; 95% interval about +-20 points.

| Format | uniform | single | nested | tree | events | sparse | longtext | All (of 84) |
| --- | --: | --: | --: | --: | --: | --: | --: | --: |
| TOON | 12 | 11 | 11† | 9† | 9 (10†) | 12 | 10 | 74 (88%) |
| JSON minified | 10 | 11 | 11 | 9 | 9 | 11 | 10 | 71 (85%) |
| JSON pretty | 11 | 11 | 11 | 11 | 9 | 9 | 10 | 72 (86%) |
| YAML | 11 | 12 | 11 | 10 | 8 | 12 | 9 | 73 (87%) |
| Markdown | 11 | 12 | 11 | 9 | 9 | 12 | 10 | 74 (88%) |
| TOML | - | 10 | 9 | - | - | - | - | - |
| XML | - | - | 11 | 9 | - | - | - | - |
| JSON minified, nulls omitted | - | - | - | - | - | 12 | - | - |

The "All" column uses michi's TOON where michi can render the shape and the reference encoder for nested and tree. Paired against minified JSON the five formats are separated by at most 3 questions in the totals (71 to 74 of 84), and the questions where exactly one of two formats was right split about evenly (TOON 6 for / 3 against minified JSON, p = 0.51; pretty JSON 5 / 4; YAML 6 / 4; Markdown 5 / 2).

### Accuracy, Sonnet 5.5 and Opus 5.5 (three shapes, 192 calls each)

Correct out of 12 per cell. Sonnet / Opus.

| Format        | uniform |    tree |  sparse | Pooled (of 36) |
| ------------- | ------: | ------: | ------: | -------------: |
| TOON          |  10 / 9 | 8 / 9 † | 12 / 12 |        30 / 30 |
| JSON minified |  9 / 11 | 11 / 11 | 11 / 12 |        31 / 34 |
| JSON pretty   | 10 / 10 | 10 / 10 | 10 / 12 |        30 / 32 |
| YAML          | 12 / 12 |  8 / 11 | 12 / 12 |        32 / 35 |
| Markdown      | 11 / 11 |  8 / 11 | 12 / 11 |        31 / 33 |
| XML           |       - |  9 / 10 |       - |              - |

Across the same three shapes and all three models (108 questions per format), strict scores are TOON 93, minified JSON 95, pretty JSON 93, YAML 100, Markdown 96. YAML is highest at 93%, but its lead over minified JSON is 5 questions out of 108, inside the noise. With the last line of a multi-line reply also accepted (below), the order changes to JSON minified 107, YAML 106, Markdown 106, pretty JSON 105, TOON 102.

### What the errors were

- **Lookups were never wrong** on Haiku and Opus in any format (17 of 17 and 7 of 7 per format), and wrong once on Sonnet (TOON, 6 of 7).
- **Not a bare answer.** Even with the minimal system prompt, 8% to 13% of Haiku's replies (3% to 14% of Sonnet's and Opus's, by format) worked the answer out in an earlier line and gave it on the last one. Strict scoring reads only the first line, so these count as wrong. Accepting the last line as well lifts Haiku's five formats to 96% to 99% and the three-shape, three-model pool to 94% to 99%. This is a sensitivity check; the headline stays strict, as in the TOON pilot. It is also a reason not to build agents that parse the first line of a reply.
- **Wrong item, same in every format.** `tree-07` ("largest file", two files within 10% of each other at six digits) was missed by Haiku and Sonnet in five of six formats; `tree-12` (largest file under `src/parse`) by Sonnet in two formats. These are question difficulty, not format.
- **Possibly format-specific, weak evidence.** `tree-04` (count files under `tests/` at any depth) was answered 9 (true 6) by Haiku and Sonnet on the reference-TOON tree and correctly in every other format. That is two of three models on one question; Opus got it right with working shown. It matches the TOON project's own finding that nested data is where TOON is neither smaller nor clearer, but it is not enough to call.
- **TOML nulls.** TOML cannot hold null, so the null-count questions on `single` and `nested` cannot be answered from it (Haiku said 0, true 2). That is the lossy rendering, not a model error.

### Exact copy

148 calls asked for an id, sha or path to be returned exactly (82 on Haiku, 33 each on Sonnet and Opus). 138 were exactly right. Of the 10 misses, 8 had the correct id on the last line after working aloud, and 2 (Sonnet, `tree-12`) picked the wrong file. None had a changed character. Identifiers do not need a particular format to survive a copy at this length; this says nothing about longer ids, ids that must be assembled from parts, or copying after many turns.

## What held, and what did not

**Held:**

- TOON's token advantage on flat tables (uniform, sparse, long text) over minified JSON, pretty JSON, YAML and, by a smaller margin, Markdown tables. It holds under both OpenAI tokenizers and under Claude's own count.
- Pretty JSON costing the most tokens in every shape with no accuracy return.
- Every format being readable cold (nothing was explained to the model). TOON, YAML, TOML and XML all scored within noise of JSON.

**Did not hold or was not shown:**

- A TOON advantage on nested, hierarchical or mixed data. By `cl100k` it is 9% smaller on the nested record, level on the tree, and larger than minified JSON on events. The weak `tree-04` signal points the wrong way.
- Any accuracy difference between formats. The apparent leaders and laggards swap by model and shape (YAML led on Sonnet and Opus, pretty JSON was last on sparse for Haiku and Sonnet and 12 of 12 on Opus).
- That Markdown tables lose against TOON on accuracy: they tied everywhere.

## Limits

- 12 questions per cell and one repeat. A single wrong answer moves a cell 8 points. Sonnet and Opus cover three of the seven shapes and 16 of 41 cells; `single`, `nested`, `events` and `longtext` have Haiku only.
- Three Claude models. GPT, Gemini and open-weight models are not measured (their quota was reserved).
- Synthetic data, one seed per shape, and renderings of 150 to 5,300 tokens. Much larger inputs (where position and length effects appear), other seeds and different field mixes are untested.
- The questions are mine. They include some hard-by-design items (sums over 8 durations, counting files in a nested tree) and a few with close ties; they do not cover open-ended tasks. Some arms carry a known loss: TOML drops nulls, Markdown cannot tell null from empty or absent from null.
- Counts for Claude's tokenizer are an estimate (see above). Costs printed by the CLI are not comparable between models (Sonnet's calls hit the prompt cache, Haiku's and Opus's did not), so no dollar figures are given beyond "about $17 for 876 calls".
- Haiku was run first on the full grid and picked the shapes for the other two models, so the subset is chosen by a result from the same data; the comparison between formats inside it is still on fresh model runs.
- No legend or explanation was added for any format. The TOON pilot did add a one-line description of TOON's header to its prompt, so TOON's numbers here are not like-for-like with the pilot's. On this evidence TOON did not need it (Haiku: 12 of 12 on `uniform` and `sparse` cold).

## What this suggests for defaults

This section stays inside the data above; "untested" marks what the data does not cover.

- **Flat tables (uniform lists, sparse tables, lists with long text):** TOON is the cheapest option and as accurate as the others in these runs. Markdown tables are within 4% to 15% of its size and tied it on accuracy, so they are a fine fallback where a Markdown body is wanted. Keep TOON as the default for these.
- **Pretty JSON:** do not use it as the body of agent-facing output. It costs 27% to 64% more tokens than minified JSON, and nothing in these runs shows it reading better. Minified JSON is the safe general fallback.
- **Single record:** the formats are within a few tokens of each other and within noise on accuracy, so token cost does not choose. Key-value TOON, Markdown-KV and YAML are as good as JSON here.
- **Nested records and trees:** no format is clearly better on tokens or accuracy. TOON (reference encoder) is 9% smaller on the nested record and level on the tree; a Markdown outline is 21% smaller than minified JSON on the tree and no more accurate. michi has no renderer for these shapes, and this data gives no reason to add one for token savings. Whether a TOON nested renderer would read worse on trees is the one weak signal (`tree-04`) and is untested beyond it.
- **Mixed event logs:** the flat TOON table that `toon::list` gives is as big as minified JSON and 24% smaller than a Markdown table here, so it is not a saving. Grouping events by type into separate tables is untested and is the obvious thing to try before choosing a format for this shape.
- **Long text fields:** TOON and Markdown tables kept accuracy and saved 14% to 17% in `cl100k`, but a default `toon::list` call cuts cells at 200 characters. If a `Format` option ships, it should make clear that a truncated cell is not the data.
- **TOML:** not suitable where nulls matter (it cannot represent them). **XML:** 43% larger than minified JSON on the nested record and 4% smaller on the tree; no accuracy reason to prefer it.
- **Exact copy of identifiers:** no format effect at 16 to 24 characters in these runs. Untested for longer ids and for copying across tool calls.
- **Replies:** all three models sometimes explain before answering despite a bare-answer system prompt, in every format (3% to 14% of replies, by model and format). Do not rely on a reply being only the answer.

## Reproduce

From `evals/shapes/` (full instructions in its [README](../../evals/shapes/README.md)):

```bash
./run_all.sh        # corpus, renders (michi, reference TOON, other formats), token counts, prompts, self-test
python3 run_eval.py --model-cmd 'cmd/claude.sh claude-haiku-5-5' --label haiku-5-5 --jobs 4
python3 summarize.py
```

Raw answers per call (with cost, latency and Claude's token usage), the generated summary and the lists of wrong answers are in `evals/shapes/results/`.

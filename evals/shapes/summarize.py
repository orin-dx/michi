#!/usr/bin/env python3
"""Summarize token counts and accuracy into results/summary.md (+ results/misses_<model>.md per model).

Inputs: results/token_counts.csv, results/raw_<label>.jsonl (every model run), prompts/all.jsonl.
Needs tiktoken only to turn Claude's reported input tokens into "prompt tokens" (see fixed_overhead()).

Noise: every accuracy cell carries a 95% Wilson half-width; with 12 questions per shape-format cell it is about +-20
points, so cell-level gaps are only meaningful when they are very large. The pooled comparison pairs the same question
across two formats and runs an exact two-sided sign test on the questions where exactly one of the two was right.
"""
import collections
import csv
import glob
import json
import math
import os
import statistics

import tiktoken

from score import is_correct

ROOT = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(ROOT, "results")
SHAPES = ["uniform", "single", "nested", "tree", "events", "sparse", "longtext"]
SHAPE_TITLE = {"uniform": "1. Uniform list of flat records (50 rows)", "single": "2. Single record (17 fields)",
               "nested": "3. Nested record (issue with 8 comments)", "tree": "4. File tree (4 levels)",
               "events": "5. Heterogeneous event log (40 events, 4 types)", "sparse": "6. Sparse table (40 x 12, about 35% null)",
               "longtext": "7. Records with long free text (24 rows)"}
FMT_ORDER = ["toon", "toon_ref", "json_min", "json_min_omit", "json_pretty", "yaml", "md", "toml", "xml"]
FMT_TITLE = {"toon": "TOON (michi)", "toon_ref": "TOON (reference encoder, not michi)", "json_min": "JSON minified",
             "json_min_omit": "JSON minified, nulls omitted", "json_pretty": "JSON pretty", "yaml": "YAML", "toml": "TOML",
             "xml": "XML", "md": "Markdown"}
MD_FORM = {"uniform": "table", "sparse": "table", "events": "table", "longtext": "table", "single": "key-value",
           "nested": "outline", "tree": "outline"}


def wilson(k, n, z=1.96):
    if n == 0:
        return 0.0, 0.0
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def cell(k, n):
    lo, hi = wilson(k, n)
    return "%d/%d (%.0f%%, %.0f to %.0f)" % (k, n, 100.0 * k / n, 100 * lo, 100 * hi) if n else "n/a"


def sign_test(b, c):
    """Exact two-sided sign test on b vs c discordant pairs."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    p = 2 * sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return min(1.0, p)


def primary_toon(shape):
    return "toon_ref" if shape in ("nested", "tree") else "toon"


def clip(text, n=80):
    """One line, cut at a word boundary so a clipped word is never left behind."""
    text = " ".join(str(text).split())
    return text if len(text) <= n else text[:n].rsplit(" ", 1)[0] + " ..."


def last_line(raw):
    lines = [l for l in str(raw).strip().splitlines() if l.strip()]
    return lines[-1] if lines else ""


def lenient(r):
    """Sensitivity check only: also accept the answer when it is the last line of a multi-line reply."""
    return bool(r["correct"] or (not r["error"] and is_correct(last_line(r["raw"]), r["expected"], r["answer_type"])))


def bare(r):
    return not r["error"] and len([l for l in r["raw"].strip().splitlines() if l.strip()]) == 1


def label_of(path):
    return os.path.basename(path)[len("raw_"):-len(".jsonl")]


def load_runs():
    runs = {}
    for p in sorted(glob.glob(os.path.join(RESULTS, "raw_*.jsonl"))):
        runs[label_of(p)] = [json.loads(l) for l in open(p, encoding="utf-8")]
    return runs


def fixed_overhead(recs, prompts):
    """Intercept of Claude's reported input tokens against o200k tokens of the prompt text: the part of every call that is
    not our prompt (system prompt, wrapper). Least squares over all calls that reported usage."""
    enc = tiktoken.get_encoding("o200k_base")
    pm = {(p["id"], p["format"]): p["prompt"] for p in prompts}
    xs, ys = [], []
    for r in recs:
        m = r.get("meta")
        if not m or m.get("input_tokens") is None:
            continue
        xs.append(len(enc.encode(pm[(r["id"], r["format"])])))
        ys.append((m["input_tokens"] or 0) + (m["cache_creation"] or 0) + (m["cache_read"] or 0))
    if len(xs) < 5:
        return None, None
    mx, my = statistics.mean(xs), statistics.mean(ys)
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum((x - mx) ** 2 for x in xs)
    return my - b * mx, b


def main():
    runs = load_runs()
    prompts = [json.loads(l) for l in open(os.path.join(ROOT, "prompts", "all.jsonl"), encoding="utf-8")]
    manifest = json.load(open(os.path.join(ROOT, "renders", "manifest.json"), encoding="utf-8"))
    tok = {}
    for r in csv.DictReader(open(os.path.join(RESULTS, "token_counts.csv"))):
        tok[(r["shape"], r["format"], r["tokenizer"])] = int(r["tokens"])
    out = []

    # ---- tokens
    haiku = next((v for k, v in runs.items() if k.startswith("haiku")), [])
    a, b = fixed_overhead(haiku, prompts)
    claude_tok = collections.defaultdict(list)
    for r in haiku:
        m = r.get("meta")
        if m and m.get("input_tokens") is not None and a is not None:
            claude_tok[(r["shape"], r["format"])].append((m["input_tokens"] or 0) + (m["cache_creation"] or 0) + (m["cache_read"] or 0) - a)
    out.append("## Tokens per rendering\n")
    out.append("`cl100k` and `o200k` are tiktoken counts of the rendering alone. `Claude` is Claude Haiku 5.5's own reported input "
               "tokens for the whole prompt (rendering + the fixed instruction and question, about 100 to 130 tokens, the same for "
               "every format in a shape), median over the shape's 12 questions, minus the fixed per-call overhead%s. "
               "`vs JSON min` is the cl100k saving against minified JSON (positive = smaller).\n" %
               (" (about %d tokens, estimated as the intercept of a fit against prompt size, slope %.2f)" % (a, b) if a else ""))
    for shape in SHAPES:
        out.append("**%s**\n" % SHAPE_TITLE[shape])
        out.append("| format | cl100k | o200k | Claude | vs JSON min | note |")
        out.append("|---|--:|--:|--:|--:|---|")
        base = tok[(shape, "json_min", "cl100k_base")]
        for fmt in FMT_ORDER:
            if fmt not in manifest["cells"][shape]:
                continue
            c, o = tok[(shape, fmt, "cl100k_base")], tok[(shape, fmt, "o200k_base")]
            cl = claude_tok.get((shape, fmt))
            name = FMT_TITLE[fmt] + (" (%s)" % MD_FORM[shape] if fmt == "md" else "")
            out.append("| %s | %d | %d | %s | %+.0f%% | %s |" % (name, c, o, "%d" % statistics.median(cl) if cl else "-",
                       100.0 * (1 - c / base), manifest["cells"][shape][fmt]["note"]))
        skipped = manifest["skipped"][shape]
        out.append("")
        if skipped:
            out.append("Not run: " + "; ".join("%s (%s)" % (FMT_TITLE[f], r) for f, r in skipped.items()) + ".\n")

    # ---- accuracy
    for label, recs in runs.items():
        errs = [r for r in recs if r["error"]]
        ok = [r for r in recs if not r["error"]]
        out.append("## Accuracy: %s\n" % label)
        out.append("%d calls recorded, %d command errors (scored wrong; none are dropped). Cells show correct/n (percent, 95%% Wilson interval).\n" % (len(recs), len(errs)))
        by = collections.defaultdict(lambda: [0, 0])
        for r in recs:
            k = by[(r["shape"], r["format"])]
            k[0] += r["correct"]
            k[1] += 1
        shapes_run = [s for s in SHAPES if any(sh == s for sh, _ in by)]
        out.append("| shape | format | correct | exact_copy | other kinds |")
        out.append("|---|---|---|---|---|")
        for shape in shapes_run:
            for fmt in FMT_ORDER:
                if (shape, fmt) not in by:
                    continue
                k, n = by[(shape, fmt)]
                rs = [r for r in recs if r["shape"] == shape and r["format"] == fmt]
                ex = [r for r in rs if r["kind"] == "exact_copy"]
                rest = [r for r in rs if r["kind"] != "exact_copy"]
                out.append("| %s | %s | %s | %d/%d | %d/%d |" % (shape, FMT_TITLE[fmt] + (" (%s)" % MD_FORM[shape] if fmt == "md" else ""),
                           cell(k, n), sum(r["correct"] for r in ex), len(ex), sum(r["correct"] for r in rest), len(rest)))
        out.append("")
        # pooled by kind
        kinds = sorted({r["kind"] for r in recs})
        pool_fmts = ["toon*", "json_min", "json_pretty", "yaml", "md"]

        def fkey(r):
            return "toon*" if r["format"] == primary_toon(r["shape"]) and r["format"].startswith("toon") else r["format"]
        full = {s for s in SHAPES if len({r["id"] for r in recs if r["shape"] == s}) == 12}
        sel = [r for r in recs if r["shape"] in full and fkey(r) in pool_fmts]
        out.append("Pooled over the %d shapes this run covers in full (`toon*` is michi's TOON where michi can render the shape, the reference encoder for nested and tree).\n" % len(full))
        out.append("| kind | " + " | ".join(pool_fmts) + " |")
        out.append("|---|" + "--:|" * len(pool_fmts))
        for kd in kinds + ["ALL"]:
            row = []
            for pf in pool_fmts:
                rs = [r for r in sel if fkey(r) == pf and (kd == "ALL" or r["kind"] == kd)]
                row.append("%d/%d (%.0f%%)" % (sum(r["correct"] for r in rs), len(rs), 100.0 * sum(r["correct"] for r in rs) / len(rs)) if rs else "-")
            out.append("| %s | %s |" % (kd, " | ".join(row)))
        out.append("")
        # paired against json_min
        qmap = collections.defaultdict(dict)
        for r in sel:
            qmap[r["id"]][fkey(r)] = r["correct"]
        out.append("Paired against minified JSON on the same questions: `format only right` / `JSON min only right` (questions where both or neither were right are dropped), exact two-sided sign test.\n")
        out.append("| format | only this format right | only JSON min right | p |")
        out.append("|---|--:|--:|--:|")
        for pf in pool_fmts:
            if pf == "json_min":
                continue
            b1 = sum(1 for d in qmap.values() if pf in d and "json_min" in d and d[pf] and not d["json_min"])
            c1 = sum(1 for d in qmap.values() if pf in d and "json_min" in d and not d[pf] and d["json_min"])
            out.append("| %s | %d | %d | %.2f |" % (pf, b1, c1, sign_test(b1, c1)))
        out.append("")
        # compliance: replies that were not a single bare line
        out.append("Replies that were not a single bare line (the model worked the answer out aloud; strict scoring reads only the first line), and the score if the last line is also accepted. This is a sensitivity check, not the headline.\n")
        out.append("| format | not bare | strict | last line also accepted |")
        out.append("|---|--:|--:|--:|")
        for pf in pool_fmts:
            rs = [r for r in sel if fkey(r) == pf]
            out.append("| %s | %d/%d | %d/%d | %d/%d |" % (pf, sum(not bare(r) for r in rs), len(rs), sum(r["correct"] for r in rs), len(rs),
                       sum(lenient(r) for r in rs), len(rs)))
        out.append("")
        out.append("Per shape and format with the last line also accepted:\n")
        out.append("| shape | format | strict | last line also accepted | not bare |")
        out.append("|---|---|--:|--:|--:|")
        for shape in shapes_run:
            for fmt in FMT_ORDER:
                rs = [r for r in recs if r["shape"] == shape and r["format"] == fmt]
                if rs:
                    out.append("| %s | %s | %d/%d | %d/%d | %d |" % (shape, fmt, sum(r["correct"] for r in rs), len(rs), sum(lenient(r) for r in rs), len(rs),
                               sum(not bare(r) for r in rs)))
        out.append("")
        # cost / latency
        costs = [r["meta"]["cost_usd"] for r in ok if r.get("meta") and r["meta"].get("cost_usd") is not None]
        lat = [r["latency_s"] for r in ok]
        if costs:
            out.append("Cost as reported by the CLI: $%.2f over %d calls (mean $%.3f); wall time per call: median %.1fs, max %.1fs.\n" %
                       (sum(costs), len(costs), statistics.mean(costs), statistics.median(lat), max(lat)))
        # misses
        with open(os.path.join(RESULTS, "misses_%s.md" % label), "w", encoding="utf-8") as f:
            f.write("# Wrong answers: %s\n\n| id | format | kind | expected | got |\n|---|---|---|---|---|\n" % label)
            for r in sorted(recs, key=lambda r: (r["id"], r["format"])):
                if not r["correct"]:
                    f.write("| %s | %s | %s | `%s` | `%s` |\n" % (r["id"], r["format"], r["kind"], json.dumps(r["expected"]),
                            clip(r["error"] or r["raw"]).replace("|", "\\|")))
    # ---- all models on the shapes every model ran in full
    def full_shapes(recs):
        return {s for s in SHAPES if len({r["id"] for r in recs if r["shape"] == s}) == 12}
    shared = set(SHAPES)
    for recs in runs.values():
        shared &= full_shapes(recs)
    if len(runs) > 1 and shared:
        pf = ["toon*", "json_min", "json_pretty", "yaml", "md"]
        key = lambda r: "toon*" if r["format"] == primary_toon(r["shape"]) and r["format"].startswith("toon") else r["format"]
        out.append("## All models, shared shapes (%s)\n" % ", ".join(sorted(shared)))
        out.append("Strict score, with the last-line-accepted score in brackets. Each model-format cell is %d questions; the last column pools all models.\n" % (12 * len(shared)))
        out.append("| format | " + " | ".join(runs) + " | all models |")
        out.append("|---|" + "--:|" * (len(runs) + 1))
        for f in pf:
            cells, tk, tl, tn = [], 0, 0, 0
            for recs in runs.values():
                rs = [r for r in recs if r["shape"] in shared and key(r) == f]
                k, l = sum(r["correct"] for r in rs), sum(lenient(r) for r in rs)
                cells.append("%d/%d (%d)" % (k, len(rs), l))
                tk, tl, tn = tk + k, tl + l, tn + len(rs)
            out.append("| %s | %s | %d/%d (%d) |" % (f, " | ".join(cells), tk, tn, tl))
        out.append("")
    with open(os.path.join(RESULTS, "summary.md"), "w", encoding="utf-8") as f:
        f.write("# Shape x format evaluation: generated summary\n\nGenerated by `summarize.py`; see `docs/research/shape-format-eval.md` for the write-up.\n\n" + "\n".join(out) + "\n")
    print("wrote results/summary.md")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Count tokens for every render under renders/ with tiktoken cl100k_base and o200k_base.

Writes results/token_counts.csv (one row per dataset/size/format/tokenizer) and
results/summary.md (tables used by results.md). Run via run_all.sh (sets PYTHONPATH and
TIKTOKEN_CACHE_DIR so the encodings load offline from tiktoken_cache/).
"""
import csv
import os
import statistics

import tiktoken

ROOT = os.path.dirname(os.path.abspath(__file__))
RENDERS = os.path.join(ROOT, "renders")
RESULTS = os.path.join(ROOT, "results")
FORMATS = [("toon", "toon"), ("json_pretty", "json"), ("json_min", "json"), ("md_kv", "md"), ("md_table", "md")]
ENCODINGS = ["cl100k_base", "o200k_base"]
SIZES = [5, 25, 100]


def load():
    stems = sorted(f[:-5] for f in os.listdir(os.path.join(RENDERS, "toon")) if f.endswith(".toon"))
    encs = {e: tiktoken.get_encoding(e) for e in ENCODINGS}
    rows = []
    for stem in stems:
        dataset, size = stem.rsplit("_", 1)
        for fmt, ext in FORMATS:
            with open(os.path.join(RENDERS, fmt, "%s.%s" % (stem, ext)), encoding="utf-8") as f:
                text = f.read()
            for e, enc in encs.items():
                rows.append({"dataset": dataset, "size": int(size), "format": fmt, "tokenizer": e,
                             "tokens": len(enc.encode(text)), "chars": len(text),
                             "bytes": len(text.encode("utf-8"))})
    return rows


def pct(a, b):
    return 100.0 * (1 - a / b)


def main():
    os.makedirs(RESULTS, exist_ok=True)
    rows = load()
    with open(os.path.join(RESULTS, "token_counts.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    t = {(r["dataset"], r["size"], r["format"], r["tokenizer"]): r["tokens"] for r in rows}
    datasets = sorted({r["dataset"] for r in rows})
    fmts = [f for f, _ in FORMATS]
    out = []
    for e in ENCODINGS:
        out.append("### Token counts — %s\n" % e)
        out.append("| dataset | rows | " + " | ".join(fmts) + " | TOON vs pretty | TOON vs min | TOON vs MD-KV | TOON vs MD table |")
        out.append("|---|---:|" + "---:|" * len(fmts) + "---:|---:|---:|---:|")
        for d in datasets:
            for s in SIZES:
                vals = [t[(d, s, f, e)] for f in fmts]
                toon = vals[0]
                out.append("| %s | %d | %s | %+.1f%% | %+.1f%% | %+.1f%% | %+.1f%% |" % (
                    d, s, " | ".join(str(v) for v in vals),
                    pct(toon, vals[1]), pct(toon, vals[2]), pct(toon, vals[3]), pct(toon, vals[4])))
        out.append("\n(Percentages are TOON's savings: positive = TOON smaller, negative = TOON larger.)\n")
    out.append("### Savings summary (median, min..max across the 9 datasets)\n")
    out.append("| tokenizer | rows | vs pretty JSON | vs minified JSON | vs Markdown-KV | vs Markdown table |")
    out.append("|---|---:|---|---|---|---|")
    for e in ENCODINGS:
        for s in SIZES + ["all"]:
            sizes = SIZES if s == "all" else [s]
            cells = []
            for base in fmts[1:]:
                xs = [pct(t[(d, z, "toon", e)], t[(d, z, base, e)]) for d in datasets for z in sizes]
                cells.append("%.1f%% (%.1f..%.1f)" % (statistics.median(xs), min(xs), max(xs)))
            out.append("| %s | %s | %s |" % (e, s, " | ".join(cells)))
    out.append("")
    out.append("### List datasets only (excludes service_status KV), 100 rows\n")
    out.append("| tokenizer | vs pretty JSON | vs minified JSON | vs Markdown-KV | vs Markdown table |")
    out.append("|---|---|---|---|---|")
    for e in ENCODINGS:
        cells = []
        for base in fmts[1:]:
            xs = [pct(t[(d, 100, "toon", e)], t[(d, 100, base, e)]) for d in datasets if d != "service_status"]
            cells.append("%.1f%% (%.1f..%.1f)" % (statistics.median(xs), min(xs), max(xs)))
        out.append("| %s | %s |" % (e, " | ".join(cells)))
    with open(os.path.join(RESULTS, "summary.md"), "w") as f:
        f.write("\n".join(out) + "\n")
    print("\n".join(out))


if __name__ == "__main__":
    main()

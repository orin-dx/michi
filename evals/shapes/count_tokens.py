#!/usr/bin/env python3
"""Count tokens of every render in renders/manifest.json with tiktoken cl100k_base and o200k_base.

Writes results/token_counts.csv (shape, format, tokenizer, tokens, chars). Claude's own counts come for free with
the model calls (cmd/claude.sh records input-token usage); summarize.py joins them. Run via run_all.sh, which sets
PYTHONPATH and TIKTOKEN_CACHE_DIR.
"""
import csv
import json
import os

import tiktoken

ROOT = os.path.dirname(os.path.abspath(__file__))
EXT = {"toon": "toon", "toon_ref": "toon", "json_pretty": "json", "json_min": "json", "json_min_omit": "json",
       "yaml": "yaml", "toml": "toml", "xml": "xml", "md": "md"}
ENCODINGS = ["cl100k_base", "o200k_base"]


def main():
    manifest = json.load(open(os.path.join(ROOT, "renders", "manifest.json"), encoding="utf-8"))
    encs = {e: tiktoken.get_encoding(e) for e in ENCODINGS}
    rows = []
    for shape, cells in manifest["cells"].items():
        for fmt in cells:
            text = open(os.path.join(ROOT, "renders", fmt, "%s.%s" % (shape, EXT[fmt])), encoding="utf-8").read()
            for e, enc in encs.items():
                rows.append({"shape": shape, "format": fmt, "tokenizer": e, "tokens": len(enc.encode(text)), "chars": len(text)})
    os.makedirs(os.path.join(ROOT, "results"), exist_ok=True)
    with open(os.path.join(ROOT, "results", "token_counts.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    print("counted %d renders x %d tokenizers" % (len(rows) // len(ENCODINGS), len(ENCODINGS)))


if __name__ == "__main__":
    main()

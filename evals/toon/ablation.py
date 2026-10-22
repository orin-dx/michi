#!/usr/bin/env python3
"""Ablations isolating where TOON's savings go (rendered with the same michi-render binary).

Variants of the 100-row corpus, written under ablation/corpus/ and rendered to ablation/renders/:
  github_issues_flatlabels  labels array -> one comma-joined string ("bug, docs"), so michi does not
                            emit a JSON-in-a-string cell ("[\"bug\",\"docs\"]").
  github_issues_nolabels    labels column dropped entirely.
  sparse_contacts_dense     every optional field present (no nulls), to compare with the sparse case.
  test_failures_noquote     test names use "." instead of "::" so the cell needs no quotes.
Run after run_all.sh: python3 ablation.py (needs the env from run_all.sh for tiktoken).
"""
import copy
import json
import os
import subprocess
import sys

import tiktoken

ROOT = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ROOT)
import render_formats as rf  # noqa: E402

AB = os.path.join(ROOT, "ablation")
BIN = os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.environ.get("TMPDIR", "/tmp"), "toon-eval-target")),
                   "release", "michi-render")


def load(name):
    with open(os.path.join(ROOT, "corpus", name + "_100.json"), encoding="utf-8") as f:
        return json.load(f)


def variants():
    base = {n: load(n) for n in ("github_issues", "sparse_contacts", "test_failures")}
    out = {"github_issues_base": base["github_issues"], "sparse_contacts_base": base["sparse_contacts"],
           "test_failures_base": base["test_failures"]}
    d = copy.deepcopy(base["github_issues"])
    for r in d["rows"]:
        r["labels"] = ", ".join(r["labels"])
    out["github_issues_flatlabels"] = d
    d = copy.deepcopy(base["github_issues"])
    for r in d["rows"]:
        del r["labels"]
    out["github_issues_nolabels"] = d
    d = copy.deepcopy(base["sparse_contacts"])
    fill = {"email": "x@example.com", "phone": "+1-555-0000", "company": "Acme", "title": "SRE",
            "github": "x-1", "timezone": "UTC", "notes": "met at cache conf"}
    for r in d["rows"]:
        for k, v in fill.items():
            r.setdefault(k, v)
    out["sparse_contacts_dense"] = d
    d = copy.deepcopy(base["test_failures"])
    for r in d["rows"]:
        r["test"] = r["test"].replace("::", ".")
    out["test_failures_noquote"] = d
    return out


def main():
    os.makedirs(os.path.join(AB, "corpus"), exist_ok=True)
    for name, doc in variants().items():
        with open(os.path.join(AB, "corpus", name + "_100.json"), "w", encoding="utf-8") as f:
            json.dump(doc, f, ensure_ascii=False)
    subprocess.run([BIN, AB], check=True)
    encs = {e: tiktoken.get_encoding(e) for e in ("cl100k_base", "o200k_base")}
    lines = ["| variant (100 rows) | tokenizer | toon | json_pretty | json_min | md_kv | md_table | TOON vs min | TOON vs MD table |",
             "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    for name in sorted(variants()):
        with open(os.path.join(AB, "corpus", name + "_100.json"), encoding="utf-8") as f:
            doc = json.load(f)
        with open(os.path.join(AB, "renders", "toon", name + "_100.toon"), encoding="utf-8") as f:
            toon = f.read()
        texts = [toon] + [fn(doc) for _, (_, fn) in rf.FORMATS.items()]
        for e, enc in encs.items():
            n = [len(enc.encode(t)) for t in texts]
            lines.append("| %s | %s | %s | %+.1f%% | %+.1f%% |" % (
                name, e, " | ".join(map(str, n)), 100 * (1 - n[0] / n[2]), 100 * (1 - n[0] / n[4])))
    os.makedirs(os.path.join(ROOT, "results"), exist_ok=True)
    with open(os.path.join(ROOT, "results", "ablation.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()

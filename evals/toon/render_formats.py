#!/usr/bin/env python3
"""Render each corpus file as pretty JSON, minified JSON, Markdown-KV and a Markdown table.

The TOON render comes from michi-core (michi-render/, see run_all.sh). Output:
renders/<format>/<dataset>_<size>.<ext>, formats: json_pretty, json_min, md_kv, md_table.

Equivalence rules (what each format encodes of a row):
- JSON: the decoded value TOON describes: {"<type_name>": [rows]} for lists, {key: value} for KV.
  Sparse rows omit absent keys (JSON's natural encoding; TOON must write null there).
- Markdown-KV: one "## <type_name> <n>" heading per row then one "key: value" line per present
  field, blank line between rows. Absent keys omitted, explicit null written as "null".
- Markdown table: header = union of keys in first-seen order; absent and null cells are empty.
- In both Markdown formats arrays are written as comma-joined items ("bug, docs"; "" if empty),
  strings verbatim except LF -> literal "\\n" (and "|" -> "\\|" in tables) so rows stay one line.
"""
import json
import os

ROOT = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(ROOT, "corpus")
RENDERS = os.path.join(ROOT, "renders")


def md_scalar(v, null="null"):
    if v is None:
        return null
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, list):
        return ", ".join(md_scalar(x) for x in v)
    if isinstance(v, (int, float)):
        return repr(v)
    return str(v).replace("\r", "\\r").replace("\n", "\\n")


def union_keys(rows):
    keys = []
    for r in rows:
        for k in r:
            if k not in keys:
                keys.append(k)
    return keys


def json_value(doc):
    if doc["kind"] == "kv":
        return {r["key"]: r["value"] for r in doc["rows"]}
    return {doc["type_name"]: doc["rows"]}


def md_kv(doc):
    if doc["kind"] == "kv":
        return "\n".join("%s: %s" % (r["key"], md_scalar(r["value"])) for r in doc["rows"])
    blocks = []
    for i, r in enumerate(doc["rows"], 1):
        lines = ["## %s %d" % (doc["type_name"], i)]
        lines += ["%s: %s" % (k, md_scalar(v)) for k, v in r.items()]
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks)


def md_table(doc):
    if doc["kind"] == "kv":
        rows = [{"key": r["key"], "value": r["value"]} for r in doc["rows"]]
    else:
        rows = doc["rows"]
    keys = union_keys(rows)
    cell = lambda v: md_scalar(v, null="").replace("|", "\\|")
    out = ["| " + " | ".join(keys) + " |", "|" + "|".join("---" for _ in keys) + "|"]
    for r in rows:
        out.append("| " + " | ".join(cell(r.get(k)) for k in keys) + " |")
    return "\n".join(out)


FORMATS = {
    "json_pretty": ("json", lambda d: json.dumps(json_value(d), indent=2, ensure_ascii=False)),
    "json_min": ("json", lambda d: json.dumps(json_value(d), separators=(",", ":"), ensure_ascii=False)),
    "md_kv": ("md", md_kv),
    "md_table": ("md", md_table),
}


def main():
    for fmt in FORMATS:
        os.makedirs(os.path.join(RENDERS, fmt), exist_ok=True)
    names = sorted(f for f in os.listdir(CORPUS) if f.endswith(".json"))
    for name in names:
        with open(os.path.join(CORPUS, name), encoding="utf-8") as f:
            doc = json.load(f)
        stem = name[:-5]
        for fmt, (ext, fn) in FORMATS.items():
            with open(os.path.join(RENDERS, fmt, "%s.%s" % (stem, ext)), "w", encoding="utf-8") as f:
                f.write(fn(doc))
    print("rendered %d corpus files x %d formats" % (len(names), len(FORMATS)))


if __name__ == "__main__":
    main()

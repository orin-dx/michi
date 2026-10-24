#!/usr/bin/env python3
"""Render each corpus shape in every applicable text format and write renders/manifest.json.

TOON renders already exist when this runs: renders/toon_michi/ (michi-render, shapes michi can emit) and
renders/toon_ref/ (toon_ref.mjs, the official reference encoder, every shape). This script adds JSON
(pretty, minified, and minified-without-nulls for the sparse shape), YAML, TOML, XML and Markdown, checks each
render against the source where a parser is available, and records which (shape, format) cells are skipped and why.

Format ids: toon (michi), toon_ref (reference encoder, not michi), json_pretty, json_min, json_min_omit, yaml, toml,
xml, md (table / key-value / outline depending on shape).

Per-shape Markdown form: uniform, sparse, events, longtext -> table; single -> Markdown-KV (`key: value` lines);
nested, tree -> headings + bullet outline.
"""
import json
import os
import sys
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape, quoteattr

import tomli_w
import yaml

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib

ROOT = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(ROOT, "corpus")
RENDERS = os.path.join(ROOT, "renders")
EXT = {"toon": "toon", "toon_ref": "toon", "json_pretty": "json", "json_min": "json", "json_min_omit": "json",
       "yaml": "yaml", "toml": "toml", "xml": "xml", "md": "md"}
SHAPES = ["uniform", "single", "nested", "tree", "events", "sparse", "longtext"]
MD_FORM = {"uniform": "table", "sparse": "table", "events": "table", "longtext": "table", "single": "key-value",
           "nested": "outline", "tree": "outline"}
LIST_SHAPES = {"uniform", "events", "sparse", "longtext"}

# Fixed cells: what each shape is run in. Everything not listed is skipped, with the reason below.
APPLICABLE = {
    "uniform": ["toon", "json_min", "json_pretty", "yaml", "md"],
    "single": ["toon", "json_min", "json_pretty", "yaml", "md", "toml"],
    "nested": ["toon_ref", "json_min", "json_pretty", "yaml", "md", "toml", "xml"],
    "tree": ["toon_ref", "json_min", "json_pretty", "yaml", "md", "xml"],
    "events": ["toon", "toon_ref", "json_min", "json_pretty", "yaml", "md"],
    "sparse": ["toon", "json_min", "json_min_omit", "json_pretty", "yaml", "md"],
    "longtext": ["toon", "json_min", "json_pretty", "yaml", "md"],
}
SKIP = {
    ("uniform", "toon_ref"): "same bytes as michi's output (checked), so a second arm would add nothing",
    ("sparse", "toon_ref"): "same bytes as michi's output (checked)",
    ("longtext", "toon_ref"): "same bytes as michi's output (checked)",
    ("single", "toon_ref"): "same bytes as michi's output (checked)",
    ("nested", "toon"): "michi emits flat tables and key-value blocks only; a nested record has no michi rendering",
    ("tree", "toon"): "michi emits flat tables and key-value blocks only; a hierarchy has no michi rendering",
    ("uniform", "toml"): "TOML is only run on the single-record and nested shapes (it has no null and no top-level array)",
    ("events", "toml"): "TOML is only run on the single-record and nested shapes",
    ("sparse", "toml"): "TOML has no null, so a sparse table cannot be written losslessly",
    ("longtext", "toml"): "TOML is only run on the single-record and nested shapes",
    ("tree", "toml"): "TOML is only run on the single-record and nested shapes",
    ("uniform", "xml"): "XML is only run on the nested shapes",
    ("single", "xml"): "XML is only run on the nested shapes",
    ("events", "xml"): "XML is only run on the nested shapes",
    ("sparse", "xml"): "XML is only run on the nested shapes",
    ("longtext", "xml"): "XML is only run on the nested shapes",
    ("nested", "json_min_omit"): "only the sparse shape has null cells worth omitting",
    ("single", "json_min_omit"): "only the sparse shape has null cells worth omitting",
    ("uniform", "json_min_omit"): "no nulls in this shape",
    ("tree", "json_min_omit"): "no nulls in this shape",
    ("events", "json_min_omit"): "only the sparse shape is run with nulls omitted",
    ("longtext", "json_min_omit"): "only the sparse shape is run with nulls omitted",
}


def wrapped(doc):
    return {doc["wrapper"]: doc["data"]} if doc["wrapper"] else doc["data"]


def drop_none(v):
    if isinstance(v, dict):
        return {k: drop_none(x) for k, x in v.items() if x is not None}
    if isinstance(v, list):
        return [drop_none(x) for x in v]
    return v


# ------------------------------------------------------------------ Markdown
def md_scalar(v, null="null"):
    if v is None:
        return null
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, list):
        return ", ".join(md_scalar(x) for x in v)
    if isinstance(v, (int, float)):
        return repr(v)
    return str(v)


def union_keys(rows):
    keys = []
    for r in rows:
        for k in r:
            if k not in keys:
                keys.append(k)
    return keys


def md_table(rows):
    keys = union_keys(rows)
    out = ["| " + " | ".join(keys) + " |", "|" + "|".join("---" for _ in keys) + "|"]
    for r in rows:
        out.append("| " + " | ".join(md_scalar(r.get(k), null="") for k in keys) + " |")
    return "\n".join(out)


def md_single(rec):
    return "\n".join("%s: %s" % (k, md_scalar(v)) for k, v in rec.items())


def md_nested(issue):
    out = ["# Issue %s: %s" % (issue["number"], issue["title"]), ""]
    for k, v in issue.items():
        if k in ("number", "title", "comments"):
            continue
        out.append("- %s: %s" % (k, md_scalar(v)))
    out += ["", "## Comments (%d)" % len(issue["comments"])]
    for i, c in enumerate(issue["comments"], 1):
        out += ["", "### Comment %d" % i]
        out += ["- %s: %s" % (k, md_scalar(v)) for k, v in c.items()]
    return "\n".join(out)


def md_tree(node, depth=0):
    pad = "  " * depth
    if node["type"] == "file":
        return ["%s- %s (size %d, sha %s)" % (pad, node["name"], node["size"], node["sha"])]
    lines = ["%s- %s/" % (pad, node["name"])]
    for ch in node["children"]:
        lines += md_tree(ch, depth + 1)
    return lines


# ------------------------------------------------------------------ XML
SINGULAR = {"labels": "label", "assignees": "assignee", "comments": "comment"}


def xml_value(tag, v, ind):
    pad = "  " * ind
    if v is None:
        return ["%s<%s nil=\"true\"/>" % (pad, tag)]
    if isinstance(v, list):
        inner = []
        for x in v:
            inner += xml_value(SINGULAR[tag], x, ind + 1)
        return ["%s<%s>" % (pad, tag)] + inner + ["%s</%s>" % (pad, tag)]
    if isinstance(v, dict):
        inner = []
        for k, x in v.items():
            inner += xml_value(k, x, ind + 1)
        return ["%s<%s>" % (pad, tag)] + inner + ["%s</%s>" % (pad, tag)]
    text = "true" if v is True else "false" if v is False else str(v)
    return ["%s<%s>%s</%s>" % (pad, tag, escape(text), tag)]


def xml_tree(node, ind=0):
    pad = "  " * ind
    if node["type"] == "file":
        return ["%s<file name=%s size=\"%d\" sha=%s/>" % (pad, quoteattr(node["name"]), node["size"], quoteattr(node["sha"]))]
    out = ["%s<dir name=%s>" % (pad, quoteattr(node["name"]))]
    for ch in node["children"]:
        out += xml_tree(ch, ind + 1)
    return out + ["%s</dir>" % pad]


# ------------------------------------------------------------------ per-format renderers
def render(shape, fmt, doc):
    v, data = wrapped(doc), doc["data"]
    if fmt == "json_pretty":
        return json.dumps(v, indent=2, ensure_ascii=False)
    if fmt == "json_min":
        return json.dumps(v, separators=(",", ":"), ensure_ascii=False)
    if fmt == "json_min_omit":
        return json.dumps(drop_none(v), separators=(",", ":"), ensure_ascii=False)
    if fmt == "yaml":
        return yaml.safe_dump(v, sort_keys=False, allow_unicode=True, width=10**6).rstrip("\n")
    if fmt == "toml":
        return tomli_w.dumps(drop_none(v)).rstrip("\n")
    if fmt == "xml":
        return "\n".join(xml_value("issue", data, 0) if shape == "nested" else xml_tree(data))
    if fmt == "md":
        form = MD_FORM[shape]
        if form == "table":
            return md_table(data)
        if form == "key-value":
            return md_single(data)
        return md_nested(data) if shape == "nested" else "\n".join(md_tree(data))
    raise KeyError(fmt)


def check(shape, fmt, doc, text):
    """Round-trip where a parser exists. Returns a short note (empty when the render is exact)."""
    v = wrapped(doc)
    if fmt in ("json_pretty", "json_min"):
        assert json.loads(text) == v
    elif fmt == "json_min_omit":
        assert json.loads(text) == drop_none(v)
        return "null cells omitted; a missing key reads as null"
    elif fmt == "yaml":
        assert yaml.safe_load(text) == v
    elif fmt == "toml":
        assert tomllib.loads(text) == drop_none(v)
        return "TOML has no null: null fields are omitted"
    elif fmt == "xml":
        ET.fromstring(text)
        return "nulls written as nil=\"true\"" if shape == "nested" else ""
    elif fmt == "md":
        form = MD_FORM[shape]
        flat = json.dumps(doc["data"], ensure_ascii=False)
        assert "|" not in flat and "\\n" not in flat, "a cell would need escaping"
        if shape in ("sparse",):
            return "null cells are empty; the source has no empty strings, so nothing is lost"
        if shape == "events":
            return "absent fields and null trace_id are both empty cells"
    return ""


def main():
    manifest = {"cells": {}, "skipped": {}}
    for shape in SHAPES:
        with open(os.path.join(CORPUS, shape + ".json"), encoding="utf-8") as f:
            doc = json.load(f)
        cells = {}
        for fmt in APPLICABLE[shape]:
            d = os.path.join(RENDERS, fmt)
            os.makedirs(d, exist_ok=True)
            if fmt in ("toon", "toon_ref"):
                src = os.path.join(RENDERS, "toon_michi" if fmt == "toon" else "toon_ref", shape + ".toon")
                text = open(src, encoding="utf-8").read()
                note = "reference encoder, not michi" if fmt == "toon_ref" else \
                    "michi union table: fields absent for an event type are null" if shape == "events" else ""
            else:
                text = render(shape, fmt, doc)
                note = check(shape, fmt, doc, text)
            with open(os.path.join(d, "%s.%s" % (shape, EXT[fmt])), "w", encoding="utf-8") as f:
                f.write(text)
            cells[fmt] = {"note": note, "chars": len(text)}
        manifest["cells"][shape] = cells
        skipped = {}
        for fmt in ("toon", "toon_ref", "json_min_omit", "toml", "xml"):
            if fmt not in APPLICABLE[shape]:
                skipped[fmt] = SKIP[(shape, fmt)]
        manifest["skipped"][shape] = skipped
    # The "same bytes" skips are claims; check them.
    for shape, fmt in [k for k, r in SKIP.items() if "same bytes" in r]:
        a = os.path.join(RENDERS, "toon_michi", shape + ".toon")
        b = os.path.join(RENDERS, "toon_ref", shape + ".toon")
        if open(a, encoding="utf-8").read() != open(b, encoding="utf-8").read():
            sys.exit("toon_ref differs from michi on %s: remove that skip and run both arms" % shape)
    with open(os.path.join(RENDERS, "manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=1)
    print("rendered %d cells" % sum(len(c) for c in manifest["cells"].values()))


if __name__ == "__main__":
    main()

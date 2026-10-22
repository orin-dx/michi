#!/usr/bin/env python3
"""Build the retrieval-accuracy pilot from the 100-row corpus and the rendered contexts.

Outputs (all deterministic):
  pilot/questions.jsonl          one line per question: id, dataset, kind, question, answer, answer_type
  pilot/prompts/<format>.jsonl   one line per (question, format): id, dataset, format, kind, prompt,
                                 expected, answer_type, context_file
Ground truth is computed from corpus/<dataset>_100.json, never from a render.

answer_type: "string" (normalized exact match), "int"/"number" (numeric equality; ints compared
as exact digit strings so IDs above 2^53 cannot round), "list" (unordered set of strings),
"null" (the answer is a missing/null value).
"""
import json
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SIZE = 100
SEED = 7
FORMATS = {
    "toon": ("toon", "TOON (Token-Oriented Object Notation: a `name[N]{fields}:` header, then one comma-separated row per item)"),
    "json_pretty": ("json", "JSON"),
    "json_min": ("json", "JSON"),
    "md_kv": ("md", "Markdown key-value (one `key: value` block per item)"),
    "md_table": ("md", "Markdown table"),
}
PROMPT = """You are given the output of a tool call, in {fmt_name} format.

<data>
{context}
</data>

Question: {question}

Reply with only the answer, no explanation. If the answer is a list, give the values separated by commas. \
If the value is missing or null, reply null. Give numbers exactly as they appear in the data."""


def load(dataset):
    with open(os.path.join(ROOT, "corpus", "%s_%d.json" % (dataset, SIZE)), encoding="utf-8") as f:
        return json.load(f)


SINGULAR = {"matches": "match", "issues": "issue", "packages": "package", "failures": "failure",
            "findings": "finding", "files": "file", "contacts": "contact", "transactions": "transaction"}


def show(v):
    """A value as it is written in the question text (JSON spelling for booleans)."""
    return ("true" if v else "false") if isinstance(v, bool) else str(v)


def unique_rows(rows, field):
    counts = {}
    for r in rows:
        counts[r.get(field)] = counts.get(r.get(field), 0) + 1
    return [r for r in rows if r.get(field) is not None and counts[r.get(field)] == 1]


class Q:
    def __init__(self, dataset):
        self.dataset, self.items = dataset, []

    def add(self, kind, question, answer, answer_type):
        if answer_type == "list":
            answer = sorted(str(a) for a in answer)
        self.items.append({"dataset": self.dataset, "kind": kind, "question": question,
                           "answer": answer, "answer_type": answer_type})


def generic(dataset, noun, key, lookups, cat, num, list_cond, rng, rev_field=None):
    """Ten questions: 3 lookups, reverse lookup, 2 counts, max, min, filtered list, positional."""
    rows = load(dataset)["rows"]
    q = Q(dataset)
    one = SINGULAR[noun]
    uniq = unique_rows(rows, key)
    for field in lookups:
        r = rng.choice([u for u in uniq if u.get(field) is not None])
        v = r.get(field)
        q.add("lookup", "What is the %s of the %s whose %s is %s?" % (field, one, key, r[key]),
              v, "null" if v is None else ("int" if isinstance(v, int) and not isinstance(v, bool) else "string"))
    # reverse lookup on a field whose value is unique
    rev_field = rev_field or lookups[0]
    cand = [r for r in unique_rows(rows, rev_field) if r[key] in {u[key] for u in uniq}]
    r = rng.choice(cand)
    q.add("reverse", "Which %s has %s equal to %s? Give its %s." % (one, rev_field, r[rev_field], key), r[key],
          "int" if isinstance(r[key], int) else "string")
    # counts
    vals = sorted({show(r.get(cat)) for r in rows if r.get(cat) is not None})
    v = rng.choice(vals)
    q.add("count", "How many %s have %s equal to %s?" % (noun, cat, v), sum(1 for r in rows if show(r.get(cat)) == v), "int")
    nums = sorted(r[num] for r in rows if r.get(num) is not None)
    threshold = nums[int(len(nums) * rng.uniform(0.55, 0.9))]
    q.add("count_filter", "How many %s have %s greater than %s?" % (noun, num, threshold),
          sum(1 for r in rows if r.get(num) is not None and r[num] > threshold), "int")
    # max / min (ask max as "which", min as value)
    mx = max(nums)
    winners = [r[key] for r in rows if r.get(num) == mx]
    if len(winners) == 1:
        q.add("argmax", "Which %s has the highest %s? Give its %s." % (one, num, key), winners[0],
              "int" if isinstance(winners[0], int) else "string")
    else:
        q.add("max", "What is the highest %s?" % num, mx, "int" if isinstance(mx, int) else "number")
    q.add("min", "What is the lowest %s among all %s?" % (num, noun), min(nums), "int" if isinstance(min(nums), int) else "number")
    # filtered list
    cfield, cval, nfield = list_cond
    hits = [r for r in rows if show(r.get(cfield)) == show(cval) and r.get(nfield) is not None]
    hits.sort(key=lambda r: r[nfield], reverse=True)
    top = hits[:4]
    if len(hits) > 4:
        cut = hits[3][nfield]
        top = [r for r in hits if r[nfield] >= cut]
        q.add("filter_list", "List the %s of every %s with %s equal to %s and %s of at least %s." % (
            key, one, cfield, show(cval), nfield, cut), [r[key] for r in top], "list")
    else:
        q.add("filter_list", "List the %s of every %s with %s equal to %s." % (key, one, cfield, show(cval)),
              [r[key] for r in hits], "list")
    # positional: the Nth item
    n = rng.randint(40, 90)
    q.add("positional", "What is the %s of item number %d in the list (counting from 1)?" % (key, n), rows[n - 1][key],
          "int" if isinstance(rows[n - 1][key], int) else "string")
    return q


def build():
    rng = random.Random(SEED)
    qs = []
    qs.append(generic("code_search", "matches", "path", ["line", "language", "column"], "language", "score",
                      ("language", "rust", "score"), rng))
    q = generic("github_issues", "issues", "number", ["title", "author", "comments"], "state", "comments",
                ("state", "closed", "comments"), rng)
    rows = load("github_issues")["rows"]
    q.add("nested", "How many issues have the label \"bug\"?", sum(1 for r in rows if "bug" in r["labels"]), "int")
    qs.append(q)
    qs.append(generic("package_search", "packages", "name", ["version", "license", "downloads"], "license",
                      "downloads", ("license", "MIT", "downloads"), rng))
    q = generic("test_failures", "failures", "test", ["file", "duration_ms", "message"], "retried", "duration_ms",
                ("retried", True, "duration_ms"), rng)
    qs.append(q)
    q = generic("dependency_audit", "findings", "id", ["package", "fixed_in", "severity"], "severity", "cvss",
                ("severity", "critical", "cvss"), rng)
    rows = load("dependency_audit")["rows"]
    q.add("null_count", "How many findings have no fixed version (fixed_in is null)?",
          sum(1 for r in rows if r["fixed_in"] is None), "int")
    qs.append(q)
    qs.append(generic("file_listing", "files", "path", ["size", "modified", "mode"], "type", "size",
                      ("type", "file", "size"), rng))
    qs.append(generic("sparse_contacts", "contacts", "id", ["email", "company", "timezone"], "company", "id",
                      ("company", "Acme", "id"), rng))
    rows = load("sparse_contacts")["rows"]
    qs[-1].add("null_count", "How many contacts have no phone number?", sum(1 for r in rows if "phone" not in r), "int")
    no_tz = [r for r in rows if "timezone" not in r and "email" in r]
    r = rng.choice(no_tz)
    qs[-1].add("null_lookup", "What is the timezone of the contact whose email is %s?" % r["email"], None, "null")
    q = generic("ledger_bigids", "transactions", "txn_id", ["counterparty", "memo", "account_id"], "counterparty",
                "amount_cents", ("counterparty", "Globex, LLC", "amount_cents"), rng, rev_field="account_id")
    qs.append(q)
    # KV service status
    rows = load("service_status")["rows"]
    q = Q("service_status")
    kv = {r["key"]: r["value"] for r in rows}
    keys = list(kv)
    for k in rng.sample([k for k in keys if kv[k] is not None], 4):
        v = kv[k]
        t = "string" if isinstance(v, (str, bool)) else ("int" if isinstance(v, int) else "number")
        q.add("lookup", "What is the value of %s?" % k, str(v).lower() if isinstance(v, bool) else v, t)
    nullk = [k for k in keys if kv[k] is None]
    if nullk:
        q.add("lookup", "What is the value of %s?" % rng.choice(nullk), None, "null")
    q.add("count", "How many keys ending in .state have the value degraded?",
          sum(1 for k in keys if k.endswith(".state") and kv[k] == "degraded"), "int")
    q.add("filter_list", "List every key ending in .healthy whose value is false.",
          [k for k in keys if k.endswith(".healthy") and kv[k] is False], "list")
    lat = {k: v for k, v in kv.items() if k.endswith(".latency_ms")}
    q.add("argmax", "Which key ending in .latency_ms has the highest value?", max(lat, key=lat.get), "string")
    q.add("count", "How many keys start with db.?", sum(1 for k in keys if k.startswith("db.")), "int")
    q.add("positional", "What is the 37th key in the data (counting from 1)?", keys[36], "string")
    qs.append(q)

    out, seen = [], set()
    for q in qs:
        for i, item in enumerate(q.items, 1):
            item = dict(id="%s-%02d" % (item["dataset"], i), **item)
            assert item["question"] not in seen, item["question"]
            seen.add(item["question"])
            out.append(item)
    return out


def main():
    questions = build()
    with open(os.path.join(HERE, "questions.jsonl"), "w", encoding="utf-8") as f:
        for q in questions:
            f.write(json.dumps(q, ensure_ascii=False) + "\n")
    os.makedirs(os.path.join(HERE, "prompts"), exist_ok=True)
    for fmt, (ext, fmt_name) in FORMATS.items():
        with open(os.path.join(HERE, "prompts", "%s.jsonl" % fmt), "w", encoding="utf-8") as f:
            for q in questions:
                rel = os.path.join("renders", fmt, "%s_%d.%s" % (q["dataset"], SIZE, ext))
                with open(os.path.join(ROOT, rel), encoding="utf-8") as c:
                    context = c.read()
                prompt = PROMPT.format(fmt_name=fmt_name, context=context, question=q["question"])
                f.write(json.dumps({"id": q["id"], "dataset": q["dataset"], "format": fmt, "kind": q["kind"],
                                    "prompt": prompt, "expected": q["answer"], "answer_type": q["answer_type"],
                                    "context_file": rel}, ensure_ascii=False) + "\n")
    by = {}
    for q in questions:
        by[q["dataset"]] = by.get(q["dataset"], 0) + 1
    print("%d questions (%s) x %d formats" % (len(questions), ", ".join("%s=%d" % kv for kv in sorted(by.items())),
                                              len(FORMATS)))


if __name__ == "__main__":
    main()

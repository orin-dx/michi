"""Strict answer scoring, shared by the runner and the summary.

Same rules as evals/toon/pilot/run_pilot.py (first non-empty line only, numbers by value, lists as sets,
case-insensitive strings) plus an "exact" type for identifiers and paths: case-sensitive string equality after
stripping whitespace, a code fence, and one pair of surrounding quotes or backticks. Nothing else is forgiven,
so a single wrong or dropped character in an id fails.
"""
import re

NUM = re.compile(r"^[-+]?\d+(\.\d+)?([eE][-+]?\d+)?$")


def first_line(s):
    s = re.sub(r"^```\w*\s*|\s*```$", "", str(s).strip()).strip()
    return next((line for line in s.splitlines() if line.strip()), "").strip()


def norm(s):
    s = first_line(s)
    s = re.sub(r"^(answer|a)\s*[:\-]\s*", "", s, flags=re.I)
    while len(s) >= 2 and s[0] == s[-1] and s[0] in "\"'`":
        s = s[1:-1].strip()
    return s.rstrip(".").strip().replace("**", "").lower()


def num_eq(got, expected):
    g = norm(got).replace(",", "")
    if not NUM.match(g):
        return False
    e = str(expected)
    if re.fullmatch(r"-?\d+", e) and re.fullmatch(r"-?\d+", g):
        return int(g) == int(e)
    try:
        return abs(float(g) - float(e)) <= 0.005 + 1e-9
    except ValueError:
        return False


def is_correct(raw, expected, answer_type):
    if answer_type == "exact":
        s = first_line(raw)
        if len(s) >= 2 and s[0] == s[-1] and s[0] in "\"'`":
            s = s[1:-1]
        return s == str(expected)
    if answer_type == "null":
        return norm(raw) in {"null", "none", "missing", "n/a", "(none)", ""}
    if answer_type in ("int", "number"):
        return num_eq(raw, expected)
    if answer_type == "list":
        text = re.sub(r"^```\w*\s*|\s*```$", "", str(raw).strip())
        parts = [p for p in re.split(r"[,\n]", text) if p.strip()]
        got = {norm(re.sub(r"^\s*[-*\d.)]+\s+", "", p)) for p in parts}
        return got == {norm(e) for e in expected}
    return norm(raw) == norm(expected)

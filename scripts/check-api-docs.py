#!/usr/bin/env python3
"""Fail when a diff changes public Rust API without touching any documentation.

Usage: python3 scripts/check-api-docs.py [BASE [HEAD]] [--pr-body TEXT]

BASE..HEAD defaults to origin/main..HEAD (diffed from their merge base, like a PR). The PR body comes from --pr-body
or the PR_BODY environment variable; a line starting `No spec change needed: <reason>` skips the check.

Public API means added or removed `pub` item lines (fn, struct, enum, trait, type, const, static, mod, use, union,
and `pub name: Type` fields) in crates/*/src/**/*.rs, outside test files and `#[cfg(test)]` modules. Lines that are
identical on both sides (code moved between files) cancel out. Enum variants, trait items and anything a macro
generates are not visible to this check.
"""

import argparse
import collections
import os
import re
import subprocess
import sys

SRC = re.compile(r"^crates/[^/]+/src/.+\.rs$")
TEST_FILE = re.compile(r"(^|/)(tests?|benches|examples)/|(^|/)tests?\.rs$|_tests?\.rs$")
DOCS = re.compile(r"^(docs/spec/|docs/adr/|crates/[^/]+/README\.md$|README\.md$|ARCHITECTURE\.md$)")
KINDS = "fn|struct|enum|trait|type|const|static|mod|use|union"
ITEM = re.compile(rf'^\s*pub\s+(?:(?:async|const|unsafe|extern\s+"[^"]*")\s+)*(?:{KINDS})\b')
FIELD = re.compile(r"^\s*pub\s+[A-Za-z_]\w*\s*:(?!:)")
ESCAPE = re.compile(r"^No spec change needed:[ \t]*\S", re.MULTILINE)
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def vcs(*args):
    """Run a version-control command and return its stdout, or None when it fails."""
    result = subprocess.run(["git", *args], capture_output=True, text=True)
    return result.stdout if result.returncode == 0 else None


def test_lines(text):
    """Line numbers (1-based) inside `#[cfg(test)]` modules, found by counting braces."""
    lines = text.splitlines()
    inside = set()
    i = 0
    while i < len(lines):
        if lines[i].strip().startswith("#[cfg(test)]"):
            j = i + 1
            while j < len(lines) and lines[j].strip().startswith("#["):
                j += 1
            if j < len(lines) and re.match(r"\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{", lines[j]):
                depth = 0
                k = j
                for k in range(j, len(lines)):
                    code = lines[k].split("//")[0]
                    depth += code.count("{") - code.count("}")
                    inside.add(k + 1)
                    if depth <= 0:
                        break
                i = k
        i += 1
    return inside


def api_signature(line):
    """Whitespace-normalised text of a public API line, or None when it is not one."""
    code = line.strip()
    if code.startswith(("//", "*", "/*")):
        return None
    if ITEM.match(code):
        return " ".join(code.rstrip("{;, ").split())
    if FIELD.match(code):
        return " ".join(code.rstrip(",").split())
    return None


def changed_api(base, head):
    """Map each signature with a nonzero net change to (net count, first file)."""
    diff = vcs("diff", "-U0", "--no-color", "--no-ext-diff", f"{base}...{head}", "--", "crates")
    if diff is None:
        sys.exit(f"check-api-docs: cannot diff {base}...{head} (CI needs actions/checkout with fetch-depth: 0)")
    net = collections.Counter()
    where = {}
    skip = True
    old_tests = new_tests = set()
    old_line = new_line = 0
    path = None
    for raw in diff.splitlines():
        if raw.startswith("+++ "):
            path = raw[6:] if raw.startswith("+++ b/") else None
            skip = path is None or not SRC.match(path) or bool(TEST_FILE.search(path))
            if not skip:
                old = vcs("show", f"{base}:{path}")
                new = vcs("show", f"{head}:{path}")
                old_tests = test_lines(old) if old else set()
                new_tests = test_lines(new) if new else set()
        elif skip:
            continue
        elif m := HUNK.match(raw):
            old_line, new_line = int(m.group(1)), int(m.group(3))
        elif raw.startswith("+"):
            if new_line not in new_tests and (sig := api_signature(raw[1:])):
                net[sig] += 1
                where.setdefault(sig, path)
            new_line += 1
        elif raw.startswith("-"):
            if old_line not in old_tests and (sig := api_signature(raw[1:])):
                net[sig] -= 1
                where.setdefault(sig, path)
            old_line += 1
    return {sig: (n, where[sig]) for sig, n in net.items() if n != 0}


def changed_docs(base, head):
    """Documentation files touched by the diff."""
    names = vcs("diff", "--name-only", f"{base}...{head}") or ""
    return [n for n in names.splitlines() if DOCS.match(n)]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("base", nargs="?", default="origin/main")
    parser.add_argument("head", nargs="?", default="HEAD")
    parser.add_argument("--pr-body", default=os.environ.get("PR_BODY", ""))
    args = parser.parse_args()

    api = changed_api(args.base, args.head)
    if not api:
        print("check-api-docs: no public API change")
        return 0
    docs = changed_docs(args.base, args.head)
    if docs:
        print(f"check-api-docs: public API changed ({len(api)} lines), docs touched: {', '.join(docs)}")
        return 0
    if ESCAPE.search(args.pr_body or ""):
        print("check-api-docs: public API changed, skipped by 'No spec change needed:' in the PR body")
        return 0

    print("check-api-docs: public API changed but no documentation was updated.\n")
    for sig, (n, path) in sorted(api.items(), key=lambda kv: (kv[1][1], kv[0])):
        print(f"  {'+' if n > 0 else '-'} {sig}  ({path})")
    print(
        "\nUpdate the spec in the same PR: docs/spec/ (API reference), docs/adr/ (a decision), the crate README.md,"
        "\nREADME.md or ARCHITECTURE.md. If the change really needs no doc change, add a line to the PR body:"
        "\n\n  No spec change needed: <reason>\n\nthen re-run this check."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Fail if a permissive crate's shipped dependency tree contains a non-permissive license.

michi-graph is MIT inside an FSL-1.1-MIT workspace: permissive crates depend on it, so nothing FSL (or otherwise
non-permissive) may enter its normal or build dependencies. Dev-dependencies don't reach consumers and are skipped.

Usage: cargo metadata --format-version 1 | scripts/check-permissive-deps.py michi-graph [more crates...]
"""

import json
import re
import sys

ALLOWED = {
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "CC0-1.0",
}

# License exceptions that keep an allowed license permissive.
ALLOWED_WITH = {"Apache-2.0 WITH LLVM-exception"}


def permissive(expr: str) -> bool:
    """Whether an SPDX license expression can be satisfied using only allowed licenses.

    Precedence follows the SPDX spec: WITH binds tighter than AND, which binds tighter than OR, and parentheses
    group. `A OR B` needs one side allowed; `A AND B` needs both. The legacy `/` separator means OR. An expression
    that does not parse is not permissive.
    """
    tokens = re.findall(r"\(|\)|[^\s()]+", expr.replace("/", " OR "))
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else None

    def take():
        nonlocal pos
        token = peek()
        pos += 1
        return token

    def or_expr():
        value = and_expr()
        while peek() == "OR":
            take()
            right = and_expr()
            value = value or right
        return value

    def and_expr():
        value = term()
        while peek() == "AND":
            take()
            right = term()
            value = value and right
        return value

    def term():
        token = take()
        if token == "(":
            value = or_expr()
            if take() != ")":
                raise ValueError("unbalanced parentheses")
            return value
        if token is None or token in ("AND", "OR", "WITH", ")"):
            raise ValueError(f"unexpected {token!r}")
        name = token
        if peek() == "WITH":
            take()
            exception = take()
            if exception is None or exception in ("AND", "OR", "WITH", "(", ")"):
                raise ValueError("WITH needs an exception")
            name = f"{token} WITH {exception}"
        return name in ALLOWED or name in ALLOWED_WITH

    try:
        value = or_expr()
        return value and pos == len(tokens)
    except ValueError:
        return False


def main() -> int:
    roots = sys.argv[1:]
    if not roots:
        print(__doc__, file=sys.stderr)
        return 2
    meta = json.load(sys.stdin)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    failed = False
    for root in roots:
        root_ids = [pid for pid, p in packages.items() if p["name"] == root and p["source"] is None]
        if not root_ids:
            print(f"error: no workspace crate named {root}", file=sys.stderr)
            return 2
        seen, stack = set(), list(root_ids)
        while stack:
            pid = stack.pop()
            if pid in seen:
                continue
            seen.add(pid)
            for dep in nodes[pid]["deps"]:
                if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                    stack.append(dep["pkg"])
        for pid in sorted(seen):
            p = packages[pid]
            license_expr = p.get("license") or ""
            if not permissive(license_expr):
                print(f"error: {root} depends on {p['name']} {p['version']} licensed {license_expr or 'unknown'!r}")
                failed = True
        if not failed:
            print(f"{root}: {len(seen) - 1} shipped dependencies, all permissive")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

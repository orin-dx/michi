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
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "CC0-1.0",
}


def permissive(expr: str) -> bool:
    """An SPDX expression is permissive if some OR-alternative has only allowed AND-terms."""
    expr = expr.replace("/", " OR ").replace("(", " ").replace(")", " ")
    for alternative in re.split(r"\s+OR\s+", expr.strip()):
        terms = [t.strip() for t in re.split(r"\s+AND\s+", alternative) if t.strip()]
        if terms and all(t in ALLOWED for t in terms):
            return True
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

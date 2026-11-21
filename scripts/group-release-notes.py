#!/usr/bin/env python3
"""Print the GitHub release notes for one michi version: every package's CHANGELOG section for it.

Usage: group-release-notes.py <version> [repo root]

Callisto attaches the group's one GitHub release (michi@{version}) to michi-core, so the page it writes holds only
michi-core's notes. Rust pipeline users and npm users would find nothing there. This joins each package's section
under its own heading, in the order a reader is most likely to look for it. A package whose section only says it was
released with the group is left out, and so is one with no section for the version.
"""

import pathlib
import re
import sys

PACKAGES = [
    ("michi-core (Rust)", "crates/michi-core/CHANGELOG.md"),
    ("michi-pipeline (Rust)", "crates/michi-pipeline/CHANGELOG.md"),
    ("michi-graph (Rust)", "crates/michi-graph/CHANGELOG.md"),
    ("@orin-dx/michi (npm)", "packages/michi-node/CHANGELOG.md"),
]

# The line Callisto writes for a fixed-group member with no changes of its own.
GROUP_ONLY = "- Released together with the `michi` fixed group."


def section(changelog: str, version: str) -> str:
    """The body under `## {version}`, up to the next `## ` heading, without the date/compare line."""
    lines = changelog.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line.strip() == f"## {version}":
            start = i + 1
            break
    if start is None:
        return ""
    body = []
    for line in lines[start:]:
        if line.startswith("## "):
            break
        body.append(line)
    text = "\n".join(body).strip()
    # Callisto's `_date · [compare](...)_` line repeats per package; the release page has its own date.
    text = re.sub(r"\A_[^\n]*compare[^\n]*_\s*", "", text).strip()
    return text


def notes(version: str, root: pathlib.Path) -> str:
    parts = []
    for title, path in PACKAGES:
        file = root / path
        if not file.is_file():
            continue
        body = section(file.read_text(encoding="utf-8"), version)
        if not body or body == GROUP_ONLY:
            continue
        # Demote `### Minor Changes` and the like so they sit under the package heading.
        body = re.sub(r"^### ", "#### ", body, flags=re.M)
        parts.append(f"## {title}\n\n{body}")
    return "\n\n".join(parts) + ("\n" if parts else "")


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    root = pathlib.Path(sys.argv[2]) if len(sys.argv) == 3 else pathlib.Path(".")
    sys.stdout.write(notes(sys.argv[1], root))
    return 0


if __name__ == "__main__":
    sys.exit(main())

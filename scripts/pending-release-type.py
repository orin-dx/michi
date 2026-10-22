#!/usr/bin/env python3
"""Print the cargo-semver-checks release type for the michi fixed group's pending changesets.

Only the YAML front matter of `.changeset/*.md` counts, and only entries naming a fixed-group package: michi-graph
versions on its own, so its bump must not loosen the check for michi-core and michi-pipeline. cargo-semver-checks
names release types literally, so before 1.0 Callisto's minor bump (0.4 -> 0.5) is `major` here.

Prints `infer` when no changeset names a fixed-group package: then the version in the manifests is the guide (on
Callisto's version PR the changesets are already applied and the versions bumped).

Usage: scripts/pending-release-type.py <current michi-core version> [changeset dir]
"""

import pathlib
import re
import sys

GROUP = {"michi-core", "michi-pipeline", "michi-node", "@orin-dx/michi"}
RANK = {"none": 0, "patch": 1, "minor": 2, "major": 3}


def front_matter_bumps(text: str):
    """(package, level) pairs from a changeset's front matter, or nothing if it has none."""
    match = re.match(r"\A---[ \t]*\r?\n(.*?)\r?\n---[ \t]*(\r?\n|\Z)", text, re.S)
    if not match:
        return []
    bumps = []
    for line in match.group(1).splitlines():
        entry = re.match(r"""^\s*["']?([^"':]+?)["']?\s*:\s*["']?(major|minor|patch|none)["']?\s*$""", line)
        if entry:
            bumps.append((entry.group(1), entry.group(2)))
    return bumps


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    version = sys.argv[1]
    directory = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else ".changeset")
    level = None
    for path in sorted(directory.glob("*.md")):
        for package, bump in front_matter_bumps(path.read_text()):
            if package in GROUP and (level is None or RANK[bump] > RANK[level]):
                level = bump
    if level is None:
        print("infer")
    elif level == "major" or (level == "minor" and version.startswith("0.")):
        print("major")
    elif level == "minor":
        print("minor")
    else:
        print("patch")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Self-test for pending-release-type.py. Run: python3 scripts/test_pending_release_type.py"""

import pathlib
import subprocess
import sys
import tempfile

SCRIPT = pathlib.Path(__file__).with_name("pending-release-type.py")

CASES = [
    # (changesets, version, expected)
    # No changeset for the group (a version PR has applied them): the version bump decides.
    ({}, "0.5.0", "infer"),
    # michi-graph is in the group; body text never counts.
    ({"a.md": "---\nmichi-graph: patch\n---\n\nImpact: major\nmichi-core: major\n"}, "0.4.0", "patch"),
    ({"a.md": "---\nunrelated: major\n---\n"}, "0.4.0", "infer"),
    ({"a.md": "---\nmichi-core: none\n---\n"}, "0.4.0", "patch"),
    ({"a.md": "---\nmichi-pipeline: patch\n---\n\nx\n"}, "0.4.0", "patch"),
    # Before 1.0 a minor bump is breaking.
    ({"a.md": '---\n"michi-pipeline": minor\n---\n\nx\n'}, "0.4.0", "major"),
    ({"a.md": "---\nmichi-core: minor\n---\n\nx\n"}, "1.2.0", "minor"),
    ({"a.md": "---\nmichi-core: patch\n---\n", "b.md": "---\n'@orin-dx/michi': major\n---\n"}, "1.2.0", "major"),
    ({"a.md": "michi-core: major\n"}, "0.4.0", "infer"),
]

failures = 0
for changesets, version, expected in CASES:
    with tempfile.TemporaryDirectory() as directory:
        for name, text in changesets.items():
            pathlib.Path(directory, name).write_text(text)
        got = subprocess.run(
            [sys.executable, str(SCRIPT), version, directory], capture_output=True, text=True, check=True
        ).stdout.strip()
    if got != expected:
        failures += 1
        print(f"{changesets} at {version}: got {got}, expected {expected}")
print(f"{len(CASES) - failures}/{len(CASES)} cases pass")
sys.exit(1 if failures else 0)

#!/usr/bin/env python3
"""Self-test for group-release-notes.py. Run: python3 scripts/test_group_release_notes.py"""

import pathlib
import subprocess
import sys
import tempfile

SCRIPT = pathlib.Path(__file__).with_name("group-release-notes.py")

CORE = """# michi-core

## 0.11.0

_2026-11-21 · [compare](https://github.com/orin-dx/michi/compare/michi@0.10.0...michi@0.11.0)_

### Minor Changes

- **Breaking:** core change.

## 0.10.0

- older core change.
"""

PIPELINE = """# michi-pipeline

## 0.11.0

_2026-11-21 · [compare](x)_

### Minor Changes

- pipeline change.

### Patch Changes

- pipeline fix.
"""

GRAPH = """# michi-graph

## 0.11.0

_2026-11-21 · [compare](x)_

- Released together with the `michi` fixed group.
"""

NODE = """# @orin-dx/michi

## 0.11.0

### Minor Changes

- js change.
"""


def run(version, files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for path, text in files.items():
            (root / path).parent.mkdir(parents=True, exist_ok=True)
            (root / path).write_text(text, encoding="utf-8")
        out = subprocess.run(
            [sys.executable, str(SCRIPT), version, str(root)], capture_output=True, text=True, check=False
        )
        return out.stdout, out.returncode


ALL = {
    "crates/michi-core/CHANGELOG.md": CORE,
    "crates/michi-pipeline/CHANGELOG.md": PIPELINE,
    "crates/michi-graph/CHANGELOG.md": GRAPH,
    "packages/michi-node/CHANGELOG.md": NODE,
}

EXPECTED = """## michi-core (Rust)

#### Minor Changes

- **Breaking:** core change.

## michi-pipeline (Rust)

#### Minor Changes

- pipeline change.

#### Patch Changes

- pipeline fix.

## @orin-dx/michi (npm)

#### Minor Changes

- js change.
"""

CASES = [
    # (name, version, files, expected stdout)
    ("every package with changes, group-only and older sections left out", "0.11.0", ALL, EXPECTED),
    ("a version no package has", "9.9.9", ALL, ""),
    ("a missing changelog is skipped", "0.11.0", {"packages/michi-node/CHANGELOG.md": NODE},
     "## @orin-dx/michi (npm)\n\n#### Minor Changes\n\n- js change.\n"),
    ("a version prefix of another is not matched", "0.1", ALL, ""),
]


def main():
    failed = 0
    for name, version, files, expected in CASES:
        stdout, code = run(version, files)
        if code != 0 or stdout != expected:
            failed += 1
            print(f"FAIL {name}: exit {code}\n--- got ---\n{stdout}--- expected ---\n{expected}")
    print(f"{len(CASES) - failed}/{len(CASES)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

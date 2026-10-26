#!/usr/bin/env python3
"""Self-test for released-version.sh. Run: python3 scripts/test_released_version.py"""

import json
import pathlib
import subprocess
import sys
import tempfile

SCRIPT = pathlib.Path(__file__).with_name("released-version.sh")


def outcome(kind, role, version):
    return {"outcome": {"kind": kind}, "operation": {"role": {"kind": role}, "version": version}}


CASES = [
    # (name, outcomes or None for a missing receipt, expected stdout, expected exit code)
    ("missing receipt", None, "", 0),
    ("empty receipt", [], "", 0),
    ("published", [outcome("published", "registryPublish", "0.8.0")], "0.8.0", 0),
    ("adopted from an earlier attempt", [outcome("alreadySatisfied", "registryPublish", "0.8.0")], "0.8.0", 0),
    (
        "several packages, one version",
        [outcome("published", "registryPublish", "0.8.0"), outcome("published", "registryPublish", "0.8.0")],
        "0.8.0",
        0,
    ),
    ("a skipped publish does not count", [outcome("skipped", "registryPublish", "0.8.0")], "", 0),
    ("a non-registry operation does not count", [outcome("published", "gitTag", "0.8.0")], "", 0),
    (
        "two versions fail",
        [outcome("published", "registryPublish", "0.8.0"), outcome("published", "registryPublish", "0.7.1")],
        None,
        1,
    ),
]


def main():
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for name, outcomes, want_out, want_code in CASES:
            receipt = pathlib.Path(tmp) / "receipt.json"
            receipt.unlink(missing_ok=True)
            if outcomes is not None:
                receipt.write_text(json.dumps({"outcomes": outcomes}))
            run = subprocess.run(["bash", str(SCRIPT), str(receipt)], capture_output=True, text=True)
            ok = run.returncode == want_code and (want_out is None or run.stdout.strip() == want_out)
            if not ok:
                failures += 1
                print(f"FAIL {name}: code {run.returncode}, stdout {run.stdout!r}, stderr {run.stderr!r}")
    if failures:
        sys.exit(1)
    print(f"ok: {len(CASES)} cases")


main()

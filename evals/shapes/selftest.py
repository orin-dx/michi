#!/usr/bin/env python3
"""Check the harness end to end with fake models, no API calls.

oracle must score 100%, wrong 0%, and case-flip must fail every "exact" question and nothing it should not. Exits
non-zero on any deviation.
"""
import collections
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(ROOT, "results")
CASES = [("oracle", ""), ("wrong", " --wrong"), ("flip", " --case-flip")]


def main():
    bad = 0
    for name, flag in CASES:
        path = os.path.join(RESULTS, "selftest_selftest-%s.jsonl" % name)
        if os.path.exists(path):
            os.remove(path)
        subprocess.run([sys.executable, os.path.join(ROOT, "run_eval.py"), "--model-cmd",
                        "python3 %s%s" % (os.path.join(ROOT, "cmd", "oracle.py"), flag),
                        "--label", "selftest-" + name, "--jobs", "8"], check=True, cwd=ROOT)
        ok, total = collections.Counter(), collections.Counter()
        for line in open(path, encoding="utf-8"):
            r = json.loads(line)
            total[r["answer_type"]] += 1
            ok[r["answer_type"]] += r["correct"]
        print(name, {k: "%d/%d" % (ok[k], total[k]) for k in sorted(total)})
        for k in total:
            want = {"oracle": total[k], "wrong": 0, "flip": 0 if k == "exact" else total[k]}[name]
            if k == "null" and name == "wrong":
                want = 0
            if ok[k] != want:
                # case-flip upper-cases numbers and "null" too, which still score; strings are case-insensitive
                print("  UNEXPECTED for", k, ok[k], "!=", want)
                bad += 1
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()

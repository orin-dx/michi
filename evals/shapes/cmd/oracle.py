#!/usr/bin/env python3
"""Self-test "model": reads a prompt on stdin and prints the ground-truth answer (expect 100%).

`--wrong` prints a deliberately wrong answer (expect 0%); `--case-flip` upper-cases the answer (expect every
"exact" question to fail and everything else to pass), which checks that exact-copy scoring is case-sensitive.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
prompt = sys.stdin.read()
question = prompt.split("\nQuestion: ", 1)[1].split("\n", 1)[0]
answers = {}
for line in open(os.path.join(HERE, "questions.jsonl"), encoding="utf-8"):
    q = json.loads(line)
    answers[q["question"]] = q["answer"]
a = answers[question]
if "--wrong" in sys.argv:
    print("definitely-not-the-answer")
elif a is None:
    print("null")
elif isinstance(a, list):
    print(", ".join(a))
else:
    print(str(a).upper() if "--case-flip" in sys.argv else a)

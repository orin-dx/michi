#!/usr/bin/env python3
"""Self-test "model": reads a pilot prompt on stdin and prints the ground-truth answer.

Used to check the harness and scorer end to end (`run_pilot.py --model-cmd 'python3 pilot/oracle.py'`
must score 100%). Pass --wrong to print a deliberately wrong answer (must score 0%).
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
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
    print(a)

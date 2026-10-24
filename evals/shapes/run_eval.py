#!/usr/bin/env python3
"""Run prompts/all.jsonl against a model command and record scored answers.

The model command reads one prompt on stdin and prints the answer on stdout (see cmd/claude.sh, cmd/oracle.py).
Results append to results/raw_<label>.jsonl (self-tests use a label starting with "selftest", which writes
results/selftest_<label>.jsonl instead and is not counted against the call cap). Re-running with the same label resumes:
cells already recorded are skipped.

Quota guards:
  --max-calls N    refuse to start if the calls already recorded in results/raw_*.jsonl (all labels, errors included)
                   plus this run would pass N (default 900, the cap for this task)
  --stop-after K   stop submitting new calls after K consecutive command errors (default 3), so a rate limit or an
                   expired login ends the run instead of burning the budget on failures
Selection: --shapes, --formats, --cells shape:fmt,..., --ids id:fmt,..., --limit N (first N after a seeded shuffle).
"""
import argparse
import concurrent.futures as cf
import glob
import json
import os
import random
import re
import shlex
import subprocess
import sys
import threading
import time

from score import is_correct

ROOT = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(ROOT, "results")


def call(cmd, prompt, timeout):
    t0 = time.time()
    meta = None
    try:
        r = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout)
        out = r.stdout
        err = None
        for line in r.stderr.splitlines():
            if line.startswith("META "):
                meta = json.loads(line[5:])
        if r.returncode:
            err = "exit %d: %s" % (r.returncode, r.stderr[-400:])
        elif not out.strip():
            err = "empty answer"
    except subprocess.TimeoutExpired:
        out, err = "", "timeout"
    except OSError as e:  # the command could not even start
        out, err = "", "cannot run command: %s" % e
    return out, err, meta, round(time.time() - t0, 2)


def recorded_calls():
    n = 0
    for p in glob.glob(os.path.join(RESULTS, "raw_*.jsonl")):
        n += sum(1 for _ in open(p, encoding="utf-8"))
    return n


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model-cmd", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--shapes", default="")
    ap.add_argument("--formats", default="")
    ap.add_argument("--cells", default="", help="comma list of shape:format to run")
    ap.add_argument("--ids", default="", help="comma list of question-id:format to run (pilot)")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--repeat", type=int, default=0, help="repeat index to record (default 0)")
    ap.add_argument("--timeout", type=float, default=240)
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--max-calls", type=int, default=900)
    ap.add_argument("--stop-after", type=int, default=3)
    a = ap.parse_args()

    prompts = [json.loads(l) for l in open(os.path.join(ROOT, "prompts", "all.jsonl"), encoding="utf-8")]
    shapes = {s for s in a.shapes.split(",") if s}
    formats = {s for s in a.formats.split(",") if s}
    cells = {tuple(c.split(":")) for c in a.cells.split(",") if c}
    ids = {tuple(c.split(":")) for c in a.ids.split(",") if c}
    prompts = [p for p in prompts
               if (not shapes or p["shape"] in shapes) and (not formats or p["format"] in formats)
               and (not cells or (p["shape"], p["format"]) in cells) and (not ids or (p["id"], p["format"]) in ids)]
    selftest = a.label.startswith("selftest")
    out_path = os.path.join(RESULTS, ("selftest_%s" if selftest else "raw_%s") % re.sub(r"[^\w.-]", "_", a.label) + ".jsonl")
    done = set()
    if os.path.exists(out_path):
        for line in open(out_path, encoding="utf-8"):
            r = json.loads(line)
            done.add((r["id"], r["format"], r["repeat"]))
    work = [p for p in prompts if (p["id"], p["format"], a.repeat) not in done]
    random.Random(1).shuffle(work)
    if a.limit:
        work = work[:a.limit]
    if not selftest:
        used = recorded_calls()
        if used + len(work) > a.max_calls:
            sys.exit("refusing: %d calls recorded + %d planned > cap %d" % (used, len(work), a.max_calls))
        print("%d calls recorded so far, %d planned, cap %d" % (used, len(work), a.max_calls), file=sys.stderr)
    os.makedirs(RESULTS, exist_ok=True)
    cmd = shlex.split(a.model_cmd)
    lock = threading.Lock()
    state = {"consecutive": 0, "stop": False, "ran": 0, "errors": 0}

    def task(p):
        with lock:
            if state["stop"]:
                return None
        raw, err, meta, latency = call(cmd, p["prompt"], a.timeout)
        with lock:
            state["ran"] += 1
            state["errors"] += bool(err)
            state["consecutive"] = state["consecutive"] + 1 if err else 0
            if state["consecutive"] >= a.stop_after:
                state["stop"] = True
        return p, raw, err, meta, latency

    with open(out_path, "a", encoding="utf-8") as out, cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
        for i, res in enumerate(ex.map(task, work), 1):
            if res is None:
                continue
            p, raw, err, meta, latency = res
            rec = {"id": p["id"], "shape": p["shape"], "format": p["format"], "kind": p["kind"], "repeat": a.repeat,
                   "model_cmd": a.model_cmd, "expected": p["expected"], "answer_type": p["answer_type"], "raw": raw,
                   "correct": bool(not err and is_correct(raw, p["expected"], p["answer_type"])), "error": err,
                   "latency_s": latency, "meta": meta}
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")
            out.flush()
            if i % 20 == 0:
                print("  %d/%d (%d errors)" % (i, len(work), state["errors"]), file=sys.stderr)
    print("ran %d calls, %d errors%s -> %s" % (state["ran"], state["errors"],
          ", STOPPED after %d consecutive errors" % a.stop_after if state["stop"] else "", out_path), file=sys.stderr)
    sys.exit(2 if state["stop"] else 0)


if __name__ == "__main__":
    main()

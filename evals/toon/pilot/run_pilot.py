#!/usr/bin/env python3
"""Run the retrieval pilot against any model reachable from a shell command, then score it.

The model command receives one prompt on stdin and must print the answer on stdout. Examples:
  --model-cmd 'claude -p --model claude-haiku-4-5'
  --model-cmd 'llm -m gpt-4o-mini'
  --model-cmd 'ollama run llama3.1:8b'
  --model-cmd 'python3 pilot/oracle.py'      # self-test: answers from questions.jsonl, expect 100%

Usage:
  python3 pilot/run_pilot.py --model-cmd CMD --label NAME [--formats toon,md_kv] [--datasets ...]
                             [--limit N] [--repeats R] [--timeout S] [--jobs J]
  python3 pilot/run_pilot.py --score-only results/pilot_NAME.jsonl

Writes results/pilot_<label>.jsonl (one record per prompt and repeat: id, format, repeat, raw answer,
correct, latency) and appends nothing else. Re-running with the same label resumes: prompts already
recorded are skipped. A summary (accuracy by format, by format x dataset, by format x question kind)
is printed and written to results/pilot_<label>_summary.md.
"""
import argparse
import concurrent.futures as cf
import json
import os
import re
import shlex
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
FORMATS = ["toon", "json_pretty", "json_min", "md_kv", "md_table"]
NUM = re.compile(r"^[-+]?\d+(\.\d+)?([eE][-+]?\d+)?$")


def norm(s):
    s = str(s).strip()
    s = re.sub(r"^```\w*\s*|\s*```$", "", s).strip()
    # first non-empty line only: the prompt asks for the bare answer
    s = next((line for line in s.splitlines() if line.strip()), "").strip()
    s = re.sub(r"^(answer|a)\s*[:\-]\s*", "", s, flags=re.I)
    while len(s) >= 2 and s[0] == s[-1] and s[0] in "\"'`":
        s = s[1:-1].strip()
    s = s.rstrip(".").strip()
    return s.replace("**", "").lower()


def num_eq(got, expected):
    g = norm(got).replace(",", "") if not isinstance(expected, str) else norm(got)
    if not NUM.match(g):
        return False
    e = str(expected)
    if re.fullmatch(r"-?\d+", e) and re.fullmatch(r"-?\d+", g):
        return int(g) == int(e)  # exact digits: IDs above 2^53 must not round
    try:
        return abs(float(g) - float(e)) <= 1e-9 * max(1.0, abs(float(e)))
    except ValueError:
        return False


def is_correct(raw, expected, answer_type):
    if answer_type == "null":
        return norm(raw) in {"null", "none", "missing", "n/a", "(none)", ""}
    if answer_type in ("int", "number"):
        return num_eq(raw, expected)
    if answer_type == "list":
        text = str(raw).strip()
        text = re.sub(r"^```\w*\s*|\s*```$", "", text)
        parts = [p for p in re.split(r"[,\n]", text) if p.strip()]
        got = {norm(re.sub(r"^\s*[-*\d.)]+\s+", "", p)) for p in parts}
        return got == {norm(e) for e in expected}
    return norm(raw) == norm(expected)


def load_prompts(formats, datasets):
    items = []
    for fmt in formats:
        with open(os.path.join(HERE, "prompts", "%s.jsonl" % fmt), encoding="utf-8") as f:
            for line in f:
                p = json.loads(line)
                if not datasets or p["dataset"] in datasets:
                    items.append(p)
    return items


def call(cmd, prompt, timeout):
    t0 = time.time()
    try:
        r = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout)
        out, err = r.stdout, (r.stderr[-500:] if r.returncode else None)
    except subprocess.TimeoutExpired:
        out, err = "", "timeout"
    return out, err, round(time.time() - t0, 3)


def summarize(path):
    recs = [json.loads(l) for l in open(path, encoding="utf-8")]
    def table(keyf, title):
        agg = {}
        for r in recs:
            k = keyf(r)
            a = agg.setdefault(k, [0, 0])
            a[0] += r["correct"]
            a[1] += 1
        lines = ["### " + title, "", "| group | correct | n | accuracy |", "|---|---:|---:|---:|"]
        for k in sorted(agg):
            c, n = agg[k]
            lines.append("| %s | %d | %d | %.1f%% |" % (" / ".join(k) if isinstance(k, tuple) else k, c, n, 100.0 * c / n))
        return "\n".join(lines) + "\n"
    errors = sum(1 for r in recs if r.get("error"))
    out = "# Retrieval pilot: %s\n\n%d records, %d command errors/timeouts (scored as wrong)\n\n" % (
        os.path.basename(path), len(recs), errors)
    out += table(lambda r: r["format"], "Accuracy by format")
    out += "\n" + table(lambda r: (r["dataset"], r["format"]), "Accuracy by dataset x format")
    out += "\n" + table(lambda r: (r["kind"], r["format"]), "Accuracy by question kind x format")
    summary_path = path[:-6] + "_summary.md"
    with open(summary_path, "w", encoding="utf-8") as f:
        f.write(out)
    print(out)
    print("summary written to", summary_path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model-cmd")
    ap.add_argument("--label")
    ap.add_argument("--formats", default=",".join(FORMATS))
    ap.add_argument("--datasets", default="")
    ap.add_argument("--limit", type=int, default=0, help="max prompts per format (0 = all)")
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--timeout", type=float, default=180)
    ap.add_argument("--jobs", type=int, default=1)
    ap.add_argument("--score-only")
    a = ap.parse_args()
    if a.score_only:
        return summarize(a.score_only)
    if not (a.model_cmd and a.label):
        ap.error("--model-cmd and --label are required")
    formats = [f for f in a.formats.split(",") if f]
    datasets = {d for d in a.datasets.split(",") if d}
    prompts = load_prompts(formats, datasets)
    if a.limit:
        per = {}
        prompts = [p for p in prompts if per.setdefault(p["format"], []).append(p) or len(per[p["format"]]) <= a.limit]
    os.makedirs(os.path.join(ROOT, "results"), exist_ok=True)
    out_path = os.path.join(ROOT, "results", "pilot_%s.jsonl" % re.sub(r"[^\w.-]", "_", a.label))
    done = set()
    if os.path.exists(out_path):
        for line in open(out_path, encoding="utf-8"):
            r = json.loads(line)
            done.add((r["id"], r["format"], r["repeat"]))
    work = [(p, rep) for rep in range(a.repeats) for p in prompts if (p["id"], p["format"], rep) not in done]
    cmd = shlex.split(a.model_cmd)
    print("%d prompts to run (%d already recorded) -> %s" % (len(work), len(done), out_path), file=sys.stderr)
    with open(out_path, "a", encoding="utf-8") as out, cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
        futs = {ex.submit(call, cmd, p["prompt"], a.timeout): (p, rep) for p, rep in work}
        for i, fut in enumerate(cf.as_completed(futs), 1):
            p, rep = futs[fut]
            raw, err, latency = fut.result()
            rec = {"id": p["id"], "dataset": p["dataset"], "format": p["format"], "kind": p["kind"], "repeat": rep,
                   "model_cmd": a.model_cmd, "expected": p["expected"], "answer_type": p["answer_type"],
                   "raw": raw, "correct": bool(not err and is_correct(raw, p["expected"], p["answer_type"])),
                   "error": err, "latency_s": latency}
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")
            out.flush()
            if i % 25 == 0:
                print("  %d/%d" % (i, len(work)), file=sys.stderr)
    summarize(out_path)


if __name__ == "__main__":
    main()

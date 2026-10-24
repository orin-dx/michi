#!/usr/bin/env python3
"""Generate the shape corpus and its questions, deterministically.

Writes corpus/<shape>.json ({"shape", "wrapper", "data", "michi"}) and questions.jsonl (one line per
question: id, shape, kind, question, answer, answer_type). Every answer is computed from the source
data in this file, never from a render. Synthetic data only: no real customer data.

answer_type: "string" (normalized, case-insensitive), "int", "number" (abs tolerance 0.005), "list"
(unordered set), "null" (the value is null/missing), "exact" (case-sensitive string equality: opaque ids
and paths the agent must copy into a follow-up call).
"""
import datetime as dt
import json
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
SEED = 20261024
HEX = "0123456789abcdef"
ALNUM = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789"


def hexid(r, n):
    return "".join(r.choice(HEX) for _ in range(n))


def tok(r, n):
    return "".join(r.choice(ALNUM) for _ in range(n))


def iso(base, seconds):
    return (base + dt.timedelta(seconds=seconds)).strftime("%Y-%m-%dT%H:%M:%SZ")


class Qs:
    def __init__(self, shape):
        self.shape, self.items = shape, []

    def add(self, kind, question, answer, atype):
        if atype == "list":
            answer = sorted(str(a) for a in answer)
        self.items.append({"id": "%s-%02d" % (self.shape, len(self.items) + 1), "shape": self.shape,
                           "kind": kind, "question": question, "answer": answer, "answer_type": atype})


def itype(v):
    return "int" if isinstance(v, int) and not isinstance(v, bool) else "string"


# ---------------------------------------------------------------- 1. uniform list
def uniform(r):
    services = ["auth", "billing", "catalog", "checkout", "gateway", "search", "notify", "reports", "ingest",
                "media", "orders", "payments", "profiles", "scheduler"]
    regions = ["us-east-1", "us-west-2", "eu-west-1", "eu-central-1", "ap-south-1", "ap-northeast-1"]
    owners = ["mira", "tomas", "dev", "lena", "ana", "kofi", "ivo", "sana"]
    pairs = r.sample([(s, g) for s in services for g in regions], 50)
    cpus = r.sample(range(1, 100), 50)
    secs = r.sample(range(0, 86400 * 30), 50)
    base = dt.datetime(2026, 9, 1)
    rows = []
    for i, (s, g) in enumerate(pairs):
        rows.append({"uid": hexid(r, 24), "service": s, "region": g,
                     "status": r.choice(["running", "running", "running", "degraded", "stopped", "failed"]),
                     "replicas": r.randint(1, 12), "cpu_pct": cpus[i],
                     "version": "v%d.%d.%d" % (r.randint(1, 3), r.randint(0, 19), r.randint(0, 9)),
                     "deployed_at": iso(base, secs[i]), "owner": r.choice(owners)})
    q = Qs("uniform")
    pk = lambda row: "service %s in region %s" % (row["service"], row["region"])
    a, b, c, d = r.sample(rows, 4)
    q.add("lookup", "What is the replicas value of the deployment of %s?" % pk(a), a["replicas"], "int")
    q.add("lookup", "Who is the owner of the deployment of %s?" % pk(b), b["owner"], "string")
    q.add("lookup", "What is the version of the deployment of %s?" % pk(c), c["version"], "string")
    q.add("filter_count", "How many deployments have status failed?", sum(x["status"] == "failed" for x in rows), "int")
    reg = "eu-west-1"
    q.add("filter_count", "How many deployments in region %s have status running?" % reg,
          sum(x["region"] == reg and x["status"] == "running" for x in rows), "int")
    q.add("aggregate", "What is the total replicas across all deployments with status running?",
          sum(x["replicas"] for x in rows if x["status"] == "running"), "int")
    q.add("aggregate", "What is the lowest replicas value among deployments with status degraded?",
          min(x["replicas"] for x in rows if x["status"] == "degraded"), "int")
    top = max(rows, key=lambda x: x["cpu_pct"])
    q.add("aggregate", "Which service has the highest cpu_pct?", top["service"], "string")
    cnt = {}
    for x in rows:
        cnt[x["owner"]] = cnt.get(x["owner"], 0) + 1
    best = max(cnt.values())
    top_owners = [o for o, n in cnt.items() if n == best]
    assert len(top_owners) == 1, "owner tie; change the seed"
    q.add("multihop", "Find the owner with the most deployments. What is the highest cpu_pct among that owner's deployments?",
          max(x["cpu_pct"] for x in rows if x["owner"] == top_owners[0]), "int")
    latest = max(rows, key=lambda x: x["deployed_at"])
    q.add("multihop", "What is the region of the deployment with the latest deployed_at?", latest["region"], "string")
    e = r.choice([x for x in rows if x is not a])
    q.add("exact_copy", "Return the uid of the deployment of %s, exactly as written." % pk(e), e["uid"], "exact")
    lo = min(rows, key=lambda x: x["cpu_pct"])
    q.add("exact_copy", "Return the uid of the deployment with the lowest cpu_pct, exactly as written.", lo["uid"], "exact")
    return rows, q.items


# ---------------------------------------------------------------- 2. single record
def single(r):
    rec = {"id": hexid(r, 24), "name": "api-prod-07", "status": "running", "region": "eu-west-2",
           "instance_type": "m6i.2xlarge", "cpu_cores": 8, "memory_gb": 32, "disk_gb": 500,
           "public_ip": "203.0.113.%d" % r.randint(20, 240), "owner": "platform-team", "monitoring": True,
           "cost_per_hour": 0.425, "created_at": "2026-03-14T09:22:41Z", "uptime_hours": 3217,
           "snapshot_id": tok(r, 20), "terminated_at": None, "maintenance_window": None}
    q = Qs("single")
    q.add("lookup", "What is the region of the instance?", rec["region"], "string")
    q.add("lookup", "What is the instance_type?", rec["instance_type"], "string")
    q.add("lookup", "What is the public_ip?", rec["public_ip"], "string")
    q.add("lookup", "Is monitoring enabled? Answer true or false.", "true" if rec["monitoring"] else "false", "string")
    q.add("aggregate", "What is cpu_cores plus memory_gb plus disk_gb?", rec["cpu_cores"] + rec["memory_gb"] + rec["disk_gb"], "int")
    q.add("aggregate", "What is memory_gb divided by cpu_cores?", rec["memory_gb"] // rec["cpu_cores"], "int")
    q.add("aggregate", "What does the instance cost per day (cost_per_hour times 24)? Give the number only.",
          round(rec["cost_per_hour"] * 24, 2), "number")
    q.add("aggregate", "How many whole days is uptime_hours (uptime_hours divided by 24, rounded down)?", rec["uptime_hours"] // 24, "int")
    q.add("null_count", "How many fields of the instance are null?", sum(v is None for v in rec.values()), "int")
    q.add("filter_count", "How many fields have a numeric value (integer or decimal, not text, not boolean)?",
          sum(isinstance(v, (int, float)) and not isinstance(v, bool) for v in rec.values()), "int")
    q.add("exact_copy", "Return the instance id, exactly as written.", rec["id"], "exact")
    q.add("exact_copy", "Return the snapshot_id, exactly as written.", rec["snapshot_id"], "exact")
    return rec, q.items


# ---------------------------------------------------------------- 3. nested issue
def nested(r):
    base = dt.datetime(2026, 10, 2, 8, 0, 0)
    assignees = ["mira.k", "tomas.v"]
    # (author, fact kind); dev.s / lena.r / ana.p comment once so questions about them are unambiguous.
    plan = [("mira.k", "pod"), ("tomas.v", "latency"), ("dev.s", "commit"), ("mira.k", "version"),
            ("lena.r", "ticket"), ("tomas.v", "host"), ("mira.k", "pct"), ("ana.p", "timeout")]
    opens = ["I can reproduce this on staging.", "This started right after the last deploy.",
             "Looks like the retry loop is not backing off.", "The error rate is back to normal for now.",
             "Customers are still reporting failed payments.", "I checked the load balancer logs and they look clean.",
             "We saw the same thing last quarter.", "Not sure this is only the gateway."]
    comments, t = [], 0
    facts = {}
    for i, (author, kind) in enumerate(plan):
        t += r.randint(900, 7200)
        if kind == "commit":
            f = hexid(r, 7); line = "The change landed in commit %s." % f
        elif kind == "ticket":
            f = "OPS-%d" % r.randint(1000, 9999); line = "Tracking this in %s." % f
        elif kind == "timeout":
            f = str(r.choice([15, 20, 45, 90])); line = "The upstream timeout is set to %s seconds." % f
        elif kind == "pod":
            f = "payments-gw-%s" % tok(r, 5).lower(); line = "The failing pod was %s." % f
        elif kind == "latency":
            f = str(r.randint(800, 4000)); line = "p99 latency hit %s ms." % f
        elif kind == "version":
            f = "v2.%d.%d" % (r.randint(1, 30), r.randint(0, 9)); line = "Rolled back to %s." % f
        elif kind == "host":
            f = "ip-10-0-%d-%d" % (r.randint(1, 250), r.randint(1, 250)); line = "Only %s shows the problem." % f
        else:
            f = "%d" % r.randint(2, 40); line = "About %s percent of requests fail." % f
        facts[(author, kind)] = f
        body = " ".join([opens[i], line])
        comments.append({"id": tok(r, 22), "author": author, "timestamp": iso(base, t), "body": body})
    issue = {"id": hexid(r, 24), "number": 4172,
             "title": "Checkout API returns 502 after payments-gw deploy", "state": "open",
             "reporter": "sana.o", "labels": ["bug", "p1", "payments", "regression"], "assignees": assignees,
             "created_at": iso(base, -3600), "milestone": None, "closed_at": None, "comments": comments}
    q = Qs("nested")
    q.add("lookup", "Who reported the issue (reporter)?", issue["reporter"], "string")
    q.add("filter_count", "How many comments were written by mira.k?", sum(c["author"] == "mira.k" for c in comments), "int")
    mid = comments[4]["timestamp"]
    q.add("filter_count", "How many comments have a timestamp after %s?" % mid, sum(c["timestamp"] > mid for c in comments), "int")
    q.add("aggregate", "What is the timestamp of the earliest comment?", min(c["timestamp"] for c in comments), "string")
    q.add("multihop", "Who wrote the last (latest) comment?", max(comments, key=lambda c: c["timestamp"])["author"], "string")
    cnt = {a: sum(c["author"] == a for c in comments) for a in assignees}
    assert len(set(cnt.values())) == 2
    q.add("multihop", "Which assignee wrote the most comments?", max(cnt, key=cnt.get), "string")
    q.add("multihop", "How many comments were written by people who are not assignees?",
          sum(c["author"] not in assignees for c in comments), "int")
    q.add("lookup", "Which commit does the comment by dev.s refer to?", facts[("dev.s", "commit")], "string")
    q.add("lookup", "What ticket is mentioned in the comment by lena.r?", facts[("lena.r", "ticket")], "string")
    q.add("null_count", "How many top-level fields of the issue are null?", sum(v is None for v in issue.values()), "int")
    first_t = next(c for c in comments if c["author"] == "tomas.v")
    q.add("exact_copy", "Return the id of the first comment written by tomas.v, exactly as written.", first_t["id"], "exact")
    q.add("exact_copy", "Return the id of the issue (the top-level id, not a comment id), exactly as written.", issue["id"], "exact")
    return issue, q.items


# ---------------------------------------------------------------- 4. file tree
TREE = {
    "Cargo.toml": None, "README.md": None, "LICENSE": None,
    "src": {"main.rs": None, "lib.rs": None,
            "bin": {"cli.rs": None, "serve.rs": None},
            "render": {"mod.rs": None, "table.rs": None, "cells.rs": None, "escape.rs": None,
                       "theme": {"default.rs": None, "dark.rs": None, "mod.rs": None}},
            "parse": {"mod.rs": None, "lexer.rs": None, "parser.rs": None, "ast.rs": None, "errors.rs": None},
            "util": {"mod.rs": None, "strings.rs": None, "paths.rs": None}},
    "tests": {"render_tests.rs": None, "parse_tests.rs": None,
              "fixtures": {"small.json": None, "large.json": None, "edge_cases.json": None, "unicode.json": None}},
    "docs": {"guide.md": None, "api.md": None,
             "adr": {"0001-format.md": None, "0002-errors.md": None, "0003-caching.md": None}},
    "scripts": {"release.sh": None, "bench.sh": None},
}


def tree(r):
    used = set()

    def size():
        while True:
            v = r.choice([r.randint(40, 900), r.randint(900, 20000), r.randint(20000, 400000)])
            if v not in used:
                used.add(v)
                return v

    def build(spec, path):
        out = []
        for name, sub in spec.items():
            if sub is None:
                out.append({"name": name, "type": "file", "size": size(),
                            "sha": hexid(r, 20)})
            else:
                out.append({"name": name, "type": "dir", "children": build(sub, path + [name])})
        return out
    root = {"name": "repo", "type": "dir", "children": build(TREE, [])}
    files = []

    def walk(node, path, depth):
        for ch in node["children"]:
            p = path + [ch["name"]]
            if ch["type"] == "file":
                files.append({"path": "/".join(p), "name": ch["name"], "size": ch["size"], "sha": ch["sha"], "depth": len(p)})
            else:
                walk(ch, p, depth + 1)
    walk(root, [], 0)
    sizes = [f["size"] for f in files]
    assert len(set(sizes)) == len(sizes)
    names = [f["name"] for f in files]
    uniq_names = [f for f in files if names.count(f["name"]) == 1]
    q = Qs("tree")
    under = lambda d: [f for f in files if f["path"].startswith(d + "/")]
    f1 = r.choice([f for f in uniq_names if f["name"].endswith(".rs")])
    q.add("lookup", "What is the size of the file named %s?" % f1["name"], f1["size"], "int")
    f2 = r.choice([f for f in uniq_names if f is not f1])
    q.add("lookup", "What is the size of the file named %s?" % f2["name"], f2["size"], "int")
    q.add("filter_count", "How many files have a name ending in .rs?", sum(f["name"].endswith(".rs") for f in files), "int")
    q.add("filter_count", "How many files are anywhere under the tests directory (at any depth)?", len(under("tests")), "int")
    q.add("filter_count", "How many files are at the deepest level of the tree (the most directories deep)?",
          sum(f["depth"] == max(g["depth"] for g in files) for f in files), "int")
    q.add("aggregate", "What is the total size of all files under src/render (at any depth)?", sum(f["size"] for f in under("src/render")), "int")
    big = max(files, key=lambda f: f["size"])
    q.add("aggregate", "What is the name of the largest file in the whole tree (by size)?", big["name"], "string")
    q.add("multihop", "In which directory (give its name only) is the largest file with a .rs extension located?",
          max([f for f in files if f["name"].endswith(".rs")], key=lambda f: f["size"])["path"].split("/")[-2], "string")
    q.add("multihop", "How many direct children (files and directories) does src/render have?", len(TREE["src"]["render"]), "int")
    f3 = r.choice([f for f in files if f["depth"] >= 3])
    q.add("exact_copy", "Return the sha of the file at %s, exactly as written." % f3["path"], f3["sha"], "exact")
    f4 = r.choice([f for f in files if f["depth"] >= 3 and f is not f3 and 16 <= len(f["path"]) <= 24])
    q.add("exact_copy", "Return the full path (from the repo root, without the leading repo/) of the file whose sha is %s, exactly as written." % f4["sha"],
          f4["path"], "exact")
    f5 = max(under("src/parse"), key=lambda f: f["size"])
    q.add("exact_copy", "Return the full path (from the repo root, without the leading repo/) of the largest file under src/parse, exactly as written.",
          f5["path"], "exact")
    return root, q.items


# ---------------------------------------------------------------- 5. event log
def events(r):
    base = dt.datetime(2026, 10, 20, 6, 0, 0)
    types = ["login"] * 12 + ["deploy"] * 8 + ["alert"] * 9 + ["error"] * 11
    r.shuffle(types)
    svc = ["auth", "billing", "catalog", "checkout", "gateway", "search"]
    users = ["mira", "tomas", "dev", "lena", "ana", "kofi"]
    secs = sorted(r.sample(range(0, 86400), 40))
    values = r.sample(range(500, 990), 9)
    nth = {"alert": 0, "error": 0}
    errs = []
    evs = []
    for i, ty in enumerate(types):
        e = {"ts": iso(base, secs[i]), "type": ty}
        if ty == "login":
            e.update(user=r.choice(users), ip="198.51.100.%d" % r.randint(2, 250), success=r.random() < 0.6,
                     method=r.choice(["password", "sso", "token"]))
        elif ty == "deploy":
            e.update(service=r.choice(svc), version="v%d.%d.%d" % (r.randint(1, 3), r.randint(0, 19), r.randint(0, 9)),
                     env=r.choice(["prod", "staging"]), duration_s=r.randint(20, 900), build_id=hexid(r, 16))
        elif ty == "alert":
            v = values[nth["alert"]] / 10.0; nth["alert"] += 1
            e.update(name=r.choice(["cpu_high", "disk_full", "latency_p99", "queue_depth"]),
                     severity=r.choice(["info", "warning", "critical"]), host="ip-10-1-%d-%d" % (r.randint(1, 60), r.randint(1, 250)),
                     value=v, threshold=round(v * r.choice([0.5, 0.7, 0.9]), 1))
        else:
            e.update(service=r.choice(svc), code=r.choice([500, 502, 503, 504, 429]),
                     message=r.choice(["upstream timed out", "connection reset by peer", "queue full", "bad gateway", "rate limit exceeded"]),
                     trace_id=tok(r, 20))
            errs.append(e)
        evs.append(e)
    for e in r.sample(errs, 3):
        e["trace_id"] = None
    q = Qs("events")
    c = lambda **kw: [e for e in evs if all(e.get(k) == v for k, v in kw.items())]
    q.add("filter_count", "How many events have type error?", len(c(type="error")), "int")
    q.add("filter_count", "How many login events have success false?", len(c(type="login", success=False)), "int")
    q.add("filter_count", "How many alert events have severity critical?", len(c(type="alert", severity="critical")), "int")
    deps = c(type="deploy")
    q.add("aggregate", "What is the total duration_s of all deploy events?", sum(e["duration_s"] for e in deps), "int")
    q.add("aggregate", "What is the longest duration_s among deploy events?", max(e["duration_s"] for e in deps), "int")
    al = c(type="alert")
    top = max(al, key=lambda e: e["value"])
    q.add("aggregate", "Which host raised the alert with the highest value?", top["host"], "string")
    fl = [e for e in c(type="login", success=False)]
    cnt = {}
    for e in fl:
        cnt[e["user"]] = cnt.get(e["user"], 0) + 1
    m = max(cnt.values()); tops = [u for u, n in cnt.items() if n == m]
    assert len(tops) == 1, "failed-login tie"
    q.add("multihop", "Which user has the most failed logins?", tops[0], "string")
    ec = {}
    for e in c(type="error"):
        ec[e["service"]] = ec.get(e["service"], 0) + 1
    m = max(ec.values()); tops = [s for s, n in ec.items() if n == m]
    assert len(tops) == 1, "error service tie"
    last_dep = [e for e in deps if e["service"] == tops[0]]
    assert last_dep, "top error service never deployed"
    q.add("multihop", "Find the service with the most error events. What version did its most recent deploy event deploy?",
          max(last_dep, key=lambda e: e["ts"])["version"], "string")
    q.add("null_count", "How many error events have no trace_id (null)?", sum(e["trace_id"] is None for e in c(type="error")), "int")
    pd = [e for e in deps if e["env"] == "staging"]
    seen = {}
    for e in pd:
        seen.setdefault(e["service"], []).append(e)
    single_stg = [v[0] for v in seen.values() if len(v) == 1]
    d = r.choice(single_stg)
    q.add("lookup", "What version did the staging deploy event of service %s deploy?" % d["service"], d["version"], "string")
    q.add("exact_copy", "Return the build_id of the staging deploy event of service %s, exactly as written." % d["service"], d["build_id"], "exact")
    errs_t = [e for e in c(type="error") if e["trace_id"]]
    cand = [e for e in errs_t if sum(x["service"] == e["service"] and x["code"] == e["code"] for x in c(type="error")) == 1]
    t = r.choice(cand)
    q.add("exact_copy", "Return the trace_id of the error event from service %s with code %d, exactly as written." % (t["service"], t["code"]),
          t["trace_id"], "exact")
    return evs, q.items


# ---------------------------------------------------------------- 6. sparse table
def sparse(r):
    first = ["Ada", "Bram", "Cleo", "Dara", "Eli", "Faye", "Gus", "Hana", "Ines", "Jonas", "Kira", "Leo", "Mina", "Nils", "Opal", "Pia",
             "Quin", "Rhea", "Sven", "Tess"]
    last = ["Ahn", "Bauer", "Cruz", "Diaz", "Egan", "Frost", "Gupta", "Hale", "Ito", "Jain"]
    names = r.sample([f + " " + l for f in first for l in last], 40)
    cities = ["Lisbon", "Austin", "Pune", "Oslo", "Leeds", "Osaka", "Lyon", "Quito"]
    companies = ["Northwind", "Contoso Labs", "Fabrikam", "Initech", "Globex", "Umbra", "Hooli", "Vandelay"]
    titles = ["CTO", "Engineer", "Designer", "Analyst", "Director", "Founder"]
    rows = []
    mrrs = r.sample(range(1000, 99900), 40)
    for i, n in enumerate(names):
        slug = n.lower().replace(" ", ".")
        cells = {"email": "%s@%s.example" % (slug, r.choice(["mail", "corp", "work"])), "phone": "+1-555-%04d" % r.randint(0, 9999),
                 "city": r.choice(cities), "company": r.choice(companies), "title": r.choice(titles),
                 "plan": r.choice(["free", "pro", "team", "enterprise"]), "seats": r.randint(1, 60),
                 "mrr": mrrs[i] / 100.0, "signed_up": iso(dt.datetime(2025, 1, 1), r.randint(0, 86400 * 600))[:10],
                 "last_login": iso(dt.datetime(2026, 8, 1), r.randint(0, 86400 * 80))[:10]}
        row = {"id": tok(r, 20), "name": n}
        for k, v in cells.items():
            row[k] = None if r.random() < 0.42 else v
        rows.append(row)
    top = max(rows, key=lambda x: x["mrr"] if x["mrr"] is not None else -1)
    top["city"] = top["city"] or "Lisbon"
    top["plan"] = top["plan"] or "pro"
    q = Qs("sparse")
    nn = lambda col: [x for x in rows if x[col] is not None]
    a = r.choice(nn("email"))
    q.add("lookup", "What is the email of %s?" % a["name"], a["email"], "string")
    b = r.choice([x for x in rows if x["phone"] is None])
    q.add("null_lookup", "What is the phone of %s? (reply null if there is none)" % b["name"], None, "null")
    c = r.choice(nn("company"))
    q.add("lookup", "What is the company of %s?" % c["name"], c["company"], "string")
    q.add("null_count", "How many rows have no email (null)?", sum(x["email"] is None for x in rows), "int")
    q.add("null_count", "How many rows have a null phone but a non-null email?",
          sum(x["phone"] is None and x["email"] is not None for x in rows), "int")
    q.add("filter_count", "How many rows have plan enterprise and seats greater than 20?",
          sum(x["plan"] == "enterprise" and (x["seats"] or 0) > 20 for x in rows), "int")
    q.add("aggregate", "What is the sum of seats across all rows (ignore nulls)?", sum(x["seats"] for x in nn("seats")), "int")
    q.add("aggregate", "What is the highest mrr in the table? Give the number only.", top["mrr"], "number")
    q.add("aggregate", "What is the lowest non-null seats value?", min(x["seats"] for x in nn("seats")), "int")
    q.add("multihop", "What is the city of the person with the highest mrr?", top["city"], "string")
    q.add("multihop", "How many rows have a non-null plan equal to pro and a non-null city?",
          sum(x["plan"] == "pro" and x["city"] is not None for x in rows), "int")
    d = r.choice([x for x in rows if x is not a])
    q.add("exact_copy", "Return the id of the row for %s, exactly as written." % d["name"], d["id"], "exact")
    return rows, q.items


# ---------------------------------------------------------------- 7. long free text
def longtext(r):
    cats = ["billing", "shipping", "account", "product", "other"]
    people = ["Ravi", "Ellen", "Joao", "Mei", "Samir", "Dana"]
    opens = ["The customer wrote in about an unexpected charge on their last statement.",
             "A package was marked as delivered but never arrived at the address on file.",
             "The user cannot sign in after changing their phone number last week.",
             "They asked whether the larger model ships in a second colour.",
             "The account owner wants to merge two workspaces into one.",
             "A recurring order was skipped without any notice.",
             "The refund promised on the previous call has not shown up yet.",
             "They reported that the mobile app closes right after the splash screen."]
    mids = ["Support checked the order history and confirmed the details were correct on our side.",
            "The agent reproduced the problem on a test account and attached the logs.",
            "An earlier ticket on the same topic was closed without a clear resolution.",
            "The customer has been with us for three years and has never opened a ticket before.",
            "Engineering said the fix needs a schema change and will not ship this sprint.",
            "The carrier tracking page shows no scan events after the first depot.",
            "Finance asked for the invoice PDF before they will release the payment."]
    ends = ["We offered a refund and a discount on the next order.", "The customer prefers a call back in the morning.",
            "No further action is needed once the customer confirms.", "Escalated to the second line for review.",
            "The customer was told to expect an update within two business days."]
    n = 24
    rating = [r.randint(1, 5) for _ in range(n)]
    rows = []
    refs = r.sample(range(1000, 9999), n)
    heads = ["Charge", "Missing parcel", "Login failure", "Colour question", "Workspace merge", "Skipped order", "Late refund",
             "App crash", "Wrong address", "Invoice copy", "Plan change", "Delivery delay"]
    for i in range(n):
        sents = [r.choice(opens)] + r.sample(mids, r.randint(0, 2)) + [r.choice(ends)]
        sents.insert(1, "Reference %s-%d." % (r.choice(["ZK", "QM", "TR"]), refs[i]))
        rows.append({"id": tok(r, 22), "title": "%s #%d" % (r.choice(heads), 100 + i), "author": r.choice(people),
                     "category": r.choice(cats), "rating": rating[i],
                     "assignee": None if r.random() < 0.3 else r.choice(people), "summary": " ".join(sents)})
    q = Qs("longtext")
    a, b, c = r.sample(rows, 3)
    q.add("lookup", "Who is the author of the ticket titled \"%s\"?" % a["title"], a["author"], "string")
    q.add("lookup", "What is the rating of the ticket titled \"%s\"?" % b["title"], b["rating"], "int")
    q.add("filter_count", "How many tickets have category billing?", sum(x["category"] == "billing" for x in rows), "int")
    q.add("filter_count", "How many tickets have a rating of 4 or higher?", sum(x["rating"] >= 4 for x in rows), "int")
    kw = "refund"
    q.add("filter_count", "In how many tickets does the summary contain the word \"%s\" (any capitalization)?" % kw,
          sum(kw in x["summary"].lower() for x in rows), "int")
    q.add("aggregate", "What is the sum of the ratings of all tickets with category shipping?",
          sum(x["rating"] for x in rows if x["category"] == "shipping"), "int")
    q.add("aggregate", "What is the lowest rating among tickets with category account?",
          min(x["rating"] for x in rows if x["category"] == "account"), "int")
    q.add("null_count", "How many tickets have no assignee (null)?", sum(x["assignee"] is None for x in rows), "int")
    ref = lambda x: x["summary"].split("Reference ")[1].split(".")[0]
    d = r.choice([x for x in rows if x["assignee"]])
    q.add("multihop", "What is the category of the ticket whose summary mentions reference %s?" % ref(d), d["category"], "string")
    q.add("multihop", "Who is the assignee of the ticket whose summary mentions reference %s?" % ref(d), d["assignee"], "string")
    e = r.choice([x for x in rows if x is not d])
    q.add("exact_copy", "Return the id of the ticket whose summary mentions reference %s, exactly as written." % ref(e), e["id"], "exact")
    f = r.choice([x for x in rows if x is not d and x is not e])
    q.add("exact_copy", "Return the id of the ticket titled \"%s\", exactly as written." % f["title"], f["id"], "exact")
    return rows, q.items


SHAPES = [
    # name, fn, wrapper key, michi mode
    ("uniform", uniform, "deployments", "list"),
    ("single", single, None, "kv"),
    ("nested", nested, None, None),
    ("tree", tree, None, None),
    ("events", events, "events", "list"),
    ("sparse", sparse, "contacts", "list"),
    ("longtext", longtext, "tickets", "list"),
]


def main():
    os.makedirs(os.path.join(HERE, "corpus"), exist_ok=True)
    allq = []
    for i, (name, fn, wrapper, michi) in enumerate(SHAPES):
        # Unlucky seeds (ties for an argmax question, no usable candidate) are retried with the next seed.
        for attempt in range(200):
            try:
                data, qs = fn(random.Random(SEED + i + 1000 * attempt))
                break
            except (AssertionError, IndexError, ValueError):
                continue
        else:
            raise SystemExit("no usable seed for " + name)
        assert len(qs) == 12, (name, len(qs))
        with open(os.path.join(HERE, "corpus", name + ".json"), "w", encoding="utf-8") as f:
            json.dump({"shape": name, "wrapper": wrapper, "michi": michi, "data": data}, f, ensure_ascii=False, indent=1)
        allq += qs
    with open(os.path.join(HERE, "questions.jsonl"), "w", encoding="utf-8") as f:
        for q in allq:
            f.write(json.dumps(q, ensure_ascii=False) + "\n")
    print("wrote %d shapes, %d questions" % (len(SHAPES), len(allq)))


if __name__ == "__main__":
    main()

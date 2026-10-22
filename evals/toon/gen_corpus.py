#!/usr/bin/env python3
"""Deterministic corpus of realistic agent-tool responses for the TOON value evaluation.

Writes corpus/<dataset>_<size>.json, each {"dataset", "type_name", "kind", "rows"}.
kind "list": rows is a list of flat-ish objects (rendered with michi `toon::list`).
kind "kv":   rows is a list of {"key", "value"} pairs (rendered with michi `render_kv`).

Every dataset/size is generated from its own seeded RNG (seed = SEED + dataset index), and
the smaller sizes are prefixes of the 100-row generation, so the corpus is byte-stable.
All string values are kept under 200 chars so michi's default max_cell_len (200) never
truncates: the TOON renders are lossless and comparable to the JSON/Markdown renders.
"""
import json
import os
import random

SEED = 20261021
SIZES = (5, 25, 100)
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpus")

WORDS = ("cache retry token parser render config client server stream buffer index query worker "
         "schedule session handler router timeout payload schema migration lock queue batch "
         "export import encode decode snapshot replay limit budget circuit breaker").split()
USERS = ["alice", "bob", "carol", "dmitri", "eun-ji", "farah", "gabe", "hiro", "ines", "jamal",
         "kofi", "lena", "mateo", "noor", "oskar", "priya"]
LANGS = {"rs": "rust", "ts": "typescript", "py": "python", "go": "go", "mjs": "javascript"}
DIRS = ["src", "src/toon", "src/kv", "crates/core/src", "packages/node/src", "lib", "internal/api",
        "pkg/server", "tests", "app/routes"]
LICENSES = ["MIT", "Apache-2.0", "MIT OR Apache-2.0", "BSD-3-Clause", "ISC", "MPL-2.0"]
SEVERITIES = ["low", "moderate", "high", "critical"]


def ts(rng, year=2026):
    return "%d-%02d-%02dT%02d:%02d:%02dZ" % (year, rng.randint(1, 10), rng.randint(1, 28),
                                             rng.randint(0, 23), rng.randint(0, 59), rng.randint(0, 59))


def phrase(rng, lo=2, hi=5):
    return " ".join(rng.choice(WORDS) for _ in range(rng.randint(lo, hi)))


def code_search(rng, n):
    rows = []
    for _ in range(n):
        ext = rng.choice(list(LANGS))
        ident = "%s_%s" % (rng.choice(WORDS), rng.choice(WORDS))
        snippets = {
            "rs": "pub fn %s(&self, input: &str) -> Result<Output, Error> {" % ident,
            "ts": "export async function %s(input: string): Promise<Output> {" % ident,
            "py": "def %s(self, payload: dict[str, Any]) -> Output:" % ident,
            "go": "func (s *Server) %s(ctx context.Context, req *Request) (*Response, error) {" % ident,
            "mjs": "const %s = (opts = {}) => render(opts, { strict: true });" % ident,
        }
        rows.append({
            "path": "%s/%s.%s" % (rng.choice(DIRS), rng.choice(WORDS), ext),
            "line": rng.randint(1, 2400),
            "column": rng.randint(1, 40),
            "language": LANGS[ext],
            "snippet": snippets[ext],
            "score": round(rng.uniform(0.2, 1.0), 3),
        })
    return rows


def github_issues(rng, n):
    rows, number = [], 1200
    label_pool = ["bug", "enhancement", "docs", "good first issue", "perf", "breaking", "needs-triage"]
    verbs = ["Fix", "Add", "Support", "Remove", "Refactor", "Document", "Investigate"]
    for _ in range(n):
        number += rng.randint(1, 9)
        title = "%s %s" % (rng.choice(verbs), phrase(rng))
        if rng.random() < 0.25:
            title += ": %s" % phrase(rng, 1, 3)
        rows.append({
            "number": number,
            "title": title,
            "state": rng.choice(["open", "open", "open", "closed"]),
            "author": rng.choice(USERS),
            "labels": sorted(rng.sample(label_pool, rng.randint(0, 3))),
            "comments": rng.randint(0, 40),
            "created_at": ts(rng),
        })
    return rows


def packages(rng, n):
    rows, seen = [], set()
    for _ in range(n):
        while True:
            name = "%s-%s" % (rng.choice(WORDS), rng.choice(WORDS))
            if name not in seen:
                seen.add(name)
                break
        rows.append({
            "name": name,
            "version": "%d.%d.%d" % (rng.randint(0, 5), rng.randint(0, 30), rng.randint(0, 20)),
            "description": "A %s for %s, with %s support" % (phrase(rng, 1, 2), phrase(rng, 1, 3), phrase(rng, 1, 2)),
            "downloads": rng.randint(50, 9_000_000),
            "license": rng.choice(LICENSES),
            "updated": ts(rng)[:10],
        })
    return rows


def test_failures(rng, n):
    rows = []
    for i in range(n):
        exp, got = rng.randint(0, 500), rng.randint(0, 500)
        kind = rng.randint(0, 3)
        if kind == 0:
            msg = "assertion `left == right` failed\n  left: %d\n right: %d" % (exp, got)
        elif kind == 1:
            msg = 'expected "%s" but got "%s"' % (rng.choice(WORDS), rng.choice(WORDS))
        elif kind == 2:
            msg = "timeout after %dms waiting for %s" % (rng.randint(100, 30000), rng.choice(WORDS))
        else:
            msg = "panicked at src/%s.rs:%d:%d: called `Option::unwrap()` on a `None` value" % (
                rng.choice(WORDS), rng.randint(1, 900), rng.randint(1, 60))
        rows.append({
            "test": "%s::tests::%s_%s_%d" % (rng.choice(WORDS), rng.choice(WORDS), rng.choice(WORDS), i),
            "file": "tests/%s.rs" % rng.choice(WORDS),
            "line": rng.randint(10, 1500),
            "duration_ms": rng.randint(1, 30000),
            "retried": rng.random() < 0.2,
            "message": msg,
        })
    return rows


def audit_findings(rng, n):
    rows = []
    alphabet = "23456789cfghjmpqrvwx"
    for _ in range(n):
        ghsa = "GHSA-" + "-".join("".join(rng.choice(alphabet) for _ in range(4)) for _ in range(3))
        major = rng.randint(0, 4)
        fixed = None if rng.random() < 0.3 else "%d.%d.%d" % (major, rng.randint(5, 20), rng.randint(0, 9))
        rows.append({
            "id": ghsa,
            "package": "%s-%s" % (rng.choice(WORDS), rng.choice(WORDS)),
            "severity": rng.choice(SEVERITIES),
            "cvss": round(rng.uniform(1.0, 10.0), 1),
            "installed": "%d.%d.%d" % (major, rng.randint(0, 4), rng.randint(0, 9)),
            "fixed_in": fixed,
            "title": "%s in %s allows %s" % (rng.choice(["Prototype pollution", "ReDoS", "Path traversal",
                                                        "Uncontrolled recursion", "Integer overflow"]),
                                             rng.choice(WORDS), phrase(rng, 1, 3)),
        })
    return rows


def file_listing(rng, n):
    rows, seen = [], set()
    for _ in range(n):
        while True:
            is_dir = rng.random() < 0.15
            path = "%s/%s%s" % (rng.choice(DIRS), rng.choice(WORDS),
                                "" if is_dir else "." + rng.choice(list(LANGS) + ["md", "json", "toml"]))
            if path not in seen:
                seen.add(path)
                break
        rows.append({
            "path": path,
            "type": "dir" if is_dir else "file",
            "size": 0 if is_dir else rng.randint(0, 250_000),
            "mode": "drwxr-xr-x" if is_dir else rng.choice(["-rw-r--r--", "-rwxr-xr-x"]),
            "modified": ts(rng),
        })
    return rows


def service_status(rng, n):
    """Key-value status of one service: n distinct keys, mixed value types (render_kv)."""
    rows, seen = [], set()
    subsystems = ["db.primary", "db.replica", "cache", "queue", "http", "grpc", "scheduler", "auth", "storage"]
    metrics = [("latency_ms", "int"), ("error_rate", "float"), ("state", "state"), ("healthy", "bool"),
               ("connections", "int"), ("version", "ver"), ("last_error", "err"), ("uptime_s", "int")]
    i = 0
    while len(rows) < n:
        sub = rng.choice(subsystems)
        if rng.random() < 0.5:
            sub = "%s.%d" % (sub, rng.randint(0, 9))
        metric, kind = rng.choice(metrics)
        key = "%s.%s" % (sub, metric)
        i += 1
        if key in seen:
            continue
        seen.add(key)
        if kind == "int":
            value = rng.randint(0, 90000)
        elif kind == "float":
            value = round(rng.uniform(0, 0.2), 4)
        elif kind == "state":
            value = rng.choice(["ready", "ready", "degraded", "draining", "down"])
        elif kind == "bool":
            value = rng.random() < 0.8
        elif kind == "ver":
            value = "v%d.%d.%d" % (rng.randint(1, 3), rng.randint(0, 20), rng.randint(0, 9))
        else:
            value = None if rng.random() < 0.6 else "connection reset by peer: %s" % rng.choice(WORDS)
        rows.append({"key": key, "value": value})
    return rows


def sparse_contacts(rng, n):
    """Sparse records: optional fields are *absent* (not null) in most rows."""
    rows = []
    optional = [("email", 0.6), ("phone", 0.25), ("company", 0.4), ("title", 0.3),
                ("github", 0.2), ("timezone", 0.35), ("notes", 0.1)]
    for i in range(n):
        name = rng.choice(USERS)
        row = {"id": 5000 + i, "name": name.capitalize() + " " + rng.choice(["Ng", "Okafor", "Silva", "Kim", "Novak"])}
        for field, p in optional:
            if rng.random() < p:
                if field == "email":
                    row[field] = "%s%d@example.com" % (name, i)
                elif field == "phone":
                    row[field] = "+1-555-%04d" % rng.randint(0, 9999)
                elif field == "company":
                    row[field] = rng.choice(["Acme", "Globex", "Initech", "Umbrella"])
                elif field == "title":
                    row[field] = rng.choice(["Staff Engineer", "SRE", "PM", "Designer"])
                elif field == "github":
                    row[field] = "%s-%d" % (name, rng.randint(1, 99))
                elif field == "timezone":
                    row[field] = rng.choice(["UTC", "America/New_York", "Europe/Berlin", "Asia/Tokyo"])
                else:
                    row[field] = "met at %s conf" % rng.choice(WORDS)
        rows.append(row)
    return rows


def ledger_bigids(rng, n):
    """Large integer IDs (> 2^53, some > i64::MAX) and strings full of commas and quotes."""
    rows = []
    for _ in range(n):
        big = rng.random() < 0.2
        txn = rng.randint(2**63, 2**64 - 1) if big else rng.randint(2**53 + 1, 2**63 - 1)
        amount = rng.randint(100, 99_999_999)
        memo_kind = rng.randint(0, 2)
        if memo_kind == 0:
            memo = 'Refund for order #%d, "%s" (partial)' % (rng.randint(1000, 9999), phrase(rng, 1, 2))
        elif memo_kind == 1:
            memo = "Invoice %d, net-30, \"%s\", paid" % (rng.randint(100, 999), rng.choice(USERS))
        else:
            memo = "Transfer to acct ending %04d" % rng.randint(0, 9999)
        rows.append({
            "txn_id": txn,
            "account_id": rng.randint(2**53 + 1, 2**62),
            "amount_display": "$%s.%02d" % ("{:,}".format(amount // 100), amount % 100),
            "amount_cents": amount,
            "memo": memo,
            "counterparty": '%s, %s' % (rng.choice(["Acme Corp", "Globex", "Initech"]), rng.choice(["Inc.", "LLC", "Ltd."])),
            "posted": ts(rng)[:10],
        })
    return rows


DATASETS = [
    # (dataset, type_name, kind, generator)
    ("code_search", "matches", "list", code_search),
    ("github_issues", "issues", "list", github_issues),
    ("package_search", "packages", "list", packages),
    ("test_failures", "failures", "list", test_failures),
    ("dependency_audit", "findings", "list", audit_findings),
    ("file_listing", "files", "list", file_listing),
    ("service_status", "status", "kv", service_status),
    ("sparse_contacts", "contacts", "list", sparse_contacts),
    ("ledger_bigids", "transactions", "list", ledger_bigids),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    for idx, (name, type_name, kind, gen) in enumerate(DATASETS):
        full = gen(random.Random(SEED + idx), max(SIZES))
        for size in SIZES:
            rows = full[:size]
            for r in rows:
                for v in (r.values() if kind == "list" else [r["value"]]):
                    assert not isinstance(v, str) or len(v) < 200, (name, v)
            doc = {"dataset": name, "type_name": type_name, "kind": kind, "rows": rows}
            with open(os.path.join(OUT, "%s_%d.json" % (name, size)), "w", encoding="utf-8") as f:
                json.dump(doc, f, ensure_ascii=False, indent=1)
                f.write("\n")
    print("wrote %d files to %s" % (len(DATASETS) * len(SIZES), OUT))


if __name__ == "__main__":
    main()

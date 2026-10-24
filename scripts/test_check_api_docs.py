#!/usr/bin/env python3
"""Self-test for check-api-docs.py. Run: python3 scripts/test_check_api_docs.py"""

import os
import pathlib
import subprocess
import sys
import tempfile

SCRIPT = pathlib.Path(__file__).with_name("check-api-docs.py")
ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "t",
    "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t",
    "GIT_COMMITTER_EMAIL": "t@example.com",
}
LIB = "crates/c/src/lib.rs"
BASE_LIB = "/// Doc.\npub fn a() {}\n\nfn private() {}\n"
BASE = {LIB: BASE_LIB, "docs/spec/01.md": "spec\n"}
NEW_FN = BASE_LIB + "pub fn b() {}\n"

CASES = [
    # (name, files written in the second commit, PR body, expected exit code, text expected in stdout)
    ("new pub fn, no docs", {LIB: NEW_FN}, "", 1, "pub fn b()"),
    ("new pub fn, spec updated", {LIB: NEW_FN, "docs/spec/01.md": "more\n"}, "", 0, "docs touched"),
    ("new pub fn, crate README updated", {LIB: NEW_FN, "crates/c/README.md": "x\n"}, "", 0, ""),
    ("new pub fn, ADR added", {LIB: NEW_FN, "docs/adr/0001-x.md": "x\n"}, "", 0, ""),
    ("new pub fn, ARCHITECTURE updated", {LIB: NEW_FN, "ARCHITECTURE.md": "x\n"}, "", 0, ""),
    ("new pub fn, unrelated markdown", {LIB: NEW_FN, "docs/research/x.md": "x\n"}, "", 1, ""),
    ("escape hatch with reason", {LIB: NEW_FN}, "Why\n\nNo spec change needed: internal move\n", 0, "skipped"),
    ("escape hatch without reason", {LIB: NEW_FN}, "No spec change needed:\n", 1, ""),
    ("escape hatch must start the line", {LIB: NEW_FN}, "see: No spec change needed: x\n", 1, ""),
    ("private fn", {LIB: BASE_LIB + "fn c() {}\n"}, "", 0, "no public API"),
    (
        "pub(crate), pub(super), pub(in ..)",
        {LIB: BASE_LIB + "pub(crate) fn c() {}\npub(super) fn d() {}\npub(in crate::x) fn e() {}\n"},
        "",
        0,
        "no public API",
    ),
    ("doc comment only", {LIB: BASE_LIB.replace("/// Doc.", "/// Better doc.")}, "", 0, "no public API"),
    ("comment mentioning pub fn", {LIB: BASE_LIB + "// pub fn old() {}\n    /// pub fn x()\n"}, "", 0, "no public API"),
    ("whitespace only", {LIB: BASE_LIB.replace("pub fn a() {}", "pub  fn a()  {}")}, "", 0, "no public API"),
    (
        "pub fn in cfg(test) module",
        {LIB: BASE_LIB + "#[cfg(test)]\nmod tests {\n    pub fn helper() {\n        if true {}\n    }\n}\n"},
        "",
        0,
        "no public API",
    ),
    (
        "pub fn after cfg(test) module",
        {LIB: BASE_LIB + "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\npub fn after() {}\n"},
        "",
        1,
        "pub fn after()",
    ),
    ("removed pub fn", {LIB: "fn private() {}\n"}, "", 1, "- pub fn a()"),
    ("changed signature", {LIB: BASE_LIB.replace("pub fn a()", "pub fn a(x: u8)")}, "", 1, "pub fn a(x: u8)"),
    ("pub field", {LIB: BASE_LIB + "pub struct S {\n    pub x: u8,\n}\n"}, "", 1, "pub x: u8"),
    ("pub use", {LIB: BASE_LIB + "pub use crate::a::B;\n"}, "", 1, "pub use"),
    ("pub async fn and const fn", {LIB: BASE_LIB + "pub async fn f() {}\npub const fn g() {}\n"}, "", 1, "pub const fn g()"),
    ("pub fn in an integration test", {"crates/c/tests/t.rs": "pub fn helper() {}\n"}, "", 0, "no public API"),
    ("pub fn in a bench", {"crates/c/benches/b.rs": "pub fn helper() {}\n"}, "", 0, "no public API"),
    ("pub fn in a tests.rs file", {"crates/c/src/tests.rs": "pub fn helper() {}\n"}, "", 0, "no public API"),
    (
        "code moved between files",
        {LIB: "fn private() {}\n", "crates/c/src/other.rs": "/// Doc.\npub fn a() {}\n"},
        "",
        0,
        "no public API",
    ),
    ("docs only", {"docs/spec/01.md": "more\n"}, "", 0, "no public API"),
]


def vcs(cwd, *args):
    """Run a version-control command in cwd."""
    config = ["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"]
    subprocess.run(["git", *config, *args], cwd=cwd, env=ENV, check=True, capture_output=True)


def write(root, files):
    """Write each file under root, replacing existing content."""
    for name, text in files.items():
        path = pathlib.Path(root, name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


failures = 0
for name, changed, body, want_code, want_text in CASES:
    with tempfile.TemporaryDirectory() as directory:
        vcs(directory, "init", "-q", "-b", "main")
        write(directory, BASE)
        vcs(directory, "add", "-A")
        vcs(directory, "commit", "-q", "-m", "base")
        write(directory, changed)
        vcs(directory, "add", "-A")
        vcs(directory, "commit", "-q", "-m", "change")
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "HEAD^", "HEAD"],
            cwd=directory,
            env={**ENV, "PR_BODY": body},
            capture_output=True,
            text=True,
        )
    if result.returncode != want_code or want_text not in result.stdout:
        failures += 1
        print(f"{name}: exit {result.returncode} (want {want_code}), want {want_text!r} in:\n{result.stdout}{result.stderr}")
print(f"{len(CASES) - failures}/{len(CASES)} cases pass")
sys.exit(1 if failures else 0)

#!/usr/bin/env python3
"""Self-test for check-permissive-deps.py's SPDX evaluation. Run: python3 scripts/test_check_permissive_deps.py"""

import importlib.util
import pathlib
import sys

spec = importlib.util.spec_from_file_location("check", pathlib.Path(__file__).with_name("check-permissive-deps.py"))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

CASES = {
    "MIT": True,
    "MIT OR Apache-2.0": True,
    "MIT/Apache-2.0": True,
    "Apache-2.0 WITH LLVM-exception": True,
    "MIT AND (GPL-3.0-only OR Apache-2.0)": True,
    "Apache-2.0 AND MIT OR GPL-2.0": True,
    # AND binds tighter than OR, and parentheses group: each of these requires GPL.
    "(MIT OR GPL-3.0-only) AND GPL-3.0-only": False,
    "GPL-2.0 OR MIT AND GPL-2.0": False,
    "GPL-3.0-only WITH Classpath-exception-2.0": False,
    "MIT WITH Unknown-exception": False,
    "FSL-1.1-MIT": False,
    "": False,
    "(MIT": False,
    "MIT AND": False,
    "MIT OR OR Apache-2.0": False,
}

failures = [(expr, want) for expr, want in CASES.items() if check.permissive(expr) != want]
for expr, want in failures:
    print(f"permissive({expr!r}) should be {want}")
print(f"{len(CASES) - len(failures)}/{len(CASES)} cases pass")
sys.exit(1 if failures else 0)

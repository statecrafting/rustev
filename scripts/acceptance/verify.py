"""Partition whole-spec acceptance without changing its commands."""
import collections
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
# Balanced on measured per-spec seconds (eight runs, 2026-10-23): 004 alone
# takes about 24 minutes, so it runs alone and the rest share four groups.
# integrations-core reached its 45-minute limit once 037 joined it
# (2026-10-27), so the specs no spec text places in it moved to
# integrations-base: about 22 minutes each.
# Remeasured 2026-10-27 (integrations-core 34 minutes, integrations-base
# 17): 040 (about 7 minutes) moves to integrations-base.
# 041 joins cli-backends beside 014 and 029, the specs it amends.
# 044 joins runtime-evidence beside 003 and 009, the specs it amends (about
# 70 seconds with its test targets rebuilt, 2026-10-29).
GROUPS = {
    "evaluation": "004".split(),
    "cli-backends": "001 005 006 014 029 041".split(),
    "integrations-base": "002 010 012 015 017 019 020 033 038 040".split(),
    "integrations-core": "022 034 035 036 037 045".split(),
    "runtime-evidence": "003 007 008 009 013 023 026 027 028 039 044 049".split(),
}


def validate(expected, groups):
    assigned = [spec for group in groups.values() for spec in group]
    if not expected or len(set(expected)) != len(expected):
        raise ValueError("authoritative spec list is empty or contains duplicates")
    if not groups or any(not group for group in groups.values()):
        raise ValueError("acceptance groups must be nonempty")
    if collections.Counter(assigned) != collections.Counter(expected):
        raise ValueError("acceptance groups must cover VERIFIED_SPECS exactly once")


def run_group(specs):
    rows = ["| Spec | Seconds | Result |", "|---|---:|---|"]
    try:
        for spec in specs:
            start = time.monotonic()
            print(f"::group::Acceptance {spec}", flush=True)
            result = subprocess.run(
                ["make", "verify", f"VERIFIED_SPECS={spec}"], cwd=ROOT
            )
            elapsed = time.monotonic() - start
            print(f"::endgroup::\nacceptance {spec}: {elapsed:.1f}s, exit {result.returncode}", flush=True)
            rows.append(f"| {spec} | {elapsed:.1f} | {'passed' if result.returncode == 0 else 'failed'} |")
            if result.returncode:
                return 1
        return 0
    finally:
        if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(summary, "a", encoding="utf-8") as stream:
                stream.write("\n".join(rows) + "\n")


def main():
    expected = subprocess.check_output(
        ["make", "--no-print-directory", "-s", "verified-specs"], cwd=ROOT, text=True
    ).split()
    validate(expected, GROUPS)
    if sys.argv[1:] == ["--matrix"]:
        print(json.dumps({"group": list(GROUPS)}))
        return 0
    if len(sys.argv) != 2 or sys.argv[1] not in GROUPS:
        raise ValueError("select --matrix or a known acceptance group")
    return run_group(GROUPS[sys.argv[1]])


if __name__ == "__main__":
    sys.exit(main())

"""Partition whole-spec acceptance without changing its commands."""
import collections
import json
import os
from pathlib import Path
import shutil
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
# 030 joins integrations-base (about 17 to 22 minutes; integrations-core is
# near its 45-minute limit).
# 059 joins integrations-base beside 010, 012 and 030, the specs it amends.
# 043 joins runtime-evidence beside 026, which it amends (about 3.5 minutes
# with its test targets rebuilt locally, 2026-10-30).
# 031 joins integrations-core beside 045, the release family's other spec
# (about 1 minute with its test targets built, 2026-11-03).
# 042 joins integrations-base beside 010 (about 1 minute with its test
# targets built, 2026-11-03).
# 021 joins integrations-core beside 031, which amends it (its targets are a
# subset of 031's, about 1 minute built, 2026-11-03).
# 061 joins cli-backends beside 029 and 041, the specs it amends.
# 011 joins cli-repeat beside 005, the other backend (about 1 minute built,
# 2026-11-06).
# 063 joins runtime-evidence beside 027, the spec it amends (about 94
# seconds locally with its test targets built, 2026-11-06).
# 060 joins integrations-core beside 021 and 045, the release family's
# other specs (about 18 seconds locally with its test targets built,
# 2026-11-06).
# 064 joins cli-repeat beside 029 and 061, the specs it amends.
# 065 joins cli-repeat beside 029 and 061, the specs it amends (its
# targets are a subset of 061's).
# 2026-11-05: the runner limit rose to 500 jobs, so the three groups over
# 25 minutes split in two on that day's queue run (per-spec seconds:
# cli-backends 2,200, integrations-base 2,172, integrations-core 1,611).
# Each half is at most about 21 minutes, under 004's 20 alone plus setup.
# 2026-11-07: cli-repeat (001 005 011 029 061 064 065) ran 33 to 43 minutes
# on the merge queue against the job's 45-minute limit and one run was
# cancelled while compiling; the selection specs 029, 061, 064 and 065
# move to their own group.
GROUPS = {
    "evaluation": "004".split(),
    "cli-backends": "006 014 041".split(),
    "cli-repeat": "001 005 011".split(),
    "selection": "029 061 064 065".split(),
    "integrations-base": "002 010 033 038 040 042".split(),
    "integrations-remote": "012 015 017 019 020 030 059".split(),
    "integrations-core": "021 036 037 045 060".split(),
    "integrations-plans": "022 031 034 035 066".split(),
    "runtime-evidence": "003 007 008 009 013 023 026 027 028 039 043 044 049 063".split(),
}


def validate(expected, groups):
    assigned = [spec for group in groups.values() for spec in group]
    if not expected or len(set(expected)) != len(expected):
        raise ValueError("authoritative spec list is empty or contains duplicates")
    if not groups or any(not group for group in groups.values()):
        raise ValueError("acceptance groups must be nonempty")
    if collections.Counter(assigned) != collections.Counter(expected):
        raise ValueError("acceptance groups must cover VERIFIED_SPECS exactly once")


# 2026-11-04: integrations-base ran out of disk with 41 GB free after the
# workflow's cleanup: its thirteen specs build different features and test
# targets into one target directory, which only grows. Before each spec the
# free space is recorded and, below the floor, rebuildable output is
# dropped: incremental artifacts first, then the whole target directory.
FLOOR = int(os.environ.get("ACCEPTANCE_DISK_FLOOR_GIB", "20")) * 2**30


def target_dir():
    return Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")


def free_bytes(path):
    while not path.exists():
        path = path.parent
    return shutil.disk_usage(path).free


def ensure_disk(floor=FLOOR):
    """Free rebuildable output when free space is below `floor`; returns the
    step taken ("none", "incremental", "clean" or "clean_failed") for the
    log. A failed clean is reported, never fatal: the spec still runs."""
    target = target_dir()
    free = free_bytes(target)
    print(f"acceptance disk: {free / 2**30:.1f} GiB free, floor {floor / 2**30:.0f} GiB", flush=True)
    if free >= floor:
        return "none"
    for profile in ("debug", "release"):
        shutil.rmtree(target / profile / "incremental", ignore_errors=True)
    free = free_bytes(target)
    print(f"acceptance disk: {free / 2**30:.1f} GiB free after dropping incremental artifacts", flush=True)
    if free >= floor:
        return "incremental"
    cleaned = subprocess.run(["cargo", "clean"], cwd=ROOT, check=False)
    step = "clean" if cleaned.returncode == 0 else "clean_failed"
    print(f"acceptance disk: {free_bytes(target) / 2**30:.1f} GiB free after cargo clean ({step})", flush=True)
    return step


def run_group(specs):
    rows = ["| Spec | Seconds | Result |", "|---|---:|---|"]
    try:
        for spec in specs:
            ensure_disk()
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

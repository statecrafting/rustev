#!/usr/bin/env python3
"""Run the optimized probe once, collecting child-process peak RSS.

Usage: measure.py ABS_PROBE ABS_MODEL_DIR ABS_RUNTIME_PATH
Run this in a fresh Python process: RUSAGE_CHILDREN must cover only this child.
The platform key of spec 011 3.6 names the output files: macos-aarch64 on
macOS, where ru_maxrss is in bytes; linux-aarch64-gnu or linux-x86_64-gnu on
Linux, where ru_maxrss is in KiB and is converted to bytes.
"""
import hashlib
import json
import platform
from pathlib import Path
import resource
import subprocess
import sys

HERE = Path(__file__).resolve().parent
MACHINE = platform.machine()
if sys.platform == "darwin" and MACHINE == "arm64":
    KEY, RSS_UNIT, UNIT_NOTE = "macos-aarch64", 1, "macOS bytes"
elif sys.platform == "linux" and MACHINE in ("aarch64", "x86_64"):
    KEY, RSS_UNIT, UNIT_NOTE = f"linux-{MACHINE}-gnu", 1024, "Linux KiB times 1024"
else:
    sys.exit(f"no platform key of 3.6 for {sys.platform} {MACHINE}")
probe, model, runtime = map(Path, sys.argv[1:])
assert all(p.is_absolute() for p in (probe, model, runtime))
with (HERE / f"{KEY}.json").open("w") as output, \
        (HERE / f"{KEY}-stderr.txt").open("w") as errors:
    result = subprocess.run([str(probe), str(model), str(runtime), str(HERE / "reference.json")],
                            stdout=output, stderr=errors, check=False)
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
(HERE / f"{KEY}-resource.json").write_text(json.dumps({
    "exit_code": result.returncode, "peak_rss_bytes": usage.ru_maxrss * RSS_UNIT,
    "user_seconds": str(usage.ru_utime), "system_seconds": str(usage.ru_stime),
    "collector": f"resource.getrusage(RUSAGE_CHILDREN); fresh parent, one child; {UNIT_NOTE}",
    "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
}, indent=2) + "\n")
sys.exit(result.returncode)

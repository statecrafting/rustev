#!/usr/bin/env python3
"""Run the optimized probe once, collecting macOS child-process peak RSS.

Usage: measure.py ABS_PROBE ABS_MODEL_DIR ABS_RUNTIME_PATH
Run this in a fresh Python process: RUSAGE_CHILDREN must cover only this child.
"""
import hashlib
import json
from pathlib import Path
import resource
import subprocess
import sys

HERE = Path(__file__).resolve().parent
assert sys.platform == "darwin", "ru_maxrss units below are macOS bytes"
probe, model, runtime = map(Path, sys.argv[1:])
assert all(p.is_absolute() for p in (probe, model, runtime))
with (HERE / "macos-aarch64.json").open("w") as output, \
        (HERE / "macos-aarch64-stderr.txt").open("w") as errors:
    result = subprocess.run([str(probe), str(model), str(runtime), str(HERE / "reference.json")],
                            stdout=output, stderr=errors, check=False)
usage = resource.getrusage(resource.RUSAGE_CHILDREN)
(HERE / "macos-aarch64-resource.json").write_text(json.dumps({
    "exit_code": result.returncode, "peak_rss_bytes": usage.ru_maxrss,
    "user_seconds": str(usage.ru_utime), "system_seconds": str(usage.ru_stime),
    "collector": "resource.getrusage(RUSAGE_CHILDREN); fresh parent, one child; macOS bytes",
    "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
}, indent=2) + "\n")
sys.exit(result.returncode)

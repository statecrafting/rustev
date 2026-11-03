#!/usr/bin/env python3
"""Re-record the 2026-10-22 feasibility record under spec 011 as amended by A-73.

A-73 (2026-11-01) moves the Node binding's `local-embed` build out of stage 3
(3.2.2) into platform qualification (3.2.3, 3.6.3). No measurement is taken
again: the native probe, the graph and the encoder numbers are the ones
measured on 2026-10-22. Stage 3's dependency-graph check is repeated with the
3.18 boundary rules, which did not exist when the record was made.

Usage: rerecord.py BOUNDARIES_COMMIT
Reads and rewrites feasibility.json and feasibility.sha256 beside this file.
Standard library only; the encoding is record.py's canonical encoding.
"""
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA = "rustev.embed-feasibility/1"
PLATFORM = "macos-aarch64"
OLD_DIGEST = "sha256:3a2cf3bb7e785476e7e976f39345fc4fdf05762ab21bb9faaac29dee3827b86a"
NODE_OLD = (
    "Node local-embed native binding not run: feature/backend not implemented; "
    "binding changes outside task territory; native matrix unavailable on this host"
)
STAGE3_INCOMPLETE = "Stage 3 incomplete: Node native binding not run"
NODE_NOW = (
    "Node local-embed build not run: under A-73 it is a condition of platform "
    "qualification (3.2.3, 3.6.3), not of stage 3, and needs the backend of increment 3"
)


def canonical(value):
    if isinstance(value, dict):
        return {k: canonical(v) for k, v in sorted(value.items())}
    if isinstance(value, list):
        return [canonical(v) for v in value]
    return value


def encode(record):
    return json.dumps(canonical(record), ensure_ascii=False, separators=(",", ":")).encode()


def digest(encoded):
    return "sha256:" + hashlib.sha256(SCHEMA.encode() + b"\0" + encoded).hexdigest()


def restage(stages, boundaries_commit):
    linking, perf = stages[2], stages[3]
    assert linking["name"] == "linking" and linking["verdict"] == "refuse"
    assert linking["reason"] == NODE_OLD
    assert perf["name"] == "performance_and_numerics" and perf["verdict"] == "refuse"
    linking["verdict"] = "pass"
    linking["reason"] = (
        "Pass under 3.2.2 as amended by A-73: the native probe passes (the absolute-path, "
        "digest-verified runtime reports 1.31.0; exact tokenizer ids), and the resolved graph "
        "with the pinned ort and tokenizers passes make boundaries with the 3.18 rules at "
        f"{boundaries_commit} with no HTTP family, hf-hub or download feature. The Node "
        "local-embed build is a qualification condition (3.2.3), not part of this stage"
    )
    assert STAGE3_INCOMPLETE in perf["reasons"]
    perf["reasons"] = [r for r in perf["reasons"] if r != STAGE3_INCOMPLETE]
    perf["scope"] = (
        "Encoder measurements of 2026-10-22, taken at the owner's explicit request; stage 4 "
        "refuses on p95 and on the complete-backend items of 3.13 that were not measured"
    )


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: rerecord.py BOUNDARIES_COMMIT")
    boundaries_commit = sys.argv[1]
    path = HERE / "feasibility.json"
    raw = path.read_bytes()
    assert digest(raw.rstrip(b"\n")) == OLD_DIGEST, "not the 2026-10-22 record"
    record = json.loads(raw)

    restage(record["stages"], boundaries_commit)
    restage(record["platforms"][PLATFORM]["stages"], boundaries_commit)

    reasons = record["verdict"][PLATFORM]["reasons"]
    assert reasons[:2] == [NODE_OLD, STAGE3_INCOMPLETE]
    record["verdict"][PLATFORM]["reasons"] = [NODE_NOW] + reasons[2:]
    assert record["verdict"][PLATFORM]["verdict"] == "refuse"

    m = record["measurements"]
    m["node_binding"] = {"status": "not run", "reason": NODE_NOW}
    m["dependency_graph"]["boundaries"] = (
        f"pass with the spec 011 3.18 rules: make boundaries at {boundaries_commit} with these "
        "pins, 2026-11-02; third-party packages unchanged from dependency-tree.txt"
    )

    record["operator_note"] += (
        f" Re-recorded 2026-11-02 under A-73 from {OLD_DIGEST} by rerecord.py, with no new "
        "measurement: stage 3 passes on macos-aarch64, so stages 1 to 3 pass for one platform "
        "(3.2.3); its verdict stays refuse because stage 4 refuses and the Node local-embed "
        "build has not run."
    )

    encoded = encode(record)
    path.write_bytes(encoded + b"\n")
    (HERE / "feasibility.sha256").write_text(digest(encoded) + "\n")
    print(digest(encoded))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Add one Linux platform's measurement to the feasibility record (spec 011 3.2).

Takes the record re-recorded under A-73 and adds what measure.py wrote for
a Linux platform key: the probe output, the resource statistics, stderr and
the host description. Stages 1 to 4 are judged for that platform alone; no
other platform's verdict changes. The probe and measure.py gained Linux
support for this measurement, so the record binds their new bytes and keeps
the digests macos-aarch64 was measured with.

Usage: rerecord_linux.py PLATFORM BOUNDARIES_COMMIT DATE
Reads and rewrites feasibility.json and feasibility.sha256 beside this file.
Standard library only; the encoding is record.py's canonical encoding.
"""
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA = "rustev.embed-feasibility/1"
OLD_DIGEST = "sha256:6a85fb28eba4222e75d1efde7cbd25c1869eda60403d695a6d137c12d09899e5"
LINUX = ("linux-aarch64-gnu", "linux-x86_64-gnu")
NODE = (
    "Node local-embed build not run: under A-73 it is a condition of platform "
    "qualification (3.2.3, 3.6.3), not of stage 3, and needs the backend of increment 3"
)
BACKEND = (
    "No complete backend bundle, heads, Linux-reference logits or support-routing plan "
    "decisions; encoder measurements alone do not satisfy 3.13"
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


def evidence(name):
    data = (HERE / name).read_bytes()
    return {"name": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def limit(value, bound, numeric):
    ok = (value <= bound) if numeric else (float(value) <= float(bound))
    return {"value": value, "limit": bound, "verdict": "pass" if ok else "refuse"}


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in LINUX:
        sys.exit("usage: rerecord_linux.py linux-aarch64-gnu|linux-x86_64-gnu BOUNDARIES_COMMIT DATE")
    key, boundaries_commit, date = sys.argv[1:]
    path = HERE / "feasibility.json"
    raw = path.read_bytes()
    assert digest(raw.rstrip(b"\n")) == OLD_DIGEST, "not the A-73 record"
    record = json.loads(raw)
    assert record["verdict"][key]["reasons"] == ["not measured on this host"]

    probe = json.loads((HERE / f"{key}.json").read_bytes())
    usage = json.loads((HERE / f"{key}-resource.json").read_bytes())
    host = json.loads((HERE / f"{key}-host.json").read_bytes())
    assert usage["exit_code"] == 0, "the probe did not succeed"
    candidate = record["candidate"]
    reference = json.loads((HERE / "reference.json").read_bytes())
    assert probe["graph_sha256"] == reference["graph_sha256"]
    assert probe["tokenizer_source_sha256"] == reference["tokenizer_sha256"]
    assert probe["runtime_reported_version"] == candidate["runtime"]["onnxruntime"]
    assert probe["runtime_sha256"] == host["runtime_library"]["sha256"]
    for name in ("provider", "auto_device", "intra_op_threads", "inter_op_threads",
                 "execution", "graph_optimization"):
        assert probe[name] == record["measurements"]["native_probe"][name], name
    comparisons = probe["comparisons"]
    assert len(comparisons) == 19 and all(c["agrees"] and c["bitwise_repeatable"] for c in comparisons)

    limits = candidate["limits"]
    measured = {
        "cold_setup_ms": limit(probe["cold_setup_ms"], limits["cold_setup_ms"], False),
        "p95_ms": limit(probe["p95_ms"], limits["p95_ms"], False),
        "p99_ms": limit(probe["p99_ms"], limits["p99_ms"], False),
        "peak_rss_bytes": limit(usage["peak_rss_bytes"], limits["peak_rss_bytes"], True),
    }
    exceeded = [f"{n} exceeded: {m['value']} > {m['limit']}"
                for n, m in measured.items() if m["verdict"] == "refuse"]
    refused = [n for n, m in measured.items() if m["verdict"] == "refuse"]

    stage4_reasons = [BACKEND] + exceeded
    record["platforms"][key] = {
        "hardware": host["hardware"],
        "cpu": host["cpu"],
        "cores": host["cores"],
        "memory": host["memory"],
        "os": host["os"],
        "kernel": host["kernel"],
        "c_library": host["c_library"],
        "runtime_library": host["runtime_library"],
        "stages": [
            {"stage": 1, "name": "checkpoint_and_license", "verdict": "pass",
             "reason": "Pinned files downloaded at the recorded revision; every selected file's "
                       "SHA-256 equals the record's; license evidence unchanged"},
            {"stage": 2, "name": "graph_and_operators", "verdict": "pass",
             "reason": "Shipped graph loads with CPU provider 1.31.0 on this platform; all 19 "
                       "inferred fixture embeddings agree with the reference; 2 expected input refusals"},
            {"stage": 3, "name": "linking", "verdict": "pass",
             "reason": "The native probe passes: the official release tarball's library, verified "
                       "against its published digest, is loaded from an absolute path and reports "
                       "1.31.0; exact tokenizer ids. The resolved graph with the pinned ort and "
                       f"tokenizers passes make boundaries with the 3.18 rules at {boundaries_commit}. "
                       "The Node local-embed build is a qualification condition (3.2.3)"},
            {"stage": 4, "name": "performance_and_numerics", "verdict": "refuse",
             "scope": f"Encoder measurements of {date} with the 3.13 protocol of the macos-aarch64 "
                      "measurement; stage 4 refuses on the complete-backend items of 3.13 that were "
                      "not measured" + (" and on " + ", ".join(refused) if refused else ""),
             "reasons": stage4_reasons},
        ],
    }
    record["verdict"][key] = {"verdict": "refuse", "reasons": [NODE] + stage4_reasons}

    m = record["measurements"]
    m.setdefault("platform_measurements", {})[key] = {
        "date": date,
        "measured_encoder_limits": measured,
        "resource_usage": usage,
        "native_probe": {k: v for k, v in probe.items() if k != "warm_request_ms"},
        "host": host["environment"],
    }
    files = m["evidence_files"]
    previous = {f["name"]: f["sha256"] for f in files}
    m["macos_aarch64_measured_with"] = {
        "measure.py": previous["measure.py"],
        "../examples/feasibility.rs": previous["../examples/feasibility.rs"],
        "note": "measure.py and the probe gained Linux support afterwards; these are the bytes "
                "macos-aarch64 was measured with",
    }
    names = [f["name"] for f in files] + [f"{key}.json", f"{key}-resource.json",
                                          f"{key}-stderr.txt", f"{key}-host.json"]
    m["evidence_files"] = [evidence(n) for n in names]

    record["operator_note"] += (
        f" On {date} rerecord_linux.py added {key} from {OLD_DIGEST}: stages 1 to 3 pass there; "
        "stage 4's encoder limits are " + ("met" if not exceeded else "not all met") + ", and "
        "its verdict stays refuse for the unmeasured complete-backend items and the Node "
        "local-embed build. No other platform's verdict changes."
    )

    encoded = encode(record)
    path.write_bytes(encoded + b"\n")
    (HERE / "feasibility.sha256").write_text(digest(encoded) + "\n")
    print(digest(encoded))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Add the native Linux stage 4 measurements of RUN_DIR to the record (spec 011 3.2).

RUN_DIR (beside this file) holds what the measurement workflow
`measure-011.yml` (kept there) uploaded for each Linux platform key, plus a
hand-written `<key>-host.json` summarizing `<key>-host-raw.txt`: the encoder
probe through measure.py, the complete backend through `EmbedLinearBackend`
(`tests/runtime/hand.rs`, `hand_complete_backend`) in a measurement build
whose only difference is the platform table, the support-routing plan
decisions with the synthetic bundle, and the Node binding's `local-embed`
build on the platform's native matrix target. Each platform is judged
against the 3.3 limits as recorded in `candidate.limits`; nothing here
changes a limit. `linux-x86_64-gnu` is the 3.13.2 reference platform, and
every other measured platform's logits are compared with its logits for the
same bundle bytes (the same artifact). A platform qualifies (3.2.3, 3.6.3)
only when all four stages pass and the Node build passed.

Usage: rerecord_stage4.py RUN_DIR DATE BOUNDARIES_COMMIT RUN_URL
Reads and rewrites feasibility.json and feasibility.sha256 beside this file.
Standard library only; the encoding is record.py's canonical encoding.
"""
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA = "rustev.embed-feasibility/1"
OLD_DIGEST = "sha256:5213cc048ef44c8452ae6d03d4b87c6252b0de9db08e47aa2e7a15e6f74cc464"
REFERENCE = "linux-x86_64-gnu"
KEYS = ("linux-x86_64-gnu", "linux-aarch64-gnu")
HARNESS_MEASURED = "a3c8b2116fbe404fec002b257dd666ac93e125742dee29674847466d83d32b64"
PER_PLATFORM = ("host-raw.txt", "host.json", "runtime.txt", "checkpoint.txt", "probe-Cargo.toml",
                "probe-tree.txt", ".json", "-resource.json", "-stderr.txt", "decisions.txt",
                "stage4.json", "stage4-resource.json", "stage4-stdout.txt", "stage4-stderr.txt",
                "stage4-measurement-build.diff", "node-local-embed.txt")


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


def limit(value, bound):
    ok = float(value) <= float(bound)
    return {"value": value, "limit": bound, "verdict": "pass" if ok else "refuse"}


def close(a, b, absolute, relative):
    d = abs(a - b)
    return d <= absolute or d <= relative * max(abs(a), abs(b))


def name(run, key, suffix):
    sep = "" if suffix.startswith((".", "-")) else "-"
    return f"{run}/{key}{sep}{suffix}"


def main():
    if len(sys.argv) != 5:
        sys.exit("usage: rerecord_stage4.py RUN_DIR DATE BOUNDARIES_COMMIT RUN_URL")
    run, date, boundaries_commit, run_url = sys.argv[1:]
    path = HERE / "feasibility.json"
    raw = path.read_bytes()
    assert digest(raw.rstrip(b"\n")) == OLD_DIGEST, "not the 2026-11-02 record"
    record = json.loads(raw)
    candidate = record["candidate"]
    limits = candidate["limits"]
    absolute, relative = float(limits["abs_logit"]), float(limits["rel_logit"])
    margin = float(limits["decision_margin"])
    reference = json.loads((HERE / "reference.json").read_bytes())
    native = record["measurements"]["native_probe"]

    def load(key, suffix):
        return json.loads((HERE / name(run, key, suffix)).read_bytes())

    def text(key, suffix):
        return (HERE / name(run, key, suffix)).read_text()

    stage4 = {k: load(k, "stage4.json") for k in KEYS}
    ref = stage4[REFERENCE]
    assert all(s["artifact"] == ref["artifact"] for s in stage4.values()), "one bundle"
    results = {}
    for key in KEYS:
        probe, usage, host = load(key, ".json"), load(key, "-resource.json"), load(key, "host.json")
        s4, s4_usage = stage4[key], load(key, "stage4-resource.json")
        assert usage["exit_code"] == 0 and s4_usage["exit_code"] == 0
        assert probe["graph_sha256"] == reference["graph_sha256"]
        assert probe["tokenizer_source_sha256"] == reference["tokenizer_sha256"]
        assert probe["runtime_reported_version"] == candidate["runtime"]["onnxruntime"]
        assert probe["runtime_sha256"] == host["runtime_library"]["sha256"] == s4["runtime_sha256"]
        assert s4["platform"] == key
        for n in ("provider", "auto_device", "intra_op_threads", "inter_op_threads",
                  "execution", "graph_optimization"):
            assert probe[n] == native[n], n
        comparisons = probe["comparisons"]
        stage2 = len(comparisons) == 19 and all(c["agrees"] and c["bitwise_repeatable"]
                                                for c in comparisons)
        assert stage2, "stage 2 on this platform"
        assert text(key, "checkpoint.txt").count("\n") == 1 + len(candidate["checkpoint"]["files"])

        encoder = {
            "cold_setup_ms": limit(probe["cold_setup_ms"], limits["cold_setup_ms"]),
            "p95_ms": limit(probe["p95_ms"], limits["p95_ms"]),
            "p99_ms": limit(probe["p99_ms"], limits["p99_ms"]),
            "peak_rss_bytes": limit(usage["peak_rss_bytes"], limits["peak_rss_bytes"]),
        }
        backend = {
            "bundle_bytes": limit(s4["bundle_bytes"], limits["bundle_bytes"]),
            "cold_setup_ms": limit(s4["cold_setup_ms"], limits["cold_setup_ms"]),
            "p95_ms": limit(s4["p95_ms"], limits["p95_ms"]),
            "p99_ms": limit(s4["p99_ms"], limits["p99_ms"]),
            "peak_rss_bytes": limit(s4_usage["peak_rss_bytes"], limits["peak_rss_bytes"]),
        }
        # 3.13.2: logits against the head over the stage 2 reference
        # embedding, run to run in one process, and against the reference
        # platform's logits for the same bundle.
        numerics = s4["fixtures_agree_and_repeatable"] and s4["warm_bitwise_equal_to_first"]
        cross, cross_max, decision_changes = True, 0.0, []
        for mine, theirs in zip(s4["fixtures"], ref["fixtures"]):
            assert mine["text_sha256"] == theirs["text_sha256"]
            a = [float(v) for v in mine["logits"]]
            b = [float(v) for v in theirs["logits"]]
            for x, y in zip(a, b):
                cross_max = max(cross_max, abs(x - y))
                cross &= close(x, y, absolute, relative)
            order_a = sorted(range(len(a)), key=lambda i: (-a[i], i))
            order_b = sorted(range(len(b)), key=lambda i: (-b[i], i))
            if order_a != order_b:
                gap = min(abs(b[i] - b[j]) for i in range(len(b)) for j in range(i + 1, len(b)))
                decision_changes.append({"text_sha256": mine["text_sha256"],
                                         "deciding_gap": repr(gap),
                                         "within_margin": gap <= margin})
        decisions_ok = all(c["within_margin"] for c in decision_changes)
        log = text(key, "decisions.txt")
        plan_decisions = "test result: ok." in log and " 0 failed" in log
        node_log = text(key, "node-local-embed.txt")
        node = "conformance native: 1 case(s), 0 failure(s)" in node_log and "# fail 0" in node_log
        assert node_log.count("Finished `release` profile") >= 1

        measured = {f"encoder_{k}": v for k, v in encoder.items()}
        measured.update({f"backend_{k}": v for k, v in backend.items()})
        refused = [f"{n} exceeded: {m['value']} > {m['limit']}"
                   for n, m in measured.items() if m["verdict"] == "refuse"]
        if not numerics:
            refused.append("complete backend logits outside the 3.3 bounds of the reference "
                           "head or not bitwise repeatable in process")
        if not cross:
            refused.append(f"logits differ from {REFERENCE} beyond the 3.3 bounds")
        if not decisions_ok:
            refused.append("a decision changed outside decision_margin")
        if not plan_decisions:
            refused.append("support-routing plan decisions with the synthetic bundle changed")
        stage4_pass = not refused
        results[key] = (stage4_pass, node, refused)

        scope = (f"Measured on {date} on a GitHub-hosted {host['runner']} runner ({run_url}): "
                 "the encoder probe with the 3.13 protocol, and the complete backend through "
                 "EmbedLinearBackend::open and DecisionBackend::infer on an onnx bundle of the "
                 "pinned graph and tokenizer with one generated head, built with this platform "
                 "in the qualified table (the measurement build's only difference); one process, "
                 "one worker, one request at a time, 256-token inputs, 50 warm-ups, 1,000 timed "
                 "requests, CPU provider, one intra-op and one inter-op thread; the "
                 "support-routing plans with the synthetic bundle; logits against "
                 + ("the stage 2 reference (this is the 3.13.2 reference platform)"
                    if key == REFERENCE else f"the stage 2 reference and {REFERENCE}"))
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
                 "reason": "Pinned files downloaded at the recorded revision; every selected "
                           "file's length and SHA-256 equals the record's; license evidence "
                           "unchanged"},
                {"stage": 2, "name": "graph_and_operators", "verdict": "pass",
                 "reason": "Shipped graph loads with CPU provider 1.31.0 on this platform; all "
                           "19 inferred fixture embeddings agree with the reference and are "
                           "bitwise repeatable in process; 2 expected input refusals"},
                {"stage": 3, "name": "linking", "verdict": "pass",
                 "reason": "The native probe passes: the official release tarball's library, "
                           "verified against its published digest, is loaded from an absolute "
                           "path and reports 1.31.0; exact tokenizer ids. The resolved graph "
                           "with the pinned ort and tokenizers passes make boundaries with the "
                           f"3.18 rules at {boundaries_commit}"},
                {"stage": 4, "name": "performance_and_numerics",
                 "verdict": "pass" if stage4_pass else "refuse",
                 "scope": scope, "reasons": refused or ["every 3.3 limit and 3.13 item met"]},
            ],
        }
        reasons = list(refused)
        if not node:
            reasons.append("Node local-embed build on the native matrix target did not pass")
        record["verdict"][key] = (
            {"verdict": "pass", "reasons": [
                "Stages 1 to 4 pass and the Node binding builds with local-embed on the "
                "native matrix target and runs the local-embed case (3.2.3, 3.6.3)"]}
            if stage4_pass and node else {"verdict": "refuse", "reasons": reasons})

        m = record["measurements"]["platform_measurements"]
        if key in m:
            record["measurements"].setdefault("superseded_platform_measurements", {})[key] = {
                "measurement": m[key],
                "note": f"Superseded on {date} by the native measurement; its evidence files "
                        "stay bound",
            }
        m[key] = {
            "date": date,
            "run": run_url,
            "host": host["environment"],
            "measured_encoder_limits": encoder,
            "measured_backend_limits": backend,
            "resource_usage": usage,
            "backend_resource_usage": s4_usage,
            "native_probe": {k: v for k, v in probe.items() if k != "warm_request_ms"},
            "complete_backend": {k: v for k, v in s4.items()
                                 if k not in ("warm_request_ms", "fixtures")},
            "logits_vs_reference_head": {
                "within_bounds_and_bitwise_repeatable": s4["fixtures_agree_and_repeatable"],
                "max_abs_difference": s4["max_abs_logit_difference_from_reference_head"]},
            "logits_vs_reference_platform": {
                "reference_platform": REFERENCE, "within_bounds": cross,
                "max_abs_difference": repr(cross_max), "decision_changes": decision_changes},
            "bitwise_in_process_repeatability": numerics,
            "support_routing_plan_decisions": "unchanged: the synthetic support-routing cases "
                "and the local-embed conformance case equal the committed bytes"
                if plan_decisions else "changed",
            "node_local_embed": "pass" if node else "fail",
        }

    qualified = sorted(k for k in KEYS if record["verdict"][k]["verdict"] == "pass")
    if any(results[k][0] for k in KEYS):
        record["stages"][3] = {
            "stage": 4, "name": "performance_and_numerics", "verdict": "pass",
            "scope": f"Per platform: passes on {', '.join(k for k in KEYS if results[k][0])} "
                     f"({date}); every other platform refuses (see platforms)",
            "reasons": ["Stage 4 is judged per platform; see platforms"],
        }
    files = record["measurements"]["evidence_files"]
    previous = {f["name"]: f["sha256"] for f in files}
    record["measurements"]["linux_aarch64_gnu_measured_with"] = {
        "../examples/feasibility.rs": previous["../examples/feasibility.rs"],
        "note": "the probe gained the linux-x86_64-gnu runtime digest afterwards; these are the "
                "bytes linux-aarch64-gnu was measured with on 2026-11-02",
    }
    record["measurements"]["stage4_harness_measured_with"] = {
        "../tests/runtime/hand.rs": HARNESS_MEASURED,
        "note": "hand_complete_backend as run by the measurement workflow; the bound bytes add "
                "only the qualified-platform branch of hand_real_encoder's last check",
    }
    names = [f["name"] for f in files]
    names += [f"{run}/measure-011.yml", "rerecord_stage4.py", "../tests/runtime/hand.rs"]
    names += [name(run, k, s) for k in KEYS for s in PER_PLATFORM]
    record["measurements"]["evidence_files"] = [evidence(n) for n in names]
    record["date"] = date
    record["operator_note"] += (
        f" On {date} rerecord_stage4.py added the native Linux measurements of {run_url} "
        f"from {OLD_DIGEST}: " + "; ".join(
            f"{k} stage 4 {'pass' if results[k][0] else 'refuse'}, Node local-embed "
            f"{'pass' if results[k][1] else 'fail'}" for k in KEYS)
        + f". Qualified: {', '.join(qualified) or 'none'}. No limit changed.")

    encoded = encode(record)
    path.write_bytes(encoded + b"\n")
    (HERE / "feasibility.sha256").write_text(digest(encoded) + "\n")
    print(digest(encoded), "qualified:", qualified)
    for k in KEYS:
        print(k, results[k])


if __name__ == "__main__":
    main()

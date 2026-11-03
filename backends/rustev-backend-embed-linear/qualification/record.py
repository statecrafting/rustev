#!/usr/bin/env python3
"""Assemble the offline gate record after running reference.py and the probe.

Usage: record.py ABS_MODEL_DIRECTORY ABS_RUNTIME_PATH ABS_BASE_CARDS_DIRECTORY
No downloads, inference or model/runtime redistribution.
"""
import ctypes
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
SCHEMA = "rustev.embed-feasibility/1"
FIELDS = {"schema", "candidate", "stages", "platforms", "measurements", "verdict",
          "date", "operator_note"}



def file_record(path, name):
    data = path.read_bytes()
    return {"name": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def canonical(value):
    if isinstance(value, dict):
        return {k: canonical(v) for k, v in sorted(value.items())}
    if isinstance(value, list):
        return [canonical(v) for v in value]
    if value is None or isinstance(value, float):
        raise ValueError("identity-bearing records use explicit absence and decimal strings")
    return value


def validate(record):
    # deny_unknown_fields at the record boundary. Nested member shapes are
    # descriptive: spec 3.2.1 defines their contents, not named Rust types.
    assert set(record) == FIELDS
    assert record["schema"] == SCHEMA
    assert len(record["stages"]) == 4
    assert set(record["platforms"]) == {
        "linux-x86_64-gnu", "linux-aarch64-gnu", "macos-aarch64"}
    for value in record["verdict"].values():
        assert value["verdict"] in ("pass", "refuse")
    canonical(record)


def main():
    model, runtime, cards = map(Path, sys.argv[1:])
    assert model.is_absolute() and runtime.is_absolute() and cards.is_absolute()
    files = [file_record(p, p.relative_to(model).as_posix())
             for p in sorted(model.rglob("*")) if p.is_file()]
    graph = next(f for f in files if f["name"] == "onnx/model.onnx")
    assert graph["sha256"] == "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452"
    weights = next(f for f in files if f["name"] == "pytorch_model.bin")
    assert weights["sha256"] == "c3a85f238711653950f6a79ece63eb0ea93d76f6a6284be04019c53733baf256"
    tree = (HERE / "dependency-tree.txt").read_text()
    forbidden_names = {"hyper", "reqwest", "axum", "actix-web", "warp", "tonic", "ureq",
                       "isahc", "surf", "h2", "http", "hyper-util", "http-body",
                       "http-body-util", "hf-hub"}
    forbidden_features = {"download-binaries", "copy-dylibs", "fetch-models", "http"}
    package_matches = forbidden_names.intersection(re.findall(r'([a-z0-9_-]+) v[0-9]', tree))
    feature_matches = forbidden_features.intersection(re.findall(r'feature "([^"]+)"', tree))
    hardware = json.loads(subprocess.check_output(
        ["/usr/sbin/system_profiler", "SPHardwareDataType", "-json"], text=True))
    hardware = hardware["SPHardwareDataType"][0]
    reference = json.loads((HERE / "reference.json").read_text(), parse_float=str)
    probe = json.loads((HERE / "macos-aarch64.json").read_text(), parse_float=str)
    usage = json.loads((HERE / "macos-aarch64-resource.json").read_text())
    assert usage["exit_code"] == 0
    rss = usage["peak_rss_bytes"]
    assert rss > 0
    assert reference["verdict"] == "pass"
    assert reference["weights_sha256"] == weights["sha256"]
    assert len(probe["warm_request_ms"]) == probe["requests"] == 1000
    assert probe["warm_up"] == 50
    samples = sorted(map(float, probe["warm_request_ms"]))
    assert samples[949] == float(probe["p95_ms"])
    assert samples[989] == float(probe["p99_ms"])
    assert probe["runtime_reported_version"] == "1.31.0"
    assert not package_matches and not feature_matches
    assert all(c["agrees"] for c in probe["comparisons"])
    assert probe["graph_sha256"] == graph["sha256"]
    assert probe["runtime_sha256"] == file_record(runtime, runtime.name)["sha256"]
    assert probe["tokenizer_source_sha256"] == reference["tokenizer_sha256"]
    base = file_record(cards / "README.md", "base-model/README.md")
    upstream = file_record(cards / "upstream-README.md", "base-model/upstream-README.md")
    assert base["sha256"] == "4c0efe99b6adc8fac8ed3af25f7fcb81ce0e832bd4543158c39bdb6c65d2f63e"
    assert upstream["sha256"] == "57d6772ba7a25afe078fc96042c029779100ba2ab483f95ca6193255ee1d9379"
    node_reason = "Node local-embed native binding not run: feature/backend not implemented; binding changes outside task territory; native matrix unavailable on this host"
    perf_reasons = ["Stage 3 incomplete: Node native binding not run",
                    "No complete backend bundle, heads, Linux-reference logits or support-routing plan decisions; encoder measurements alone do not satisfy 3.13"]
    measured_limits = {}
    for name, value, limit in [("cold_setup_ms", float(probe["cold_setup_ms"]), 2000),
                               ("peak_rss_bytes", rss, 536870912),
                               ("p95_ms", float(probe["p95_ms"]), 100),
                               ("p99_ms", float(probe["p99_ms"]), 200)]:
        measured_limits[name] = {"value": str(value) if isinstance(value, float) else value,
                                 "limit": str(limit) if isinstance(value, float) else limit,
                                 "verdict": "pass" if value <= limit else "refuse"}
        if value > limit:
            perf_reasons.append(f"{name} exceeded: {value} > {limit}")
    stages = [
        {"stage": 1, "name": "checkpoint_and_license", "verdict": "pass",
         "reason": "Pinned local files verified; Apache-2.0 checkpoint over MIT base cards permits host-side use without redistribution"},
        {"stage": 2, "name": "graph_and_operators", "verdict": "pass",
         "reason": "Shipped graph loads with CPU provider 1.31.0; all 19 inferred fixture embeddings agree; 2 expected input refusals"},
        {"stage": 3, "name": "linking", "verdict": "refuse", "reason": node_reason,
         "native_probe": "pass: absolute-path verified runtime reports 1.31.0; exact tokenizer ids; dependency graph and current boundaries pass"},
        {"stage": 4, "name": "performance_and_numerics", "verdict": "refuse",
         "reasons": perf_reasons,
         "scope": "Supplemental encoder measurements at owner's explicit request, not a stage-4 qualification after stage-3 refusal"},
    ]
    unmeasured = [{"stage": n, "verdict": "refuse", "reason": "not measured on this host"}
                  for n in range(1, 5)]
    libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
    libc.NSVersionOfRunTimeLibrary.argtypes = [ctypes.c_char_p]
    libc.NSVersionOfRunTimeLibrary.restype = ctypes.c_int
    libc_version = libc.NSVersionOfRunTimeLibrary(b"System")
    assert libc_version >= 0
    libc_version = f"{libc_version >> 16}.{(libc_version >> 8) & 255}.{libc_version & 255}"
    platforms = {
        "macos-aarch64": {
            "hardware": hardware["machine_model"], "cpu": hardware["chip_type"],
            "cores": hardware["number_processors"], "memory": hardware["physical_memory"],
            "os": subprocess.check_output(["sw_vers", "-productVersion"], text=True).strip(),
            "os_build": subprocess.check_output(["sw_vers", "-buildVersion"], text=True).strip(),
            "kernel": platform.release(),
            "c_library": "Darwin libSystem " + libc_version + " (NSVersionOfRunTimeLibrary)",
            "stages": stages,
        },
    }
    for key in ("linux-x86_64-gnu", "linux-aarch64-gnu"):
        platforms[key] = {"hardware": "not measured", "os": "not measured",
                          "c_library": "not measured", "cpu": "not measured",
                          "cores": "not measured", "memory": "not measured", "stages": unmeasured}
    record = {
        "schema": SCHEMA,
        "candidate": {
            "checkpoint": {"repository": "sentence-transformers/all-MiniLM-L6-v2",
                           "revision": "1110a243fdf4706b3f48f1d95db1a4f5529b4d41",
                           "revision_evidence": "owner-supplied pinned download; graph and weights match spec Fact 1",
                           "files": files, "license": "apache-2.0 (README.md metadata)",
                           "base_model": {"repository": "nreimers/MiniLM-L6-H384-uncased",
                                          "revision": "3276f0fac9d818781d7a1327b3ff818fc4e643c0",
                                          "license": "mit", "card": base,
                                          "upstream": {"repository": "microsoft/MiniLM-L12-H384-uncased",
                                                       "revision": "44acabbec0ef496f6dbc93adadea57f376b7c0ec",
                                                       "license": "mit", "card": upstream},
                                          "evidence": "Owner fetched cards/API metadata on 2026-10-22; local card SHA-256 reverified"},
                           "training_data_description": "Checkpoint card: contrastive fine-tuning on 1,170,060,424 sentence pairs from Reddit, S2ORC, WikiAnswers, PAQ, Stack Exchange, MS MARCO and other listed datasets. Base card: 6 layers selected from Microsoft MiniLM-L12-H384-uncased; upstream card: distilled from in-house UniLMv2 BERT-base. Cards do not enumerate the full base pretraining dataset. No quality claim."},
            "graph": "onnx/model.onnx, shipped f32; no export or conversion",
            "runtime": {"ort": "2.0.0-rc.15", "onnxruntime": "1.31.0",
                        "library": file_record(runtime, runtime.name),
                        "release_tarball_sha256_owner_supplied": "1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc",
                        "provider": "cpu", "graph_optimization": "all",
                        "intra_op_threads": 1, "inter_op_threads": 1},
            "tokenizer": {"implementation": "tokenizers@1.0.0-rc.3", "max_tokens": 256,
                          "truncation": "refuse"},
            "pooling": {"mode": "mean_masked", "normalize": "l2"},
            "redistribution": "none", "heads": "none selected: backend implementation deferred",
            "limits": {"bundle_bytes": 157286400, "cold_setup_ms": "2000",
                       "peak_rss_bytes": 536870912, "p95_ms": "100", "p99_ms": "200",
                       "abs_logit": "0.00001", "rel_logit": "0.0001", "decision_margin": "0.001"},
        },
        "stages": stages, "platforms": platforms,
        "measurements": {
            "selected_file_bytes_including_reference_weights": sum(f["bytes"] for f in files),
            "bundle_bytes": "not measured: no manifest or heads; reference weights are not runtime bundle files",
            "graph_plus_tokenizer_bytes": graph["bytes"] + (model / "tokenizer.json").stat().st_size,
            "reference": reference,
            "native_probe": probe,
            "probe_peak_rss_bytes": rss,
            "resource_usage": usage,
            "earlier_attempt": "Unmeasured default-provider attempt failed during CoreML compilation; corrected to explicit CPU before measurements. /usr/bin/time -l could not read sysctl in sandbox; resource.getrusage used instead.",
            "peak_rss_scope": "macOS resource.getrusage child process high-water mark: setup, fixture comparisons, 50 warm-ups, 1000 requests and JSON output; includes file buffers, not Python",
            "measured_encoder_limits": measured_limits,
            "evidence_files": [file_record(HERE / name, name) for name in
                               ("reference.py", "reference.json", "macos-aarch64.json", "macos-aarch64-resource.json", "macos-aarch64-stderr.txt", "measure.py", "../examples/feasibility.rs")],
            "runtime_reported_version": probe["runtime_reported_version"],
            "dependency_graph": {"file": "dependency-tree.txt", "edges": "normal,build,features",
                                 "sha256": hashlib.sha256((HERE / "dependency-tree.txt").read_bytes()).hexdigest(),
                                 "forbidden_package_or_feature_matches": sorted(package_matches | feature_matches),
                                 "boundaries": "pass for existing rules; spec 3.18 extension outside territory and not implemented"},
            "node_binding": {"status": "not run", "reason": node_reason},
            "bitwise_in_process_repeatability": all(c["bitwise_repeatable"] for c in probe["comparisons"]),
            "cross_platform_logits": "not measured: Linux reference unavailable; no head selected",
            "support_routing_plan_decisions": "not run: backend and synthetic bundle not implemented",
        },
        "verdict": {key: {"verdict": "refuse", "reasons": [node_reason] + perf_reasons if key == "macos-aarch64"
                                                             else ["not measured on this host"]}
                    for key in platforms},
        "date": "2026-10-22",
        "operator_note": "Owner authorized gate and supplied base-license evidence on 2026-10-22. Offline inputs only. Stages 1 and 2 pass. Native stage-3 probe passes but Node qualification is not run, so stage 3 refuses. Per explicit owner request, supplemental encoder performance measurements were collected; stage 4 remains refused under 3.2 and 3.13. No platform is qualified or inference dependency merge eligibility established. Source tokenizer bytes are hashed then upgraded in memory by the pinned crate's canonicalize_str reader, with exact ids checked against reference; the graph is unchanged. No backend is implemented. External feasibility.sha256 identifies this record.",
    }
    validate(record)
    encoded = json.dumps(canonical(record), ensure_ascii=False, separators=(",", ":")).encode()
    (HERE / "feasibility.json").write_bytes(encoded + b"\n")
    digest = hashlib.sha256(SCHEMA.encode() + b"\0" + encoded).hexdigest()
    (HERE / "feasibility.sha256").write_text("sha256:" + digest + "\n")


if __name__ == "__main__":
    main()

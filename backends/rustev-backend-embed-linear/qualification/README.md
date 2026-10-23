# Spec 011 feasibility measurement, 2026-10-22

`feasibility.json` is the authoritative `rustev.embed-feasibility/1` record.
All three platforms have verdict `refuse`. Both Linux platforms say
`not measured on this host`. No backend implementation or platform qualification
is claimed. No downloaded model, tokenizer, head or runtime bytes are included.

| Stage on macos-aarch64 | Result |
|---|---|
| 1: checkpoint and license | Pass. Selected file hashes, Apache-2.0 checkpoint card and both MIT base-model cards are recorded. |
| 2: graph and operators | Pass. Shipped f32 graph, standard ONNX domain opset 14, CPU provider 1.31.0. All 19 inferred fixtures agree with Sentence Transformers; two input refusals are recorded. |
| 3: linking | Refuse. Native probe passes, but the Node local-embed check was not run. |
| 4: performance and numerics | Refuse. Encoder p95 exceeds 100 ms; complete bundle, Linux logits and package-plan comparisons are unavailable. |

The owner supplied the base cards/API evidence fetched on 2026-10-22, including
pinned revisions. Both card hashes were reverified locally. The checkpoint card
records contrastive fine-tuning on 1,170,060,424 sentence pairs. The base cards
describe layer selection and distillation, but do not enumerate the full base
pretraining dataset. This evidence permits host-side use without redistribution.

## Measurements

Apple M1 Max, MacBookPro18,2, 10 cores (8 performance, 2 efficiency), 64 GB;
macOS 26.5.1 build 25F80; Darwin libSystem 1356.0.0. CPU only, sequential
execution, intra-op 1, inter-op 1, all graph optimizations, release build.

| Encoder probe measurement | Measured | Limit | Result |
|---|---:|---:|---|
| Cold setup | 727.9165 ms | 2,000 ms | Pass for measured scope |
| Process peak RSS | 361,512,960 bytes (344.8 MiB) | 536,870,912 bytes | Pass for measured scope |
| Warm p95, nearest rank | 105.372917 ms | 100 ms | Refuse |
| Warm p99, nearest rank | 111.628458 ms | 200 ms | Pass for measured scope |

One process, one request at a time, 256-token inputs, 50 encoder warm-ups,
1,000 timed requests including tokenization, inference, pooling and normalization.
Every request duration is retained. Setup includes file reading/hashing, tokenizer
parsing and its in-memory format upgrade, runtime loading/version check, and
session creation. It excludes parsing the probe's reference-result file and
complete backend setup. RSS is the process high-water mark across setup,
fixture comparisons, warm-up, timed requests and JSON output, collected by a
fresh Python parent's `resource.getrusage(RUSAGE_CHILDREN)` on macOS. It excludes
the parent Python process. File buffers remain included. This was one successful
CPU measurement run; cold means a fresh process, not flushed OS disk caches.

All 21 fixture token-id sequences match exactly. Nineteen inputs are inferred;
whitespace and 257-token inputs are refused before inference. The maximum
PyTorch/Python-ONNX component difference is 0.0000002682209014892578; the
maximum PyTorch/Rust-ONNX difference is 0.00000028312206268310547. Each inferred
fixture's two in-process embeddings are bitwise equal. Cosines and component
differences are recorded per fixture. This does not qualify a backend cache.

The pinned tokenizer reader rejects legacy JSON directly. Its bundled
`canonicalize_str` upgrades the verified source in memory before `from_json`;
source and in-memory hashes are recorded. The source file and graph are
unchanged. No handwritten tokenizer or dependency version change is used.
The loaded dylib reports `1.31.0` through `OrtGetApiBase/GetVersionString` and
its SHA-256 is `2a50a63e7f41f8d4d551d3ee47248df3864f6540109fce8ad520ca3b51af4b70`.

An earlier attempt accidentally selected ort's default CoreML provider and failed
during compilation. The measured probe explicitly selects `PreferCPU`, preventing
automatic accelerator registration. The earlier failure is retained in
`macos-aarch64-time.txt`. That collector also failed to read sandboxed sysctl;
RSS for the successful run comes from `macos-aarch64-resource.json`.
Successful runtime cache-directory diagnostics are retained verbatim in stderr.

## Reproduction and evidence

- `reference.py ABS_MODEL_DIR OUTPUT_JSON`: use the supplied Python venv.
  It sets HF_HOME, TORCH_HOME and XDG_CACHE_HOME under this worktree's target,
  sets offline flags, and loads local PyTorch weights only. The 16 support-routing
  tickets plus whitespace, Unicode, punctuation and 256/257-token cases are used.
- Build the probe with `cargo build -p rustev-backend-embed-linear --locked --release --example feasibility`.
- `measure.py ABS_PROBE ABS_MODEL_DIR ABS_RUNTIME_PATH`: run in a fresh macOS
  Python process. It writes the probe JSON, resource statistics and stderr here.
  Obtain the binary path from Cargo's shared target configuration; do not override it.
- `record.py ABS_MODEL_DIR ABS_RUNTIME_PATH ABS_BASE_CARDS_DIR`: standard-library
  Python assembles the record from those results, checks digests and percentiles,
  rejects unknown top-level fields and writes the external tagged digest.
- `cargo run -p rustev-backend-embed-linear --locked --example validate_feasibility -- backends/rustev-backend-embed-linear/qualification/feasibility.json backends/rustev-backend-embed-linear/qualification/feasibility.sha256`
  validates a serde `deny_unknown_fields` envelope and the repository contract's
  canonical bytes and digest. Identity is SHA-256(schema, zero byte, canonical
  JSON), without the final newline. Decimal measurements are strings.

`dependency-tree.txt` contains locked normal, build and feature edges. No HTTP
family, hf-hub or download feature is present. Current `make boundaries` rules
pass; the spec 3.18 extension remains outside territory and unimplemented.
The Node local-embed feature/backend and native matrix are unavailable here.
Stage 3 therefore refuses. At the owner's explicit request, supplemental encoder
performance was measured, but stage 4 does not bypass the gate's stage ordering.
A full bundle size, linear heads, Linux-reference logits and support-routing
plan decisions remain unmeasured. No thresholds have been relaxed.

## Requested checks

Build examples, clippy with warnings denied, workspace formatting and current
boundaries pass on this host. The record validator also passes. These checks
establish neither full spec 011 acceptance nor inference-dependency merge eligibility.

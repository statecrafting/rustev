# Spec 011 feasibility measurement, 2026-10-22

**Re-recorded 2026-11-02 under A-73** by `rerecord.py` from
`sha256:3a2cf3bb7e785476e7e976f39345fc4fdf05762ab21bb9faaac29dee3827b86a`,
with no new measurement. A-73 moved the Node binding's `local-embed` build
out of stage 3 into platform qualification (3.2.3, 3.6.3), and stage 3's
dependency-graph check was repeated with the 3.18 boundary rules. Stage 3
now passes on macos-aarch64, so stages 1 to 3 pass for one platform and the
inference dependencies may enter (3.2.3). The platform's verdict stays
`refuse`: stage 4 refuses on p95 and the Node build has not run.
`examples/feasibility.rs` is kept byte-identical as measured; it uses
`dlopen` and is not built under the workspace's `unsafe_code = "forbid"`.

`feasibility.json` is the authoritative `rustev.embed-feasibility/1` record.
All three platforms have verdict `refuse`. Both Linux platforms say
`not measured on this host`. No backend implementation or platform qualification
is claimed. No downloaded model, tokenizer, head or runtime bytes are included.

| Stage on macos-aarch64 | Result |
|---|---|
| 1: checkpoint and license | Pass. Selected file hashes, Apache-2.0 checkpoint card and both MIT base-model cards are recorded. |
| 2: graph and operators | Pass. Shipped f32 graph, standard ONNX domain opset 14, CPU provider 1.31.0. All 19 inferred fixtures agree with Sentence Transformers; two input refusals are recorded. |
| 3: linking | Pass (re-recorded 2026-11-02 under A-73). Native probe passes; the resolved graph with the pinned `ort` and `tokenizers` passes `make boundaries` with the 3.18 rules. The Node local-embed build is now a qualification condition, not part of this stage. |
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
- `cargo test -p rustev-backend-embed-linear --locked --test feasibility_record`
  checks the closed `deny_unknown_fields` envelope, the contract's canonical
  encoding, the external tagged digest and every evidence file's length and
  SHA-256. `make code` runs it.  Identity is SHA-256(schema, zero byte, canonical JSON), without the final
  newline. Decimal measurements are strings.
- `rerecord.py BOUNDARIES_COMMIT` applies the 2026-11-02 re-recording under
  A-73 to the 2026-10-22 record and rewrites the digest.
- The probe example is kept byte-identical as measured and is not built
  (`autoexamples = false`): it reads the runtime version through `dlopen`,
  which the workspace's `unsafe_code = "forbid"` refuses. To rebuild it,
  copy it into a crate without the workspace lints.

`dependency-tree.txt` contains locked normal, build and feature edges. No HTTP
family, hf-hub or download feature is present. On 2026-10-22 the then-current
`make boundaries` rules passed and the 3.18 extension did not exist. On
2026-11-02 `make boundaries`, with the 3.18 rules, passed on `main` at
`3dc1e1ce` with these pins; the third-party packages are unchanged from this
tree. The Node local-embed build has not run: under A-73 it is a condition of
platform qualification, not of stage 3. At the owner's explicit request,
encoder performance was measured; stage 4 refuses on p95. A full bundle size,
linear heads, Linux-reference logits and support-routing plan decisions remain
unmeasured. No thresholds have been relaxed.

## Checks

Clippy with warnings denied, workspace formatting, `make boundaries` and the
record test pass. They establish neither full spec 011 acceptance nor any
platform's qualification.

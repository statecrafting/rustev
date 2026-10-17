---
id: "011-semantic-backend"
title: "Local semantic backend: frozen embeddings and a linear head"
status: approved
implementation: pending
created: "2026-09-23"
summary: >
  The first local model backend, `backends/rustev-backend-embed-linear/`:
  an ordinary `DecisionBackend` that embeds projected text with a frozen,
  host-provisioned sentence encoder run by ONNX Runtime through `ort`, and
  answers `classify`, `proposition` and `rubric` with logits from an
  externally produced, identified task-specific linear head. One canonical
  `rustev.embed-linear/1` manifest binds checkpoint, graph, tokenizer,
  preprocessing, truncation, pooling, precision, runtime, heads, labels,
  cost and tolerance into the artifact identity. Implementation is gated by
  a staged feasibility record (`rustev.embed-feasibility/1`) that passes or
  refuses each selection of #69 against measured limits before any
  inference dependency lands. No network, no download, no training, no
  provider; repeatability is a declared tolerance, not bitwise. Optional
  `local-embed` features in the Node binding, the service and the facade,
  and an opt-in in-process embedding cache. Amends spec 001 only by adding
  a local inference runtime family and one placement rule to the boundary
  check. Ratified (A-28) under R-36, which amends R-17 for this spec;
  R-43 authorizes the feasibility stages and pins the revision that ships
  an ONNX graph; implementation is pending.
amends:
  - "001-boundaries-and-authority"
establishes:
  - { kind: directory, path: "backends/rustev-backend-embed-linear/", planned: true }
extends:
  # The crate as a workspace member and its pinned dependencies (2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 011 to `make verify` once implemented.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # The local inference runtime family and placement rule (3.18).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "007-support-routing-package"
  - "010-integrations"
  - "017-general-runtime-optimization"
  - "023-real-data-quality-qualification"
  - "027-node-and-wasm-bindings"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "The backend never grants authority: it supplies logits or a classified failure only, every supplied value is model-derived, and no output, embedding, cache entry or setup result is authorization or an input fact."
    anchor: "3-1-position-authority-and-provenance"
  - id: "I-2"
    kind: invariant
    text: "Every behavior-affecting input (checkpoint, graph, tokenizer, preprocessing, truncation, pooling, precision, runtime version and settings, operators, heads, labels, text templates, limits, cost and tolerance) is in the `rustev.embed-linear/1` manifest, whose digest is the artifact identity; a mutable name, alias or path is never an identity."
    anchor: "3-5-identity"
  - id: "I-3"
    kind: invariant
    text: "The crate performs no network access and no download at build, setup or inference; it reads only host-supplied absolute paths, verifies every byte it uses against the manifest before use, and refuses a missing, oversized or mismatched artifact with a typed code."
    anchor: "3-7-setup-and-refusals"
  - id: "I-4"
    kind: invariant
    text: "The descriptor declares exactly the operations and options of the bundled heads, output `logits` only and determinism `tolerance`; no `rank`, no label outside a head's trained options, no distribution or calibration claim."
    anchor: "3-10-capabilities-and-the-descriptor"
  - id: "I-5"
    kind: invariant
    text: "Cancellation is acknowledged `stopped` only when the backend has established that the run ended; otherwise `unconfirmed`, and the worker slot stays occupied until the run ends."
    anchor: "3-12-deadlines-and-cancellation"
  - id: "I-6"
    kind: invariant
    text: "Repeatability is declared as a tolerance measured under recorded conditions; no bitwise or cross-platform claim is made beyond what the feasibility record measured."
    anchor: "3-13-numerical-repeatability"
  - id: "R-1"
    kind: requirement
    text: "No inference dependency enters the workspace until the feasibility record passes stages 1 to 3 for at least one platform; a platform is supported only when all four stages pass for it, and a refusal is recorded, never hidden."
    anchor: "3-2-the-feasibility-gate"
  - id: "R-2"
    kind: requirement
    text: "An embedding cache hit supplies exactly the bits a miss computed in the same process for the same artifact, principal handle and token sequence, never crosses a principal handle, and is refused on a platform without measured in-process bitwise repeatability."
    anchor: "3-14-embedding-cache"
  - id: "R-3"
    kind: requirement
    text: "Builds of the Node binding, the service and the facade without `local-embed` contain no crate of the local inference runtime family, and the boundary check refuses that family outside the backend crate except through an optional dependency on it."
    anchor: "3-18-boundary-check-extension"
---

# 011: Local semantic backend: frozen embeddings and a linear head

Ratified by the owner (A-28, under the 2026-10-16 delivery work order and
R-36) as its own reviewable change before any code (R-16); implementation
is pending. It implements #76 child C4 (#69) and backlog items RUSTEV-001
to RUSTEV-003. R-36 (OD-1 of #76) amends R-17 for this spec: R-17's
prohibition of an `ort` dependency and of semantic inference is lifted for
spec 011 only, within this contract and its feasibility gate (3.2). R-01's
direction stands and is made concrete here; R-13's split from spec 005 and
R-25's condition (the spec 006 workflow is usable; spec 006 is complete)
are both satisfied. R-43 (2026-10-17) authorizes acquisition and local
inference for the feasibility record (Decision 18) and moves the pinned
revision of #69's checkpoint to one that ships an ONNX graph (Decision 2).
Approval makes no quality, calibration, usefulness, release or
publication claim.

## 1. Purpose

Hosts need a no-network, low-latency, fixed-cost backend for high-volume
closed-set questions (classification, proposition checks, rubric levels),
and backend-swap comparisons (spec 004, #76 E2) need a model backend that
does not depend on a provider. This spec adds one: a frozen sentence
encoder plus a task-specific linear head per question, run in process,
that:

1. discloses exactly what it returns (logits over a head's trained
   options), what it costs (local quota units) and how repeatable it is
   (a measured tolerance);
2. binds every behavior-affecting input into one artifact identity, so a
   plan names exactly the model, preprocessing and head it was compiled
   against;
3. uses only host-provisioned, digest-verified local artifacts and a
   host-provisioned ONNX Runtime library, with no download at any time;
4. is admitted only after a staged feasibility record passes the measured
   limits #69 sets, platform by platform; and
5. is interchangeable with the rules backend (spec 005) and remote
   backends (specs 009, 012, 030) under the existing contracts, with no
   change to any authority path.

## 2. Territory

Nothing is claimed until the implementing change. Then this spec claims:

- `backends/rustev-backend-embed-linear/` (new crate
  `rustev-backend-embed-linear`): the manifest and head parsers, bundle
  and runtime verification, platform table, tokenization and text
  assembly, the encoder (ONNX and synthetic, 3.19), pooling and head
  arithmetic, the worker pool, the embedding cache, the `DecisionBackend`
  implementation, `check_plan`, the feasibility measurement harness
  (`examples/feasibility.rs`, 3.2), the committed feasibility records under
  `qualification/`, synthetic test bundles, and its tests.
- `Cargo.toml` and `Cargo.lock` (spec 001, additive): the crate as a
  member and its exact-pinned dependencies `ort` (no default features;
  `load-dynamic`, `std` and nothing that downloads or links) and
  `tokenizers` (no default features; no `http`), at the versions the
  feasibility record names.
- `Makefile` (spec 001, additive): `011` joins `VERIFIED_SPECS` with the
  implementation.
- `tools/rustev-boundaries/` (spec 001, additive): 3.18.
- `integrations/rustev-node/` (spec 027), `integrations/rustev-serve/`
  (spec 010) and the facade `integrations/rustev/` (spec 031): the
  optional `local-embed` features of 3.17, additive. Those directories are
  only planned today, so this spec declares no `extends` edge for them;
  the change that adds a feature, made in or after the change that writes
  the crate, declares the additive edge then. Spec 031 already lists the
  facade feature and depends on this spec.

**Amendment of spec 001.** Spec 001 3.5 says adding a forbidden family is a
change to that spec. This spec adds a local inference runtime family and
one placement rule for it (3.18). No rule is relaxed and 001's approved
text is not edited.

**What does not change.** `rustev-contract`, `rustev-core`,
`rustev-runtime`, `rustev-eval` and `rustev-cli` gain nothing: spec 002's
descriptor (`determinism: tolerance` already exists), spec 003's backend
seam and spec 005's pattern of a derived descriptor suffice. No plan
construct, document consumed by the core or operator is added.

### 2.1 Binding principles of #76

- **Proposals, never grants.** 3.1: logits or classified failures only; no
  authorization or `Permitted` type; setup success is not a grant.
- **Derivation classes and lineage.** Every supplied value is
  `model-derived` (spec 001 3.3.1); dependents are `mixed-derived` as the
  core already computes; lineage is the plan's binding (backend id,
  `ArtifactId`, `DescriptorId`) and the step's projected inputs (3.15).
- **Unresolved, never a default; no silent kind conversion.** A missing,
  null or mistyped field, an over-long input under `refuse`, a non-finite
  output or a runtime error is a failure the runtime supplies as an
  unresolved reason; logits are never turned into labels, distributions or
  calibrated probabilities by the backend (3.8, 3.9).
- **Pure core; no async runtime in contract and core.** Neither is
  touched; the backend crate itself uses std threads, no async executor
  (3.11), and 3.18 keeps the inference runtime out of their graphs.
- **New dependency classes only under `integrations/`.** The listed
  classes (HTTP, cloud SDK, Node-API, `wasm-bindgen`, telemetry
  exporters) are not used by this crate. The new local inference runtime
  class is placed by 3.18 and `make boundaries` is extended for it.
- **No dynamic plugin loading (D-06).** The ONNX Runtime library is one
  fixed native dependency of this one backend, named by digest in the
  manifest, loaded from an explicit absolute path; no discovery, search
  path, custom operator library or execution-provider plugin exists
  (3.6, Decision 6).
- **No identity, authority or knowledge store.** Bundles, runtime
  libraries and their provisioning are the host's; Rustev reads what it is
  given and keeps only bounded in-memory state.
- **Plan identities.** Plans that do not bind this backend are untouched;
  every existing plan and definition golden keeps its bytes and `PlanId`
  (3.20).

## 3. Behavior

### 3.1 Position, authority and provenance

1. `EmbedLinearBackend` is an ordinary in-process backend under spec 003's
   seam, constructed explicitly by the host. Nothing it computes
   (embedding, cache state, timing, setup verdict) is authorization,
   becomes an input fact, alters a threshold or policy parameter (R-14),
   or changes retries, fallback or budgets.
2. Each attempt yields exactly one of `RawOutput::Logits` or a classified
   failure (3.7.3), with a charge and a cancellation acknowledgement.
3. A value it supplies is evidence that this artifact produced it from
   those inputs, never independent evidence about the claim the inputs
   describe (spec 001 3.3.3), exactly as spec 005 3.14 states for rules.

### 3.2 The feasibility gate

1. **Record.** `rustev.embed-feasibility/1` is a committed JSON document
   under the crate's `qualification/` directory, identified by its
   contract digest (spec 002 3.3.2), with `deny_unknown_fields`. Members:
   `schema`, `candidate` (3.3 selections, with checkpoint revision and the
   SHA-256 of every selected file), `stages` (four, below), `platforms`
   (per platform key of 3.6: hardware, OS and C library version, CPU,
   cores, memory, each stage's verdict), `measurements`, `verdict`
   (`pass` or `refuse` per platform with reasons), `date` and
   `operator_note`. A Markdown rendering may accompany it; the JSON is
   authoritative.
2. **Stages, in order** (#69's feasibility order). A refusal stops later
   stages for that candidate or platform and is recorded with its cause.
   1. *Checkpoint and license:* the pinned revision exists; selected files
      and their SHA-256 are recorded; the license (model card metadata and
      the base model's) permits host-side use without redistribution by
      Rustev; the training data description is recorded.
   2. *ONNX graph and operators:* the pinned revision ships the graph
      `onnx/model.onnx` (Fact 1), which is used as shipped and bound by
      its SHA-256; nothing is exported or converted. The record holds the
      graph's SHA-256 (equal to the one recorded in stage 1), its inputs
      and output signature, the operator set by domain and opset, and
      reference token ids and pooled embeddings for the fixture texts
      (3.2.4) from the reference implementation (the same revision's
      `pytorch_model.bin` through Sentence Transformers). The graph's
      pooled embeddings must agree with the reference within the 3.3
      bounds, or the stage refuses. A graph that ONNX Runtime's CPU
      execution provider at the pinned version cannot load, or whose
      operators need a custom library, refuses. A refusal is recovered
      only through an amendment of this spec.
   3. *Linking per platform:* the exact `ort` and ONNX Runtime versions;
      the crate's resolved dependency graph (normal and build) with the
      chosen features, which passes `make boundaries` and contains no HTTP
      family, `hf-hub` or download feature; the runtime library's SHA-256
      per platform; dynamic loading from an absolute path works and the
      library reports the pinned version; the Node binding (spec 027 3.13)
      builds with `local-embed` on each native matrix target (3.17).
   4. *Performance and numerics:* the limits of 3.3 measured per platform
      with the protocol of 3.13.
3. **Gate.** No change that adds the inference dependencies merges
   before stages 1 to 3 pass for at least one platform, and may list a
   platform as qualified (3.6.3) only when all four pass for it. A failed
   runtime selects another runtime through an amendment of this spec
   without reopening R-01 (R-01: "subject to a later bounded check").
   Thresholds are never relaxed after measurement; a threshold change is a
   spec amendment.
4. **Fixture texts.** Synthetic only (R-04): the support-routing synthetic
   dataset's texts, a generated set covering empty-after-trim refusal,
   Unicode (accents, CJK, emoji), punctuation runs and exactly
   `max_tokens` and `max_tokens + 1` tokens. No real-user data.
5. **What this spec authorizes.** Under R-43, producing the record may
   download the pinned public checkpoint files and an ONNX Runtime
   library and run local CPU inference on the synthetic fixture texts of
   3.2.4 (Decision 18). Nothing in this spec authorizes a provider call or
   spend, real-user data, or publication or redistribution of any
   downloaded byte, and no checkpoint, graph, tokenizer, head or runtime
   byte is committed to the repository except the synthetic test bundles
   of 3.19.

### 3.3 Selections

These are #69's decisions, with the revision moved by R-43 (Decision 2),
each subject to the measured pass or refusal of 3.2.

| Question | Selection |
|---|---|
| Checkpoint | `sentence-transformers/all-MiniLM-L6-v2` at revision `1110a243fdf4706b3f48f1d95db1a4f5529b4d41` (R-43; #69 named `f5610b47471b118dafc55f4c387822dbfc8413ae`, whose weights, configuration and tokenizer are byte-identical, Fact 1); selected files bound by SHA-256 |
| Graph | the revision's `onnx/model.onnx` (f32), used as shipped and bound by SHA-256; the revision's optimized (`model_O1` to `model_O4`), quantized and OpenVINO files are not selected |
| Redistribution | none: the host provisions immutable artifacts verified by digest; no first-run download; Rustev publishes no checkpoint, graph, tokenizer, head or runtime |
| Runtime | `ort` with ONNX Runtime, exact versions pinned in the record; dynamic loading from a verified absolute path |
| Platforms | `linux-x86_64-gnu`, `linux-aarch64-gnu`, `macos-aarch64`, CPU only, each qualified independently |
| Artifact size | bundle (graph, tokenizer, heads, manifest) at most 150 MiB |
| Cold setup | at most 2 000 ms from `open` to ready, runtime library already on disk |
| Peak RSS | at most 512 MiB for setup plus 1 000 warm requests with one worker |
| Warm latency | one worker, `max_tokens` inputs, 1 000 requests after 50 warm-up: p95 at most 100 ms, p99 at most 200 ms |
| Numerics | per logit, `abs(a - b) <= 0.00001` or `abs(a - b) <= 0.0001 * max(abs(a), abs(b))`; every output finite |
| Decisions | no label, ordering or judgment change except where the deciding quantity lies within 0.001 of a threshold or tie |
| Linear head | produced outside Rustev, identified by digest, with its training-data description recorded; Rustev never trains |
| Order | checkpoint and license, graph and operators, linking per platform, then performance |

### 3.4 The bundle and manifest

1. **Bundle.** A host-provisioned directory holding `manifest.json`
   (`rustev.embed-linear/1`) and the files it names. Every file is named by
   a relative path without `..`, symbolic links are refused, and each
   file's byte length and SHA-256 are declared. The bundle is at most
   150 MiB.
2. **Manifest members**, all required, parsed with the contract's bounded
   parser and `deny_unknown_fields`; numbers that are not counts are
   canonical decimal strings:

| Member | Content |
|---|---|
| `schema` | `rustev.embed-linear/1` |
| `backend_id` | 1 to 64 bytes of `[a-z0-9._-]` |
| `checkpoint` | `{repository, revision (40 hex), files: [{name, sha256}], license, base_model}`; provenance, `synthetic` for test bundles |
| `encoder` | `{kind: onnx or synthetic, graph: {file, bytes, sha256, source}, inputs: [names], output, hidden_size, precision: f32}`; `source` is the graph's path in the checkpoint revision (`onnx/model.onnx`), or `synthetic` |
| `tokenizer` | `{file, bytes, sha256, implementation: "tokenizers@<exact version>", max_tokens, truncation: refuse or truncate_end}` |
| `pooling` | `{mode: mean_masked, normalize: l2 or none}` |
| `runtime` | `{onnxruntime: <exact version>, ort: <exact version>, execution_provider: cpu, graph_optimization: disabled, basic, extended or all, intra_op_threads, inter_op_threads, libraries: [{platform, sha256}]}` |
| `heads` | 1 to 32 `{operation, task, question, options, text, weights: {file, bytes, sha256}, interpretation, provenance}` (3.9) |
| `limits` | `{max_projection_bytes, max_text_bytes}` |
| `cost` | `{units_per_call, units_per_token}` |
| `tolerance` | `{abs_logit, rel_logit, decision_margin, feasibility_record}`: the 3.3 bounds and the digest of the record that measured them, or `none` for a synthetic bundle |

3. Construction refuses an incomplete or inconsistent manifest (for
   example a head whose options are not unique, a proposition whose
   options are not exactly `["false", "true"]`, `intra_op_threads` of 0,
   `max_tokens` above the graph's position limit, or, for `kind: onnx`, a
   graph whose `source` is not among `checkpoint.files` with the same
   SHA-256) and sends nothing to the runtime.

### 3.5 Identity

1. The artifact identity is the contract digest (spec 002 3.3.2) of the
   manifest's canonical bytes under `rustev.embed-linear/1`. Since the
   manifest names every file by SHA-256, the identity transitively covers
   checkpoint provenance, graph and its source, tokenizer, preprocessing,
   truncation, pooling, precision, runtime version and settings, the
   accepted runtime library per platform, operators (through the graph),
   head weights and biases, ordered labels, text templates,
   interpretations, limits, cost and tolerance.
2. Repository names, revisions without digests, model aliases, file paths
   and the runtime path are never identity. The runtime path, worker
   count, queue bound and embedding cache configuration are configuration
   (3.7.1): they do not affect a supplied output (3.11, 3.14).
3. The descriptor is derived from the manifest alone (3.10), so a changed
   byte in any bundle file changes the `ArtifactId`, the `DescriptorId` and
   the `PlanId` of every plan compiled against it, and invalidates any
   calibration bound to the old artifact (spec 002 category 10). The
   runtime's `prepare` refuses a descriptor mismatch (spec 003 3.6.5).
4. The evaluation semantics (text assembly, tokenizer use, pooling, head
   arithmetic, conversion) are fixed by the schema version; changing them
   is `rustev.embed-linear/2`. The crate version is not identity; the
   tokenizer implementation version is, because it can change token ids.
5. Platforms share one identity: a plan compiled against a manifest is the
   same plan on every qualified platform. The cross-platform difference is
   bounded by the declared tolerance (3.13), not hidden.

### 3.6 Runtime loading and platforms

1. **Explicit path.** The host passes an absolute path to the ONNX Runtime
   shared library. The backend refuses a relative path, never consults
   `ORT_DYLIB_PATH`, `LD_LIBRARY_PATH`, `DYLD_*` or any search path, reads
   the file, verifies its SHA-256 against the manifest entry for the
   current platform, then loads it through `ort`'s dynamic loading and
   checks the version the library reports.
2. **One runtime per process.** ONNX Runtime is process-global. A second
   backend in the same process whose verified library digest differs is
   refused `runtime_conflict`; an equal digest reuses the loaded runtime.
3. **Platforms.** A platform key is the target's architecture, OS and C
   library: `linux-x86_64-gnu`, `linux-aarch64-gnu`, `macos-aarch64`. The
   crate compiles a table of platforms the committed feasibility record
   qualifies; construction on any other target, including musl, Windows
   and `darwin-x64`, is refused `unqualified_platform`. The crate still
   compiles there (3.17).
4. **Session.** CPU execution provider only, the manifest's graph
   optimization level and thread counts, no profiling, no custom operator
   library, no execution-provider plugin, no external initializer file. The
   graph is loaded from verified in-memory bytes, so the file is not reread
   after verification.
5. **Limitation stated.** Digest verification protects against a wrong or
   corrupted library, not against a party that can write to the path
   between verification and load; the host keeps the directory immutable
   and owned (Decision 7).

### 3.7 Setup and refusals

1. **Open.** `EmbedLinearBackend::open(EmbedConfig)` with
   `{bundle_dir, runtime_path, expected_artifact, workers (1 to 16),
   max_queued (0 to 1 024), embedding_cache}`. It reads the manifest,
   verifies every file's length and digest, checks that the computed
   `ArtifactId` equals `expected_artifact`, verifies the platform and the
   runtime, creates the session, checks the graph's input and output
   names, element types and hidden size against the manifest, parses the
   heads and starts the workers. Setup is the backend's only file I/O. It
   performs no network access and spawns no process.
2. **Setup refusals** (`EmbedSetupError`, each naming the item):

| Code | When |
|---|---|
| `bundle_missing` | the directory, manifest or a named file is absent or a symbolic link |
| `bundle_too_large` | the bundle or a file exceeds its declared or fixed limit |
| `manifest_invalid` | parse, limit or consistency failure (3.4.3) |
| `digest_mismatch` | a file's length or SHA-256 differs from the manifest |
| `artifact_mismatch` | the computed `ArtifactId` differs from `expected_artifact` |
| `unqualified_platform` | the target is not in the qualified table |
| `runtime_missing` | the path is relative, absent or unreadable |
| `runtime_digest_mismatch` | the library's SHA-256 is not the manifest's entry for this platform |
| `runtime_version_mismatch` | the loaded library reports another version |
| `runtime_conflict` | another library digest is already loaded in the process |
| `graph_rejected` | the session cannot be created (including unsupported operators) |
| `graph_signature_mismatch` | inputs, output, element type or hidden size differ from the manifest |
| `head_invalid` | wrong weights length, non-finite weight or bias |
| `cache_not_equivalent` | an embedding cache is configured on a platform without measured in-process bitwise repeatability (3.14) |
| `feature_disabled` | a binding built without `local-embed` (3.17) |

3. **Request failures** (`AdapterFailure` class, detail
   `embed:<code>:<item>` within 256 bytes; the runtime supplies them as
   `backend_unavailable`, spec 003 3.6.4):

| Code | Class | When |
|---|---|---|
| `projection_too_large` | permanent | projection over `max_projection_bytes` |
| `task_not_found` | permanent | no head with the projection's operation, task, question and options |
| `field_missing`, `field_type` | permanent | a declared text field absent, null or not a string |
| `text_empty`, `text_too_long` | permanent | assembled text empty after trimming, or over `max_text_bytes` |
| `too_many_tokens` | permanent | over `max_tokens` with `truncation: refuse` |
| `queue_full` | overloaded | no worker and `max_queued` reached |
| `runtime_error` | permanent | the session run failed |
| `non_finite` | permanent | a non-finite embedding or logit |
| `cancelled` | (cancellation) | 3.12 |

### 3.8 Text, tokenization and truncation

1. **Text template.** Each head's `text` is `{fields: [{label, ref}],
   separator}` with 1 to 8 fields; `ref` is `input:<name>` or
   `bind:<name>` with an optional `/<member>`, read from the projection's
   `values` exactly as spec 005 3.4 reads fields. Every field must be a
   string; nothing is coerced. The text is the fields in order, each as
   `label` + `": "` + value when `label` is nonempty, joined by
   `separator`. No other normalization is applied before the tokenizer.
2. **Task matching.** The head is the one whose operation, task, question
   and options (in order) equal the projection's, as spec 005 3.5 matches.
3. **Tokenizer.** The bundle's `tokenizer.json` is loaded from verified
   bytes with the pinned `tokenizers` crate (no `http` feature, never
   `from_pretrained`), encoding one sequence with special tokens. The
   feasibility record's stage 2 fixes reference token ids for the fixture
   texts; the implementation must reproduce them exactly.
4. **Truncation.** `refuse` fails a sequence over `max_tokens`;
   `truncate_end` keeps the first `max_tokens` tokens including special
   tokens. Either is declared in the manifest and therefore in the
   identity; neither is silent. The selected checkpoint's sentence
   configuration uses 256 (Fact 2); a manifest above the graph's position
   limit (512) is refused.

### 3.9 Encoder, pooling and the linear head

1. **Encoder.** One session run per request with `input_ids`,
   `attention_mask` and, when the graph declares it, `token_type_ids` of
   zeros; the declared output is the last hidden state, f32.
2. **Pooling.** `mean_masked`: the mean of token vectors weighted by the
   attention mask, summed in token order in f32; then, with `normalize:
   l2`, division by the L2 norm (a zero norm is `non_finite`). This is the
   selected checkpoint's configured pooling (Fact 2).
3. **Head.** A weights file holds `options x hidden_size` little-endian
   f32 values, row per option in declared option order, followed by
   `options` biases. The logit for option `i` is `b_i + sum_j W_ij e_j`,
   computed in f64 over f32 inputs converted exactly, summed in index
   order. The result is `RawOutput::Logits` keyed by every option, values
   as computed; ties are returned equal (the core breaks ties).
4. **Provenance.** Each head's `provenance` is `{producer, method,
   training_data_description, training_data_sha256 or none, synthetic}`.
   Rustev never fits a head (R-01, #76 out of scope "model training"). A
   head is never represented as supporting labels other than its options.
5. **Interpretation.** Each head's `interpretation` (1 to 1 024 bytes)
   states what its logits mean. They are linear scores, not
   log-probabilities. The core's softmax and any calibration artifact
   (spec 002 3.6) establish only which transformation was applied;
   calibration claims need spec 023's admitted data.

### 3.10 Capabilities and the descriptor

1. Derived from the manifest: `backend_id`; `artifact` is the identity of
   3.5; one operation entry per operation with at least one head, in the
   order `classify`, `proposition`, `rubric`, each with output `logits`
   and `max_options` the largest option count among its heads;
   `input_limit` is `max_projection_bytes` with `on_excess: refuse`;
   `determinism` is `tolerance`.
2. `rank` and `scores` are not offered (Decision 4); `label`,
   `distribution` and calibrated outputs are never returned. A plan step
   requiring `bitwise` determinism or another output kind is refused at
   compilation by spec 002's capability matching (`no_capable_backend`)
   or uses its declared fallback.
3. `cost_model` is `bounded`; `cost_bound` is `bounded{max_units:
   units_per_call + units_per_token * max_tokens}`.
4. `check_plan(plan)` reports every semantic step bound to this backend
   (primary or fallback) without a matching head, and every template
   field whose `ref` is not in the step's `project` list or is not a text
   or enum input; the runtime does not call it (as spec 005 3.11.4).
5. No batch capability is declared (spec 017 3.3); adding one is a later
   change measured under spec 017.

### 3.11 Resources: workers, queue, memory and cost

1. **Workers.** `workers` std threads share one session; each runs one
   request at a time. A dispatched attempt is tokenized on the caller's
   poll, then handed to a free worker or to a FIFO queue of at most
   `max_queued`; a full queue fails `queue_full` at once with
   `observed{0}`. The attempt future completes through a one-shot channel;
   the crate depends on no async executor.
2. **Bounded work per call:** one bounded projection parse, at most 8
   fields, `max_text_bytes` of text, `max_tokens` tokens, one session run
   and one head product.
3. **Memory.** Bounded by the session, `workers` input buffers of
   `max_tokens`, the queue and the cache's byte bound; the measured peak
   for one worker is a 3.3 limit. No other allocation grows with traffic.
4. **Cost units.** Local quota units, not money: no provider is called and
   no external charge exists. A call handed to a worker is charged
   `observed{units_per_call + units_per_token * tokens}` whatever its end
   (output, failure or cancellation after handoff), a call that ends
   before handoff `observed{0}`, and an embedding cache hit
   `observed{units_per_call}`. The charge never exceeds the bound, so the
   reservation always covers it.

### 3.12 Deadlines and cancellation

1. **Before handoff.** The signal is checked after tokenization and while
   queued; when raised the request leaves the queue, fails `cancelled`,
   acknowledges `stopped` and is charged `observed{0}`. A dispatch with
   `remaining_ms` of 0 is refused the same way.
2. **After handoff.** When the signal is raised the backend requests
   termination of the run through the pinned `ort` run options. If the
   worker reports that the run ended (terminated or completed) by the
   runtime's one further poll (spec 003 3.7.2), the report is `cancelled`
   or the completed output, acknowledged `stopped`. Otherwise it reports
   `cancelled`, `unconfirmed`, `possibly_continuing` (spec 013) with the
   observed charge of 3.11.4: work handed to a thread may still finish.
3. **Slot accounting.** A worker running a run whose attempt was
   abandoned stays occupied until the run ends; new attempts queue or
   fail `queue_full`, so abandoned work cannot accumulate beyond
   `workers`. Its late result is discarded and never cached.
4. **Deadlines.** The runtime raises the attempt's signal at the decision
   deadline and the attempt timeout (spec 003 3.7.1); there is no separate
   backend timer. A completed run with no observed signal reports
   `not_requested`.

### 3.13 Numerical repeatability

1. **Declared, not bitwise.** The descriptor declares `tolerance`; the
   manifest carries the bounds of 3.3 and the digest of the record that
   measured them.
2. **Protocol.** For each platform: the reference implementation's pooled
   embeddings (stage 2) versus the backend's, by cosine and per-component
   difference; the backend's logits on each platform versus
   `linux-x86_64-gnu` as reference platform for the same bundle; and run
   to run in one process with fixed settings (which decides 3.14). Each
   measurement names the bundle identity, platform, hardware, runtime
   version, thread counts and fixture set.
3. **Decisions.** The support-routing package's plans are run on each
   platform with the same synthetic bundle; any label, ordering or
   judgment change is a refusal unless its deciding quantity lies within
   `decision_margin` of a threshold or tie, and each such case is listed.
4. **Claims.** Only measured platforms and conditions are claimed. A
   change of thread counts, optimization level or runtime version changes
   the identity and needs a new measurement.

### 3.14 Embedding cache

1. **Off by default.** `embedding_cache: {max_entries, max_bytes}` enables
   an in-process memory cache of pooled embeddings. It is never
   persistent and never shared between processes.
2. **Key.** The `ArtifactId`, a domain-separated SHA-256 of the call's
   principal handle (spec 003 `CallContext`), and the SHA-256 of the token
   ids and attention mask after truncation. Heads are applied after the
   lookup, so heads of one bundle share entries for the same text and
   principal; entries never cross principal handles (spec 017 I-1).
3. **Equivalence.** A hit supplies exactly the embedding bits a miss
   computed in this process, so its logits equal those of that miss.
   Enabling the cache is refused `cache_not_equivalent` on a platform
   whose feasibility record does not show bitwise run-to-run repeatability
   in one process for the manifest's settings; there the cache would make
   a supplied value depend on history.
4. **Admission and eviction.** Only finite embeddings from completed,
   non-cancelled runs are admitted. Eviction is least recently used,
   deterministic for an observation order, bounded by entries and bytes.
5. **Invalidation.** `clear()` and `erase_principal(handle)` increment a
   generation; a run that started under an earlier generation is not
   stored. Erasure removes every entry for that handle before returning.
6. **Disclosure.** Hits are visible in the charge (3.11.4) and in counters
   (`hits`, `misses`, `admitted`, `evicted`, `bytes`, `invalidations`)
   read through `cache_stats()`. This is a backend-internal memo below the
   attempt: the attempt is still dispatched, recorded and charged, and
   spec 017's runtime caches are unaffected.

### 3.15 Evidence, replay and evaluation

1. **Run records** are spec 003's; the backend adds no document. Steps it
   answers are `model-derived` with their projected inputs as lineage and
   the plan's binding naming the artifact.
2. **Replay.** Offline replay (spec 004) consumes captured logits and
   never loads the bundle or the runtime.
3. **Evaluation.** Spec 004 reports compare this backend with the rules
   backend on the support-routing package (spec 007) under the same
   dataset, split and evaluator configuration; with spec 029, repeat runs
   and diffs apply unchanged. On synthetic data and synthetic heads these
   reports are mechanics evidence only (R-04, R-37). Any quality,
   calibration or usefulness claim needs spec 023's admitted data, kept
   outside this repository, and a head whose provenance that admission
   covers.
4. **Swap.** A test runs the package's synthetic cases through the runtime
   with the rules backend and with this backend (synthetic encoder and
   synthetic heads), captures both, evaluates both, records the gate
   verdict as mechanics, and asserts that the compiled plans' policy
   sections and output declarations are identical, as spec 007 3.4 does.

### 3.16 Relationship to remote backends

This backend is one more implementation of spec 003's seam. It does not
impersonate Jev or any provider, exposes no remote wire, claims no Jev
compatibility (R-05), and changes nothing in specs 009, 012, 013, 020, 022
or 030: their served-model identity, cost, retry, timeout and remote-state
rules stay as written. A plan may bind it as primary or fallback beside a
remote backend; each keeps its own descriptor and identity.

### 3.17 Optional features in bindings, service and facade

1. **Node binding (spec 027).** `rustev-node` gains a Cargo feature
   `local-embed`, off by default, that adds an optional dependency on this
   crate and a `LocalEmbedBackend.open(config)` call taking the 3.7.1
   configuration; the result registers like the rules backend (a Rust
   backend, not a host-implemented seam). Without the feature the call
   rejects with a typed error carrying `feature_disabled`, and
   `features().localEmbed` is false. The prebuilt platform packages of
   spec 027 3.13 are built without the feature (Decision 10); a host
   enables it by building the binding from source with the feature.
   Because the runtime is loaded dynamically, the binding with the feature
   compiles and links on every native matrix target with no ONNX Runtime
   present; construction on an unqualified platform is refused (3.6.3).
2. **WebAssembly.** `rustev-wasm` has no such feature; local inference is
   not offered there.
3. **Service (spec 010 3.7).** `rustev-serve` gains `local-embed`, off by
   default: the host registers local backends from configuration at
   startup (bundle directory, runtime path, expected `ArtifactId`); a
   request can never name a bundle, path or runtime. It lands with or
   after the service's implementing change (#76 A2).
4. **Facade (spec 031 3.4).** The `local-embed` feature re-exports this
   crate as `rustev::local_embed` when the crate is admitted to the
   release family; admission and publication are spec 031's acts, not
   this spec's.
5. **Parity.** The shared conformance suite (spec 027 3.12) gains
   local-embed cases with the synthetic encoder: the Rust API, the native
   binding and, once it exists, each service transport produce
   byte-identical judgments and run records.

### 3.18 Boundary check extension

1. `tools/rustev-boundaries` gains a local inference runtime family
   matched by package name: `ort`, `ort-sys`, `onnxruntime`,
   `onnxruntime-sys`, `tokenizers`, `tract-*`, `candle-*`, `tch`,
   `torch-sys`; and `hf-hub` as a model hub client.
2. **Rules.** (a) The family is refused anywhere in the normal and build
   graph of `rustev-contract` and `rustev-core` (3.4.1). (b) Only
   `backends/rustev-backend-embed-linear/` may depend on a family member
   directly; any other workspace crate reaches one only through an
   optional dependency on that crate (new rule 3.4.6 of spec 001). (c)
   `hf-hub` and `ort`'s download features (`download-binaries`,
   `copy-dylibs`, `fetch-models`) are refused everywhere.
3. Unit tests over synthetic metadata include a violating case for each
   rule and a passing case for an optional dependency of the Node binding
   on the backend crate.

### 3.19 Synthetic encoder and test bundles

1. `encoder.kind: synthetic` is a deterministic, dependency-free encoder
   (token ids hashed into `hidden_size` buckets with fixed f32 weights,
   mean-pooled) compiled only with the crate's `synthetic` feature. Its
   manifests set `checkpoint.repository: synthetic`, `tolerance` bounds
   with `feasibility_record: none`, and `SYNTHETIC` in every head's
   interpretation. It needs no ONNX Runtime and is bitwise repeatable.
2. Synthetic heads for the support-routing tasks are generated by a
   documented formula from a fixed seed, not fitted to data, and are
   committed under `tests/fixtures/` with their manifests. They prove
   mechanics only.
3. Default tests and CI use only synthetic bundles and need no network,
   checkpoint or runtime library. Tests against the real bundle and
   runtime run by hand on qualified hosts with `RUSTEV_EMBED_BUNDLE` and
   `RUSTEV_EMBED_RUNTIME` set, and their results enter a feasibility or
   qualification record, never declared verification.

### 3.20 Compatibility

1. No contract, core, runtime, eval or CLI change; no document consumed by
   the core is added or changed; every existing plan, definition,
   descriptor and evaluation golden keeps its bytes and `PlanId`, which the
   implementing change asserts with a golden.
2. The workspace, the Node binding, the service and the facade build and
   pass their tests without `local-embed`, and their resolved graphs then
   contain no crate of the 3.18 family (spec 001 3.7; spec 031 3.5).
3. Removing this crate leaves every other crate building.

## 4. Out of scope

Training or fine-tuning (encoder or head); redistribution of any model or
runtime artifact; a download, hub client or first-run fetch; GPU and other
execution providers; quantized or reduced-precision graphs; Windows, musl,
`darwin-x64` and WebAssembly inference; `rank`, `scores`, `generate` and
`extract`; batch capability; a persistent or cross-process embedding cache;
the second local backend (spec 024); quality, calibration and usefulness
claims (R-04, R-37); release and publication (spec 021, 031).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| One byte of the graph, tokenizer or a head changed | `digest_mismatch` at setup; with a re-written manifest, a different `ArtifactId`, `DescriptorId` and `PlanId`. |
| `expected_artifact` differs from the bundle's | `artifact_mismatch`; no descriptor. |
| A plan compiled against an old manifest, backend opened with a new one | `prepare` refuses `DescriptorMismatch`. |
| Relative runtime path, or `ORT_DYLIB_PATH` set and no path | `runtime_missing`; the environment is not read. |
| Runtime library of another version or digest | `runtime_digest_mismatch` or `runtime_version_mismatch`. |
| Second backend with another runtime digest in one process | `runtime_conflict`. |
| Opened on `x86_64-unknown-linux-musl` or Windows | `unqualified_platform`. |
| A symbolic link or `..` in the bundle | `bundle_missing` or `manifest_invalid`. |
| Graph needing a custom operator | `graph_rejected`. |
| Hidden size in manifest differs from the graph | `graph_signature_mismatch`. |
| Head weights one value short, or a NaN bias | `head_invalid`. |
| Plan step requires `rank` or `bitwise` determinism | Refused at compilation (`no_capable_backend`) or declared fallback. |
| Projection whose options differ from every head | `task_not_found`, supplied `backend_unavailable`. |
| Template field null or an integer | `field_missing` or `field_type`; the step is unresolved. |
| 257 tokens with `max_tokens: 256`, `truncation: refuse` | `too_many_tokens`. |
| Queue full | `queue_full`, `overloaded`, `observed{0}`. |
| Signal raised while queued | `cancelled`, `stopped`, `observed{0}`. |
| Signal raised during a run that does not end by the next poll | `cancelled`, `unconfirmed`, `possibly_continuing`; slot held until it ends; result discarded. |
| Cache enabled where in-process bitwise repeatability was not measured | `cache_not_equivalent`. |
| Two principals, same text | Two cache misses; no shared entry. |
| `erase_principal` while that principal's run is in flight | Its result is not stored. |
| Node binding built without `local-embed` | `LocalEmbedBackend.open` rejects `feature_disabled`; graph has no `ort`. |
| `rustev-runtime` adds `ort` directly | `make boundaries` fails naming rule 3.4.6. |
| Any crate depends on `hf-hub` or enables `ort/download-binaries` | `make boundaries` fails. |

## Facts relied on

Read on 2026-10-17 from the sources named, metadata only (file listings
and Git and LFS object ids through the Hugging Face API; no checkpoint
byte was downloaded); re-confirmed in stage 1.

| # | Fact | Source |
|---|---|---|
| 1 | Revision `1110a243fdf4706b3f48f1d95db1a4f5529b4d41` (head of `main`, last modified 2026-06-01; card license `apache-2.0`, base model `nreimers/MiniLM-L6-H384-uncased`) lists `onnx/model.onnx` (90 405 214 bytes, LFS SHA-256 `6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452`), added on 2024-10-10 by Sentence Transformers' `push_to_hub` with an ONNX backend (discussion #83, by its maintainer). Its `pytorch_model.bin` (LFS SHA-256 `c3a85f238711653950f6a79ece63eb0ea93d76f6a6284be04019c53733baf256`), `tf_model.h5`, `rust_model.ot`, `config.json`, `modules.json`, `1_Pooling/config.json`, `sentence_bert_config.json`, `tokenizer.json`, `tokenizer_config.json`, `special_tokens_map.json` and `vocab.txt` have the same Git and LFS object ids as at #69's revision `f5610b47471b118dafc55f4c387822dbfc8413ae` (2022-11-07), which lists no `onnx/` file and is not an ancestor of `main` (it only adds a two-byte file `212`). | https://huggingface.co/api/models/sentence-transformers/all-MiniLM-L6-v2/tree/1110a243fdf4706b3f48f1d95db1a4f5529b4d41?recursive=true, the same for `f5610b47471b118dafc55f4c387822dbfc8413ae`, https://huggingface.co/api/models/sentence-transformers/all-MiniLM-L6-v2/commits/main, https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/discussions/83 |
| 2 | At both revisions: `max_seq_length` 256; mean token pooling over 384 dimensions; modules Transformer, Pooling, Normalize; a BERT model with 6 layers, 12 heads, hidden size 384, 512 positions, vocabulary 30 522, base `nreimers/MiniLM-L6-H384-uncased`. | the revisions' identical `sentence_bert_config.json`, `1_Pooling/config.json`, `modules.json`, `config.json` |
| 3 | `ort` (latest on docs.rs: 2.0.0-rc.14, for ONNX Runtime 1.31) has default features including `download-binaries` and `copy-dylibs`; `load-dynamic` enables `disable-linking` and `preload-dylibs` (`libloading`); `fetch-models` adds `ureq`. | https://docs.rs/crate/ort/latest/features |

## Acceptance

- The feasibility record exists, parses as `rustev.embed-feasibility/1`,
  covers the four stages in order, and qualifies each platform in the
  crate's table; refusals are recorded.
- Manifest validation: each refusal of 3.4.3 and each limit at and one
  past its bound; every setup code of 3.7.2 has a test (runtime codes with
  a synthetic non-library file and with no runtime).
- Identity: changing any manifest member or any byte of a named file
  changes `ArtifactId`, `DescriptorId` and `PlanId`; runtime path,
  workers, queue and cache configuration do not.
- Capability matching: the descriptor advertises exactly the heads'
  operations with `logits` and `tolerance`; `rank`, `bitwise` and other
  output kinds are refused at compilation; `check_plan` reports each
  mismatch kind.
- Text, tokenization and truncation: templates, field failures, both
  truncation modes; the reference token ids of stage 2 reproduced by the
  pinned tokenizer (by hand on the real bundle).
- Resources: `queue_full` at the bound; charges equal 3.11.4 for output,
  failure, cancellation before and after handoff, and cache hits; a hard
  budget below the bound refuses dispatch.
- Cancellation: before handoff `stopped`; after handoff `stopped` when the
  run ends by the next poll and `unconfirmed` otherwise (with a blocking
  synthetic encoder); slot held; late result discarded.
- Tolerance: the synthetic encoder is bitwise repeatable; the real
  bundle's measurements are in the record, not in default tests.
- Embedding cache: equivalence, principal isolation, eviction bounds,
  `clear`, `erase_principal` racing an in-flight run, refusal where not
  equivalent.
- Evidence and replay: steps are `model-derived` with projected-input
  lineage; a captured run reproduces offline without the bundle.
- Swap: the support-routing synthetic cases through rules and this
  backend; identical policy sections and output declarations.
- Offline: the crate's resolved graph contains no HTTP family, `hf-hub`
  or download feature, and tests assert no network access.
- Partial adoption: workspace, Node binding, service and facade build
  without `local-embed` with no family crate in their graphs; the binding
  builds with `local-embed` on every spec 027 native matrix target.
- Binding parity: conformance cases with the synthetic encoder are
  byte-identical through the Rust API and the native binding, and through
  the service transports once they exist.
- `make boundaries` unit tests for 3.18; existing plan goldens unchanged.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation. The implementing change adds `011` to `make
verify` with deterministic, offline verification over synthetic bundles for
every acceptance item that does not need the real artifacts; the
feasibility record and hand-run real-bundle results are evidence, not
declared verification.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely, with "ratification of drafts 011, 022 and 023
with the decisions recorded in #68, #69 and #76" (A-21's quotation of the
work order). #69 settles the selections of 3.3; the points below were left
open and are decided by the implementer within this spec, as written.

1. **Crate and schema names.** `backends/rustev-backend-embed-linear` and
   `rustev.embed-linear/1`, as RUSTEV-003 proposed; it is a backend, not an
   integration, because it uses none of the integration-only classes.
2. **ONNX graph provenance (settled by R-43).** #69's revision has no ONNX
   file; a later revision of the same model ships an official graph with
   byte-identical weights, configuration and tokenizer (Fact 1). Under the
   owner's direction (R-43) the pinned revision moves to it, and its
   shipped `onnx/model.onnx` is bound by SHA-256 in place of an export made
   outside Rustev (3.2.2). #69's model selection is unchanged; stage 2
   still measures the graph against the reference implementation.
3. **Numerical criterion.** #69's absolute and relative limits are combined
   as "either holds", the usual closeness test; requiring both would fail
   any logit near zero on rounding alone.
4. **No `rank`.** The backend offers `classify`, `proposition` and
   `rubric` only (RUSTEV-003, R-28's composed `rank`); C2 (#67) may later
   name it as a pointwise scorer through its own spec.
5. **Linux arm64.** #69 adds `linux-aarch64-gnu` to RUSTEV-002's two
   platforms; it is qualified independently like the others.
6. **Dynamic loading is not plugin loading.** One runtime library, fixed
   by digest in the identity, at a host-given absolute path, with no
   discovery or extension point, is an explicit native dependency of one
   backend; D-06 forbids loading extension code, which nothing here does.
7. **Verification before load.** The library and every bundle file are
   hashed before use and the graph is loaded from memory; the residual
   race on the library path is stated (3.6.5), not hidden.
8. **Tokenizer.** The pinned `tokenizers` crate, without `http`, loading
   the bundle's `tokenizer.json`; its version is identity. A hand-written
   WordPiece would add an unverified reimplementation.
9. **Truncation** is declared per manifest (`refuse` or `truncate_end`),
   never silent, matching spec 002's existing input-excess vocabulary.
10. **Prebuilt Node packages without the feature.** "Hosts that do not want
    ONNX Runtime do not pay for it" (#69); the default packages stay as
    spec 027 defines them, and the feature is a source build until a
    later change adds feature-bearing packages under spec 031.
11. **Embedding cache placement.** Inside the backend, keyed by artifact,
    principal and input digest (#69's "keyed by artifact identity and
    input digest", plus the principal because spec 017 I-1 forbids
    crossing scopes), and gated on measured bitwise equivalence so it
    cannot change a supplied value.
12. **Charges.** Local quota units per call and per token, observed at
    handoff; a cache hit is charged the per-call part only.
13. **Cancellation through termination.** The pinned `ort` run
    termination is requested; `stopped` only when the run is seen to end.
14. **One session shared by workers**, which keeps memory within the 3.3
    bound; thread counts are identity because they can change numerics.
15. **Reference platform** for cross-platform comparison is
    `linux-x86_64-gnu`.
16. **Head arithmetic in f64** over exact f32 conversions in index order,
    so the head adds no platform variation beyond the embedding.
17. **No batch capability** now; spec 017 governs adding one.
18. **Acquisition and local inference for the feasibility record (settled
    by R-43).** The owner authorized it on 2026-10-17: "for spec 011 I
    authorize downloading the pinned public checkpoint and an ONNX Runtime
    library, and running local CPU inference on synthetic texts with no
    spend". Feasibility stages 1 to 4 may download the pinned public
    checkpoint files and an ONNX Runtime library and run local CPU
    inference on synthetic texts (3.2.4). No spend, no provider call, no
    real-user data, and no publication or redistribution of any
    downloaded byte. Everything that needs no real artifact (manifest,
    verification, synthetic encoder, cache, bindings, boundaries) may
    proceed independently.

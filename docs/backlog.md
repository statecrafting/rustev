# Rustev backlog

This file is the canonical inventory of unfinished Rustev product work. It
contains only tasks. Completed implementation and historical evidence stay in
their owning specs, decision records, reports, and retained evidence stores.

## Immediate sequence

### RUSTEV-001: Decide spec 011

- Type: feature contract and owner decision.
- State: owner review required.
- Input: clean signed draft commit
  `303446c7cdf8a4b40124920ffe112705a27ad26f` on
  `spec/011-semantic-backend-readiness`.
- Task: review section 7 of `011-semantic-backend`, revise it if needed, then
  personally ratify it or continue its deferral.
- Decisions: checkpoint, redistribution posture, `ort` and ONNX Runtime
  versions and linking, supported platforms, benchmark hosts and thresholds,
  artifact delivery, feasibility ordering, and linear-head provenance.
- Done when: the exact draft is either approved in the repository's governed
  record or explicitly returned for revision or deferral.
- Boundary: an agent may prepare evidence but may not ratify the spec.

### RUSTEV-002: Produce the local-backend feasibility record

- Type: feature feasibility and supply-chain qualification.
- State: blocked on RUSTEV-001 approval and owner selections.
- Task: evaluate the selected immutable checkpoint, tokenizer,
  preprocessing, ONNX export and operators, pinned `ort` and ONNX Runtime,
  platform and linking matrix, packaging, offline acquisition, licenses,
  redistribution, memory, latency, numerical tolerance, and pass-or-refuse
  thresholds.
- Candidate to evaluate if retained by the owner:
  `sentence-transformers/all-MiniLM-L6-v2` at repository revision
  `f5610b47471b118dafc55f4c387822dbfc8413ae`, with selected bytes bound by
  SHA-256.
- Proposed initial posture: host-provisioned immutable artifacts, no
  redistributed checkpoint or runtime, verified absolute runtime path, Linux
  x86_64 CPU and macOS arm64 CPU qualified independently, and no first-run
  download.
- Proposed limits to measure, not assume: artifact at most 150 MiB, cold setup
  at most 2,000 ms, peak RSS at most 512 MiB, one-worker warm maximum-token p95
  at most 100 ms and p99 at most 200 ms, finite outputs, absolute logit
  difference at most 0.00001, relative difference at most 0.0001, and no label
  or judgment change outside a 0.001 threshold margin.
- Done when: an immutable local record gives a supported pass or explicit
  refusal for every selected platform without adding an inference dependency
  to the repository.
- Boundary: no fabricated measurements, network-at-runtime claim, provider
  call, paid inference, or real-user data.

### RUSTEV-003: Implement the first local semantic backend

- Type: feature.
- State: blocked on approved spec 011 and accepted RUSTEV-002 evidence.
- Task: add `backends/rustev-backend-embed-linear` as an ordinary
  `DecisionBackend` using frozen embeddings and an externally produced,
  identified task-specific linear head.
- Required behavior: canonical `rustev.embed-linear/1` artifact identity;
  declared `classify`, `proposition`, and `rubric` capabilities returning
  logits; no native `rank`; verified offline artifacts; bounded tokenization,
  projection, workers, queue, memory, and local quota units; honest deadline
  and cancellation behavior; typed refusal for missing or mismatched artifacts;
  `model-derived` lineage; tolerance-based repeatability; capture, replay, and
  evaluation compatibility; backend interchangeability with Jev without a Jev
  compatibility claim.
- Acceptance: compile-time boundaries, no implicit network, artifact identity
  and mismatch tests, resource refusal, capability matching, cancellation,
  numerical tolerance, evidence identity, replay/evaluation, backend swap, and
  partial-adoption builds all pass under the approved spec.
- Done when: the approved spec is implementation-complete with its declared
  verification and review evidence. Release and production qualification are
  separate tasks.

### RUSTEV-004: Reconcile stale public documentation

- Type: fix.
- State: ready as a separate governed documentation unit.
- Task: update `README.md` and
  `docs/design/001-decision-engine-architecture.md` to reflect the implemented
  generalized engine, completed specs 007 and 009 and 012 through 016,
  adapter-scoped batching versus deferred general batching, the delivered
  remote adapter choice, current packages, and the true status of specs and
  increments.
- Preserve: the design remains proposed where it is not normative, and no
  deferred work is relabeled as delivered.
- Done when: every lifecycle and capability statement agrees with governed
  specs and current source, with an owning documentation/spec unit and passing
  repository gates.

## Optimization and runtime work

### RUSTEV-005: Qualify adapter-scoped batching

- Type: enhancement and external qualification.
- State: implemented but disabled by default; evidence required before use.
- Task: qualify specs 009 and 012 shared-state same-decision batching for the
  intended adapter, including batch bounds, member attribution, partial-member
  liability, cancellation, deadlines, cost settlement, and failure behavior.
- Done when: retained adapter evidence supports an explicit deployment posture
  and enabling the feature does not broaden plan capabilities or erase unknown
  charge.
- Boundary: the closed unbatched stage-3 qualification must not be rerun or
  relabeled as batched evidence.

### RUSTEV-006: Specify general runtime optimization

- Type: enhancement contract.
- State: intentionally deferred; no owning spec exists.
- Task: draft one separately reviewable spec covering runtime-planned batching,
  cross-request duplicate suppression, reusable inference caches, and
  persistent caches. Treat adapter-scoped batching as an existing special case.
- Required contract: equivalence, tenant and principal isolation, artifact and
  input identity, invalidation, expiry, erasure races, byte budgets, admission,
  cancellation, deadlines, cost attribution, evidence, and failure semantics.
- Done when: the owner has reviewed and ratified a contract with executable
  negative cases and no implementation bundled into the approval change.

### RUSTEV-007: Implement general runtime optimization

- Type: enhancement.
- State: blocked on RUSTEV-006.
- Task: implement only the ratified batching, duplicate-suppression, and cache
  contract, preserving bounded admission, deterministic core semantics,
  request isolation, explicit unresolved outcomes, and honest evidence.
- Done when: the owning spec is implementation-complete and benchmarked for
  cold and warm p50, p95, and p99 behavior under declared resource limits.

### RUSTEV-008: Add live re-execution

- Type: feature.
- State: intentionally deferred by R-20; no owning spec exists.
- Task: specify and implement fresh re-execution separately from spec 004's
  offline historical reproduction. Define authorization inputs, new
  observations, backend dispatch, cost, retention, divergence, cancellation,
  and evidence without rewriting historical records.
- Done when: live work is unmistakable from offline replay and cannot silently
  contact a provider or widen authority.

### RUSTEV-009: Add dynamic questions as a narrow class

- Type: feature.
- State: intentionally deferred by D-05; no owning spec exists.
- Task: define dynamic questions as a separately evaluated class with narrower
  claims, bounded labels and context, capability disclosure, abstention,
  injection-resistant data handling, cost limits, and dedicated evaluation.
- Done when: dynamic questions cannot silently broaden registered-plan
  capability or manufacture calibration, probability, or authority.

## Backend, evidence, and quality work

### RUSTEV-010: Qualify a production remote transport and served identity

- Type: fix and production qualification.
- State: dependent on owner decision and external evidence.
- Task: qualify TypeSafe's direct API with a pinned version, or prove and record
  equivalent Gateway pinning. Bind request and response shapes, transport,
  endpoint, served model identity, privacy posture, retries, timeouts, TLS,
  connection behavior, and cost reconciliation.
- Done when: production runs no longer report an unobservable mutable served
  model and the transport passes a separately approved spec 012 amendment.
- Boundary: current Gateway evidence with `served: unknown` is development
  evidence only.

### RUSTEV-011: Produce real labeled quality and calibration evidence

- Type: quality enhancement and external qualification.
- State: dependent on external evidence.
- Task: identify an appropriately licensed public dataset or independently
  human-labeled dataset, record provenance, labeling method, splits and
  limitations, bind the dataset and evaluator configuration, then measure
  quality, calibration, abstention, and regression against exact and rules
  baselines.
- Done when: claims are scoped to the evaluated population and task, held-out
  evidence is retained, and synthetic fixtures are not presented as
  independent semantic-quality evidence.

### RUSTEV-012: Add a second local semantic backend

- Type: feature.
- State: intentionally deferred; no owning spec exists.
- Task: add a second local backend only after the first backend and evaluation
  path expose a concrete comparative need. Declare actual capabilities,
  artifact identity, resource use, numerical behavior, and swap evidence.
- Done when: comparable reports show the backend swap changes no authority
  path and requires no core contract change unless separately approved.

### RUSTEV-013: Admit wire-witness exchanges as replay-corpus input

- Type: integration enhancement.
- State: deferred; no Rustev change is currently required.
- Depends on: wire-witness exchange normalization and Rustev semantic backend
  availability.
- Task: define how redacted, attempt-bound wire-witness exchange records map to
  Rustev remote-exchange and replay inputs while preserving requested versus
  served identity, explicit unknowns, retention, privacy, cost provenance, and
  host-owned authorization.
- Done when: a versioned adapter or importer validates the record shape and
  produces bounded replay input without treating captured coding-agent traffic
  as a Rustev decision or granting authority.

## Packages, integrations, and distribution

### RUSTEV-014: Deliver the lodging reference package

- Type: feature.
- State: intentionally deferred and unsupported by current authority.
- Proposed owner: new spec 008.
- Task: implement `rustev-pkg-lodging` with bounded candidate fan-out,
  deterministic eligibility, trusted-context handling, semantic budgets,
  explicit abstention, plan identities, synthetic mechanics fixtures, and
  separately governed real-quality evidence.
- Done when: the second domain package builds without core changes and passes
  partial-adoption, replay, evaluation, and backend-swap acceptance.

### RUSTEV-015: Deliver optional ecosystem integrations and hosting

- Type: feature.
- State: intentionally deferred and unsupported by current authority.
- Proposed owner: new spec 010.
- Task: specify and implement only evidenced needs for `rustev-aicortex`,
  `rustev-rahi`, and `rustev-serve` under `integrations/`.
- Required boundaries: Aicortex supplies attributed context and receives only
  proposals; Rahi may host storage and evidence sinks but does not own domain
  judgment; the HTTP service grants no authority; standalone library builds
  remain valid without any integration crate.
- Done when: each integration is independently optional, versioned, bounded,
  qualified, and removable without changing core decisions.

### RUSTEV-016: Refine the remote protocol and transport

- Type: enhancement.
- State: intentionally deferred; no owning spec exists.
- Task: evaluate and separately specify `rustev.run/2` structured remote
  evidence, runtime-planned batch dispatch, per-phase timeouts, connection
  reuse, and a principal option for host-operated remote endpoints.
- Done when: each adopted refinement has a compatibility and migration story,
  explicit identity effects, failure semantics, and negative acceptance tests.

### RUSTEV-017: Define packaging and publish the first release

- Type: release feature.
- State: unsupported by current authority; no release spec exists.
- Task: define crate boundaries and publishability, version policy, registry
  contents, feature policy, dependency and license audit, reproducible package
  checks, signed tag and release artifacts, release notes, and registry-only
  consumer qualification.
- Done when: the governed release spec is approved and complete, published
  crates resolve without path or Git overrides, the signed release identity is
  verified, and a fresh consumer proves the public surface.
- Boundary: current `0.1.0`, `publish = false` manifests and immutable Git
  consumption are not a release.

## Deferred research and non-default directions

### RUSTEV-018: Evaluate an optional action facade

- Type: research and optional feature.
- State: intentionally deferred pending evidence of need.
- Task: evaluate `rustev-act` only as a generic facade over application-owned
  authorization. Rustev must never construct, infer, or widen permission.
- Done when: a demonstrated consumer need justifies a separate contract and
  the facade can be removed without weakening the authority boundary.

### RUSTEV-019: Evaluate custom training or distillation

- Type: research.
- State: intentionally deferred.
- Task: define data rights, provenance, privacy, reproducibility, evaluation,
  compute budgets, artifact identity, supply chain, and model-update policy
  before any training or distillation work.
- Done when: the owner authorizes a bounded research spec and its evidence can
  support only the claims actually measured.

### RUSTEV-020: Evaluate distributed execution and replicated storage

- Type: research.
- State: intentionally deferred.
- Task: establish a concrete scale or durability need, then specify failure
  domains, consistency, replay, idempotency, admission, cancellation, budget
  accounting, evidence durability, recovery, and authority boundaries.
- Done when: measured need justifies complexity beyond the current host-owned
  evidence-sink and local-runtime seams.

### RUSTEV-021: Evaluate extension mechanisms without dynamic plugin loading

- Type: research.
- State: not schedulable under D-06 without an explicit amendment.
- Task: if static Rust traits and versioned remote adapters prove insufficient,
  produce evidence and compare sandboxed extensions against current explicit
  composition. Do not introduce dynamic plugin loading or a marketplace by
  default.
- Done when: an owner-approved amendment defines isolation, provenance,
  signing, capability limits, resource limits, compatibility, and revocation.

### RUSTEV-022: Evaluate a Jev-compatible HTTP surface

- Type: research.
- State: not schedulable under R-05 and R-26 without an explicit amendment.
- Task: require a demonstrated interoperability need and assess whether a
  compatibility surface can exist without impersonating Jev, obscuring served
  identity, or weakening Rustev's typed contracts.
- Done when: the owner either rejects the direction or approves a bounded
  compatibility contract with conformance evidence and trademark-safe naming.

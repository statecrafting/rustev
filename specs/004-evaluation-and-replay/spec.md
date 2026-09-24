---
id: "004-evaluation-and-replay"
title: "Evaluation and replay"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-eval` reproduces judgments offline from a separate,
  versioned replay bundle (`rustev.replay/1`) that references the plan, the
  snapshot and every supplied backend output or runtime reason under an
  explicit, bounded retention mode; compares a candidate plan only on
  requests that are semantically equivalent; keeps live re-execution apart
  as new observations; computes metrics with explicit denominators,
  abstention handling and comparable-case coverage; fits calibration on a
  named split; and emits the versioned report envelope from
  `rustev-contract` (R-06). Quality claims require real, independently
  labeled data (R-04).
establishes:
  - { kind: directory, path: "crates/rustev-eval/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "005-reference-backends"
---

# 004: Evaluation and replay

Draft. A proposal, not a claim about code. Rationale: design section 11;
owner decisions R-04 and R-06. Refined after specs 003 and 005 were
delivered, against their actual interfaces, following the owner's replay
direction of 2026-09-23 (recorded with R-12 to R-18). Section 5 lists the
contract and runtime amendments this spec would need; none is made here.

## 1. Purpose

Make every quality, calibration and latency statement a measured, named
result; make an absent measurement `unknown`; and make "this judgment can be
reproduced" a checked claim about retained inputs, never an inference from a
digest.

## 2. Territory

`crates/rustev-eval/`. The report envelope stays in `rustev-contract`
(R-06); the replay bundle document would join it (5.1). Metrics, datasets,
replay execution and fitting live here. Depends on `rustev-contract` and
`rustev-core`; on `rustev-runtime` only for live re-execution (3.6), which
may be split out if the dependency is unwanted.

## 3. Behavior (to be made concrete before approval)

### 3.1 What exists after specs 003 and 005

1. A `rustev.run/1` record carries the core's `EvidenceRecord`, the
   `PlanId`, the execution-policy identity, attempts, timing and cost. It
   carries the `SnapshotId` but not the snapshot, and each request's result
   as `output{target}` or a runtime reason, but not the output. A run record
   alone therefore cannot reproduce a judgment, and this spec does not
   change that: run records stay observational records with the retention
   obligations spec 003 gives them (none beyond delivery).
2. The core reproduces a judgment from a plan, a snapshot, an evaluation
   time and the sequence of supplied values (`Evaluation::supply`, spec 003
   3.8). The runtime does not currently hand the supplied outputs to its
   caller (5.3).
3. Spec 005's rules backend is `bitwise` deterministic and in-process, so it
   supplies execution fixtures whose live re-execution is exactly
   comparable. It is a software fixture, not a quality baseline.

### 3.2 The replay bundle, `rustev.replay/1`

A separate, versioned document per decision, written by the host from what
it retained, never derived from the run record alone:

- `decision_id`, `plan_id`, and the plan: embedded canonical bytes, or a
  reference with its digest;
- `evaluation_time_ms`;
- the snapshot: `snapshot_id` and one retention item (3.3);
- per request, in the order the core listed them: `step`, `instance`, the
  request identity (3.5), and exactly one supplied value, either a
  `rustev.backend-output/1` document or a runtime reason of spec 002
  3.10.1, each as a retention item; plus the run record's attempt id that
  produced it, as a cross-reference;
- the run record's `decision_id` and a digest of the delivered record, as a
  cross-reference only.

A bundle is bounded by the plan's `max_semantic_requests` and parsed under a
named limit set. It is not identified evidence of provenance: it says what
the host retained.

### 3.3 Retention and availability

1. Each retention item declares one mode:
   - `retained{digest, expires_at_ms}`: the bytes are held with the bundle
     (or its store) until the stated expiry;
   - `external{reference, digest}`: the bytes are held elsewhere and
     resolved on demand;
   - `digest_only{digest}`: only the digest was kept.
   Retention is explicit and bounded: a bundle states its expiry, and no
   mode is unlimited by default. `digest_only` never establishes
   replayability.
2. At replay, each item resolves to exactly one availability:
   `available`, `missing` (never retained, or `digest_only`), `expired`,
   `erased` (removed on request before expiry), `corrupt` (bytes whose
   digest differs from the recorded one), `inaccessible` (the resolver
   failed or refused), or `mismatched` (intact bytes that belong to another
   plan, snapshot, request or decision). The seven are distinct and are
   reported as such; none is ever read as a pass.
3. A digest establishes integrity relative to the expected bytes. It does
   not establish who produced them or that they are true; the bundle's
   cross-references to the run record are what connect them to an
   execution, and those are only as trustworthy as the host's storage.
4. Erasure and expiry remove replay availability. A case whose snapshot or
   any supplied value is `erased` or `expired` becomes `incomparable` with
   that reason; it is counted in coverage (3.8) and excluded from every
   quality numerator and denominator, never scored as agreement.

### 3.4 Historical reproduction (offline)

1. Reproduction re-runs the pure core only: it loads the plan
   (`Compiled::load`, which recompiles and requires byte-identical plan
   bytes), starts an evaluation over the retained snapshot at the recorded
   evaluation time, supplies each retained value through
   `Evaluation::supply` in the recorded order, and finishes. It makes no
   backend or model call.
2. The result is `reproduced` when the judgment's canonical bytes equal the
   recorded judgment's, `diverged` otherwise (a defect to investigate), or
   `incomparable{reason}` when any input is not `available`, or when the
   plan cannot be loaded under this build of the core
   (`compiler-changed`).
3. A nondeterministic backend does not prevent reproduction: its actual
   outputs are retained, and reproduction replays them. Determinism
   matters only for live re-execution (3.6).
4. Reproduction does not remeasure anything: latency, attempts and spend
   stay the historical observations in the run record, with their labels
   (observed, estimated, unknown liability never summed as spend).

### 3.5 Candidate comparison

1. A candidate plan (different definition, bindings, calibration or
   execution policy) is evaluated over the same retained snapshot and
   evaluation time. A retained value is reused for a candidate request
   only when the two requests are semantically equivalent.
2. Request identity covers every output-affecting input (spec 003
   3.10.1): backend artifact and descriptor, operation, task, question,
   options or levels, candidates, the canonical projection bytes, input
   limit handling, and, for values read after normalization, the output
   kind and normalization. A calibration applied by the core after the
   backend is not part of the request; it is applied again by the
   candidate. Step ids and instance keys alone are never equivalence.
3. A candidate request with no equivalent retained request is
   `incomparable{request-mismatch}` for that request, and the case is
   incomparable unless the candidate policy handles that step's absence
   explicitly; a changed model, question, option list, preprocessing or
   projection never reuses the old output as evidence of the new behavior.

### 3.6 Live re-execution

Open whether it belongs in this spec (default: a separately feature-gated
module). If included, it runs the candidate through `rustev-runtime` against
installed backends, produces new run records and new observations, and is
reported apart from reproduction. Its results are never labeled
`reproduced`. Against a `bitwise` backend (spec 005) outputs are compared
exactly; against a `tolerance` backend within its declared, measured
tolerance; otherwise `incomparable{nondeterministic-backend}`.

### 3.7 Datasets and provenance gate (R-04)

1. Datasets are identified by `DatasetId` and split into training, model
   selection, calibration and final test. A report names its split; a split
   used to fit is never a holdout afterwards.
2. A report over synthetic data says so in its envelope and makes no
   quality claim. Before semantic quality qualification, an appropriately
   licensed public dataset or an independently human-labeled dataset is
   recorded with provenance, license, labeling method, splits and
   limitations. Generated examples are never independent evidence of
   semantic quality, and spec 005's rules programs are software fixtures.

### 3.8 Metrics

Every metric states its definition, numerator, denominator and the cases it
excludes. Per case the outcome is one of: proposal, escalation,
`missing_evidence_from`, unresolved (each reason), or incomparable (each
reason).

1. **Coverage of comparison**: comparable cases over all cases, with
   incomparable counts by reason. Always reported first.
2. **Acceptance coverage**: proposals over comparable cases. Escalations
   and unresolved outcomes are abstentions, counted in the denominator and
   never in the numerator.
3. **Error among accepted**: wrong proposals over proposals, against
   labels; the central product measure, always shown with acceptance
   coverage (the coverage-error pair), never alone.
4. **Log loss, Brier, reliability by subgroup**: over steps that produced a
   distribution or calibrated probability and have a label; steps that were
   unresolved are counted as missing, not as a probability.
5. **Missing cases**: a case without a label is counted and excluded from
   labeled metrics, never imputed.
6. **Latency and cost** come from run records only, with queueing reported
   separately and cost labels preserved; replay adds none.
7. A regression gate declares tolerances; a missing measurement is
   `unknown`, never a pass.

### 3.9 Calibration fitting

Fits `temperature/1` on the calibration split and emits a
`rustev.calibration/1` artifact bound to artifact, task, question and
dataset. The artifact records the fit; whether it is calibrated is measured
again on a different split. A fitted calibration is refused as evidence on
the split that fitted it.

## 4. Out of scope

Training models (R-01, R-17), paid inference, dataset acquisition, model
benchmarking, any dataset without recorded provenance and license, and
changing run records' retention obligations.

## 5. Required amendments (proposed; not made by this draft)

Each would be a separately reviewable change with an `amends` edge, made
before implementation (R-16).

1. **Contract (spec 002's crate, as amended by 003):** the
   `rustev.replay/1` bundle document and its limit set; a retention item and
   availability vocabulary; a request-identity document or digest function
   (`rustev.request/1`) so that equivalence is computed one way.
2. **Core:** a function returning each pending request's identity from the
   plan binding and projection (3.5.2), so eval and runtime cannot disagree
   on equivalence.
3. **Runtime (spec 003):** a way for the host to retain what was supplied,
   without enlarging the run record: either `Decided` gains the supplied
   values per request, or a separate retention seam receives them. The
   runtime would still retain nothing itself.
4. **Plan loading:** confirm that `Compiled::load` under a different
   `rustev-core` version is reported as `compiler-changed` rather than a
   load error the caller must interpret.

## 6. Decisions still required from the owner

1. Where bundles live and who enforces expiry and erasure (the host, with
   Rustev only describing and checking), and the default and maximum
   retention bounds.
2. Whether live re-execution is part of this spec or a later one.
3. Whether request identity includes the authorized scope (principal
   handle or tenant), as spec 003 3.10.1 requires for sharing, so a
   retained value from one tenant can never serve another's candidate.
4. Which of the two shapes in 5.3 the runtime takes.

## Acceptance (draft)

- A retained run reproduces its judgment byte for byte with no backend
  call; a run with a `digest_only`, expired, erased, corrupt, inaccessible
  or mismatched input is `incomparable` with that distinct reason and is
  never counted as agreement.
- A candidate plan with a changed question, option list, projection or
  artifact does not reuse the old output; an unchanged request does.
- Live re-execution, if included, is reported apart from reproduction.
- A synthetic baseline report exists for each reference task, labeled
  synthetic, with no quality claim; it states comparable-case coverage and
  every denominator.
- A fitted calibration is refused as evidence on the split that fitted it.
- A report never presents estimated or unknown cost as observed, and never
  reports replay time as inference latency.

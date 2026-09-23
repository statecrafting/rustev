---
id: "004-evaluation-and-replay"
title: "Evaluation and replay"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-eval` replays plans over retained snapshots and
  backend outputs, compares baseline and candidate per case and subgroup,
  computes metrics including calibration and coverage against error among
  accepted decisions, fits calibration artifacts on a named split, and emits
  the versioned report envelope from `rustev-contract` (R-06). Reads
  `rustev.run/1` records for latency and cost with their certainty labels.
  Quality claims require real, independently labeled data (R-04).
establishes:
  - { kind: directory, path: "crates/rustev-eval/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
---

# 004: Evaluation and replay

Draft. A proposal, not a claim about code. Rationale: design section 11;
owner decisions R-04 and R-06. Updated after spec 003 was delivered, to
build on its actual interfaces.

## 1. Purpose

Make every quality, calibration and latency statement a measured, named
result, and make an absent measurement `unknown`.

## 2. Territory

`crates/rustev-eval/`. The report envelope stays in `rustev-contract`
(R-06); metrics, datasets, execution and fitting live here. Depends on
`rustev-contract` and `rustev-core`; on `rustev-runtime` only if live
re-execution is in scope (open question 3.3.4).

## 3. Behavior (to be made concrete before approval)

1. **Datasets.** Identified by `DatasetId`, split into training, model
   selection, calibration and final test; a report names its split, and a
   split used to fit is never a holdout afterwards.
2. **Provenance gate (R-04).** A report over synthetic data says so in its
   envelope and makes no quality claim. Before any semantic quality
   qualification, an appropriately licensed public dataset or an
   independently human-labeled dataset is recorded with provenance,
   license, labeling method, splits and limitations. Generated examples are
   never independent evidence of semantic quality.
3. **Replay.** What exists after spec 003, and what replay still needs:
   1. A `rustev.run/1` record carries the core's `EvidenceRecord` (step
      statuses, value kinds, derivation, the judgment), the `PlanId`, the
      execution policy identity, attempts, timing and cost. It carries the
      `SnapshotId` but not the snapshot, and step statuses but not the
      backend outputs. A run record alone therefore cannot reproduce a
      judgment.
   2. Replay needs a retention contract: the snapshot (or an external
      reference the context source can resolve by `SnapshotId`) and each
      supplied `rustev.backend-output/1` document or runtime reason. This
      spec must define that retention (modes such as digest-only, retained
      with a time limit, or external), its bounds, and erasure, and whether
      it amends spec 003's run record or is a separate document.
   3. Offline replay supplies the retained values through
      `Evaluation::supply` exactly as the runtime did (spec 003, 3.8), so a
      replayed judgment equals the recorded one byte for byte for the same
      `PlanId`. A candidate plan (different definition, bindings or
      execution policy) is replayed over the same retained values where
      its requests match.
   4. Open: whether live re-execution against backends belongs here. If
      it does, it runs through `rustev-runtime`, and its results are new
      observations, not reproductions (principle XIII).
   5. Each case is `comparable` or `incomparable` with a reason
      (`inputs-erased`, `inputs-expired`, `artifact-unavailable`,
      `nondeterministic-backend`, `output-not-retained`,
      `request-mismatch` when a candidate plan asks a different question).
4. **Metrics.** Per task adapter: log loss, Brier, reliability by subgroup,
   coverage against error rate among accepted decisions (the central product
   measure). Latency percentiles come from run-record timing (monotonic
   milliseconds, with queueing reported separately). Cost comes from the run
   record's cost summary and keeps its labels: observed, estimated and
   unknown liability are never summed into one number presented as spend.
5. **Calibration fitting.** Fits `temperature/1` on the calibration split
   and emits a `rustev.calibration/1` artifact bound to artifact, task,
   question and dataset. The artifact records the fit; whether it is
   calibrated is measured again on a different split.
6. **Regression.** Declared tolerances; a missing measurement is
   `unknown`, never a pass.

## 4. Out of scope

Training models (R-01 forbids custom-model training here), paid inference,
any dataset without recorded provenance and license.

## Acceptance (draft)

- A synthetic baseline report exists for each reference task, labeled
  synthetic, with no quality claim.
- A fitted calibration is refused as evidence on the split that fitted it.
- Replay of a retained run reproduces its judgment byte for byte; a run
  whose outputs were not retained is `incomparable{output-not-retained}`.
- A report never presents estimated or unknown cost as observed.

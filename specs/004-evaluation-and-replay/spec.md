---
id: "004-evaluation-and-replay"
title: "Evaluation and replay"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-eval` replays plans over recorded snapshots, compares
  baseline and candidate per case and subgroup, computes metrics including
  calibration and coverage against error among accepted decisions, fits
  calibration artifacts on a named split, and emits the versioned report
  envelope from `rustev-contract` (R-06). Quality claims require real,
  independently labeled data (R-04).
establishes:
  - { kind: directory, path: "crates/rustev-eval/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
---

# 004: Evaluation and replay

Draft. A proposal, not a claim about code. Rationale: design section 11;
owner decisions R-04 and R-06.

## 1. Purpose

Make every quality, calibration and latency statement a measured, named
result, and make an absent measurement `unknown`.

## 2. Territory

`crates/rustev-eval/`. The report envelope stays in `rustev-contract`
(R-06); metrics, datasets, execution and fitting live here.

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
3. **Replay.** Re-run a plan or a candidate plan over recorded snapshots;
   each case is `comparable` or `incomparable` with a reason
   (`inputs-erased`, `inputs-expired`, `artifact-unavailable`,
   `nondeterministic-backend`).
4. **Metrics.** Per task adapter: log loss, Brier, reliability by subgroup,
   coverage against error rate among accepted decisions (the central
   product measure), latency percentiles, cost.
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

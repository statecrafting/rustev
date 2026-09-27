---
id: "023-real-data-quality-qualification"
title: "Semantic quality and calibration qualification on real labeled data"
status: draft
implementation: deferred
created: "2026-09-27"
summary: >
  The governed path from "mechanics only" to a scoped empirical quality
  claim that backlog item RUSTEV-011 and owner decision R-04 require: how an
  appropriately licensed public dataset or an independently human-labeled
  dataset is admitted, with provenance, license, privacy basis, labeling
  method, annotator independence, splits and limitations recorded; how the
  evaluator configuration is frozen before the final-test split is read;
  how quality, calibration, abstention and regression are measured against
  exact and rules baselines under spec 004; and how every resulting claim
  is scoped to the evaluated population, task and binding identity. Draft
  and deferred: no dataset is identified yet, and no real data, live call or
  spend is authorized.
depends_on:
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "007-support-routing-package"
  - "008-reference-package-lodging"
  - "012-jev-integration"
  - "014-report-integrity"
references:
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No quality, calibration or usefulness claim cites a dataset that lacks an admission record, and synthetic or generated examples are never reported as independent evidence of semantic quality."
    anchor: "3-1-dataset-admission"
  - id: "I-2"
    kind: invariant
    text: "The evaluator configuration, gates and baselines are frozen and digested before any final-test case is evaluated; a change after that point starts a new qualification with a new final-test split."
    anchor: "3-3-freezing-before-the-final-test"
  - id: "I-3"
    kind: invariant
    text: "Every claim names the dataset id, split, evaluator configuration id, binding identity, call dates and population, and states the limits that bound it; a claim beyond them is rejected."
    anchor: "3-5-claim-scoping"
  - id: "R-1"
    kind: requirement
    text: "Every candidate is reported against the exact and rules baselines on the same dataset, split and configuration, with coverage and abstention reported before accuracy."
    anchor: "3-4-measurement"
---

# 023: Semantic quality and calibration qualification on real labeled data

Draft, and deferred: a proposal, not a claim about code or evidence.
Backlog item RUSTEV-011 has no owning spec; this is it. Deferred because no
appropriately licensed or independently labeled dataset is identified
(R-04), and because the first local semantic backend (spec 011) is
unapproved.

Spec 004 already supplies the machinery: `rustev.dataset/1` with provenance
and disjoint splits, `rustev.evaluator-config/1` and `/2` (spec 014),
denominators, calibration fitting on a calibration split, and gates. This
spec adds no mechanism by default. It governs the evidence: which data may
be used, what must be recorded and frozen, and what may be claimed.

## 1. Purpose

Every current Rustev report is mechanics evidence: synthetic fixtures (specs
007, 008) or a small synthetic-provenance set (spec 012 stage 3, 4 to 12
cases per task and split). None supports a quality claim. This spec defines
the smallest governed procedure after which a scoped claim such as "on
dataset D, split final-test, binding B, the support-routing queue task has
macro-F1 x with coverage y" is supportable, and nothing broader.

## 2. Territory

None until a dataset is admitted. Then: an admission record and a
qualification record per task in this spec's implementation record, with
canonical reports, evaluator configurations, detail documents and digest
manifests retained under a location the owner names. Committed content is
limited to aggregates and digests, as spec 012's stage 3 record does. If
section 6 question 3 is answered by amending spec 004, that amendment is a
separate spec.

## 3. Behavior

### 3.1 Dataset admission

A dataset is admitted by an owner-reviewed admission record, before any
case is evaluated, stating:

1. **Source.** Publisher, exact version or retrieval date, and content
   digest; or, for a commissioned set, who labeled it and under what
   instruction.
2. **License and redistribution.** The license text's identity, whether the
   data may be committed, retained outside Git, sent to a provider, or
   redistributed in a report.
3. **Privacy basis.** Whether the data contains personal data; a dataset
   derived from real user data needs the owner's separate privacy decision
   (spec 012 3.1.3) and is refused without it.
4. **Labeling method.** Guidelines, number of annotators per case,
   agreement statistic with its denominator, adjudication rule, and
   annotator independence from the systems under test. Labels produced by a
   model under test, or by a model of the same family, are not independent.
5. **Task mapping.** How the dataset's labels map to the task adapter's
   labels (spec 014 rules digest), with unmapped labels counted, never
   dropped.
6. **Splits.** Model-selection, calibration and final-test membership,
   disjoint by source and snapshot identity (spec 004 3.5.1), with the
   method of assignment and its seed. Near-duplicates the manifest cannot
   identify are a stated limit.
7. **Population and limitations.** Who and what the data represents, and
   which slices (spec 012 3.8.3's weaknesses: arithmetic, counting, date
   comparison, multi-hop, adversarial) are present and at what size.

The `rustev.dataset/1` provenance field is `real` only when an admission
record exists; otherwise it stays `synthetic`.

### 3.2 Contamination and exposure

1. A final-test case whose content was sent to a provider before admission
   (for example during development) is recorded as exposed; exposed cases
   are reported separately and excluded from the headline figure.
2. For a public dataset, the record states whether the backend's training
   data may include it, as a stated limit; Rustev cannot verify it.
3. For ranking tasks (spec 008), held-out choices carry the exposure caveat
   of spec 008 4.2: users chose only among what they were shown.

### 3.3 Freezing before the final test

1. Before any final-test case is evaluated, the qualification record fixes
   the evaluator configuration id, gates with thresholds and minimum
   coverage (spec 004 3.5.8), baselines, binding identities, calibration
   artifact ids, sample-size targets and the analysis to be reported.
2. Model selection and calibration fitting (spec 004 3.6) use only their
   own splits. A final-test result never feeds back into a threshold,
   calibration, option description or plan.
3. A change after the freeze is a new qualification with a new final-test
   split; the old result stays in the record as history.

### 3.4 Measurement

Per task, on the same dataset, split and configuration, candidate and
baselines side by side (spec 004 cannot put two backends in one report, so
reports are paired under identical dataset, configuration and adapter
ids):

1. comparable coverage, acceptance coverage, and abstentions by reason,
   before any accuracy figure;
2. error among accepted, accuracy or macro-F1, with every denominator;
3. mean log loss, multiclass Brier and binned reliability for probability
   outputs, uncalibrated and calibrated, calibration fitted on the
   calibration split only;
4. the same by declared slice, with slices below the minimum size reported
   as unknown, not as a figure;
5. a regression gate against the previous qualified result for the same
   task, when one exists; and
6. baselines: the exact plan steps where the task allows, and spec 005's
   rules backend.

### 3.5 Claim scoping

A qualification record's claim sentence names the dataset id, split,
evaluator configuration id, binding identity, call dates, served identity
disclosure (spec 012 3.6.3) and population, and lists its limits. It never
asserts behavior on another population, another binding, real user data
not represented, or a served model version it could not observe. Reports on
synthetic or synthetic-provenance data remain labeled as such (R-04, R-28).

## 4. Out of scope

Collecting or labeling data (an owner act, possibly commissioned). Real
user data without a privacy decision. Live calls and spend without an owner
authorization. Training or distillation (backlog RUSTEV-019). Changing any
plan, threshold or calibration from final-test results.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A dataset marked `real` has no admission record | Refused as qualification input; any claim citing it is rejected. |
| Labels were produced by the model under test | Admission refused: labels are not independent. |
| A calibration is fitted on the final-test split | Refused by spec 004 3.6.4; the qualification is void. |
| The evaluator configuration digest changes after final-test evaluation began | A new qualification is required; the result is not reported as the frozen one. |
| A slice has fewer cases than the declared minimum | Reported as unknown for that slice. |
| A report claims quality on real user mail from a public dataset result | Rejected: population outside the claim. |

## Acceptance (draft)

- One admission record and one qualification record per task, each
  reviewed by the owner, with every spec 004 denominator reconciling to its
  cohort.
- Section 5's cases are exercised on labeled synthetic fixtures before real
  data is used, to prove the procedure refuses them.

## 6. Open questions

1. **Source.** A public dataset (which, under which license) or a
   commissioned independently labeled set? Recommendation: one per
   reference task, starting with support routing, where public ticket
   datasets exist.
2. **Minimum sizes.** What minimum final-test size per task and slice
   supports a reported figure? Recommendation: fix it per task in the
   freeze, with an interval (question 3).
3. **Intervals.** Spec 004 reports point estimates only. Amend spec 004 to
   add a declared interval method (for example Wilson for proportions,
   percentile bootstrap with a recorded seed for other metrics), or compute
   intervals outside Rustev? Recommendation: a separate spec 004
   amendment, so intervals share the governed denominators.
4. **Retention location.** Where are the retained reports, details and
   digests kept, and for how long?

---
id: "023-real-data-quality-qualification"
title: "Semantic quality and calibration qualification on real labeled data"
status: approved
implementation: pending
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
  is scoped to the evaluated population, task and binding identity.
  Approved (A-22). Admitted datasets and their per-case details stay outside
  this repository, held by the host (OD-7); approval authorizes the
  procedure and its refusal checks, not any particular dataset, live call or
  spend beyond R-29.
depends_on:
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "007-support-routing-package"
  - "008-reference-package-lodging"
  - "012-jev-integration"
  - "014-report-integrity"
extends:
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
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

Approved by the owner (A-22, 2026-10-16, under the 2026-10-16 delivery work
order); implementation is pending, so this is not yet a claim about code or
evidence. Backlog item RUSTEV-011 has no other owning spec; this is it. No
appropriately licensed or independently labeled dataset is admitted yet
(R-04). Owner decision OD-7 (R-37) keeps R-04 in force: synthetic fixtures
prove mechanics, and quality claims need data admitted here, which hosts
keep outside this repository.

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

## Acceptance

- Section 5's cases are exercised on labeled synthetic fixtures before real
  data is used, to prove the procedure refuses them; each refusal is a
  repository-local test with no real data and no network.
- Evidence is recorded per dataset (amended 2026-10-22, D-023-1, A-46): when a
  dataset is admitted, its admission record and qualification record are
  reviewed by the owner, with every spec 004 denominator reconciling to its
  cohort. Until a dataset is admitted for a task, every quality claim for
  that task is refused (3.5), and that refusal is the delivered state. The
  records' data stays outside the repository; the repository holds at most
  the claim sentence, digests and aggregate figures, naming no host.
- `make code`, declared verification, and `make gate` pass.

## Verification

The first acceptance item (section 5's refusals on labeled synthetic
fixtures, repository-local, no real data, no network) is run here, by
`make verify` (023 is in `VERIFIED_SPECS`, in CI's `evaluation` group). The
second (one owner-reviewed admission and qualification record per task)
needs an admitted dataset, which no host has offered; no existing report
is a qualification under this spec.

```verify:cli
# Section 5, by row, and 3.1 to 3.5: admission, freeze, measurement and
# claim scoping on SYNTHETIC fixtures only.
cargo test -p rustev-eval --locked --test qualification
cargo clippy -p rustev-eval --all-targets --locked -- -D warnings
# Negative control: each refusal's seeded defect must be detected.
sh crates/rustev-eval/mutation/seeds.sh "spec 023"
```

## Implementation record

**2026-10-21, increment 1: the procedure and its refusal checks.** What
can be built and proven without an admitted dataset. Implementation stays
`pending`: no dataset is admitted, no qualification exists, and no quality,
calibration or usefulness claim is made or implied by anything below.

- `crates/rustev-eval/src/dataset.rs`: `rustev.dataset-admission/1`
  (`AdmissionRecord`, identified by its digest, Decimal strings only) with
  every statement of 3.1 and 3.2: source (public with publisher, version
  and content digest, or commissioned with labeler and instruction), license
  permissions, privacy basis, labeling method (guidelines, annotators per
  case, an agreement statistic with its denominator, adjudication, label
  producers), the systems under test, the task mapping bound to the spec
  014 rules digest with unmapped and unlabeled counts, split assignment
  with method, seed, per-split counts and the near-duplicate limit, the
  population with its slices, exposure (training-data statement, exposed
  final-test cases, ranking caveat) and the owner's review. `admit` refuses
  a synthetic manifest (I-1), a record for another manifest, any empty
  statement, real-user-derived data without a privacy decision, a model
  labeler that is a system under test or of its family, another adapter, a
  source label mapped twice, missing labels that do not reconcile with the
  manifest (dropped or invented cases), split counts or slice sizes the
  manifest does not hold, and exposed cases outside the final test. Only
  `admit` makes an `Admitted`.
- `crates/rustev-eval/src/report.rs`: `rustev.qualification/1`
  (`QualificationRecord`) and `freeze` (3.3): the admitted dataset only;
  the frozen evaluator configuration, version 2 with the admitted rules
  digest and adapter; an analysis declared by that configuration; a
  candidate model declared under test and of no labeler's family; an exact
  baseline or the reason the task allows none; a positive final-test
  target and a positive minimum for every declared slice, each slice a
  configuration subgroup; exactly the frozen calibrations, each qualified
  by spec 004's `fit::qualify` against the final test (missing lineage,
  the fitting split or a shared case, source or snapshot voids the
  freeze); and a replacement that supplies what it supersedes and shares
  no final-test source or snapshot with it (3.3.3). `measure` (3.4) takes
  spec 004 reports by role (candidate, calibrated candidate when
  calibrations are frozen, rules baseline, exact baseline unless not
  applicable) and refuses a missing or repeated role, a detail not bound
  to its report, a configuration other than the frozen one
  (`ConfigChanged`: a new qualification is required), another dataset or
  split, a report not over every final-test case once, and bindings other
  than the frozen ones (an exact baseline names no semantic artifact).
  Rows put coverage, exclusions by reason, acceptance coverage,
  abstentions by kind and labeled-proposal coverage, each with numerator
  and denominator, before spec 004's error among accepted, log loss, Brier
  and reliability values (R-1). `regression` runs each frozen gate through
  spec 004's `evaluate_gate`. `scope_claim` (3.5) composes the claim
  sentence from the freeze and the admission (dataset id, split,
  configuration id, binding, call dates, served identity disclosure,
  population) with the admission's limits, and rejects an unadmitted
  dataset, another freeze's measurement, another population, missing call
  dates and any unknown figure.
- Tests: `crates/rustev-eval/tests/qualification.rs`, one test per
  section 5 row (`a_real_dataset_without_an_admission_record_is_refused`,
  `labels_from_the_model_under_test_are_refused`,
  `a_calibration_fitted_on_the_final_test_voids_the_qualification`,
  `a_changed_configuration_needs_a_new_qualification`,
  `a_slice_below_its_minimum_is_unknown`,
  `a_claim_outside_the_admitted_population_is_rejected`), plus the
  positive path, admission completeness, exposure, report refusals, freeze
  preconditions and regression. The manifest is synthetic content marked
  `real` only to exercise the procedure, and every report is built by
  hand. Seeds: category `qualification refusals` in the eval harness, one
  defect per refusal; 8 of 8 detected.
- Placement: the code lives in the existing `dataset.rs` and `report.rs`
  because a new source file would change spec 021's reviewed archive list
  (`tools/rustev-release/archive-policy.json`); moving it to its own module
  is mechanical once that list may change.
- `Makefile` and `scripts/acceptance/verify.py` (additive): `023` joins
  `VERIFIED_SPECS` and the `evaluation` acceptance group, beside spec 004,
  whose crate it shares.

Limits found, for the owner (none changes this spec's text):

- A spec 004 report cannot leave cases out, and its detail records outcome
  keys, not correctness. Exposed cases (3.2.1) and slices (3.4.4) therefore
  get count figures (coverage, exclusions, acceptance, abstentions,
  labeled proposals) recomputed from per-case outcomes; error among
  accepted and probability figures are spec 004's whole-split values, shown
  per slice only where spec 004 already reports them (reliability by
  subgroup). With any exposed final-test case those values are unknown, and
  per-slice error among accepted is always unknown. Excluding exposed cases
  from, or computing per-slice, error and probability figures needs a spec
  004 amendment (a case exclusion list, or per-subgroup metrics).
- Spec 004 gates require an identical dataset, split and configuration,
  while 3.3.3 requires a new final-test split for a new qualification, so
  a regression gate against a previous qualification (3.4.5) is unknown
  unless both share the dataset; `regression` reports that unknown rather
  than a pass.
- Intervals (6.3) wait for the spec 004 amendment of #76 child E2; every
  claim states point estimates as a limit.

**Owner decision D-023-1 (A-46, 2026-10-22 delivery work order).** Acceptance
item 2 is evidence recorded per dataset rather than a precondition of
completion. This spec's implementation is the governed procedure: its
summary already states that approval "authorizes the procedure and its
refusal checks, not any particular dataset", and 6.4 (OD-7) keeps every
dataset, record and per-case detail with the host, outside this
repository. Admitting a dataset is therefore a host act performed through
the delivered procedure at any time, not a code change, and nothing in the
repository could ever mark it done. Completion, a separate change made
against this amended text, would claim exactly what is delivered: every
unit of section 2 is written, the refusals of section 5 are tests, and
verification passes. It would claim no quality figure: until a
dataset is admitted for a task, 3.5 refuses every quality claim for that
task, which is the correct, settled behavior rather than an outstanding
work item. Each admission later adds its claim sentence, digests and
aggregate figures here, as acceptance item 2 states.

## 6. Questions settled at approval

The owner settled the draft's open questions in the 2026-10-16 delivery work
order and tracking issue #76 (OD-1, OD-7):

1. **Source.** A dataset is supplied by a host, or is an appropriately
   licensed public dataset, and is admitted under 3.1. Rustev names no host
   dataset; per task, the first candidate is the one a host offers for
   admission.
2. **Minimum sizes.** Fixed per task and slice in the freeze (3.3.1), with
   the interval of question 3; below it a figure is reported as unknown.
3. **Intervals.** Computed inside Rustev by the spec 004 amendment that
   tracking issue #76 child E2 delivers (declared bootstrap intervals with a
   recorded seed), so intervals share the governed denominators. Until it
   lands, a qualification reports point estimates with that limit stated.
4. **Retention location.** Datasets, per-case details and exchanges stay
   with the host, outside this repository, under the host's retention
   policy. The repository may keep a qualification's claim sentence,
   digests and aggregate figures.

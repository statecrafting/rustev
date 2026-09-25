---
id: "015-per-case-bundle-failures"
title: "Per-case reporting of unusable replay bundles (amends 004)"
status: approved
implementation: complete
created: "2026-09-25"
summary: >
  An amendment of approved spec 004 (R-16). Evaluation takes
  parsed replay bundles only, so a retained bundle whose bytes cannot be
  read, exceed their limit or do not parse cannot be reported as one
  incomparable case: a host must either stop the whole evaluation (the
  CLI's choice, spec 006 E-31) or leave the case out, which the library
  then reports as `bundle-missing`, misstating corruption as absence.
  This amendment lets the caller supply a typed bundle-load failure per case,
  reported as incomparable with its own reason, counted in coverage and
  excluded from quality. Approved (A-11, 2026-09-25) and implemented.
amends:
  - "004-evaluation-and-replay"
extends:
  # The evaluation inputs take a parsed bundle or a load failure per case
  # (spec 004, 3.3.1 and 3.5.4).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `eval` and `calibrate fit` report an unusable bundle per case (spec 006
  # E-31 superseded).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Adds 015 to `make verify`.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
---

# 015: Per-case reporting of unusable replay bundles (amends 004)

Approved (A-11, 2026-09-25) as its own reviewable change before any code
(R-16), and implemented; see the implementation record. Drafted on 2026-09-25 under the
owner decision of 2026-09-24 to draft the spec 004 amendments once 012 was
merged, and to stop for approval. Spec 004's approved text is not edited;
this spec records the change.

## 1. Purpose

Spec 004 3.3.1 makes every unavailable input `incomparable` with a typed
reason, and 3.5.4 counts incomparable cases by reason inside the coverage
denominator. That holds for items a bundle references, but not for the
bundle itself: `report::Inputs::bundles` maps case ids to parsed
`ReplayBundle`s, and a case with no entry is `bundle-missing`. A host that
retained a bundle and cannot use it has two dishonest options: drop the
case, so corruption is counted as absence, or stop the evaluation, so one
damaged file hides every other case. Spec 006 chose to stop (E-31) and
recorded that reporting it per case would be an amendment to 004. This is
that amendment.

## 2. Territory

When approved and delivered: `crates/rustev-eval/` (the evaluation inputs,
case outcomes and detail reasons) and `crates/rustev-cli/` (`eval` and
`calibrate fit` report such a case instead of stopping; E-31 is
superseded). No contract, core or runtime change.

## 3. Behavior

### 3.1 Bundle inputs

1. Each case's entry in the evaluation inputs is either a parsed bundle,
   as today, or a bundle-load failure the caller observed:
   - `inaccessible`: the host could not read bytes it holds (an I/O error,
     a refusal, a file that is not a regular file);
   - `oversized`: the bytes exceed `REPLAY_V1`'s document limit;
   - `corrupt`: the bytes are not valid JSON, not a `rustev.replay/1`
     document, or fail its structural checks.
   These reuse the meanings of spec 004 3.1's availability vocabulary; the
   bundle is not an item, so the reasons are prefixed (3.2).
2. A case with no entry stays `bundle-missing`. A caller must not supply a
   load failure for a bundle it does not hold, and must not omit a case
   whose bundle it holds and could not use.
3. The library does not read storage (spec 004 section 2): the caller
   classifies the failure; the library only records it. A failure carries
   no bytes and no parser message beyond a bounded, non-authoritative
   note of at most 256 bytes for the operator.

### 3.2 Reporting

1. A load-failure case is `incomparable` with reason `bundle-inaccessible`,
   `bundle-oversized` or `bundle-corrupt`, has no scope in its case detail
   (as `bundle-missing` today), is inside the coverage denominator, is
   excluded from every quality metric and is counted under its reason.
   Detail counts reconcile exactly as for every other incomparable case.
2. Under a candidate, such a case is incomparable for both sides; it is
   never reused, never a candidate failure and never agreement.
3. Calibration fitting skips such a case and reports it by reason, as it
   already does for every non-reproduced case.
4. Gates see the loss only through coverage: a minimum comparable coverage
   fails when enough bundles are unusable (spec 004, 3.5.8). No gate
   passes because a damaged case was left out.

### 3.3 The CLI

1. `eval` and `calibrate fit` classify each existing bundle file per 3.1
   (`inaccessible` for an open or read error or a non-regular file,
   `oversized` past the document limit plus one byte, `corrupt` for a
   parse or schema refusal) and continue. The command's exit code and
   output follow the evaluation, as for any incomparable case.
2. Failures before any case is evaluated keep their exit codes: the
   dataset, configuration, adapter or output directory, a case id that
   cannot name a file, and the per-command byte budget (spec 006, 3.3).
   Exhausting the budget is not a per-case failure, because it depends on
   read order rather than on the bundle.

### 3.4 Compatibility

`rustev.eval-report/1`, `rustev.eval-detail/1` and every contract schema
keep their shape; three reason values are added. Reports over datasets with
no unusable bundle are byte-identical. A consumer that treats incomparable
reasons as a closed set must learn the three new values.

## 4. Out of scope

An expected bundle digest in the dataset manifest (which would turn a
substituted but well-formed bundle into `corrupt` rather than a
cross-reference failure; open question Q-2); any retry of transient read
errors; storage, retention or deletion.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| One bundle in a split is truncated JSON | That case `incomparable{bundle-corrupt}`; every other case evaluated; coverage reduced by one case. |
| One bundle is one byte past `REPLAY_V1` | `incomparable{bundle-oversized}`; at most the limit plus one byte read. |
| One bundle path is a directory, or unreadable | `incomparable{bundle-inaccessible}`. |
| A bundle file is absent | `bundle-missing`, unchanged. |
| A candidate evaluation over a corrupt bundle | Incomparable for both sides; not reused, not agreement. |
| `calibrate fit` over a split with one corrupt bundle | The case is skipped and reported as `bundle-corrupt`; the fit uses the rest. |
| Every bundle corrupt with a minimum comparable coverage declared | Coverage gate fails; quality metrics unknown. |

## Acceptance

- Eval and CLI tests cover each row of section 5; spec 006's existing
  tests that expect `eval` to stop on an unparsable bundle change to the
  per-case outcome, and that change is listed in the implementation.
- The seed harnesses gain seeds that report a load failure as
  `bundle-missing`, drop it from the coverage denominator or count it in a
  quality metric; each must be detected.

## Open questions

Owner decision of 2026-09-25 (R-31), answering Q-1 and Q-2: "Specs 014, 015 and 007 are APPROVED, accepting the recommendation written in each draft for every open question".
Each question below is therefore settled as its recommendation states; the
text is kept as the record of what was asked.

- Q-1: whether `inaccessible` should instead keep stopping the CLI, since
  an I/O error may be transient rather than a property of the bundle.
  Recommendation: report it per case; the operator reruns, and a report
  that says `bundle-inaccessible` is honest about what happened.
- Q-2: whether to add an optional expected bundle digest per case to the
  dataset manifest now. Recommendation: not in this amendment; it changes
  `rustev.dataset/1` and deserves its own change.

## Implementation record

- `crates/rustev-eval/src/bundle.rs`: `BundleInput` (a parsed bundle or a
  `LoadFailure`), `BundleLoad` (`Inaccessible`, `Oversized`, `Corrupt`,
  with their `bundle-*` codes) and the operator note, cut to 256 bytes at a
  character boundary and never written to a report.
- `crates/rustev-eval/src/report.rs`: `Inputs::bundles` maps case ids to
  `BundleInput`. A failure is `incomparable` with its code and no scope; the
  baseline's plan id is taken from a usable bundle only.
- `crates/rustev-cli/src/io.rs`: `Reads::read_classified` tells a failure of
  the file from an exhausted command budget. `crates/rustev-cli/src/eval.rs`
  `read_bundles` supplies a failure per case (open or read error or a
  non-regular file: inaccessible; past the limit: oversized; parse or schema
  refusal: corrupt) and stops only on the budget (3.3.2);
  `crates/rustev-cli/src/calibrate.rs` skips such a case by its reason. The
  standalone `replay` command still refuses an unusable bundle: it evaluates
  no cases.
- Spec 006's test that expected `eval` to stop on an unparsable bundle
  (`eval_input_errors_stop_the_command`) lost that case; the per-case
  outcome is `an_unusable_bundle_is_reported_per_case_not_fatal`.
- Section 5, by row: truncated, oversized, directory and absent bundles
  through the CLI (`crates/rustev-cli/tests/eval.rs`) and the library
  (`an_unusable_bundle_is_incomparable_for_its_case_only`); a candidate over
  a corrupt bundle (`a_candidate_over_an_unusable_bundle_is_incomparable`);
  `calibrate fit` with a corrupt calibration bundle
  (`fitting_uses_the_calibration_split_and_qualification_needs_lineage`);
  every bundle of the split corrupt
  (`unusable_bundles_fail_the_coverage_gate_and_never_pass_it`).
- Clarification of the last row: a baseline report names its plan from a
  usable bundle in the inputs. With a usable bundle outside the split, the
  report is produced, its quality metrics are unknown and the coverage gate
  fails. With no usable bundle at all, `evaluate` refuses (`EmptySplit`), as
  it already did when every bundle is absent, so no report exists and no
  gate can pass.
- Seeds, `unusable bundles` in both harnesses (run by the verification of
  specs 004 and 006): a failure reported as `bundle-missing`, dropped from
  the coverage denominator, or counted as comparable; in the CLI, a corrupt
  bundle treated as absent, an unreadable bundle stopping the command, an
  oversized bundle reported as corrupt, and fitting skipping a failure as
  missing.

## Verification

```verify:cli
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo clippy -p rustev-eval -p rustev-cli --all-targets --locked -- -D warnings
```

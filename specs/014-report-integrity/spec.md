---
id: "014-report-integrity"
title: "Task adapter rules bound into evaluation identity (amends 004)"
status: draft
implementation: pending
created: "2026-09-25"
summary: >
  A proposed amendment of approved spec 004 (R-16). The evaluator
  configuration names its task adapter only by name and version, so the
  configuration identity a report carries does not change when the
  adapter's correctness rules change under the same version, and a gate
  can compare two reports that judged correctness differently. Spec 006
  closes this for the CLI alone, by writing the adapter document beside
  each report and comparing bytes. This draft binds a digest of the
  adapter's rules into a new evaluator configuration version, so the
  report identity itself covers correctness, and makes a gate unknown
  whenever either side's rules are unbound. Proposal only; claims no code.
amends:
  - "004-evaluation-and-replay"
depends_on:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
---

# 014: Task adapter rules bound into evaluation identity (amends 004)

Draft: a proposal, not a claim about code. Drafted on 2026-09-25 under the
owner decision of 2026-09-24 to draft the spec 004 amendments once 012 was
merged, and to stop for approval. Spec 004's approved text is not edited;
if approved, this spec records the change, as 013 does for 003.

## 1. Purpose

Spec 004 3.5.3 says the evaluator configuration "identifies task
adapter/version" and that its canonical digest is the report's
`EvaluatorConfigId`. In the delivered code the configuration holds an
`AdapterRef { name, version }` and nothing about what the adapter counts as
correct. Correctness is the adapter's (3.5.5), so two reports with equal
`EvaluatorConfigId` can have computed error among accepted under different
rules. A gate over them (3.5.8) then compares numbers that do not measure
the same thing, and it passes or fails silently.

Spec 006 found this (3.8.3) and closed it for the CLI only: `eval` writes
the adapter document beside the report, and `gate` is `unknown` unless both
adapter documents are byte-identical. A host that calls
`rustev_eval::report::evaluate` and `evaluate_gate` directly, or that keeps
reports without the adapter file, has no such check, and the report's own
identity still does not cover the rules. This amendment moves the binding
into the evaluation contract.

## 2. Territory

When approved and delivered: `crates/rustev-eval/` (the configuration
document, the task adapter trait, report construction and gates) and the
mechanical update of `crates/rustev-cli/` (its declarative adapter states
its rules digest; `gate` keeps its byte comparison as a second check).
No contract, core or runtime change.

## 3. Behavior

### 3.1 The adapter's rules identity

1. The task adapter trait gains one required method returning an
   `AdapterRules` value: `bound{digest}` or `opaque`. There is no default
   implementation, so every adapter states which it is.
2. `bound{digest}` is the SHA-256 content digest (the existing
   `ContentDigest`) of a canonical document that fully determines the
   adapter's label check and correctness function. The adapter owns that
   document; the digest is computed over its canonical bytes, never over
   a debug or display rendering.
3. `opaque` is for an adapter whose rules no document determines (for
   example, Rust code). It is honest and allowed, and every comparison
   that needs the rules treats it as unknown (3.3).
4. The CLI's `rustev.task-adapter/1` is a complete declarative definition
   (spec 006, 3.8), so the CLI adapter is always `bound`, with the digest
   of its canonical document bytes, the same bytes `eval` writes as
   `adapter.json`.

### 3.2 Evaluator configuration, version 2

1. A new `rustev.evaluator-config/2` document holds everything
   `rustev.evaluator-config/1` holds, with the adapter reference extended
   to `{ name, version, rules }`, where `rules` is `{ "bound": <digest> }`
   or `"opaque"`. Its canonical digest is the `EvaluatorConfigId`, as
   before, so the report's configuration identity now changes whenever the
   rules digest changes.
2. `evaluate` refuses a configuration whose adapter reference, including
   `rules`, differs from what the supplied adapter states (the existing
   adapter-mismatch error, extended to the rules).
3. `rustev.evaluator-config/1` remains readable and evaluable, with its
   bytes and identity unchanged. Its adapter rules are unbound: a
   version 1 configuration is treated as `opaque` for every purpose of 3.3.
4. New evaluations written by the CLI use version 2. The library evaluates
   either version as supplied; it never upgrades a version 1 document or
   relabels its identity.

### 3.3 Gates

A gate (spec 004, 3.5.8) is `unknown`, with a reason naming the side,
unless both reports' configurations are version 2 with `rules: bound` and
equal digests. This adds one precondition; every existing precondition,
and every way a gate is already unknown or fails, is unchanged. Unknown
never becomes pass.

### 3.4 Compatibility

- `rustev.eval-report/1`, `rustev.eval-detail/1`, `rustev.dataset/1` and
  every contract schema are unchanged. The report envelope already carries
  the `EvaluatorConfigId`; only the configuration it identifies gains a
  version.
- Existing version 1 reports keep their bytes and identities. A gate
  between two version 1 reports, which passes or fails today, becomes
  `unknown`. That is the intended strictening, and it is the one
  observable change for existing callers (open question Q-1).
- The CLI's adapter byte comparison (spec 006, 3.8.3) stays as an
  independent check; it cannot disagree with 3.3 for two CLI-written
  version 2 reports, and it still covers version 1 reports the CLI wrote.

## 4. Out of scope

Proving that an `opaque` adapter's code is unchanged; signing or
attesting reports (a digest proves integrity relative to expected bytes,
never producer identity, spec 004 section 1); any change to how
correctness is computed; migrating stored version 1 configurations.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Two CLI evaluations with the same adapter name and version, one rule changed | Different `EvaluatorConfigId`s; gate `unknown` naming the rules. |
| A library adapter returning `opaque` on either side | Gate `unknown`; the report is still produced. |
| A version 2 configuration whose `rules` digest differs from the supplied adapter's | `evaluate` refuses with the adapter-mismatch error before any case. |
| Two version 1 reports that pass a gate today | Gate `unknown` (version 1 rules are unbound). |
| A version 1 configuration evaluated again | Identical configuration bytes and `EvaluatorConfigId`. |

## Acceptance

- Eval tests cover each row of section 5.
- The seed harness gains seeds that ignore the rules digest in the
  configuration identity, in the adapter-mismatch check and in the gate
  precondition; each must be detected.
- Existing eval and CLI suites pass with only the mechanical trait update
  and the version 1 gate expectations changed as 3.4 states.

## Open questions

- Q-1: making gates over version 1 reports `unknown` is the safe reading
  of spec 004's "incompatible measurements yield unknown". The
  alternative is to leave version 1 gates as they are and only bind
  version 2. Recommendation: `unknown`, because a passing gate over
  unbound rules is exactly the silent comparison this amendment removes.
- Q-2: whether `AdapterRules::opaque` should exist at all, or every
  library adapter should be forced to author a rules document.
  Recommendation: keep `opaque`; forcing a document on Rust adapters
  would invite a digest of something other than the rules.

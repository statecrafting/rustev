---
id: "064-selection-gate-case-regressions"
title: "Selection gate: a clause on per-case regressions against the baseline (amends 029 and 061)"
status: approved
implementation: pending
created: "2026-11-06"
summary: >
  An amendment of approved specs 029 (3.12) and 061 (3.7), for release
  0.5.1 (#537), ratified by the owner on 2026-11-06 (A-83). Every selection clause today compares `pooled`
  values of the cohort `all` or of each slice, so a candidate that improves
  most cases and collapses on a few passes `not_worse_than`. This spec adds
  one clause to `rustev.selection-gate/1`, `case_regressions`, which counts
  the dataset cases whose candidate value is worse than their baseline
  value by at least a declared absolute amount, in the metric's declared
  direction, and fails when more than a declared number regress. A case's
  value is 029 3.3.3's `pooled` computation restricted to that case, read
  from the per-case documents the baseline and each candidate already bind
  (`rustev.eval-cases/1`, or 061's `rustev.host-cases/1`), so no baseline
  document changes. The boundary is compared in exact decimal; a missing,
  partial or unknown case value makes the clause unknown, never a pass;
  `rustev.selection/1` lists the regressing cases. Gates and selections
  that do not use the clause keep their bytes and identities.
amends:
  - "029-evaluation-repeatability"
  - "061-host-scored-repeat-sets"
extends:
  # The clause, its check and evaluation in the shared selection code.
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `gate select` prints the clause result; no new flag.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # One CLI conformance case for the clause.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
depends_on:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "041-ranking-and-rubric-metrics"
  - "061-host-scored-repeat-sets"
  - "063-node-binding-parity"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A `case_regressions` clause passes only when every compared case has a known value on both sides and the number of regressing cases is at most `at_most`; a case with a measure on one side only, a measure missing from some run of a side, or an unknown or non-finite value makes the clause unknown unless the definite regressions already exceed `at_most`, and it is never counted as a pass, a non-regression or a regression by default."
    anchor: "3-4-clause-result"
  - id: "I-2"
    kind: invariant
    text: "Whether a case regresses is decided in exact decimal: the baseline and candidate case values are taken as the shortest decimals that round-trip to their binary64 values and `by` as its exact decimal, and a case worse by exactly `by` regresses."
    anchor: "3-3-regression-of-one-case"
  - id: "R-1"
    kind: requirement
    text: "A selection gate that does not use `case_regressions` has the same canonical bytes and identity as today, and `gate select` over it writes the same `rustev.selection/1` bytes, stdout and exit code."
    anchor: "3-7-compatibility"
---

# 064: Selection gate: a clause on per-case regressions against the baseline (amends 029 and 061)

Approved: ratified by the owner on 2026-11-06 (A-83) for release 0.5.1;
the implementation is pending. The approved texts of 029 and 061 are not
edited; this spec records what it adds to them.

## 1. Purpose

A host's evaluation gate rejects a candidate when too many dataset cases
regress by at least a fixed amount against the baseline, even when the
pooled value holds: for example, "at most 1 case may drop by 5 or more
points on `rubric/quality`". Spec 029 3.12.3 cannot state this. Every
clause compares `pooled` values of the cohort `all` (or of each slice
under `all_slices`), so a candidate that improves most cases and collapses
on a few passes `not_worse_than`. The host keeps this check outside Rustev
today, over per-case values Rustev already computed and bound.

This spec adds a clause over per-case values. The values it reads already
exist: every Rustev-evaluated run writes `rustev.eval-cases/1` (029 3.2.1),
every host run `rustev.host-cases/1` (061 3.3.1); a baseline binds each
run's case document digest (029 3.8.1, 061 3.7.1) and `gate select`
verifies the baseline against `--baseline-eval` before comparing. So the
baseline already carries its per-case values by digest, and no baseline
document changes.

## 2. Territory

The evaluation library `crates/rustev-eval/` (spec 004's unit, amended by
029, 041 and 061): the clause's parsing, check and evaluation in the
selection code that 029's and 061's selections share. The CLI
`crates/rustev-cli/` (spec 006's unit, amended by 029 3.13): no new flag;
stdout of `gate select` prints the new clause result. The conformance
suite `tools/rustev-conformance/` (027): one CLI case for the clause.

Ratification added the `extends` edges to `crates/rustev-eval/` (004,
amending), `crates/rustev-cli/` (006, amending) and
`tools/rustev-conformance/` (027, additive); the edges to `Makefile` and
`scripts/acceptance/` (001, additive) are added by the change that
completes this spec.

Unchanged: every plan, definition, `PlanId` and plan golden;
`rustev.repeat-config/1`, `rustev.eval-cases/1`, `rustev.host-cases/1`,
`rustev.repeat-report/1`, `rustev.host-repeat-report/1`,
`rustev.baseline/1` and `rustev.host-baseline/1`; the contract crate, its
schemas and the generated binding types; exit codes. No new dependency, no
I/O in eval, no clock.

### 2.1 Binding principles

- **Proposals, never grants.** A clause result is evidence for the host's
  choice, as 029 3.12.7 states.
- **Derivation and lineage.** A case value keeps its metric's derivation
  (041 3.2.5); the clause result names each regressing case, and the case
  documents it read are bound by the baseline and the candidate's repeat
  report.
- **Unresolved, never a default.** A case value that is missing, partial,
  unknown or non-finite is unknown, never zero, never a non-regression and
  never a regression (I-1).
- **No silent kind conversion.** Only count-ratio and mean measures have a
  per-case value; a sample (quantile) metric or a total has none, and the
  clause over it is unknown (3.2.3).

## 3. Behavior

### 3.1 The clause

1. 029 3.12.1's grammar gains one metric clause:

   ```text
   clause = ... | { metric, case_regressions:
                    { by: { absolute: decimal }, at_most: integer } }
   ```

2. `metric` must have a declared direction in `metrics` (as for every
   clause; otherwise the document is refused). `by.absolute` is a Decimal
   string (spec 002 3.4.1) greater than 0; `at_most` is an integer from 0
   to 1 000 000. Unknown fields and duplicate keys are refused, as for the
   rest of the document. A `case_regressions` clause may appear in
   `require` and in `and_any`; it is refused inside `all_slices`
   (`invalid_input`, `case-regressions-in-slices`).
3. `by` is wrapped (`{ absolute: ... }`) so that a relative amount can be
   added later without changing this form; no other wrapping is accepted
   here.

### 3.2 The per-case value

1. **Cohort.** The clause reads the cohort `all`: every case of the
   split, in manifest (or host record) order. The preconditions of 029
   3.12.2 and 061 3.7.4 still hold first, so both sides list the same case
   ids in the same order.
2. **Value.** For a case and a side, the case value is 029 3.3.3's
   `pooled` computation restricted to that case: its numerator
   contributions over the side's runs, in run order, divided by its
   denominator contributions over the same runs (integers exactly, binary64
   left to right, one division), computed by the same function that
   computes `pooled`. With N = 1 it is the run's own contribution.
3. **Kinds.** A case value exists only for a metric whose per-case measure
   is a count ratio or a mean (029 3.2.2). For a sample metric
   (`latency_ms/*`, `queue_ms/*`, `host/latency_ms/*`), a total, or a
   metric with no per-case measure (029 3.2.3, `not-case-decomposable`),
   the clause is unknown with reason `not-case-comparable`, naming the
   metric. A quantile of one case's samples is not offered.
4. **Presence.** A case is **measured** on a side when every run of that
   side holds a measure of the metric for it; **unmeasured** when no run
   does; **partial** otherwise. A case value is **unknown** when any of its
   run measures is `unknown` (029's per-case unknown contribution), when
   its denominator sum is zero, or when the division is not finite.
5. **Pairing.** A case measured on both sides with known values is
   **compared**. A case unmeasured on both sides is not compared and is
   counted (`unmeasured`); for example, a rubric metric has no measure for
   a case that abstained in every run of both sides. Every other case (a
   case measured on one side and unmeasured on the other, partial on
   either side, or with an unknown value on either side) is **unresolved**
   and named with its reason: `unpaired`, `partial` or `unknown`.

### 3.3 Regression of one case

1. With `b` the baseline case value, `c` the candidate case value and `t`
   the clause's `by.absolute`, a compared case **regresses** when:
   - higher is better: `b - c >= t`;
   - lower is better: `c - b >= t`.
2. The comparison is 029 3.12.3's exact decimal comparison: `b` and `c`
   are taken as the shortest decimals that round-trip to their binary64
   values (what Rust's `{}` prints), `t` as its exact decimal, and the
   difference and the comparison are computed exactly, never in binary64.
   A case worse by exactly `t` regresses (I-2), as the issue's "drop by 5
   or more" reads.
3. The **drop** of a regressing case is `b - c` (higher is better) or
   `c - b` (lower is better), as that exact decimal.

### 3.4 Clause result

1. With `R` the number of regressing cases and `U` the number of
   unresolved cases:
   - **fails** when `R > at_most`, whatever `U` is: a definite failure
     needs nothing else (029 3.9.2, 3.12.5);
   - **passes** when `U = 0` and `R <= at_most`;
   - is **unknown** otherwise, naming the first unresolved case in
     manifest order and its reason.
2. The clause is also unknown, before any case is read, under 029 3.12.4
   and 041 3.12.2 as for every metric clause (a candidate observation that
   is `historical`, a metric of an `opaque` scorer), and under 3.2.3. A
   precondition failure makes it unknown as every clause (029 3.12.2).
3. Verdicts, ranking and exit codes are 029 3.12.5, 3.12.6 and 3.14
   unchanged: a failing `case_regressions` clause in `require` rejects the
   candidate naming `require[i]`; an unknown one makes it `unknown` unless
   another clause fails; exit codes stay 0, 11 and 12.

### 3.5 Output

1. The clause's entry in `rustev.selection/1` (029 3.12.7) keeps
   `clause` (`require[i]` or `and_any[i]`), `result` and `slices` (always
   empty for this clause) and gains a member `cases`, written only for a
   `case_regressions` clause:

   ```text
   cases = { compared: integer, unmeasured: integer,
             regressions: { total: integer, truncated: bool,
               list: [ { case: case-id, baseline: number,
                         candidate: number, drop: decimal } ] },
             unresolved: { total: integer, truncated: bool,
               list: [ { case: case-id,
                         reason: "unpaired" | "partial" | "unknown",
                         side: "baseline" | "candidate" } ] } }
   ```

   `baseline` and `candidate` are the binary64 case values (the document
   is record canonical and already holds binary64 numbers); `drop` is the
   exact decimal of 3.3.3. Each list is in manifest order and holds at
   most 10 000 entries; `total` always states the full count and
   `truncated` whether the list is complete (as 029 3.10.3). For a case
   unresolved on both sides, `side` names the baseline.
2. When the clause is unknown before any case is read (3.4.2), `cases` is
   null.
3. Stdout of `gate select` prints, for the clause, its result, `R` with
   `at_most`, `U`, and the first ten regressing cases with their drops.
   Over host sets every figure keeps `assertion: "host-asserted"` (061
   3.5.3).

### 3.6 Host sets (amends 061)

1. Over a host baseline and host candidates (061 3.7), the case values
   come from each run's `rustev.host-cases/1` (061 3.3.1), which the host
   baseline binds and each candidate's import output holds; re-assembly
   (061 3.3.4) has verified them before any clause is read.
2. A case that is `failed` in a run has no score measure in that run
   (061 3.3.2), so a score metric over it is `partial` or `unmeasured` on
   that side under 3.2.4. Equal stable cohorts stay not required (061
   decision 4); the clause result names such cases rather than skipping
   them.
3. `host/` count-ratio metrics (`host/answered_rate` and the others of 061
   3.3.3) have per-case values (0 or 1) and may be named, though a
   regression of `by` at most 1 on them is a change of outcome;
   `host/latency_ms/*` is a sample metric (3.2.3); `host/cost_per_1000` is
   a mean and has a per-case value, unknown when the run's cost is.

### 3.7 Compatibility

1. `rustev.selection-gate/1` keeps its name. A gate that does not use the
   clause has the same canonical bytes and gate identity; a gate that uses
   it is refused by earlier releases (unknown field), never misread.
   `rustev.selection/1` keeps its name: `cases` is written only for this
   clause, so every selection over a gate without it is byte-identical
   (R-1). No new version name is introduced.
2. No plan construct changes, so no `PlanId` and no plan golden changes.
   Every existing eval, CLI, spec 029, spec 061 and conformance golden is
   byte-identical.
3. The Node binding's repeatability commands (063 3.9) pass gate
   documents and selections as bytes through the same Rust functions, so
   the clause reaches the binding with no binding change; the WebAssembly
   package has no repeatability command and gains nothing.

## 4. Out of scope

Relative amounts (`by: { relative }`); per-slice case regressions;
quantiles over one case's samples; weighting cases; any change to
`rustev.baseline/1`, `rustev.host-baseline/1` or the case documents;
diffs (029 3.10), whose per-case change rule is unchanged; significance
tests over case deltas.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `by.absolute` of `0` or negative | `invalid_input`; the gate is refused. |
| `at_most` negative or above 1 000 000 | `invalid_input`; the gate is refused. |
| `case_regressions` naming a metric with no declared direction | `invalid_input`; the gate is refused. |
| `case_regressions` inside `all_slices` | `invalid_input`, `case-regressions-in-slices`. |
| A case worse by exactly `by` | Counted as regressing. |
| A case worse by one unit of the last printed digit less than `by` | Not regressing. |
| `at_most + 1` regressing cases and other cases unresolved | Clause fails; candidate `rejected`. |
| `at_most` regressing cases and one `partial` case | Clause unknown naming the case; candidate `unknown`, exit 12. |
| A case measured by the baseline and unmeasured by the candidate | `unpaired`; clause unknown unless it already fails. |
| A case with an `unknown` measure in one run | `unknown`; clause unknown unless it already fails. |
| A case unmeasured on both sides | Not compared; counted in `unmeasured`. |
| The clause over `latency_ms/p0.95` or `host/latency_ms/p0.5` | Clause unknown, `not-case-comparable`. |
| The clause over a metric of an `opaque` scorer | Clause unknown (041 3.12.2). |
| The clause over a `historical` cost metric | Clause unknown (029 3.12.4). |
| More than 10 000 regressing cases | `list` holds the first 10 000, `total` the count, `truncated: true`. |
| A host candidate with a case `failed` in one run | That case `partial` for score metrics; clause unknown unless it already fails. |
| A gate without the clause, on today's inputs | Byte-identical selection, stdout and exit code. |

## Acceptance

- Gate parsing: the clause, every field rule of 3.1 and each refusal of
  section 5, with gate identities of existing gates unchanged.
- The case value equals, bit for bit, `pooled` computed over a one-case
  cohort, for a count ratio and a mean, at N = 1 and N = 3.
- Boundary tests in exact decimal: exactly `by`, one decimal unit below,
  and a pair whose binary64 difference would round across `by`.
- Clause results: pass, fail with unresolved cases, unknown for each
  unresolved reason, `not-case-comparable`, and truncation, in the
  library for Rustev-evaluated and host sets.
- `gate select` goldens for a qualifying, a rejected and an unknown
  candidate under the clause, and one CLI conformance case.
- Every row of section 5 has a test.
- Compatibility: every existing eval, CLI, 029, 061, conformance and plan
  golden byte-identical.
- Mutation seeds, each detected: a case worse by exactly `by` not
  counted; an unpaired case counted as non-regressing; a partial case
  computed over the runs that have a measure; the boundary compared in
  binary64; a definite failure reported unknown because of an unresolved
  case.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

## Decisions at approval

Decided in this text, with the reason beside each: the per-case value is
`pooled` restricted to one case (3.2.2, the issue's "as the pooled value
uses"); no baseline document changes, since baselines bind the case
documents (section 1); a case worse by exactly `by` regresses (3.3.2);
a definite failure dominates unresolved cases (3.4.1, 029's rule).

Ratified by the owner on 2026-11-06 (A-83), with every question the
draft left open answered as recommended:

1. **Document versions.** `rustev.selection-gate/1` and
   `rustev.selection/1` keep their names; the clause and the `cases`
   member are written only when used, so existing documents keep their
   bytes and identities and an older reader refuses a new gate rather
   than misreading it (3.7.1).
2. **Partial cases.** A case whose side lacks a measure in some of its
   runs is unresolved (`partial`) and makes the clause unknown unless it
   already fails (3.2.4, 3.4.1).
3. **Slices.** The clause is refused inside `all_slices` in 0.5.1
   (3.1.2).
4. **Sample metrics.** Unknown, `not-case-comparable`; no per-case
   quantile (3.2.3).
5. **Unpaired cases.** A case measured on one side only is unresolved
   (`unpaired`), never counted as a regression (3.2.5).

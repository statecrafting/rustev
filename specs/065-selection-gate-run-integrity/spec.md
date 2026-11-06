---
id: "065-selection-gate-run-integrity"
title: "Selection gate: run integrity failures void the comparison instead of rejecting (amends 029 and 061)"
status: draft
implementation: pending
created: "2026-11-06"
summary: >
  A proposed amendment of approved specs 029 (3.12) and 061 (3.7), for
  release 0.5.1 (#538). A run with integrity failures (results the
  evaluator could not score, cases a backend or source failed on) is not a
  measurement, yet today it can only lower coverage and reject the
  candidate, which reads as a verdict about the candidate when the run is
  the problem. This spec adds an optional integrity precondition to
  `rustev.selection-gate/1`, `integrity: { max_invalid, max_upstream_failures }`,
  counted per run on each side from documents the selection already
  verifies. A breach, or a count that cannot be established, voids the
  comparison: the candidate is `unknown` naming the run, no coverage
  minimum or clause is evaluated, and so it can never be rejected or
  qualify on a void run; a baseline run in breach voids every candidate.
  Gates without the member keep their bytes, identities and results.
amends:
  - "029-evaluation-repeatability"
  - "061-host-scored-repeat-sets"
depends_on:
  - "002-decision-contract-and-pure-core"
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
    text: "When a gate declares `integrity`, a candidate any of whose runs, or any baseline run, exceeds a declared bound or has a count that cannot be established is `unknown` naming that run, with every coverage minimum and clause unevaluated; such a candidate is never `rejected`, never `qualifies` and never ranked."
    anchor: "3-3-a-breach-voids-the-comparison"
  - id: "I-2"
    kind: invariant
    text: "An integrity count is computed only from documents the selection has verified; a count that is missing, unknown or cannot be derived (no declared scorer for `max_invalid`, a declared scorer without its scores) voids the comparison and is never taken as zero."
    anchor: "3-2-counting"
  - id: "R-1"
    kind: requirement
    text: "A selection gate without `integrity` has the same canonical bytes and identity as today, and `gate select` over it writes the same `rustev.selection/1` bytes, stdout and exit code."
    anchor: "3-6-compatibility"
---

# 065: Selection gate: run integrity failures void the comparison instead of rejecting (amends 029 and 061)

Draft: a proposal for release 0.5.1, never a claim about code.
Ratification is the owner's act. The approved texts of 029 and 061 are
not edited; this spec records what it adds to them.

## 1. Purpose

A host's evaluation gate treats a run with integrity failures as not a
measurement: the comparison is void and the run must be repeated, rather
than the candidate being rejected. Integrity failures are invalid results
(a case the evaluator could not score) and upstream failures (a backend
or source error on a case) above a small bound.

Spec 029 3.12 has three verdicts, `qualifies`, `rejected` and `unknown`. A
run below `min_comparable_coverage` makes the candidate `rejected` on the
coverage clause (029 3.12.2), which reads as a verdict about the
candidate when the run itself is the problem. `unknown` comes only from
preconditions and unknown values, so the host cannot ask for it on
integrity grounds, and a failing clause rejects regardless of unknown
ones (029 3.12.5). The host keeps this check outside Rustev today.

This spec makes integrity a declared precondition. A precondition is
evaluated before coverage and clauses, and a failed one already makes a
candidate `unknown` with every clause unknown (029 3.12.2), which is what
"void" means: no statement about the candidate is made. Whether the
selection should also name this outcome with a fourth verdict is open
question 1.

## 2. Territory

The evaluation library `crates/rustev-eval/` (spec 004's unit, amended by
029, 041 and 061): the gate member, the counts and the precondition, in
the selection code 029's and 061's selections share. The CLI
`crates/rustev-cli/` (spec 006's unit, amended by 029 3.13): no new
flag; `gate select` stdout prints the integrity results. The conformance
suite `tools/rustev-conformance/` (027): one CLI case for a void
candidate.

Edges proposed for ratification (none is declared by this draft):
`crates/rustev-eval/` (004, amending), `crates/rustev-cli/` (006,
amending), `tools/rustev-conformance/` (027, additive), and at completion
`Makefile` and `scripts/acceptance/` (001, additive).

Unchanged: every plan, definition, `PlanId` and plan golden; every run,
case, repeat, baseline and host document; the contract crate, its schemas
and the generated binding types; exit codes (under the recommended answer
to open question 1). No new dependency, no I/O in eval, no clock.

### 2.1 Binding principles

- **Proposals, never grants.** A void comparison is evidence that the
  runs must be repeated; it selects, rejects and deploys nothing.
- **Derivation and lineage.** Each count names the run and the verified
  document it was read from; no decision value is created or changed.
- **Unresolved, never a default.** A count that cannot be established
  voids the comparison (I-2); it is never zero, and a void run is never a
  rejection or a pass (I-1).
- **No silent kind conversion.** Counts are integers over cases; they are
  not rates and are not compared with coverage figures.

## 3. Behavior

### 3.1 The gate member

1. `rustev.selection-gate/1` (029 3.12.1) gains one optional member:

   ```text
   gate = { ..., integrity?: { max_invalid?: integer,
                               max_upstream_failures?: integer } }
   ```

2. When present, `integrity` names at least one bound; each bound is an
   integer from 0 to 1 000 000. Unknown fields and duplicate keys are
   refused, as for the rest of the document. A bound that is absent is not
   checked; it is not read as 0 or as unbounded on another bound's behalf.
3. Each bound applies **per run, on each side**: to every run of the
   baseline and to every run of every candidate.

### 3.2 Counting

Counts are read from documents the selection has already verified (the
baseline against `--baseline-eval`, each candidate's repeat or import
output), after the binding preconditions of 029 3.12.2 or 061 3.7.4 hold.

1. **Invalid results, Rustev-evaluated runs.** For each scorer the
   evaluator configuration declares (041, version 3 configurations), the
   number of cases that scorer left unscored in the run, for any reason
   (`unscored:<reason>`, 041 3.7.5, including `invalid_score`,
   `judge_bundle_missing`, `judge_not_reproduced`, `subject_mismatch`,
   `scorer_failed` and `scorer_overloaded`), as the run's verified report
   counts them. The run's invalid count is the largest of these per-scorer
   counts (open question 3). With `max_invalid` declared, a configuration
   that declares no scorer, or a declared scorer whose metrics are unknown
   with `scores-missing`, makes the count unestablished
   (`invalid-uncountable`).
2. **Upstream failures, Rustev-evaluated runs.** The number of cases
   whose detail outcome key (029 3.2.1) is `abstention:backend_unavailable`,
   `abstention:invalid_backend_output` or `abstention:deadline_exceeded`
   (the runtime failures of spec 002 3.10.1, including a source failure
   under spec 036), plus the cases that are `incomparable` with reason
   `historical-runtime-failure` (spec 004 3.4.5). `budget_exhausted` is
   not counted: it reports a declared limit reached (002 3.10.3, 036's
   acquisition call and cost budgets), which the candidate's plan
   determines (open question 4). A runtime failure that the plan's policy
   handled (escalated, or read `as_unmet`) does not reach the outcome key
   and is not counted here (open question 6).
3. **Invalid results, host runs (amends 061).** For each declared scorer,
   the number of `answered` cases its scores document marks `unscored`;
   the run's count is the largest (as 3.2.1). A declared scorer with no
   scores document (`scores-missing`, 061 3.2.2) makes the count
   unestablished.
4. **Upstream failures, host runs (amends 061).** The number of cases
   whose outcome is `{ failed: reason }` in the run's host run record, for
   any reason (open question 5). This is the numerator of
   `host/failed_rate` (061 3.3.3).
5. A count that the verified documents do not determine, for any reason,
   is unestablished, and the run is treated as in breach (I-2).

### 3.3 A breach voids the comparison

1. A run is **in breach** when a declared bound is exceeded (count
   greater than the bound; a count equal to it holds) or its count is
   unestablished.
2. The selection evaluates integrity after the binding preconditions and
   before the coverage minimums. Each side's runs are checked in run
   order, baseline first. A breach is a failed precondition (029 3.12.2):
   the candidate is `unknown`, the coverage minimums of 029 3.12.2 and 061
   3.7.5 are not evaluated, and every `require` and `and_any` clause is
   unknown with reason `precondition: integrity`. No clause can then fail,
   so the candidate cannot be `rejected`; it cannot qualify and is not
   ranked (I-1).
3. A baseline run in breach voids every candidate, each naming the
   baseline run. The baseline's integrity is computed once.
4. When every run holds its bounds, the selection proceeds exactly as 029
   and 061 state: a coverage minimum not met still rejects on the coverage
   clause. Integrity failures within the bounds are part of the
   measurement: an upstream failure in a Rustev-evaluated run is an
   abstention (spec 004 3.5.4), which lowers acceptance coverage, and in a
   host run a `failed` case lowers `1 - host/failed_rate`.
5. Exit codes are 029 3.14's: any void candidate makes `gate select` exit
   12 (`unknown`), even when another candidate qualifies.

### 3.4 Output

1. When the gate declares `integrity`, each candidate's clause list in
   `rustev.selection/1` gains, after `preconditions` and before the
   coverage entries, one entry per run per side:
   `baseline.runs[<r>].integrity`, then `candidate.runs[<r>].integrity`.
   Each is `pass` with its counts, or `unknown` with the breach: the bound
   exceeded with its count and limit, or the unestablished count with its
   reason. Entries after a breach are still written, so every run's
   counts are visible.
2. A void candidate's verdict is `unknown` naming the first breached
   entry, for example `{ unknown: { clause: "candidate.runs[1].integrity" } }`.
3. Stdout of `gate select` prints each breached run with its counts and
   bounds, and states `void: repeat the run` beside the verdict. Over host
   sets every figure keeps `assertion: "host-asserted"` (061 3.5.3).
4. Entry reasons use the codes `invalid-exceeded`,
   `upstream-failures-exceeded`, `invalid-uncountable` and
   `upstream-failures-uncountable`.

### 3.5 Library and bindings

The integrity counts are a public function of `rustev-eval` beside the
selection, synchronous, with no I/O and no clock, so a host can read a
run's counts before selection. The Node binding's repeatability commands
(063 3.9) pass gate documents and selections as bytes through the same
Rust functions and gain no surface; the WebAssembly package gains
nothing.

### 3.6 Compatibility

1. `rustev.selection-gate/1` keeps its name. A gate without `integrity`
   has the same canonical bytes and gate identity; a gate with it is
   refused by earlier releases (unknown field), never read as a gate
   without it.
2. `rustev.selection/1` keeps its name: the integrity entries are
   written only when the gate declares `integrity`, so every selection
   over a gate without it is byte-identical (R-1). Under open question 1's
   alternative, a new verdict would change this.
3. No plan construct changes, so no `PlanId` and no plan golden changes.
   Every existing eval, CLI, 029, 061 and conformance golden is
   byte-identical.

## 4. Out of scope

Rates or fractions as integrity bounds; per-slice integrity; repeating
runs (the host does); classifying the reasons inside a host `failed`
outcome; changing how 004, 029 or 061 compute coverage; any change to
run, case, repeat, baseline or host documents; the existing `rustev
gate`, which keeps 004 3.5.8's behavior.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `integrity: {}` | `invalid_input`; the gate is refused. |
| A negative bound or one above 1 000 000 | `invalid_input`; the gate is refused. |
| A candidate run with `max_upstream_failures + 1` backend failures | Candidate `unknown`, `candidate.runs[r].integrity`, `upstream-failures-exceeded`; exit 12. |
| A candidate run with exactly `max_upstream_failures` failures | Integrity holds; selection proceeds. |
| A candidate run in breach that would also fail a clause | `unknown`, never `rejected`. |
| A candidate run in breach that would also be below a coverage minimum | `unknown`; coverage not evaluated. |
| A baseline run in breach | Every candidate `unknown` naming `baseline.runs[r].integrity`. |
| `max_invalid` under a configuration with no declared scorer | Candidate `unknown`, `invalid-uncountable`. |
| A declared scorer with no scores document (host run) | Candidate `unknown`, `invalid-uncountable`. |
| Two scorers leaving 2 and 3 cases unscored, `max_invalid: 2` | Count 3; breach. |
| A run with `budget_exhausted` abstentions only | Not an upstream failure; integrity holds. |
| A host run with `max_upstream_failures + 1` `failed` cases | Candidate `unknown`, `upstream-failures-exceeded`. |
| One candidate void, another qualifies | `unknown`, exit 12. |
| Integrity holds, coverage below its minimum | `rejected` on the coverage clause, as today. |
| A gate without `integrity`, on today's inputs | Byte-identical selection, stdout and exit code. |

## Acceptance

- Gate parsing: the member, every field rule of 3.1 and each refusal of
  section 5; existing gate identities unchanged.
- Counts for Rustev-evaluated and host runs, each rule of 3.2, over
  fixtures with unscored cases of several reasons, each counted abstention
  kind, `budget_exhausted`, `historical-runtime-failure` and host `failed`
  cases.
- Selection results: void on the candidate side, on the baseline side,
  on an unestablished count, at exactly the bound (holds), and with a
  would-be clause failure and coverage failure (both `unknown`), in the
  library and through `gate select` with goldens, exit codes 0, 11 and 12.
- One CLI conformance case for a void candidate.
- Every row of section 5 has a test.
- Compatibility: every existing eval, CLI, 029, 061, conformance and plan
  golden byte-identical.
- Mutation seeds, each detected: a breach evaluated after the clauses (a
  void candidate rejected); an unestablished count taken as zero; a count
  equal to the bound treated as a breach; a baseline breach applied to one
  candidate only; `budget_exhausted` counted as upstream.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None. Draft.

## Decisions at approval

Decided in this text, with the reason beside each: integrity is a
precondition, evaluated before coverage and clauses, so a void run can
neither reject nor qualify (3.3.2, the issue's purpose); a baseline run
in breach voids every candidate (3.3.3, the issue); bounds are per run on
each side (3.1.3, the issue); a count equal to its bound holds (3.3.1, "at
most"); coverage minimums keep rejecting when integrity holds (3.3.4).

Open for the owner, each with a recommendation:

1. **Verdict.** `unknown` naming the breached run, with the `integrity`
   reason codes and `void` printed on stdout (recommended: `unknown`
   already never becomes qualifies, the exit code stays 12, and no
   verdict, status or exit code is added to 029 3.14); or a fourth verdict
   `void` with a new status `void` and a new exit code (14), which lets CI
   tell "repeat the run" from other unknowns by exit code alone, but
   changes `rustev.selection/1`'s verdict set and 029 3.14's table.
2. **Document versions.** Keep `rustev.selection-gate/1` and
   `rustev.selection/1`, with `integrity` and its entries written only
   when declared (recommended, as spec 036 added optional members to
   shared versions; an older reader refuses a new gate); or name `/2` of
   both, which starts 031 3.6.2's support window for version 1. Under
   question 1's `void`, `rustev.selection/2` is recommended.
3. **Invalid count across scorers.** The largest per-scorer count
   (recommended: computable from each run's verified report, which counts
   unscored cases per scorer metric, with no new per-case member); or the
   number of distinct cases any scorer left unscored, which needs a new
   per-case member in `rustev.eval-cases/1`.
4. **Which abstentions are upstream.** `backend_unavailable`,
   `invalid_backend_output` and `deadline_exceeded`, plus
   `historical-runtime-failure` (recommended); or also
   `budget_exhausted`, which the issue's "backend or source error" does
   not cover and which a plan declares.
5. **Host `failed` reasons.** Count every `failed` case (recommended for
   0.5.1: the record's reason is host-asserted and not a closed set); or
   add an optional `upstream_reasons: [reason]` to the gate so that only
   listed reasons count and other failures stay flow outcomes gated
   through `host/failed_rate`.
6. **Failures the policy handled.** Count only failures visible in the
   case's outcome key (recommended for 0.5.1: the key is already verified
   and reconciled, 029 3.2.6); or count every failed backend or source
   attempt in the retained run record, which also sees failures a policy
   escalated or read `as_unmet`, at the cost of reading run records in
   selection.

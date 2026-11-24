---
id: "074-case-regressions-exclude-failed-within-integrity"
title: "Selection gate: case regressions may leave out cases that failed within the integrity allowance (amends 064 and 065)"
status: draft
implementation: pending
created: "2026-11-24"
summary: >
  A proposed amendment of approved specs 064 (3.1, 3.2.5, 3.5, 3.6) and 065
  (3.3.4), for #597, targeting release 0.8.0. A selection gate that declares
  both a spec 065 `integrity` allowance for upstream failures and a spec 064
  `case_regressions` clause can never qualify a candidate that uses the
  allowance: a case that failed upstream has no measure on that side, so
  064 makes it `unpaired` or `partial`, the clause is unknown unless it
  already fails, and the candidate ends `unknown` (exit 12) or `rejected`.
  This spec adds an opt-in member to the clause,
  `unresolved: "exclude_failed_within_integrity"`, allowed only when the
  gate declares `integrity.max_upstream_failures`. With it, a case that
  would be unresolved only because of upstream failures that 065 counts is
  left out of the clause and counted as `excluded`, never compared and
  never a regression or a non-regression; every other unresolved case keeps
  064's meaning. Without the member, every gate, selection, stdout and exit
  code keeps its bytes.
amends:
  - "064-selection-gate-case-regressions"
  - "065-selection-gate-run-integrity"
depends_on:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "061-host-scored-repeat-sets"
  - "064-selection-gate-case-regressions"
  - "065-selection-gate-run-integrity"
  - "072-documentation-source-gaps"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A `case_regressions` clause without `unresolved` is evaluated exactly as spec 064 states, and a gate without it has the same canonical bytes, identity, selection bytes, stdout and exit code as before this spec."
    anchor: "3-6-compatibility"
  - id: "I-2"
    kind: invariant
    text: "Under `exclude_failed_within_integrity`, a case is excluded only when it would be unresolved under 064 3.2.5 and, on each side, it is either measured with a known value in every run or every run lacking a measure for it counts it as an upstream failure under 065 3.2.2 or 3.2.4 with no unknown measure in any run; an excluded case is never compared, never counted as a regression or a non-regression, and is listed with its excused side, the baseline when both sides are excused."
    anchor: "3-2-excluded-cases"
  - id: "I-3"
    kind: invariant
    text: "`exclude_failed_within_integrity` is refused unless the gate declares `integrity.max_upstream_failures`, so a case is excluded only in a run that 065 has already found within its upstream-failure bound; a run in breach still voids the comparison before any clause is read."
    anchor: "3-1-the-member"
---

# 074: Selection gate: case regressions may leave out cases that failed within the integrity allowance (amends 064 and 065)

Draft: a proposal, not a claim about code. Ratification (`approved`) is the
owner's act; the questions it needs answered are listed at the end. The
approved texts of 064 and 065 are not edited; this spec records what it adds
to them. Section numbers such as "064 3.2.5" refer to those specs.

## 1. Purpose

Spec 065 lets a gate accept a small number of upstream failures per run:
with `integrity: { max_upstream_failures: 2 }`, a run with one failed case
is within its allowance and the selection proceeds (065 3.3.4). Spec 064's
`case_regressions` clause then reads every case of the split. A case that
failed upstream in a candidate run has no score measure in that run (061
3.3.2, 064 3.6.2), so the case is `unpaired` (one candidate run) or
`partial` (several), and an unresolved case makes the clause unknown unless
it already fails (064 3.4.1). The candidate can therefore only be
`rejected` or `unknown`, never `qualifies`: the allowance 065 grants is
unusable by any gate that also asks about per-case regressions. 064's
section 5 states this outcome for a host candidate with a failed case; it
is correct under 064 and is not a defect, but no gate can opt out of it.

A common host rule counts regressions only over cases scored on both
sides and bounds failures separately. This spec lets a gate state that
rule: the integrity allowance bounds how many cases may fail, and the
clause leaves those cases out instead of reading them as unknown. The
default stays 064's, because leaving a case out is a statement the gate
author makes, not one Rustev may assume.

## 2. Territory

Proposed `extends` edges, added at ratification as 064 and 065 did:

- `crates/rustev-eval/` (004, amending): the member's parsing and check,
  the exclusion in `selection/case_regressions.rs`, the `excluded` output,
  and the regenerated `crates/rustev-eval/schema/rustev.selection-gate-1.schema.json`
  and `rustev.selection-1.schema.json` (072 3.3), checked by
  `crates/rustev-eval/tests/schemas.rs`.
- `crates/rustev-surface/` (027, amending): the clause's stdout summary of
  064 3.5.3 gains `excluded` when the member is declared.
- `crates/rustev-cli/` (006, additive): `gate select` goldens; no new flag.
- `tools/rustev-conformance/` (027, additive): one CLI case.
- `docs/features/` (072, additive): the selection gate feature page
  documents the member.
- `Makefile` and `scripts/acceptance/` (001, additive): at completion, 074
  joins `make verify` and CI's `selection` acceptance group beside 064 and
  065.

This spec's compiled shards under `.statecraft/derived/` are committed with
it, as AGENTS.md requires; the AI review leaves that directory out
(`review.exclude`), so they do not appear in its diff.

Unchanged: `rustev-contract`, `rustev-core` and every plan, definition,
`PlanId` and plan golden; every run, case, repeat, baseline and host
document; the generated binding types; exit codes; the Node binding's code
(its repeatability commands pass gate documents and selections as bytes,
064 3.7.3); the WebAssembly package.

### 2.1 Binding principles

- **Proposals, never grants.** A clause result is evidence for the host's
  choice (029 3.12.7); exclusion changes what the clause reads, not who
  decides.
- **Derivation and lineage.** Each excluded case is listed with its side,
  and the upstream failures that excuse it are the ones 065 counts from
  verified documents and reports per run (065 3.4.1).
- **Unresolved, never a default.** Nothing is excluded unless the gate
  declares it (I-1). An excluded case is not given a value: it is neither
  a regression nor a non-regression (I-2). A case unresolved for any reason
  other than counted upstream failures stays unresolved.
- **No silent kind conversion.** Unchanged: only count-ratio and mean
  metrics have case values (064 3.2.3).

## 3. Behavior

### 3.1 The member

1. 064 3.1.1's clause gains one optional member:

   ```text
   clause = ... | { metric, case_regressions:
                    { by: { absolute: decimal }, at_most: integer,
                      unresolved?: "exclude_failed_within_integrity" } }
   ```

2. `exclude_failed_within_integrity` is the only value; any other string is
   refused (`invalid_input`). There is no literal for the default: the
   default is the member's absence, so one gate has one canonical form.
3. A gate that uses the member in any clause and does not declare
   `integrity.max_upstream_failures` (065 3.1) is refused, `invalid_input`,
   with the code `exclude-failed-needs-integrity`. `max_invalid` alone is
   not enough: invalid results are scorer faults, not upstream failures,
   and stay unresolved.
4. The member is allowed wherever the clause is (`require` and `and_any`);
   the clause stays refused inside `all_slices` (064 3.1.2).

### 3.2 Excluded cases

1. **Upstream failure of a case.** A case is an upstream failure in a run
   exactly when 065 counts it in that run's upstream count: for a
   Rustev-evaluated run, its detail outcome key is one of 065 3.2.2's
   abstentions or it is `incomparable` with `historical-runtime-failure`;
   for a host run, its outcome is `{ failed: reason }` (065 3.2.4). No
   other reason counts.
2. **A side's state.** For a case and a side, with the metric of the
   clause, the side is:
   - **clean** when every run of the side holds a known measure of the
     metric for the case (064 3.2.4's measured, with no `unknown`
     measure);
   - **excused** when at least one run of the side holds no measure for the
     case, every such run counts the case as an upstream failure (3.2.1),
     and no run of the side holds an `unknown` measure for it;
   - **other** in every remaining state.
3. **Exclusion.** With the member declared, a case that 064 3.2.5 makes
   unresolved is **excluded** instead when each side is clean or excused
   and at least one side is excused. Every other unresolved case keeps its
   064 reason. A case 064 does not make unresolved (compared, or
   unmeasured on both sides) is unaffected: the member never removes a
   compared case, including a case whose host count-ratio metric has a
   value in a failed run.
4. **Which sides.** Both: the bound of 065 3.1.3 applies per run on each
   side, and a baseline run's upstream failure leaves a case unpaired in
   the same way (Open question 2).
5. **Bound.** A selection reads clauses only when every run holds its
   integrity bounds (065 3.3.2), so the cases excluded on a side are at most
   the side's runs times `max_upstream_failures`, and are each within a run
   that 065 found within its allowance (I-3).

### 3.3 Clause result (amends 064 3.4.1)

With `R` the regressing cases and `U` the unresolved cases after
exclusion, the clause fails when `R > at_most`, passes when `U = 0` and
`R <= at_most`, and is unknown otherwise, as 064 3.4.1 states. Excluded
cases count in neither `R` nor `U`. 064 3.4.2 and 3.4.3 are unchanged.

### 3.4 Output (amends 064 3.5)

1. When the clause declares the member, its `cases` entry in
   `rustev.selection/1` gains, after `unresolved`:

   ```text
   excluded = { total: integer, truncated: bool,
                list: [ { case: case-id,
                          side: "baseline" | "candidate" } ] }
   ```

   in manifest order, at most 10 000 entries, `total` exact (as 064 3.5.1).
   `side` names the excused side; when both are excused it names the
   baseline (064 3.5.1's convention). The member is written whenever the
   clause declares the option, including with `total: 0`, and never
   otherwise.
2. When the clause is unknown before any case is read (064 3.4.2), `cases`
   is null as before.
3. Stdout of `gate select` prints `excluded: E` beside `regressions` and
   `unresolved` for this clause when the member is declared. Over host sets
   every figure keeps `assertion: "host-asserted"` (061 3.5.3).

### 3.5 Library and bindings

The member is a field of the clause's limit in `rustev-eval`'s public
selection types, and `excluded` a field of its case results. The Node
binding passes gate documents and selections as bytes (064 3.7.3) and
needs no change beyond what the shared stdout renderer prints. The
WebAssembly package has no repeatability command and gains nothing.

### 3.6 Compatibility

1. `rustev.selection-gate/1` and `rustev.selection/1` keep their names. A
   gate without the member keeps its canonical bytes and identity, and its
   selection, stdout and exit code are byte-identical (I-1). A gate with it
   is refused by earlier releases as an unknown field, never read as a
   gate without it.
2. No plan construct changes, so no `PlanId` and no plan golden changes.
   Every existing eval, CLI, 029, 061, 064, 065 and conformance golden is
   byte-identical.
3. The two descriptive schemas of 072 3.3 that cover these documents
   regenerate from their types; no other schema and no binding type
   changes.
4. **Release.** A new member of a gate document and new public fields in
   `rustev-eval`'s exhaustive selection structs (`CaseRegressionLimit`,
   `CaseResults`) are a public API and consumer-contract change, which
   spec 021 3.2 (the rule 031 3.6.4 follows) assigns to the minor version
   before 1.0; `cargo semver-checks` reports a field added to a public
   struct without `#[non_exhaustive]` as a break (as A-63 recorded for
   enum variants). This spec therefore targets 0.8.0.

## 4. Out of scope

Excluding cases for any reason other than counted upstream failures
(invalid results, abstentions, unknown values); a gate-wide default; a
separate bound on excluded cases beyond 065's per-run bound; per-slice case
regressions; changes to 065's counts or verdicts; repeating runs.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `unresolved: "exclude_failed_within_integrity"` with no `integrity` | `invalid_input`, `exclude-failed-needs-integrity`; the gate is refused. |
| The member with `integrity: { max_invalid: 0 }` only | `invalid_input`, `exclude-failed-needs-integrity`. |
| `unresolved: "strict"` or any other value | `invalid_input`; the gate is refused. |
| The member inside `all_slices` | `invalid_input`, `case-regressions-in-slices` (064). |
| Section 5's reproduction (below), without the member | Clause unknown, first unresolved `c01` `unpaired` `candidate`; candidate `unknown`, exit 12 (064, unchanged). |
| The same with the member | Clause passes, `compared: 45`, `excluded.total: 1`; candidate `qualifies`, exit 0. |
| A candidate case failed in 1 of 3 runs, measured in the others | Excluded (candidate excused); never computed over the 2 measured runs. |
| A candidate case failed in one run and `unknown` in another | Unresolved `unknown`; not excluded. |
| A candidate case missing in one run with no upstream failure (abstained) | Unresolved `partial`; not excluded. |
| A case unmeasured on the baseline (no failure) and failed on the candidate | Unresolved; not excluded (baseline side is other). |
| A baseline case failed upstream, candidate measured | Excluded, `side: "baseline"`. |
| A case failed on both sides | Excluded, `side: "baseline"`. |
| A host count-ratio metric whose failed case has value 0 | Compared as today; not excluded. |
| A candidate run with `max_upstream_failures + 1` failures | Void before clauses (065); `unknown`, exit 12, never excluded. |
| `at_most + 1` regressions among compared cases, one excluded | Clause fails; candidate `rejected`. |
| More than 10 000 excluded cases | `excluded.list` holds the first 10 000 in manifest order, `total` the count, `truncated: true`. |
| A gate without the member, on today's inputs | Byte-identical selection, stdout and exit code. |

**The reproduction**, synthetic: a fictional evaluation gate over a host
set (061) of 46 cases with one score metric, higher is better. The baseline
is one host run in which every case is answered and scored. The candidate
is one host run with the same scores, except that case `c01` is
`{ failed: "upstream_failure" }`. The gate declares
`integrity: { max_invalid: 0, max_upstream_failures: 2 }` and `require`
holds a `case_regressions` clause (`by.absolute: "15"`, `at_most: 2`)
beside `not_worse_than` clauses on the same metric. Integrity holds and
the `not_worse_than` clauses pass in both rows above; only the clause
changes.

## Increments

One pull request, as 064 and 065 were each implemented: the member and its
refusals, the exclusion, the output and stdout, the regenerated schemas,
goldens for the reproduction with and without the member, the conformance
case, the feature page, the mutation seeds, 074 in `make verify` and the
`selection` group, and closure.

## Acceptance

- Gate parsing: the member, its refusals in section 5, and unchanged
  identities of existing gates.
- Exclusion over host sets and Rustev-evaluated sets: each side state of
  3.2.2 at N = 1 and N = 3, each section 5 row, both sides, and the bound
  of 3.2.5.
- The reproduction as library tests and as `gate select` goldens: `unknown`
  (exit 12) without the member, `qualifies` (exit 0) with it.
- One CLI conformance case for the reproduction with the member.
- Compatibility: every existing eval, CLI, 029, 061, 064, 065, conformance
  and plan golden byte-identical (I-1); the two schemas match their types.
- Mutation seeds, each detected: the member applied when not declared; an
  excluded case counted as a compared non-regression; a case with an
  `unknown` run measure excluded; a case missing for a reason other than
  an upstream failure excluded; a compared case excluded because it failed
  in some run.
- `make code`, declared verification and `make gate` pass.

## Verification

Declared for
`spec-spine verify 074-case-regressions-exclude-failed-within-integrity`
when the spec completes; 074 then joins `make verify` and CI's `selection`
acceptance group. Every command is repository-local and deterministic over
synthetic fixtures (R-04: mechanics, never a quality claim).

```verify:cli
# 3.1 to 3.5 and section 5 in the library, with every 029, 041, 061, 064
# and 065 eval test unchanged for the shared code, and the regenerated schemas.
cargo test -p rustev-eval --locked
# The reproduction through `gate select`; every existing CLI golden unchanged.
cargo test -p rustev-cli --locked
cargo test -p rustev-surface --locked
cargo test -p rustev-conformance --locked
# Negative control: each acceptance seed must be detected.
sh crates/rustev-eval/mutation/seeds.sh "spec 074"
```

## Open questions for the owner

1. **Release.** 0.8.0, per 021 3.2 (3.6.4); #597 does not ask for a patch.
   Recommended: 0.8.0, together with spec 073.
2. **Sides.** Exclude cases excused on either side (3.2.4), or only on the
   candidate side, as #597 words it? Recommended: both, since the
   allowance is per run on each side and a baseline failure blocks
   qualification the same way.
3. **Name and form.** `unresolved: "exclude_failed_within_integrity"` on
   the clause, as #597 suggests, with no literal for the default (3.1.2).
   Recommended: as written.
4. **Requirement on the gate.** Refuse the member unless
   `integrity.max_upstream_failures` is declared (3.1.3), rather than
   accepting it and excluding failures without a bound. Recommended:
   refuse, since "within integrity" has no meaning without the bound.
5. **Strictness.** Exclude only when every gap on a side is a counted
   upstream failure (3.2.2), so a case that also has an unknown or an
   abstained run stays unresolved. Recommended: as written.

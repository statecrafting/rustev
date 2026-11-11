---
# Feature metadata, authored. Each section below is marked derived or
# written in a comment; the provenance table at the end lists the source of
# each and what keeps it correct.
feature: selection-gate
title: Selection gate
summary: Decide whether candidate plans may replace a baseline, from repeated evaluations, without mistaking noise for improvement.
stability: stable
concepts: [repeat-evaluation, noise-band, unresolved-outcomes]
related: [evaluation, baselines, repeat-evaluation, host-scored-runs, calibration]
governed_by: ["029", "061", "064", "065"]
---

# Selection gate

Decide whether one or more candidate plans may replace a baseline, from
repeated evaluations, without mistaking noise for improvement.

You change a decision plan (new rules, a different model, a new threshold)
and need to know whether it is actually better than what runs today. One
evaluation is not enough: scores can move between runs on their own. A
selection gate compares each candidate's repeated evaluation with a recorded
baseline, applies the rules you declare, and gives each candidate one of
three verdicts:

- **qualifies**: every rule you required holds;
- **rejected**: a rule definitely fails, and the result names it;
- **unknown**: Rustev cannot decide, and the result names what is missing.

Qualifying candidates are ranked. The gate produces evidence for your
choice, not the choice: Rustev never promotes a plan.

## At a glance

<!-- derived: Cargo features, CLI command table, index.d.ts, wasm exports,
     conformance manifest, release tags -->

| | |
|---|---|
| **Stability** | Stable |
| **Since** | 0.1.0 · host-scored runs 0.4.1 · `case_regressions` and `integrity` 0.6.0 |
| **Rust** | `rustev` with feature `eval` → `rustev::eval::evaluate_selection` |
| **CLI** | `rustev gate select` |
| **Node** | `engine.gateSelect(inputs, args)` |
| **WebAssembly** | Not available (the module has the single-run `gate` only) |
| **Decision service** | Not available |
| **Reads** | `rustev.selection-gate/1`, `rustev.baseline/1`, repeat sets from `rustev eval candidates` or `rustev eval repeat import` |
| **Writes** | `rustev.selection/1` |
| **Conformance** | 5 cases, CLI and Node |

## When to use it

<!-- written -->

- **Before promoting a plan change.** In CI, a candidate must not be worse
  than production.
- **Choosing among several candidates.** Prompt variants, model versions,
  threshold sweeps: the gate ranks the ones that qualify.
- **Guarding against hidden regressions.** An average can improve while a
  few cases get much worse; the `case_regressions` rule catches that.

Use the single-run `rustev gate` when you have exactly one evaluation per
side and no repeat configuration.

## Try it

<!-- derived: commands and outputs from selection-gate/example/run.sh, run
     with rustev 0.7.0; the script checks the verdicts against
     selection-gate/example/expected-gate.json and expected-gate-strict.json -->

[`selection-gate/example/run.sh`](selection-gate/example/run.sh) runs the
whole workflow on the support-routing package's synthetic support tickets.
From the repository root, with `rustev` and `jq` on `PATH`:

```sh
sh docs/features/selection-gate/example/run.sh
# gate: gate select exit 0, selected
# gate-strict: gate select exit 0, selected
# verdicts match the expected files (/tmp/rustev-selection-gate.XXXXXX)
```

**The situation.** The routing plan reads each ticket, a classifier
estimates its topic, and the plan routes the ticket to a queue, or escalates
it to a person when the classifier's top probability is below **0.6**. You
want fewer escalations without routing tickets to the wrong queue. Two
candidates change only that threshold: `route-more` lowers it to **0.35**,
`route-less` raises it to **0.8**.

**The steps**, each a `rustev` command in the script:

1. **Compile** the baseline plan from the package's synthetic example
   definition, with the rules and calibration it is bound to.
2. **Capture.** Decide all 16 tickets once with `rustev run`, keeping a
   replay bundle for each: the inputs and the classifier's answers.
3. **Make candidates**: two copies of the definition with another threshold,
   compiled into `variants/route-more/` and `variants/route-less/`.
4. **Evaluate.** `rustev eval candidates` replays the bundles through the
   baseline and both candidates on the four model-selection tickets, twice
   each. Replay re-runs the plan on the recorded answers; no ticket is
   classified again.
5. **Record the baseline.** `rustev baseline record` freezes the baseline's
   evaluation as the reference.
6. **Select.** `rustev gate select` applies
   [`gate.json`](selection-gate/example/gate.json): no more wrong routes than
   the baseline, and at least as many tickets routed.

```json
"require": [
  { "metric": "error_among_accepted", "not_worse_than": "baseline", "within": { "absolute": "0" } },
  { "metric": "acceptance_coverage", "not_worse_than": "baseline", "within": "noise" }
],
"rank_by": ["acceptance_coverage"]
```

**What the evaluation measured:**

| Plan | Routed (`acceptance_coverage`) | Wrong among routed (`error_among_accepted`) |
|---|---|---|
| Baseline, 0.6 | 0.75 (3 of 4; one escalated as ambiguous) | 0.0 |
| `route-more`, 0.35 | 1.0 (4 of 4) | 0.0 |
| `route-less`, 0.8 | 0.0 (all 4 escalated) | unknown: nothing was routed |

**What the gate decided** (exit 0, `selected`):

| Candidate | `require[0]`: no more errors | `require[1]`: routes at least as many | Verdict |
|---|---|---|---|
| `route-more` | pass | pass | **qualifies**, ranked 1st |
| `route-less` | unknown | **fail** | **rejected** on `require[1]` |

`route-less` is rejected although one of its rules is unknown: a definite
failure decides the verdict. The next section explains the order.

**A stricter gate.** The script also runs
[`gate-strict.json`](selection-gate/example/gate-strict.json), which adds one
rule: no ticket the baseline routed may be escalated by a candidate.

```json
{ "metric": "acceptance_coverage", "case_regressions": { "by": { "absolute": "1" }, "at_most": 0 } }
```

`route-more` still qualifies. `route-less` is now rejected on this rule, and
the result names the tickets:

```json
"cases": {
  "compared": 4,
  "regressions": { "total": 3, "truncated": false, "list": [
    { "case": "sr-m01", "baseline": 1.0, "candidate": 0.0, "drop": "1" },
    { "case": "sr-m02", "baseline": 1.0, "candidate": 0.0, "drop": "1" },
    { "case": "sr-m03", "baseline": 1.0, "candidate": 0.0, "drop": "1" } ] },
  "unmeasured": 0,
  "unresolved": { "total": 0, "truncated": false, "list": [] }
}
```

This is the rule to use when an average can hide individual cases getting
worse.

## How a verdict is decided

<!-- written; each step restates spec 029 3.12 and spec 065 3.2 -->

The gate checks each candidate in this order and stops at the first step
that decides:

| Step | Checks | If it does not hold |
|---|---|---|
| 1. Comparable | Both sides evaluated the same dataset, split, evaluator configuration and repeat configuration; the baseline matches its evaluation. | **unknown**, naming the precondition |
| 2. Integrity (optional) | No run on either side exceeds the gate's `integrity` limits on invalid results or upstream failures. | **unknown**, naming the run. Nothing else is evaluated, so a broken run can never reject or qualify a candidate. |
| 3. Coverage minimums | Every run on each side meets `min_comparable_coverage` and `min_labeled_coverage`. | Below the minimum: **rejected**, naming the run. Not countable (zero denominator): an **unknown** result, combined like a rule's below. |
| 4. Rules | Every `require` rule, and at least one `and_any` rule when declared. | See below |

Step 4 combines the rule results like this:

| `require` rules | `and_any` rules (if declared) | Verdict |
|---|---|---|
| any **fails** | anything | **rejected**, naming the first failing rule |
| none fails | **every** one fails | **rejected**, naming `and_any` |
| none fails, any **unknown** | anything not all-failing | **unknown**, naming the first unknown rule |
| all **pass** | none passes, some unknown | **unknown** |
| all **pass** | at least one passes, or none declared | **qualifies** |

An unknown never turns into a pass, and a definite failure is never hidden
by an unknown.

## Concepts

<!-- written -->

**Repeat set.** The runs of one plan over one dataset split. Replay sets
re-run the plan on recorded backend answers; captured sets use separately
recorded runs; live sets call the backends again.

**Pooled value.** A metric computed once over the contributions of all runs
together, not averaged per run. Every rule compares pooled values.

**Noise band.** How far a metric may differ between baseline and candidate
before the difference counts. By default it is a multiple of the spread
between runs (`k_sd`: `k` times the standard deviation), never below the
declared `floor`. Rules use the larger of the two sides' bands.

- **On replay, the band is zero.** Replaying the same recorded answers gives
  the same result every time, so "within noise" means exact comparison, as
  in the example above. Declare a `floor` in the repeat configuration if
  you want tolerance.
- **A band is not a significance test.** It describes the variation seen
  between runs of each side. It does not say the candidate is better with
  some confidence. Exact decimal arithmetic in the comparison removes
  rounding errors, not statistical uncertainty.
- **Fewer than two runs** of a captured or live set give an unknown band,
  and every rule that needs it is unknown, never a pass.

**Coverage.**
`coverage` is comparable cases over all cases.
`acceptance_coverage` is proposals over comparable cases; escalations and
unresolved outcomes count as abstentions.
`error_among_accepted` is wrong proposals over labeled proposals, and is
unknown when nothing was proposed.

## Writing a gate

<!-- shape: hand-kept; no JSON schema is published for
     rustev.selection-gate/1 yet (source gap). Meanings written. -->

```json
{
  "schema": "rustev.selection-gate/1",
  "gate": "promote-routing",
  "per": "variant",
  "metrics": {
    "acceptance_coverage": "higher_is_better",
    "error_among_accepted": "lower_is_better"
  },
  "min_comparable_coverage": "1",
  "min_labeled_coverage": "0",
  "integrity": { "max_upstream_failures": 0 },
  "require": [
    { "metric": "error_among_accepted", "not_worse_than": "baseline", "within": { "absolute": "0" } },
    { "metric": "acceptance_coverage", "case_regressions": { "by": { "absolute": "1" }, "at_most": 0 } },
    { "metric": "acceptance_coverage", "not_worse_than": "baseline", "within": "noise" }
  ],
  "rank_by": ["acceptance_coverage"]
}
```

| Field | Meaning |
|---|---|
| `schema` | Always `rustev.selection-gate/1`. |
| `gate` | Your name for the gate. |
| `per` | Always `variant`: one verdict per candidate. |
| `metrics` | Every metric a rule or `rank_by` names, with its direction. A metric without a direction is refused. |
| `min_comparable_coverage`, `min_labeled_coverage` | Minimum coverage of every run on each side, from 0 to 1, as decimal strings. |
| `integrity` | Optional: `max_invalid` (results the evaluator could not score) and `max_upstream_failures` (cases a backend or source failed on), per run. |
| `require` | Rules that must all pass. At least one. |
| `and_any` | Optional rules of which at least one must pass. |
| `rank_by` | Order of qualifying candidates, metric by metric in its direction; ties by candidate name. |

Unknown fields and duplicate keys are refused. Decimals are strings.

### Rules

| Rule | Passes when |
|---|---|
| `{ metric, not_worse_than: "baseline", within: "noise" }` | The candidate is no worse than the baseline by more than the noise band. |
| `{ metric, not_worse_than: "baseline", within: { absolute: t } }` | The same, with a fixed tolerance `t`. Needs no band. |
| `{ metric, better_than: "baseline" }` | The candidate is better by more than the noise band. An improvement within noise is not a win. |
| `{ all_slices: <rule> }` | The inner rule holds for every declared slice of the dataset. |
| `{ metric, case_regressions: { by: { absolute: d }, at_most: n } }` | At most `n` cases got worse than their own baseline value by `d` or more. The result lists the regressing cases. |
| `{ held_out: { max_evaluations: n } }` | On a held-out split, the candidate's lineage has been evaluated there at most `n` times. |

A rule is unknown when a value, band or count it needs is unknown, or when
it reads a latency or cost metric that the candidate only observed
historically.

**Choose a `case_regressions` metric every case has.** The rule pairs each
case's baseline value with its candidate value. `acceptance_coverage` has a
value for every comparable case (routed or not), so every case pairs.
`error_among_accepted` has a value only for cases that were routed: a ticket
one side routed and the other escalated has no pair, and the rule becomes
unknown (`sr-m04: unpaired (baseline)`) rather than pass.

## Reading the result

<!-- shapes: from the example's selection.json and spec 029 3.12.7; hand-kept
     until rustev.selection/1 has a published schema (source gap) -->

`rustev gate select` writes `rustev.selection/1`:

```json
{
  "schema": "rustev.selection/1",
  "gate": "sha256:…",
  "baseline": "sha256:…",
  "ranked": ["route-more"],
  "unranked": [],
  "candidates": [
    {
      "name": "route-less",
      "repeat_report": "sha256:…",
      "verdict": { "rejected": { "clause": "require[1]" } },
      "clauses": [
        { "clause": "preconditions", "result": { "pass": { "reason": "bindings and cohorts verified" } }, "slices": [] },
        { "clause": "require[0]", "result": { "unknown": { "reason": "error_among_accepted: zero denominator" } }, "slices": [] },
        { "clause": "require[1]", "result": { "fail": { "reason": "acceptance_coverage in all" } }, "slices": [] }
      ]
    }
  ]
}
```

The `clauses` list is abridged: the full list has one entry per
precondition, integrity limit, coverage minimum and rule, in the order of
the previous section.

**`verdict` has three shapes.** Parse all of them:

| Verdict | JSON |
|---|---|
| qualifies | `"qualifies"` |
| rejected | `{ "rejected": { "clause": "require[1]" } }` |
| unknown | `{ "unknown": { "clause": "candidate.runs[0].integrity" } }` |

**Each rule result** is `{ "pass": { "reason" } }`, `{ "fail": { "reason" } }`
or `{ "unknown": { "reason" } }`. A `case_regressions` result also carries
`cases`: how many cases were `compared`, the `regressions` (with `case`,
`baseline`, `candidate` and `drop`), the `unmeasured` count and the
`unresolved` cases, each list with its `total` and whether it was
`truncated`.

**`ranked`** lists qualifying candidates in `rank_by` order. **`unranked`**
lists qualifying candidates whose ranking metric is unknown, with the
reason; they are never placed by a default.

### Exit codes

<!-- derived: CLI exit-code table -->

| Exit | Status | Meaning | What to do |
|---|---|---|---|
| 0 | `selected` | Every verdict is known and at least one candidate qualifies. | Promote from `ranked`, through your own process. |
| 11 | `none_qualified` | Every verdict is known and none qualifies. | Keep the baseline; read each candidate's failing rule. |
| 12 | `unknown` | At least one verdict is unknown, even if another qualifies, because the ranking could change. | Read the named step: a mismatch between the sides, a broken run, a missing value or a historical-only metric. |
| 4 | `invalid_input` | A gate, baseline or repeat set was refused. | Fix the document named in the output. |

### From Node

<!-- derived: GateSelectArgs in integrations/rustev-node/index.d.ts -->

```js
const { code, out, artifacts } = await engine.gateSelect(inputs, {
  selectionGate: 'gate.json',
  baseline: 'baseline.json',
  baselineEval: 'comparison',
  candidates: 'comparison',
  out: 'selection.json',
});
// code is the CLI exit code above; artifacts['selection.json'] holds the document.
// inputs maps each of those names to its document bytes or path.
```

## Guarantees and limits

<!-- written; each line names the requirement it restates -->

- **Unknown never becomes qualifies.** A missing value, an unknown band or
  an uncountable integrity limit makes the verdict unknown, and the command
  exits 12. [029 R-2](../../specs/029-evaluation-repeatability/spec.md),
  [029 I-3](../../specs/029-evaluation-repeatability/spec.md)
- **A definite failure rejects,** whatever else is unknown.
  [029 §3.12.5](../../specs/029-evaluation-repeatability/spec.md#312-selection-gates)
- **A broken run is not a verdict on the candidate.** An integrity breach on
  either side voids the comparison; a baseline breach voids every candidate.
  [065 I-1, I-2](../../specs/065-selection-gate-run-integrity/spec.md)
- **A case regression is never assumed away.** A case without a value on
  both sides makes `case_regressions` unknown, not a pass.
  [064 I-1](../../specs/064-selection-gate-case-regressions/spec.md)
- **Historical latency and cost are not the candidate's.** A rule over them
  for an offline candidate is unknown.
  [029 R-1](../../specs/029-evaluation-repeatability/spec.md)
- **Host-scored and Rustev-evaluated sets never mix.** A host repeat set is
  compared only with a host baseline, and every figure is labeled
  host-asserted. [061 I-1, I-2](../../specs/061-host-scored-repeat-sets/spec.md)
- **No promotion.** The selection is evidence for your decision; acting on
  it is your application's job.
  [029 §3.12.7](../../specs/029-evaluation-repeatability/spec.md#312-selection-gates)
- **No quality claim.** The gate compares two evaluations. Whether the
  dataset represents your production traffic is for you to establish. The
  example's data is synthetic.

## See also

<!-- derived from `related` and `concepts` -->

- [Evaluation](evaluation.md) · [Baselines](baselines.md) ·
  [Repeat evaluation](repeat-evaluation.md) ·
  [Host-scored runs](host-scored-runs.md) · [Calibration](calibration.md)
- Concepts: [Noise bands](../concepts/noise-band.md) ·
  [Unresolved outcomes](../concepts/unresolved-outcomes.md)

---

<sub>Governed by specs [029](../../specs/029-evaluation-repeatability/spec.md),
[061](../../specs/061-host-scored-repeat-sets/spec.md),
[064](../../specs/064-selection-gate-case-regressions/spec.md),
[065](../../specs/065-selection-gate-run-integrity/spec.md).</sub>

<!--
## Provenance (prototype only; not part of the published page)

| Block | Kind | Source | Kept correct by |
|---|---|---|---|
| Frontmatter | Written | this page | review |
| At a glance: Rust path | Derived | `rustev` facade features; `rustev-eval` public items | API diff |
| At a glance: CLI, Node, WASM, service | Derived | command surface, `index.d.ts`, wasm exports, serve routes | conformance manifest `surfaces` |
| At a glance: Since | Derived | first release tag containing each capability | release manifests |
| At a glance: Reads, Writes | Derived | schema names the command uses | contract schemas |
| When to use it | Written | this page | review; freshness on the governing specs |
| Try it: steps and outputs | Derived | `selection-gate/example/run.sh` | the script's verdict check, to run in CI |
| Try it: situation, tables | Written from the run | `expected-gate.json`, `expected-gate-strict.json`, the repeat report | the script's verdict check; review |
| How a verdict is decided | Written | 029 §3.12.2–5, 065 §3.2 | requirement links; freshness |
| Concepts | Written | 004 §3.5.4–5, 029 §3.3, §3.5 | freshness |
| Writing a gate | Hand-kept shape, written meaning | `SelectionGate` in `rustev-eval` | needs a published schema (gap) |
| Reading the result | Hand-kept shape | example output, 029 §3.12.7 | needs a published schema (gap) |
| Exit codes | Derived | CLI exit-code table | CLI golden tests |
| From Node | Derived | `GateSelectArgs` in `index.d.ts` | typings check |
| Guarantees and limits | Written, linked | requirements of 029, 061, 064, 065 | requirement links; freshness |
| See also, Governed by | Derived | frontmatter | link check |
-->

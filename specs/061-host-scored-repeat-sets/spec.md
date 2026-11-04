---
id: "061-host-scored-repeat-sets"
title: "Host-scored repeat sets for the selection gate (amends 029 and 041)"
status: approved
implementation: pending
created: "2026-11-03"
summary: >
  A proposed amendment of approved specs 029 and 041 (and of spec 006's
  command table through 029 3.13), for release 0.4.1. Today `rustev gate
  select` reads only repeat sets Rustev evaluated itself: every run is a
  spec 004 evaluation of a Rustev plan over a Rustev dataset manifest. A
  host whose baseline and candidate runs execute in its own runtime, and
  whose outputs are scored through `scoreViews` into `rustev.scores/1`,
  has no supported way in. This spec adds a host run record
  (`rustev.host-run/1`) carrying host dataset, flow, version and run
  identities with per-case outcomes, latency and cost; an import command,
  `rustev eval repeat import`, that verifies the host's views and scores
  and builds a host repeat set with spec 029's statistics; a host baseline;
  and selection over host sets. Host documents have their own schema
  names, so no existing document changes bytes or meaning and a host set
  can never be compared with a Rustev-evaluated one. Every figure is
  labeled host-asserted. Ratified (A-77) for release 0.4.1.
amends:
  - "029-evaluation-repeatability"
  - "041-ranking-and-rubric-metrics"
extends:
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
depends_on:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "041-ranking-and-rubric-metrics"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A host repeat set, host baseline or host selection is never combined with, compared against or verified as a Rustev-evaluated repeat set, baseline or selection; any command given both refuses with `invalid_input`, `mixed-provenance`, and writes nothing."
    anchor: "3-6-provenance-separation"
  - id: "I-2"
    kind: invariant
    text: "Every host document states `assertion: \"host\"`, and every figure in it is reported as computed by Rustev from host-asserted observations; no host figure is presented as a Rustev observation or carries a Rustev `PlanId` or `DatasetId` as its identity."
    anchor: "3-5-derivation-lineage-and-labels"
  - id: "I-3"
    kind: invariant
    text: "A host latency, cost, score or outcome that is missing, unknown or malformed is unknown or refused, never zero, never a default, and never dropped without a count."
    anchor: "3-1-the-host-run-record"
  - id: "R-1"
    kind: requirement
    text: "`rustev gate select` over a host baseline and host candidate sets applies spec 029 3.12's clauses, verdicts, ranking and exit codes 0, 11 and 12 unchanged, with the precondition substitutions of 3.7 and no others."
    anchor: "3-7-host-baselines-and-selection"
---

# 061: Host-scored repeat sets for the selection gate (amends 029 and 041)

Approved by the owner on 2026-11-03 (A-77) for release 0.4.1. The
approved texts of 029 and 041 are not edited; this spec records what it
adds to them.

## 1. Purpose

A downstream consumer gates each of its flows with spec 029's selection
gate: not worse than the baseline within noise on every slice, and better
on p50 or p95 latency or on cost, wired to exit codes 0, 11 and 12. Its
baseline and candidate runs are host runs: the host's runtime executes
them, and their outputs are scored through the Node binding's
`scoreViews` (041 3.9) into `rustev.scores/1`. Spec 029 3.1.1 defines a
repeat set as N spec 004 evaluations, so `gate select` (029 3.13.6) can
only read sets whose runs Rustev evaluated from its own bundles. Nothing
lets the host's runs in, and fabricating a `PlanId` or `DatasetId` to make
them fit would break every identity rule the gate relies on.

This spec gives host-scored runs a supported, separately labeled path:
the host supplies one identity record per run, Rustev verifies what it
can (the views, the scores, the case lists, the bindings between runs),
computes 029's statistics, intervals and bands over it, and gates host
candidates against a host baseline. What Rustev cannot verify (that the
runs happened, that their latency and cost are true) is stated as
host-asserted in every document and output.

## 2. Territory

The evaluation library `crates/rustev-eval/` (spec 004's unit, amended by
029 and 041) and the CLI `crates/rustev-cli/` (spec 006's unit, amended by
029 3.13). It extends `crates/rustev-eval/` (004, amending) and
`crates/rustev-cli/` (006, amending), edges added at ratification; at
completion it adds edges to `Makefile` (001, additive, for
`VERIFIED_SPECS`) and `scripts/acceptance/` (001, additive, for the
acceptance group).

Unchanged: every existing document, identity, plan and golden;
`rustev.repeat-config/1`, `rustev.selection-gate/1`,
`rustev.score-views/1`, `rustev.scores/1` and `rustev.evaluator-config/3`
are read as they are. No binding surface changes here (3.10). No new
dependency, no I/O in eval, no clock: `recorded_at_ms` is supplied.

### 2.1 Binding principles

- **Proposals, never grants.** A host selection is evidence for the
  host's choice, as 029 3.12.7 states; no host document is accepted by an
  executor.
- **Derivation and lineage.** Every figure keeps the derivation class 041
  3.2.5 gives its measurement and binds, per run and per case, the host
  run record it came from (3.5).
- **Unresolved, never a default.** Missing or malformed host evidence is
  unknown or refused (I-3); unknown never becomes qualifies.
- **No silent kind conversion.** Latency is a sample, cost a declared
  unit, scores keep 041's output kinds; nothing is converted between them.

## 3. Behavior

### 3.1 The host run record

1. `rustev.host-run/1`, under `REPLAY_V1` in record canonical form,
   unknown fields and duplicate keys refused, identified by its record
   digest (the **run identity** Rustev binds). The host writes one per
   run:

   ```text
   host-run = { schema: "rustev.host-run/1",
     host: { system: handle, flow: handle, flow_version: handle, run: handle },
     dataset: { id: handle, digest: content-digest, cases: integer },
     split: "training" | "model_selection" | "calibration" | "final_test",
     views: content-digest, scores: [content-digest],
     cost_unit: null | unit-name,
     recorded_at_ms: integer,
     cases: [ { case: case-id, subgroups: [name],
                outcome: "answered" | "abstained" | { failed: reason },
                latency_us: integer | { unknown: reason },
                cost: decimal | { unknown: reason } | null } ] }
   ```

2. Handles follow the contract's `Handle` rules (non-empty, at most 256
   bytes). `dataset.digest` is the host's content digest of its dataset
   (`sha256:` and 64 lowercase hex digits); `dataset.cases` is the
   dataset's case count for the split. Case ids follow spec 006 3.9.1,
   are unique, and are listed in the host's dataset order; `cases` has
   exactly `dataset.cases` entries. `reason` is 1 to 64 bytes of
   `[a-z0-9_]`. `unit-name` is 1 to 64 bytes of `[a-z0-9_.]`.
3. Split names are spec 004's, in their spec 029 3.7.1 roles. A host
   dataset carries no Rustev split assignment, so the host states the
   role; Rustev records it and cannot check it.
4. `latency_us` is the host-measured wall time of the case, in whole
   microseconds, 0 to 2^53. `cost` is a non-negative Decimal string (spec
   002 3.3.1) in `cost_unit`; with `cost_unit` null every `cost` is null
   and every cost metric is unknown (`cost-not-declared`). An `unknown`
   latency or cost is kept as unknown with its reason (I-3).
5. `views` is the record digest of the `rustev.score-views/1` document the
   run's proposals were scored over; `scores` lists the record digests of
   its `rustev.scores/1` documents, one per scorer, at most 16.

### 3.2 Import: `rustev eval repeat import`

1. Inputs: `--repeat-config <file>` (`rustev.repeat-config/1`),
   `--config <file>` (a `rustev.evaluator-config/3` whose scorers and
   rubric, agreement and human-scale declarations define the score
   metrics), `--run <dir>` repeatable 1 to 64 times in run order, and
   `--out <dir>`. Each run directory holds `host-run.json`, `views.json`
   and `scores/` with one `rustev.scores/1` file per scorer, under spec
   006 3.3's caps. A command path with a subcommand is matched before its
   parent (029 3.13), so `eval repeat import` never parses as `eval`.
2. Per run, before any statistic, the import refuses (`invalid_input`,
   naming the run and code): a malformed host run record (`host-run`); a
   `views.json` or scores file whose record digest is not the one the
   record names, or a named digest with no file (`host-binding`); views
   whose `config` is not the `--config` identity or whose `split` is not
   the record's (`host-binding`); views whose cases are not exactly the
   record's `answered` cases, in record order, or whose `subgroups` differ
   from the record's for a case (`host-binding`); and every scores check
   of 041 3.7.5 (`scorer-undeclared`, `scores-mismatch`, two documents for
   one scorer). A declared scorer with no scores document makes its
   metrics unknown (`scores-missing`), as 041 3.7.5 states.
3. Across runs (`mixed-runs`, as 029 3.1.3): every run must have equal
   `host.system`, `host.flow`, `host.flow_version`, `dataset` (id, digest
   and count), `split`, `cost_unit`, case id list and order, and the same
   views `dataset` and `plan` members. `host.run` must be unique across
   runs (`mixed-runs`, `duplicate-run`).
4. The views' `dataset` and `plan` members are bound only through the
   views digest. For a host run they are not read as Rustev identities,
   and no host document reports them as such (I-2). This is the one
   amendment of 041's rule that scores enter only bound to the views of
   the same evaluation: for a host set, the views are the host run's own,
   bound by its record, and every other 041 3.7.5 check still applies.
5. Output, all or none: `repeat-config.json`, `config.json`, per run
   `run-<r>.host-run.json`, a byte copy of its views document
   (`run-<r>.views.json`) and of each scores document
   (`run-<r>.scores-<scorer>.json`), and `run-<r>.cases.json`
   (`rustev.host-cases/1`, 3.3), and `repeat.json`
   (`rustev.host-repeat-report/1`, 3.4). Stdout is `imported` with the
   report's digest, N, and per cohort each metric's `pooled`, interval
   and band, as 029 3.13.1 prints `reported`. The input files are never
   modified.

### 3.3 Measures

1. `rustev.host-cases/1` (`REPLAY_V1`, record canonical) is 029 3.2.1's
   per-case document with the run report digest replaced by the host run
   record digest: per case in record order, the case id, its outcome,
   and its measures in 029 3.2.2's three forms.
2. **Score metrics.** Every metric 041 defines from scores
   (`rubric/<name>`, `model/rubric/<name>`, and agreements with
   `human:<scale>`) takes its 041 3.11 measure over the `answered` cases,
   computed by the same functions 041's evaluation uses. 041 3.8.5's
   `model/` separation applies unchanged. Metrics 041 or 004 define from
   Rustev decision records (coverage, acceptance coverage, error among
   accepted, ranking, constraint and origin metrics) are not produced: a
   host run has no Rustev decision record.
3. **Host metrics**, named under the `host/` prefix so no gate written for
   Rustev-evaluated sets can read them by accident:
   - `host/answered_rate`, `host/abstained_rate`, `host/failed_rate`:
     count ratios over every case of the run;
   - `host/scored_rate`: count ratio, numerator 1 for an `answered` case
     every declared scorer scored, over every case;
   - `host/latency_ms/p<q>` for q in 0.5 and 0.95: samples, each the
     case's `latency_us` divided by 1000 in binary64 (one correctly
     rounded division), over `answered` and `abstained` cases. A `failed`
     case is excluded and counted (`failed`), so a flow cannot look faster
     by failing; `host/failed_rate` gates that. Any included case with an
     unknown latency makes the metric unknown for that run, naming the
     case; it is never skipped;
   - `host/cost_per_1000`: a mean contribution of 1000 times the case's
     cost per case, over every case including `failed` ones (a failed
     case still spent). Unknown when `cost_unit` is null or any case's
     cost is unknown (029 3.2.5's rule: unknown cost is never zero). The
     unit is recorded beside the metric.
4. There is no reconciliation against a run report (029 3.2.6): none
   exists. Its role is taken by re-assembly: the baseline, the
   reproducibility check and selection rebuild every host-cases document
   and the host repeat report from the import output, which holds every
   input they need (the run records, views and scores, 3.2.5), and refuse
   any byte difference (`reconcile`).

### 3.4 Statistics, sources and bands

1. `rustev.host-repeat-report/1` (`REPLAY_V1`, record canonical) carries
   `assertion: "host"`, `source: "host"`, the repeat configuration id,
   the evaluator configuration id, the host identities of 3.1 (system,
   flow, flow version, dataset id, digest and count, split and role,
   cost unit), per run the host run record digest, its `host.run` handle
   and its host-cases digest, the stable cohort, `held_out` (3.8) and the
   cohorts with 029 3.3 to 3.6's figures.
2. Spec 029 3.3 (mean, sd, min, max, `pooled`), 3.4 (bootstrap, generator,
   stream seeds, percentile, bound, cross-platform determinism), 3.5
   (bands and floors) and 3.6 (slices from the record's `subgroups`,
   `min_cases`, `below_minimum`) apply unchanged.
3. **Source `host`.** Rustev cannot replay or re-execute a host run, so
   neither `replay` nor `live` applies, and `deterministic_replay` is
   never a band basis. Statistically a host set is a `captured` set (029
   3.1.2): each run is one separate execution, observed once. A `k_sd`
   band over N = 1 is unknown (029 3.5.2); `interval_width` is available
   at any N. 029 3.4.6's run seeds are not recorded: Rustev hands the host
   nothing, and the report states `seed: null` per run.
4. **Stable cohort.** The cases that are not `failed` in any run. It
   decides slice minimums, as in 029 3.6.2.
5. `top_k` must be null in the repeat configuration (`repeat-config`,
   `top-k-not-observed`): a host run carries no ranking parameter Rustev
   can read.
6. A deterministic host scorer is not re-scored: 041 3.6.5's replay
   evidence does not exist for host sets, and the report states
   `scorer_reproduction: "not_checked"` per scorer.

### 3.5 Derivation, lineage and labels

1. A score metric keeps 041 3.2.5's derivation: `exact-derived` for a
   deterministic scorer, `model-derived` for a model scorer (with the
   `model/` prefix). A `host/` metric is `exact-derived` arithmetic over
   host-asserted observations: no model produced it, and Rustev did not
   observe it. No new derivation class is introduced.
2. Lineage: every figure binds its host run record digests (report), each
   case measure its run's record and the case id (host-cases), and each
   score its views subject and, for a model scorer, its `judge_run` digest
   as supplied. Judge runs are not reproduced (041 3.8.3 needs the judge's
   bundles, which a host set does not supply), and the report says so.
3. Every host document carries `assertion: "host"`, and every latency and
   cost metric carries `observation: "host"` in place of 029 3.11.2's
   `candidate` or `historical`. Stdout of every command over host sets
   prints `host-asserted` beside each figure. A host latency is the host
   flow's own measure for that run, not a historical reuse, so a clause
   over it is not unknown under 029 3.12.4.

### 3.6 Provenance separation

1. Host documents have their own schema names (`rustev.host-run/1`,
   `rustev.host-cases/1`, `rustev.host-repeat-report/1`,
   `rustev.host-baseline/1`, `rustev.host-selection/1`). Spec 029's
   documents keep their schemas, bytes, meaning and identities, so
   `rustev.baseline/1` is not reused: its required `plan`, `dataset` and
   `runs` members are Rustev identities a host run does not have, and
   filling them would be fabrication. Under spec 031 3.6.1 a host shape is
   an incompatible shape and takes a new name; nothing is superseded, so
   no support window starts.
2. `baseline record`, `eval reproducibility` and `gate select` given a
   host document and a Rustev-evaluated one, in any position, refuse with
   `invalid_input`, `mixed-provenance`, before any comparison, and write
   nothing (I-1). Rustev-evaluated commands (`eval diff`, `eval
   candidates`, the existing `gate`) refuse a host document by schema.

### 3.7 Host baselines and selection

1. **Baseline.** `rustev baseline record --eval <dir>` over an import
   output writes `rustev.host-baseline/1`: 029 3.8.1's bindings with the
   host identities of 3.4.1 in place of plan, dataset, evaluator
   configuration version and adapter rules, `assertion: "host"`,
   `source: "host"`, `recorded_at_ms` supplied by `--now-ms`, and every
   cohort's `pooled`, interval, band and observation. Recording re-assembles
   the set (3.3.4) and refuses a mismatch.
2. **Reproducibility.** `rustev eval reproducibility` over a host baseline
   and a host import output applies 029 3.9 unchanged (exit 13 for
   `not_reproducible`), with "same configuration" meaning equal host
   system, flow, flow version, dataset (id, digest, count), split,
   evaluator configuration id and repeat configuration id.
3. **Selection.** `rustev gate select --baseline <host baseline>
   --baseline-eval <import dir> --candidates <dir>` reads each immediate
   subdirectory of `--candidates` as one candidate's import output, named
   by the subdirectory (029 3.13.5's name rule, at most 64), and writes
   `rustev.host-selection/1`: 029 3.12.7's content with `assertion:
   "host"`, the host baseline's digest and each candidate's host repeat
   report digest and host identities.
4. **Preconditions** (029 3.12.2 for host sets; a failed one makes the
   candidate `unknown` with the reason): the baseline verifies against
   its import output; each candidate re-assembles; equal host system,
   dataset (id, digest, count), split, case id list and order, evaluator
   configuration id, repeat configuration id and cost unit. Flow and flow
   version may differ: they are what is being selected. Equal stable
   cohorts are **not** required (decision 4): a host `failed` case is an
   observed outcome of the flow, gated through `host/failed_rate`, not a
   Rustev incomparability.
5. **Coverage minimums.** `min_comparable_coverage` applies to each run's
   `1 - host/failed_rate` and `min_labeled_coverage` to each run's
   `host/scored_rate`, on both sides, with 029's arithmetic; a value
   below its minimum rejects on the coverage clause.
6. Clauses, verdicts, ranking and exit codes are 029 3.12.3 to 3.12.6 and
   3.14 unchanged (R-1). 041 3.12.2 applies: a clause over a metric of a
   scorer whose `rules` is `"opaque"` is unknown. The consumer's gate is
   expressible as written: `require: [ { all_slices: { metric:
   "rubric/<name>", not_worse_than: "baseline", within: "noise" } } ]`,
   `and_any: [` `better_than` on `host/latency_ms/p0.5`,
   `host/latency_ms/p0.95` and `host/cost_per_1000` `]`, each declared
   `lower_is_better`.

### 3.8 Held-out splits

A host set on the `final_test` split (role `held_out`) records `held_out` as unknown with
reason `host-ledger-unsupported`: the 029 3.7.3 ledger binds a Rustev
`PlanId` per entry, which a host run lacks. A `held_out` clause over it
is unknown (exit 12), never passed. A host ledger is decision 3.

### 3.9 Exit codes

No new code. `imported` and `recorded` are exit 0; every refusal of this
spec is `invalid_input` (exit 4) naming its code; `gate select` and `eval
reproducibility` keep 0, 11, 12 and 13 as 029 3.14 states.

### 3.10 Library and bindings

The import, baseline, reproducibility and selection functions are public
in `rustev-eval` beside 029's, synchronous, with no I/O and no clock. The
Node and WebAssembly bindings gain nothing in this spec (decision 5).

## 4. Out of scope

Verifying that host runs happened or that their latency and cost are
true; reading or digesting the host's dataset; replaying or re-executing
host runs; reproducing host judge runs; `eval diff` and `eval candidates`
over host sets; comparing host and Rustev-evaluated sets in any way; a
host held-out ledger; binding surfaces; any change to an existing
document, identity, plan or golden.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `gate select` with a host baseline and Rustev-evaluated candidates, or the reverse | `invalid_input`, `mixed-provenance`, exit 4; nothing written. |
| `baseline record` or `eval reproducibility` mixing host and Rustev documents | `invalid_input`, `mixed-provenance`. |
| A `rustev.baseline/1` file given where a host baseline is expected, or the reverse | `invalid_input`, `mixed-provenance`. |
| Two runs with different flow versions or dataset digests | `invalid_input`, `mixed-runs`. |
| Two runs with the same `host.run` handle | `invalid_input`, `mixed-runs`, `duplicate-run`. |
| A scores file whose `views` digest is not the run's views | `invalid_input`, `scores-mismatch`. |
| Views whose cases are not the record's `answered` cases in order | `invalid_input`, `host-binding`. |
| A record whose `views` digest names no supplied file | `invalid_input`, `host-binding`. |
| `cases` count differing from `dataset.cases` | `invalid_input`, `host-run`. |
| A case with `latency_us: { unknown }` among answered cases | `host/latency_ms/*` unknown for that run; a clause over it unknown, exit 12. |
| `cost_unit` null | `host/cost_per_1000` unknown (`cost-not-declared`); never zero. |
| Baseline and candidate with different cost units | Candidate `unknown`. |
| A host set with N = 1 under `k_sd` | Band unknown; `within: noise` unknown, exit 12. |
| A repeat configuration with `top_k` | `invalid_input`, `top-k-not-observed`. |
| A clause over a metric of an `opaque` scorer | Clause unknown (041 3.12.2). |
| A host set on `final_test` under a `held_out` clause | Clause unknown, `host-ledger-unsupported`. |
| Candidate worse than baseline beyond the band on one slice | `rejected` naming the `all_slices` clause; exit 11 when all are rejected. |
| Candidate fails more cases than the baseline | Not `unknown`: measured by `host/failed_rate` and the coverage minimum. |
| A candidate directory changed after import | Re-assembly differs: candidate `unknown` (`reconcile`). |
| Existing commands on today's inputs | Byte-identical files, stdout and exit codes. |

## Acceptance

- `rustev.host-run/1` parsing: every field rule of 3.1, unknown fields
  and duplicate keys refused.
- Import golden over a synthetic host fixture (three runs, two slices,
  one deterministic bound scorer, one `model` scorer, one failed and one
  abstained case, an unknown latency in one run): `host-cases`, the host
  repeat report and stdout.
- The statistics of a host set equal, bit for bit, those 029's functions
  compute from the same case measures arranged as a `captured` set.
- Host baseline, reproducibility (reproducible, exactly at the band,
  beyond the band with exit 13) and selection goldens: qualifies on
  latency, qualifies on cost, rejected on a slice, rejected on coverage,
  unknown (N = 1 band, unknown latency, opaque scorer, held-out), with
  exit codes 0, 11 and 12.
- Every row of section 5 has a test.
- Compatibility: every existing eval, CLI and 029 golden and every plan
  golden with its `PlanId` is byte-identical.
- Mutation seeds, each detected: mixed provenance accepted; an unknown
  latency skipped; unknown cost counted as zero; failed cases included in
  latency; stable cohorts compared for host sets; `observation: "host"`
  treated as `historical`; a duplicate `host.run` accepted.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation. At completion: `cargo test -p rustev-eval
--locked`, `cargo test -p rustev-cli --locked` and the mutation seeds,
under `make verify`.

## Implementation record

None yet.

## Decisions at approval

Decided in this text, with the reason beside each: separate host schema
names (3.6.1); source `host` with `captured` statistics (3.4.3);
`host/` metric names (3.3.3); latency in microseconds reported in
milliseconds (3.1.4, 3.3.3); no new derivation class (3.5.1). Open for
the owner, each with a recommendation, and ratified as recommended on
2026-11-03 (A-77: "061 as recommended"):

1. **Document names.** Separate `rustev.host-*/1` schemas (recommended:
   type-level separation, no support window, I-1 holds by construction),
   or `rustev.baseline/2` and `rustev.repeat-report/2` as tagged unions
   of Rustev and host origin, which would start 031 3.6.2's support window
   for version 1.
2. **Dataset identity.** Accept the host's dataset id and digest as
   asserted (recommended for 0.4.1), or add an optional `--host-dataset
   <file>` that lets the import verify the digest over bytes the host
   names.
3. **Held-out host sets.** Unknown in this spec (recommended), with a
   `rustev.holdout-ledger/2` whose entries bind a host flow identity in a
   later amendment; or refuse host imports on `final_test`.
4. **Stable cohorts in host selection.** Not required equal (recommended,
   3.7.4), or keep 029 3.12.2's rule, under which any flaky failure makes
   a host candidate `unknown`.
5. **Binding exposure.** CLI and Rust library only in 0.4.1
   (recommended), with the Node command surface in a later amendment of
   027; or include `evalRepeatImport` in the Node binding now.
6. **Latency over failed cases.** Excluded and counted, with
   `host/failed_rate` as the guard (recommended), or included at their
   recorded time.

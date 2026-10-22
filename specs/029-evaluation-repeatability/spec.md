---
id: "029-evaluation-repeatability"
title: "Evaluation repeatability, baselines, diffs and selection gates (amends 004 and 006)"
status: approved
implementation: in-progress
created: "2026-10-16"
summary: >
  An amendment of approved specs 004 and 006 (R-16), for #76 child E2.
  Evaluation runs each configuration N times from a declared repeat
  source (deterministic replay, captured historical run sets, or live runs
  once spec 026 is implemented), reports per metric and per slice the
  mean, standard deviation, minimum, maximum, a pooled estimate and a
  case-clustered percentile bootstrap interval computed with a named,
  versioned, seeded generator, and derives declared noise bands. Adds
  named baselines (`rustev.baseline/1`), a reproducibility check, diffs
  in JSON and Markdown, multi-candidate comparison, held-out accounting
  against a host-kept ledger file, and a declarative selection gate
  (`rustev.selection-gate/1`) with stated CI exit codes. Every existing
  document, gate and CLI output keeps its bytes. Answers spec 023's open
  question 3. Approved (A-25); approval authorizes no live run, provider
  call or spend, no quality claim, and no change to `rustev.eval-report/1`
  or any plan identity.
amends:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
extends:
  # Repeat aggregation, bootstrap, bands, slices, baselines, diffs,
  # comparison, selection gates and the held-out ledger (spec 004, 3.5).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `eval --repeat-config`, `eval diff`, `eval reproducibility`,
  # `eval candidates`, `baseline record`, `gate select` and exit code 13.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Adds 029 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list (spec 021, 3.5).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "014-report-integrity"
  - "015-per-case-bundle-failures"
  - "026-live-re-execution"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Without a repeat configuration, every existing evaluation, gate and CLI invocation produces byte-identical documents, stdout and exit codes, and no plan identity changes."
    anchor: "3-15-compatibility"
  - id: "I-2"
    kind: invariant
    text: "Bootstrap intervals and per-run seeds are fully determined by the declared configuration, the case measures and the seed, through the named generator and fixed arithmetic order, and are bit-identical across platforms for identical case measures."
    anchor: "3-4-bootstrap-intervals"
  - id: "I-3"
    kind: invariant
    text: "A noise band derived from fewer than two independent runs is unknown, never zero, except for deterministic replay; an unknown band, statistic or measurement makes every clause that needs it unknown, never a pass."
    anchor: "3-5-noise-bands"
  - id: "I-4"
    kind: invariant
    text: "A held-out evaluation states its count only from a host-supplied ledger or an explicit new-ledger statement; a missing count is unknown, never zero, and Rustev keeps no store."
    anchor: "3-7-split-roles-and-held-out-discipline"
  - id: "R-1"
    kind: requirement
    text: "Latency and cost observed by a historical run are never presented as a candidate's own; a selection clause over them for an offline candidate is unknown."
    anchor: "3-11-multi-candidate-comparison"
  - id: "R-2"
    kind: requirement
    text: "A selection gate yields per candidate exactly one of qualifies, rejected with its failing clause, or unknown with its clause, and the command's exit code is 0, 11 or 12 as section 3.14 states."
    anchor: "3-12-selection-gates"
---

# 029: Evaluation repeatability, baselines, diffs and selection gates (amends 004 and 006)

Approved by the owner (A-25, 2026-10-16, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is in progress, see the implementation record. This spec implements #76 child E2 (#73). The approved texts of
specs 004 and 006, and of their amendments 014 and 015, are not edited;
this spec records what changes, as 014 does for 004. Approval authorizes no
live run, provider call or spend, no quality claim (R-04, R-37), and no
change to `rustev.eval-report/1`, `rustev.eval-detail/1`,
`rustev.dataset/1`, `rustev.evaluator-config/1` or `/2`, or any plan
identity.

## 1. Purpose

A release question such as "does candidate X keep quality at least equal
to the baseline, within noise, and win on latency or price?" needs, per
flow and per change: repeated runs, intervals, noise bands derived from
repeat variance, named baselines, automatic diffs, per-slice results with a
no-regression rule, a count of how often held-out data has been looked at,
and a declarative gate with exit codes CI can rely on. Without repeat
variance a gate either flaps or hides regressions; spec 012's qualification
claims no intervals for that reason, and spec 023 (section 6, question 3)
defers its intervals to this spec.

## 2. Territory

When delivered:

- `crates/rustev-eval/` (owner spec 004): repeat aggregation, per-case
  measures, the bootstrap and its generator, noise bands, slices, the
  held-out ledger, baselines, the reproducibility check, diffs, the
  candidate comparison and selection gates, all as pure functions over
  parsed documents. New eval-owned documents, beside `rustev.dataset/1` and
  `rustev.evaluator-config/2`, which live here today:
  `rustev.repeat-config/1`, `rustev.eval-cases/1`,
  `rustev.repeat-report/1`, `rustev.baseline/1`, `rustev.eval-diff/1`,
  `rustev.comparison/1`, `rustev.selection-gate/1`, `rustev.selection/1`
  and `rustev.holdout-ledger/1`. Test data, including the bootstrap
  reference script and its committed output, lives under
  `crates/rustev-eval/tests/data/bootstrap/`.
- `crates/rustev-cli/` (owner spec 006): the commands of 3.13 and the exit
  codes of 3.14.
- `Makefile` (owner spec 001): 029 joins `make verify` at completion.

No change to `rustev-contract`, `rustev-core`, `rustev-runtime`, any
backend or integration, any plan, definition or descriptor schema, or any
dependency: the generator and the statistics are implemented inside
`rustev-eval` with the contract's existing SHA-256 (`tagged_digest`), so
eval's normal dependencies stay contract, core, `serde` and `serde_json`
(spec 004, 2). `rustev.eval-report/1` stays the report envelope (R-06); the
new documents compose it, as spec 004's detail and configuration documents
do. The library functions are the surface the Node and WebAssembly
bindings (#76 A1) and the service (A2) expose; exposing them is those
specs' work.

### 2.1 Binding principles of #76

- **Proposals, never grants.** A selection verdict, a baseline and a
  reproducibility result are evaluation evidence for the host's release
  process. None is accepted by an executor, none is a `Permitted` value,
  and none selects or deploys a candidate.
- **Derivation classes and lineage.** No decision value is created or
  changed. Every aggregate names the run reports, case measures, seeds and
  configuration it came from (record digests), and a metric keeps the
  derivation its source report declares; a model-derived metric (#76 E1)
  is never pooled with a deterministic one.
- **Unresolved, never a default; no silent kind conversion.** Unknown runs,
  zero denominators, undefined resamples, unknown bands and missing
  held-out counts are unknown and named; no aggregate converts one metric
  kind to another, and a metric's per-run values are pooled only with
  values of the same metric name and cohort.
- **Pure core; no async runtime in contract and core.** Core and contract
  are untouched. Eval stays synchronous, performs no I/O and reads no
  clock: the CLI supplies `now_ms`.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and still passes.
- **No dynamic plugin loading.** Candidates are files the host names;
  nothing is loaded as code.
- **No identity, authority or knowledge store.** Baselines, ledgers and
  reports are files the host keeps and passes back; Rustev writes them only
  to paths it is given.
- **Plan identities.** This spec adds no plan construct. Every existing
  plan and definition golden stays byte-identical, with its `PlanId`.

## 3. Behavior

### 3.1 Repeat sources and runs

1. A **repeat set** is N evaluations (runs) of one configuration: one
   dataset, split, evaluator configuration, task adapter, repeat
   configuration (3.3) and plan (or candidate plan). N is 1 to 64. Each run
   is a spec 004 evaluation (`evaluate`) producing its report, detail and
   case measures (3.2).
2. A repeat set states its **source**, recorded in the repeat report:
   - `replay`: one set of retained bundles reproduced N times. Spec 004
     reproduction is byte-identical, so every run must yield identical case
     measures; a difference is refused as `not-reproducible` (a defect, not
     noise). The band basis is `deterministic_replay` (3.5.3).
   - `captured`: N bundle sets, each captured from a separate historical
     execution of the same plan (for example `rustev run` with capture, or
     a host's runtime with the scripted, seeded stochastic backends of R-09
     receiving the per-run seed of 3.4.6). Each set is reproduced once; the
     variation between sets is the recorded backend variation.
   - `live`: N fresh executions through spec 026's `reexecute_live`,
     extended to dataset scale by #76 E3 (#74). This spec defines only how
     such runs are aggregated; it adds no live path, and the `live` source
     is unavailable until spec 026 and its E3 amendment are implemented.
     Until then it is refused (`invalid_input`, `live-source-unavailable`).
3. Every run of a set must evaluate the same case ids in the same manifest
   order under the same plan (or candidate plan) identity; otherwise the
   set is refused (`mixed-runs`). A case may be comparable in one run and
   incomparable in another (for example a bundle unusable in one captured
   set, spec 015): the per-run outcome is kept, and the case is outside
   the set's **stable cohort**, the cases comparable in every run, which
   3.12 uses.
4. Offline candidate comparison follows spec 004, 3.4: a candidate plan is
   replayed against each run's retained raw outputs. A candidate whose
   changes prevent reuse of raw outputs (another artifact, descriptor,
   question or projection) is incomparable per case, and that is reported,
   never compared. Comparing another backend or model needs the `live`
   source.

### 3.2 Per-case measures

1. Every run in a repeat set writes a `rustev.eval-cases/1` document,
   bounded by `REPLAY_V1` and written in record canonical form (spec 004,
   5.6), binding the run's report record digest and listing, per case in
   manifest order: the case id, the record digest of the run record the
   case was measured from (or null), the outcome key of its
   `rustev.eval-detail/1` entry, the top-k list of 3.2.4 when declared,
   and its measures.
2. A **measure** is a case's contribution to one metric, of one of three
   forms: a **count ratio** contribution (integer numerator and
   denominator, each 0 or 1 for the spec 004 rates), a **mean** contribution
   (a binary64 numerator and an integer denominator, for mean log loss,
   multiclass Brier, reliability bin confidence and cost per 1000), or a
   **sample** (a binary64 value entering a nearest-rank quantile, for
   `latency_ms/*` and `queue_ms/*`). A case that does not enter a metric's
   denominator has no measure for it.
3. Spec 004's metrics map as follows: `coverage`, `acceptance_coverage`,
   `labeled_proposal_coverage`, `error_among_accepted`,
   `candidate_outcome_change` and reliability accuracy are count ratios over
   the denominators spec 004 3.5.4 and 3.5.5 define; `mean_log_loss`,
   `brier` and reliability confidence are means; latency and queue
   quantiles are samples; `cost_observed_units`, `cost_estimated_units` and
   `cost_liability_units` are **totals** and have no case-resampling
   interval (3.4.7). Metrics another spec adds (for example #76 E1's
   ranking and rubric metrics) take part when that spec defines their
   per-case measure; without one they are reported with across-run
   statistics only, and their interval is unknown (`not-case-decomposable`).
4. When the repeat configuration declares `top_k: { param, k }` (k 1 to
   100), each case records the first k candidates of that ranking
   parameter of its proposal, or null when the case has no such proposal.
5. Two derived metrics are added to repeat reports only (never to
   `rustev.eval-report/1`): `cost_per_1000/observed` and
   `cost_per_1000/estimated`, each 1000 times the summed series cost over
   the comparable cases, as a mean contribution per comparable case. Each
   is unknown when the configuration does not declare cost units
   comparable (spec 004, 3.5.7), and `cost_per_1000/observed` is unknown
   when any comparable case carries unknown cost liability. Unknown cost is
   never counted as zero.
6. Reconciliation: for each run, the measures computed over the whole
   cohort must reproduce the run report's metric values bit for bit, and
   the count-ratio sums must equal the detail's counts. A mismatch refuses
   the repeat set (`reconcile`). The per-run values are computed by the same
   functions the report uses, over the cases in manifest order.

### 3.3 Repeat configuration and statistics

1. `rustev.repeat-config/1`, under `REPLAY_V1`, canonical, carrying no
   binary64 number (fractions are Decimal strings, spec 002 3.3.1). Its
   identity (`tagged_digest` of its canonical bytes) is the repeat
   configuration id. Unknown fields and duplicate keys are refused.

   ```text
   repeat-config = { schema: "rustev.repeat-config/1",
     bootstrap: { method: "percentile-cluster/1",
                  prng: "xoshiro256starstar-splitmix64/1",
                  level: decimal, resamples: integer, seed: u64 string },
     noise: { rule: band-rule, floor: decimal,
              overrides: { metric: { rule: band-rule, floor: decimal } } },
     slices: [ { name: string, min_cases: integer } ],
     stability_metrics: [ metric ],
     top_k: null | { param: string, k: integer } }
   band-rule = { "k_sd": { k: decimal } } | "interval_width"
   ```

   `level` is in [0.5, 0.999]; `resamples` is 1 000 to 100 000; `seed` is
   the decimal digits of a `u64`; `k` is in (0, 10]; floors are
   non-negative; `min_cases` is at least 1; slice names are unique, none is
   `all`; `stability_metrics` is non-empty, unique, and names metrics the
   evaluator configuration reports or 3.2.5 derives.
2. Per metric and cohort (3.6), over the N per-run values `v_0 .. v_{N-1}`
   (run order): `mean` is their sum, added left to right, divided by N;
   `sd` is the square root of the sum of `(v_r - mean)^2`, left to right,
   divided by `N - 1`, and is unknown for N = 1 except under 3.5.3; `min`
   and `max` are the extremes. If any run's value is unknown or non-finite,
   all four are unknown, naming the runs; no run is dropped.
3. `pooled` is the metric computed once over all runs' contributions:
   for a count ratio or a mean, each case's contributions are summed over
   runs in run order, then summed over cases in manifest order (integers
   exactly, binary64 left to right), and the numerator sum is divided by
   the denominator sum (zero denominator: unknown); for a sample metric, the
   nearest-rank quantile (spec 004, 3.5.7) of all runs' samples. `pooled`
   is the value gates and diffs compare; `mean` is reported beside it.

### 3.4 Bootstrap intervals

1. **Method `percentile-cluster/1`.** A percentile bootstrap that resamples
   cases with replacement and keeps each case's N run contributions
   together (cases are clusters), so the interval reflects both case
   sampling and run-to-run variation. Each resample's statistic is
   `pooled` (3.3.3) over the resampled cases, each draw counted once per
   time it is drawn.
2. **Generator `xoshiro256starstar-splitmix64/1`.** All arithmetic is on
   unsigned 64-bit integers, wrapping. SplitMix64 with state `x`:
   `x += 0x9E3779B97F4A7C15; z = x; z = (z ^ (z >> 30)) *
   0xBF58476D1CE4E5B9; z = (z ^ (z >> 27)) * 0x94D049BB133111EB; return z ^
   (z >> 31)`. xoshiro256\*\* state `s[0..4]` is four consecutive SplitMix64
   outputs; each step: `result = rotl(s[1] * 5, 7) * 9; t = s[1] << 17;
   s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3]; s[2] ^= t;
   s[3] = rotl(s[3], 45); return result`. A later change of any step is a
   new generator name; this one never changes.
3. **Stream seeds.** A stream for label `l` starts SplitMix64 at the
   integer whose big-endian bytes are the first 8 bytes of the SHA-256
   digest in `tagged_digest("rustev.bootstrap/1", seed || 0x00 || l)`,
   where `seed` is the configuration's decimal seed digits and `l` is
   UTF-8. Bootstrap streams use `l = "cohort/" + cohort name`.
4. **Uniform index.** To draw an index in `[0, n)`, `n >= 1`: draw `u`;
   if `2^64 mod n != 0` and `u >= 2^64 - (2^64 mod n)`, draw again;
   return `u mod n`.
5. **Resampling.** For a cohort of C cases (manifest order, indexed 0 to
   C - 1), one stream produces resamples `b = 0 .. B - 1` in order, each of
   C consecutive draws in draw order. Every metric and slice statistic of
   that cohort uses the same B resamples. A resample whose statistic is
   undefined (zero denominator, no sample) is counted; if any is undefined,
   or any value is non-finite, the interval is unknown with that count;
   no resample is dropped.
6. **Run seeds.** The seed handed to run r (for a `captured` set produced
   by seeded stub backends, or a `live` run under E3 that accepts one) is
   the first `u64` of the stream with `l = "run/" + r`. The repeat report
   records it; whether a host's backend honored it is the host's
   statement.
7. **Percentile.** With `L` the level's Decimal units (level times
   10^9), sort the B values ascending; the lower bound is the value at
   one-based rank `max(1, ceil((10^9 - L) * B / (2 * 10^9)))` and the
   upper bound at rank `max(1, ceil((10^9 + L) * B / (2 * 10^9)))`, both
   computed in exact integer arithmetic (for 0.95 and B = 10 000: ranks
   250 and 9 750). Totals and metrics without a per-case measure have
   interval unknown with their reason.
8. **Bound.** A cohort with `B * C * N` above 10^9 is refused
   (`bootstrap-too-large`) before any resample is drawn.
9. **Cross-platform determinism.** Given identical case measures, every
   value of 3.3 and 3.4 is bit-identical on every target, including
   `wasm32`: integer generator, exact integer ranks, IEEE 754 binary64
   addition, subtraction, multiplication, division and square root in the
   fixed orders stated, no fused multiply-add and no transcendental
   function. Per-case measures that involve a logarithm (log loss) are
   computed by spec 004's existing code; this spec claims nothing about
   them beyond spec 004.
10. This answers spec 023's open question 3: qualification intervals are
    these intervals, over the governed denominators, with the seed and
    configuration recorded in the repeat report.

### 3.5 Noise bands

1. For each metric and cohort the band is `max(derived, floor)`, with the
   metric's override when declared. `k_sd`: `derived = k * sd`.
   `interval_width`: `derived = upper - lower` of the 3.4 interval.
2. A band is unknown when its input is unknown: `k_sd` over fewer than two
   runs of a `captured` or `live` set, or with `sd` unknown;
   `interval_width` with the interval unknown. An unknown band is never
   replaced by the floor or by zero.
3. **Deterministic replay.** For a `replay` set, `sd` is exactly 0 for any
   N (runs 2 to N only verify it, 3.1.2), so a `k_sd` band is the floor,
   with basis `deterministic_replay`. It states that replaying the same
   retained outputs varies by nothing; it says nothing about how a live
   backend varies. With a zero band, "within noise" is exact comparison:
   any regression rejects, "better than" means strictly better, and any
   difference between two repeat sets fails the reproducibility check. A
   host that wants tolerance declares a floor, which is part of the
   repeat configuration's identity.
4. The repeat report stores each band with its basis (`k_sd`,
   `interval_width`, `deterministic_replay`, or `floor` when the floor
   won); a baseline copies them (3.8).

### 3.6 Slices

1. Slices are the scenario classes datasets already declare as subgroup
   membership (`rustev.dataset/1` per-case `subgroups`, spec 004 3.5.1).
   The repeat configuration selects which subgroups are reported as slices
   and the minimum size of each. A case may belong to several slices.
2. Every metric is reported for the cohort `all` and for each declared
   slice, with its own denominators, statistics, interval (its own stream,
   3.4.3) and band. A slice with fewer stable-cohort cases than
   `min_cases` has status `below_minimum`, and every metric of it is
   unknown (spec 023, 3.4.4), never a figure.
3. A declared slice no case of the split belongs to is `below_minimum`;
   it is not refused, so a gate over it is unknown, not passed.

### 3.7 Split roles and held-out discipline

1. Spec 004's four splits have fixed roles, recorded in every repeat
   report and baseline: `training` is `train`; `model_selection` and
   `calibration` are `validation`; `final_test` is `held_out`. Roles are
   not declared per dataset, so a dataset cannot relabel its final test.
2. Optimization (prompt or parameter search, threshold or calibration
   choice) may read only non-held-out splits. Rustev enforces this where
   it offers optimization: calibration fitting already reads only the
   calibration split (spec 004, 3.6), and an evaluation run with
   `--purpose optimization` on `final_test` is a usage error. Optimization
   code outside Rustev cannot be policed; the count below makes its use of
   held-out data visible.
3. **Ledger.** A held-out repeat set requires a host-supplied lineage
   handle (non-empty, at most 256 bytes, the contract's `Handle` rules) and
   either a `rustev.holdout-ledger/1` file or the explicit statement that
   the ledger is new. The ledger (`REPLAY_V1`, canonical, no binary64) is
   `{ schema, dataset, entries }`; each entry is `{ lineage, plan,
   repeat_config, runs, recorded_at_ms, prev }`, where `runs` lists the run
   report record digests, and `prev` is null for the first entry and
   otherwise `tagged_digest("rustev.holdout-ledger/1#entry", canonical
   bytes of the previous entry)`. At most 10 000 entries. A ledger whose
   chain does not verify, whose dataset is another, or whose entry is
   malformed is refused (`invalid_input`).
4. Evaluation appends one entry and returns the new ledger, which the CLI
   writes to a new file; the input is never modified. The repeat report
   records `held_out: { lineage, count, plan_count, dataset_count,
   ledger_head_before, ledger_head_after }`: the number of entries with
   this lineage, with this plan, and in total, each including the new one,
   and the heads (digest of the last entry, or `none`). For non-held-out
   splits `held_out` is null.
5. **Limit.** The chain proves the ledger's internal consistency, not its
   completeness: a host can supply an older or truncated ledger.
   Recording the head before and after in every report lets an auditor
   check that successive reports extend one chain; Rustev claims no more.
   Without a ledger, the count is unknown, never zero.

### 3.8 Baselines

1. `rustev.baseline/1`, under `REPLAY_V1` in record canonical form,
   identified by its record digest. It binds: `name` (1 to 64 bytes of
   `[A-Za-z0-9._-]`, starting alphanumeric); the repeat report's record
   digest; each run's report, detail and case-measure digests; dataset id,
   split, split role and provenance; evaluator configuration id and
   version, and the task adapter rules (spec 014); repeat configuration id;
   plan id; source, N and run seeds; `recorded_at_ms` (supplied, never
   read); the held-out record when the split is held out; and every
   cohort's per-metric `pooled`, interval and band, copied from the repeat
   report.
2. Recording verifies every binding against the documents supplied and
   refuses a mismatch. A synthetic-provenance baseline stays labeled as
   such; a baseline is a measurement record, not a claim of quality.

### 3.9 Reproducibility check

1. Compares a repeat set against a baseline of the same configuration
   (same dataset, split, evaluator configuration id, repeat configuration
   id and plan id; otherwise `unknown`). For every stability metric
   (3.3.1) in every cohort, it is `reproducible` when `|pooled_run -
   pooled_baseline| <= band_baseline`, and `not_reproducible` when any
   exceeds it, naming each metric, cohort, delta and band.
2. It is `unknown` when any compared value or baseline band is unknown and
   no exceedance was found. An exceedance with other values unknown is
   still `not_reproducible`: a definite failure needs nothing else.
3. `not_reproducible` is a stop-and-report condition with its own exit
   code (13), distinct from a gate failure, so a host's pipeline can stop
   on it.

### 3.10 Diffs

1. `rustev.eval-diff/1` (`REPLAY_V1`, record canonical) compares a run's
   repeat set with a baseline whose dataset, split, evaluator configuration
   id, adapter rules and repeat configuration id are equal (otherwise the
   diff is `unknown` with the reason, and no table is produced).
2. Per cohort (`all`, then slices in declared order) and metric (report
   order): baseline and run `pooled`, `delta = run - baseline`, both
   intervals, `overlap` (the closed intervals intersect; unknown when
   either interval is), and `within_noise` (`|delta| <= max(band_baseline,
   band_run)`; unknown when either band is). The diff states no direction:
   better or worse is a gate's declaration (3.12).
3. Per case: a case is changed when its multiset of outcome keys over the
   runs differs between the sides, or, when `top_k` is declared, its
   multiset of top-k lists differs. Each changed case lists both sides'
   distributions and, per side, the run record digests it was measured
   from, which are the inputs of the per-case decision diff of #76 D2
   (#71). At most 10 000 changed cases are listed in manifest order; the
   total is always stated, and `truncated` says whether the list is
   complete.
4. `diff.md` is rendered deterministically from the diff document only:
   one table per cohort (metric, baseline, run, delta, intervals, overlap,
   within noise), then the changed cases. Unknown is printed as `unknown`
   with its reason. It is a rendering, never a second source.

### 3.11 Multi-candidate comparison

1. One command evaluates up to 64 named candidates (a candidate is a
   variant: a plan with its backend bindings, calibrations and profile)
   against the same dataset, split, configurations and bundle sets, as one
   repeat set per candidate, and writes `rustev.comparison/1` and
   `comparison.md`: one row per candidate after the baseline, with
   coverage, acceptance coverage, error among accepted, abstentions and
   incomparable cases by reason, `latency_ms/p0.5` and `latency_ms/p0.95`
   when the evaluator configuration declares those quantiles (otherwise
   unknown), `cost_per_1000/*`, and every other declared metric, each as
   `pooled` with its interval.
2. **Observation provenance.** Every latency, queue and cost metric in a
   repeat report states `observation`: `candidate` when the runs executed
   the evaluated plan (a baseline's own replay or capture, or a `live`
   candidate), `historical` when an offline candidate comparison reuses the
   baseline's run records (spec 004 3.5.7 measures latency from retained
   run records, which an offline candidate did not produce). A historical
   observation is never presented as the candidate's: the comparison prints
   it as `historical`, and a selection clause over it is unknown (3.12.4).

### 3.12 Selection gates

1. `rustev.selection-gate/1`, under `DESCRIPTOR_V1`, canonical, no
   binary64, unknown fields and duplicate keys refused:

   ```text
   gate = { schema: "rustev.selection-gate/1", gate: name, per: "variant",
            metrics: { metric: "higher_is_better" | "lower_is_better" },
            min_comparable_coverage: decimal, min_labeled_coverage: decimal,
            require: [clause], and_any?: [clause], rank_by: [metric] }
   clause = { metric, not_worse_than: "baseline",
              within: "noise" | { absolute: decimal } }
          | { metric, better_than: "baseline" }
          | { all_slices: metric-clause }
          | { held_out: { max_evaluations: integer } }
   ```

   Every metric a clause or `rank_by` names must have a declared direction
   (otherwise the document is refused); `require` is non-empty; `and_any`,
   when present, is non-empty; coverages are in [0, 1]. The issue's
   example is valid once it adds `schema` and `metrics`, with `rubric_score`
   (a #76 E1 metric) and `cost_per_1000/observed` as metric names.
2. **Preconditions per candidate** (otherwise that candidate is `unknown`
   with the reason): every spec 004 3.5.8 and spec 014 3.3 precondition of
   the existing gate, applied to each run of both sides (bound, equal
   adapter rules; same dataset, split and evaluator configuration id; each
   detail bound to its report; the candidate a candidate evaluation); the
   baseline document verifies against the baseline's repeat set; equal
   repeat configuration ids; equal stable cohorts. Each run on each side
   below a minimum coverage makes the candidate `rejected` on the coverage
   clause, as in spec 004.
3. **Clauses**, over `pooled` values of the cohort `all` (or each slice
   for `all_slices`), with `band = max(band_baseline, band_candidate)`:
   - `not_worse_than` within noise: higher is better passes when `c >= b -
     band`; lower is better when `c <= b + band`. `within: { absolute: t }`
     uses `t` instead of the band, so needs no band.
   - `better_than`: higher is better passes when `c > b + band`; lower is
     better when `c < b - band`. An improvement within noise is not a win.
   - `all_slices`: the inner clause holds for every declared slice; a
     `below_minimum` slice makes it unknown.
   - `held_out`: on a held-out split, passes when the candidate's lineage
     count (3.7.4) is at most `max_evaluations`, unknown when there is no
     count; on another split it passes, stating that nothing is held out.
4. A clause is unknown when a value, band or count it needs is unknown, or
   when it names a latency, queue or cost metric whose candidate
   observation is `historical`.
5. **Verdict.** A candidate is `rejected`, naming the first failing clause
   (`require[i]`, or `and_any` when every `and_any` clause fails), when any
   `require` clause fails or every `and_any` clause fails; otherwise
   `unknown`, naming the first unknown clause, when any `require` clause is
   unknown or no `and_any` clause passes; otherwise `qualifies`. A definite
   failure rejects regardless of unknown clauses; unknown never becomes
   qualifies.
6. **Ranking.** Qualifying candidates are ordered by `rank_by`
   lexicographically, each metric's `pooled` value in its declared
   direction, ties by candidate name in byte order. A qualifying candidate
   with an unknown or historical rank metric is listed as `unranked` with
   the reason, never placed by a default.
7. **Output.** `rustev.selection/1` (`REPLAY_V1`, record canonical) binds
   the gate's identity, the baseline's digest and each candidate's repeat
   report digest, and lists every candidate's verdict with every clause
   result, then `ranked` and `unranked`. It is evidence for the host's
   choice, not the choice.

### 3.13 CLI surface (amends spec 006)

Grammar, bounded file access, no-clobber writes and stdout rules of spec
006 3.1 and 3.3 apply unchanged. A command with a subcommand path is
matched before its parent, so `rustev eval diff` never parses as `rustev
eval`.

1. **`rustev eval`** gains `--repeat-config <file>`, `--repeat <N>` (1 to
   64), `--purpose measurement|optimization` (default `measurement`),
   `--lineage <handle>`, `--ledger <file>`, the switch `--ledger-new` and
   `--ledger-out <file>`; `--bundles` becomes repeatable. Rules between
   flags, each a usage error (exit 2) before any file is read: every new
   flag, and a second `--bundles`, needs `--repeat-config`; one `--bundles`
   is the `replay` source and needs `--repeat`; several are the `captured`
   source, one run per directory in the order given, and `--repeat`, if
   given, must equal their number; `--split final_test` with
   `--repeat-config` needs `--lineage`, `--ledger-out` and exactly one of
   `--ledger` or `--ledger-new`, and those flags are refused on other
   splits; `--purpose optimization` with `--split final_test` is refused.
   With `--repeat-config`, `--out` receives `config.json`, `adapter.json`,
   `repeat-config.json`, `repeat.json` (the repeat report) and per run
   `run-<r>.report.json`, `run-<r>.detail.json` and `run-<r>.cases.json`,
   all or none; stdout is `reported` with the repeat report's digest, N,
   source and, per cohort, each metric's `pooled`, interval and band.
2. **`rustev baseline record`**: `--name`, `--eval <dir>` (an `eval`
   output with a repeat report), `--now-ms`, `--out <file>`. Prints
   `recorded` with the baseline's digest.
3. **`rustev eval reproducibility`**: `--baseline <file>`, `--baseline-eval
   <dir>`, `--run <dir>`. Prints `reproducible`, `not_reproducible` or
   `unknown` with every exceedance or reason.
4. **`rustev eval diff`**: `--baseline <file>`, `--baseline-eval <dir>`,
   `--run <dir>`, `--out <dir>`. Writes `diff.json` and `diff.md`, all or
   none; prints `diffed`, or `unknown` with the reason.
5. **`rustev eval candidates`**: the inputs of `eval` with
   `--repeat-config` (except `--candidate-plan`, `--descriptor`, `--rules`
   and `--calibration`), plus `--candidates <dir>`: each immediate
   subdirectory is a candidate whose name is the subdirectory's name (the
   case id character rule of spec 006 3.9.1, at most 64 candidates) and
   holds `plan.json` and optional `descriptors/`, `rules/` and
   `calibrations/` directories whose regular files are read in byte order
   of their names, under the caps of spec 006 3.3. It creates
   `<out>/<name>/` per candidate (an existing one is `io_error` before any
   work), writes each candidate's repeat output there and
   `comparison.json` and `comparison.md` in `--out`, all or none; prints
   `compared`.
6. **`rustev gate select`**: `--selection-gate <file>`, `--baseline
   <file>`, `--baseline-eval <dir>`, `--candidates <dir>` (an `eval
   candidates` output), `--out <file>`. Writes `rustev.selection/1`;
   prints `selected`, `none_qualified` or `unknown`, with every candidate's
   verdict and the ranked list.
7. **`rustev gate`** with `--baseline`, `--candidate` and `--gate` is
   unchanged in inputs, behavior, stdout bytes and exit codes.

### 3.14 Exit codes

Spec 006 3.2's table is extended; no existing code or status changes
meaning.

| Code | New status values | Meaning |
|---|---|---|
| 0 | `recorded`, `reproducible`, `diffed`, `compared`, `selected` | `selected`: every candidate's verdict is known (`qualifies` or `rejected`) and at least one qualifies. |
| 4 | `invalid_input` | Also: a repeat configuration, case-measure, repeat report, baseline, ledger, selection gate or candidate input refused, or a repeat set refused (`not-reproducible` within a `replay` set, `mixed-runs`, `reconcile`, `bootstrap-too-large`, `live-source-unavailable`), naming it. |
| 11 | `none_qualified` | Every candidate's verdict is known and none qualifies. |
| 12 | `unknown` | A reproducibility check, diff or selection is unknown; for `gate select`, any candidate's verdict is unknown, even if another qualifies, because the ranking could change. |
| 13 | `not_reproducible` | A reproducibility check found a difference beyond the baseline's band: stop and report. |

### 3.15 Compatibility

1. Without `--repeat-config`, `rustev eval` and `rustev gate` read the
   same inputs and write the same files, stdout bytes and exit codes as
   today; the library's `evaluate` and `evaluate_gate` are unchanged.
   `rustev.eval-report/1`, `rustev.eval-detail/1`, `rustev.dataset/1` and
   `rustev.evaluator-config/1` and `/2` keep their schemas, bytes and
   identities. Existing CLI and eval goldens stay byte-identical.
2. Spec 004 5.6's list of record-canonical documents gains
   `rustev.eval-cases/1`, `rustev.repeat-report/1`, `rustev.baseline/1`,
   `rustev.eval-diff/1`, `rustev.comparison/1` and `rustev.selection/1`,
   which hold binary64 numbers; `rustev.repeat-config/1`,
   `rustev.selection-gate/1` and `rustev.holdout-ledger/1` are
   identity-bearing and hold none.
3. Spec 014's gate precondition (bound, equal adapter rules) applies to
   every comparison here; spec 015's per-case bundle failures are per-run
   case outcomes (3.1.3).
4. Plans and definitions are untouched: every plan golden keeps its bytes
   and `PlanId`.

## 4. Out of scope

Live dispatch of any kind (spec 026, #76 E3); provider calls, spend or
spend caps; the optimization objective API (#76 future work); training;
storing baselines, ledgers or reports (the host keeps them); signing or
attesting documents; Wilson or other analytic intervals; interval methods
other than `percentile-cluster/1`; significance tests or multiple
comparison corrections; changing how spec 004 computes any existing metric;
the per-case decision diff itself (#76 D2); quality claims from synthetic
fixtures (R-04).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `--bundles` given twice, or `--repeat`, without `--repeat-config` | Usage error, exit 2, nothing read. |
| `--split final_test` with `--repeat-config` and neither `--ledger` nor `--ledger-new`, or without `--lineage` | Usage error, exit 2. |
| `--purpose optimization --split final_test` | Usage error, exit 2. |
| Two `replay` runs whose case measures differ | `invalid_input`, `not-reproducible`; no repeat report. |
| Captured runs with different plan ids or case ids | `invalid_input`, `mixed-runs`. |
| Case measures that do not reproduce the report's metric bit for bit | `invalid_input`, `reconcile`. |
| A `captured` set with N = 1 under `k_sd` | Band unknown; a `within: noise` clause is unknown, exit 12. |
| A `replay` set with all runs identical under `k_sd`, floor 0 | Band 0, basis `deterministic_replay`; a candidate lower by one ulp on a higher-is-better metric is rejected. |
| A resample with a zero denominator | Interval unknown with the undefined count; no resample dropped. |
| `B * C * N` above 10^9 | `invalid_input`, `bootstrap-too-large`, before drawing. |
| A slice below `min_cases` | Every metric of the slice unknown; `all_slices` unknown. |
| A ledger with a broken `prev` chain, or another dataset | `invalid_input`. |
| A held-out repeat report without a count, under a `held_out` clause | Clause unknown. |
| Two repeat sets differing by more than the band on a stability metric | `not_reproducible`, exit 13. |
| Two repeat sets differing by exactly the band | `reproducible`, exit 0. |
| `better_than` on latency for an offline candidate | Clause unknown (`historical`). |
| One candidate qualifies, another is unknown | `unknown`, exit 12. |
| All candidates rejected | `none_qualified`, exit 11. |
| A selection gate naming a metric without a declared direction | `invalid_input`. |
| Baseline and run with different repeat configuration ids | Diff and reproducibility `unknown`; selection candidates `unknown`. |
| A gate run over version 1 evaluator configurations | Every candidate `unknown` (spec 014, 3.3). |
| `live` source requested before spec 026 is implemented | `invalid_input`, `live-source-unavailable`. |
| The existing `rustev gate` on today's inputs | Byte-identical stdout and exit code. |

## Acceptance

- Bootstrap intervals are verified against a reference implementation on
  fixed seeds: `crates/rustev-eval/tests/data/bootstrap/reference.py`
  (Python 3 standard library only, written from this spec's text, not
  translated from the Rust code) implements 3.3 and 3.4 and, from the
  committed input `cases.json` (count ratios, means, samples, a slice, a
  resample with a zero denominator, N = 1 and N = 5, at least three seeds
  and two levels), writes `expected.json` with every binary64 value as its
  16-hex-digit bit pattern. The committed output is compared bit for bit
  by a Rust test; `python3 -I reference.py --check` regenerates it and
  compares bytes. Generator known-answer vectors (SplitMix64 and
  xoshiro256\*\* outputs for fixed states, and stream seeds of 3.4.3) are
  committed beside it, from the authors' reference C implementations
  for the generators.
- The same bootstrap known-answer test passes compiled for `wasm32-wasip1`
  under a WASI runtime. This item depends on the WebAssembly toolchain of
  #76 A1; until that lands it is reported as externally blocked, never as
  passed.
- The reproducibility check fails on an injected drift larger than the
  band and passes otherwise, including a drift exactly equal to the band,
  a zero band from deterministic replay, and an unknown band.
- Gate evaluation is golden-tested for qualify, reject on quality, reject
  on slice regression, reject for no efficiency win, and unknown (unknown
  band, historical latency, held-out count missing or above its maximum),
  with the exit codes of 3.14.
- The diff report (`diff.json` and `diff.md`) and the comparison
  (`comparison.json` and `comparison.md`) are golden-tested, including a
  changed outcome, a changed top-k list, an unknown interval and
  truncation.
- Held-out ledger: counts by lineage, plan and dataset; chain, dataset and
  bound refusals; the input file never modified.
- Every row of section 5 has a test.
- Compatibility: every existing eval and CLI golden, and every plan and
  definition golden with its `PlanId`, is byte-identical.
- Seeds in the eval and CLI mutation harnesses (rank off by one, an
  undefined resample dropped, an unknown band treated as zero, unknown
  treated as qualifies, the band of one side only, the held-out count
  off by one, a historical latency counted as the candidate's, a ledger
  chain not checked, exit 13 mapped to 11) are each detected.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item, and no
existing evaluation is a repeat set under this spec.

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it, and 029 joins
`make verify` only when every item it can verify is delivered. Plan:

1. The statistics core (3.3 to 3.5): the generator, stream and run seeds,
   `rustev.repeat-config/1`, across-run statistics and `pooled`, the
   case-clustered bootstrap with exact ranks, noise bands; the Python
   reference, its committed input and output, and the generator
   known-answer vectors.
2. Per-case measures (3.2): `rustev.eval-cases/1` per run, the measures
   of spec 004's metrics and their reconciliation with the run report.
3. Repeat sets (3.1, 3.2.5, 3.6): the `replay`, `captured` and refused
   `live` sources, the stable cohort, slices, `cost_per_1000/*` and
   `rustev.repeat-report/1`.
4. The held-out ledger, baselines and the reproducibility check (3.7 to
   3.9).
5. Diffs, `rustev.eval-diff/1` and `diff.md` (3.10).
6. Multi-candidate comparison and selection gates (3.11, 3.12).
7. The CLI surface and exit codes (3.13 to 3.15), the mutation seeds,
   declared verification and completion. The `wasm32-wasip1` item stays
   externally blocked until a WASI runner exists in CI.

**Increment 1 (the statistics core).**
`crates/rustev-eval/src/generator.rs` implements
`xoshiro256starstar-splitmix64/1` on wrapping `u64` arithmetic (3.4.2),
the stream seed of 3.4.3 through the contract's `tagged_digest`, the
rejection-sampled uniform index of 3.4.4, and the run seeds of 3.4.6.
`src/repeat_config.rs` defines `rustev.repeat-config/1` (`RepeatConfig`,
`REPLAY_V1`, identified by its canonical digest, unknown fields and
duplicate keys refused, no binary64 number) and checks every rule of
3.3.1; the rule that metrics are reported is a separate check over the
names the evaluator configuration reports, which increment 2 supplies.
`src/repeat_stats.rs` takes per-case contributions in manifest and run
order (count ratio, mean or sample, 3.2.2) and computes the per-run
values, `mean`, `sd`, `min`, `max` and `pooled` (3.3.2, 3.3.3), the
`percentile-cluster/1` interval of every metric of one cohort over the
cohort's own stream (3.4.1, 3.4.5, 3.4.7), with `B * C * N` above 10^9
refused before drawing (3.4.8), and the band of 3.5 with its basis. An
unknown run value makes the four across-run statistics unknown, naming the
runs; an undefined resample makes the interval unknown with its count; an
unknown `sd` or interval makes the band unknown, never the floor or zero.
No dependency is added, and nothing reads a clock or performs I/O.

Tests: `crates/rustev-eval/tests/bootstrap.rs` compares the generators
with `tests/data/bootstrap/generators.json`, printed by `generators.c`
from the authors' reference C implementations, and compares every stream
seed, run seed, index draw, rank, per-run value, statistic and interval
with `expected.json`, written by `reference.py` (Python 3 standard library
only, written from this spec's text, which also checks the C vectors) from
`cases.json`: four sets (N = 5 with an unknown run value and a slice,
N = 1, N = 1 replay, and N = 2 with negative values and zeros of both
signs), count ratios, means and quantiles, a metric whose resamples have
zero denominators, four seeds and three levels, every binary64 value as
its bit pattern. The bootstrap function also refuses a seed, level or
resample count outside 3.3.1, and a contribution with a zero denominator
(a case outside a denominator has none). `python3 -I reference.py --check`
regenerates the output and compares bytes. `tests/repeat_config.rs`
covers every rule of 3.3.1 and its bounds, the identity, the bands and
their bases, and the section 5 rows on a one-run `captured` band and a
zero band from deterministic replay (the candidate one ulp lower falls
outside it). Acceptance items not yet satisfied: everything beyond the
reference comparison and those two rows; the `wasm32-wasip1` item is
externally blocked.

**Increment 2 (per-case measures).**
`crates/rustev-eval/src/eval_cases.rs` defines `rustev.eval-cases/1`
(`EvalCases`, `REPLAY_V1`, record canonical): the run report's record
digest, the declared top-k parameter, and per case in manifest order its
id, the record digest of the run record it was measured from (none for an
incomparable or diverged case), its detail outcome key, its top-k list and
its measures by metric (`count`, `mean`, `sample`, or `unknown` with the
reason for a case inside a denominator without a finite contribution).
`report::evaluate_with_cases` runs the unchanged spec 004 evaluation and
records, beside each case's outcome, the term the report's own function
adds for it: 0/1 counts for coverage, acceptance coverage, labeled
proposal coverage, error among accepted and candidate outcome change;
`-ln(p[label])` for mean log loss (`metrics::log_loss_term`, which
`mean_log_loss` now adds, so spec 027's corpus keeps one `ln` call site
there and `sum + -ln(p)` is the former `sum - ln(p)` bit for bit); top-label confidence and correctness for the
case's reliability bin in `all` and each of its subgroups
(`metrics::reliability_bin`, which `reliability` now uses); elapsed minus
queued and queued time for each latency and queue quantile, for a
baseline only (a candidate's run records are historical, 3.11.2). It then
reconciles (3.2.6): the document must bind the report, list the detail's
cases and outcome keys in order, hold measures only for reported
case-decomposable metrics, reproduce every such metric bit for bit (both
unknown counts as equal), and sum to the detail's counts; otherwise the
run is refused (`reconcile`). `evaluate` is unchanged and its report and
detail are byte-identical with or without the measures. A case's
`unknown` contribution makes every statistic over it unknown and every
resample drawing it undefined (`repeat_stats::Contribution::Unknown`).

Tests: `tests/reports.rs` (the support baseline and candidate: report and
detail bytes unchanged, reconciliation, the record canonical round trip,
coverage and run binding per case, no latency for a candidate; refusal of
a flipped count, a changed mean, an unknown contribution, a dropped
measure, a measure for an unreported metric or a total, another report,
another case order, another outcome and a dropped case; the lodging top-k
lists), `tests/metrics.rs` (the Brier counterexample below) and
`tests/bootstrap.rs` (unknown contributions are never dropped).

**Increment 3 (repeat sets).** `crates/rustev-eval/src/repeat.rs` adds
`repeat(RepeatInputs)` and `rustev.repeat-report/1` (`RepeatReport`,
`REPLAY_V1`, record canonical). It refuses the `live` source
(`live-source-unavailable`), N outside 1 to 64, a run whose case measures
do not reconcile (`reconcile`), runs that differ in dataset, split,
evaluator configuration, plan or metric list or do not list the split's
cases in manifest order (`mixed-runs`), a `replay` set whose runs are not
identical (`not-reproducible`), a stability metric or override that is not
reported, and a cohort over the bootstrap bound (`bootstrap-too-large`).
The report names the source, repeat configuration, dataset, split and its
fixed role (3.7.1), provenance, evaluator configuration, adapter rules,
plan, whether the runs are candidate evaluations, each run's report,
detail and case-measure digests with its run seed, the stable cohort, and
per cohort (`all`, then each declared slice with its case count, stable
case count and minimum) every metric's per-run values, `mean`, `sd`,
`min`, `max`, `pooled`, interval and band with its basis. A slice with
fewer stable cases than its minimum is `below_minimum` and every figure of
it is unknown (3.6.2, 3.6.3). A metric without a per-case measure (a cost
total, `brier`) has across-run statistics from the run reports in `all`,
`pooled` and interval unknown (`not-case-decomposable`), and is unknown in
slices. Latency, queue and cost metrics state their observation:
`historical` for candidate runs, `candidate` otherwise (3.11.2). The
case measures now also carry `cost_per_1000/observed` and `/estimated`
(3.2.5): 1000 times the case's observed or estimated units as a mean per
comparable case, unknown when the configuration does not declare cost
units comparable, and the observed series unknown for a case with
liability or an attempt of unknown charge; a candidate's cases carry none.
A held-out repeat report records `held_out` as unknown (`no ledger
supplied`) until increment 4 adds the ledger (I-4: never zero).

Tests (`tests/reports.rs`): a three-run replay set (zero
`deterministic_replay` band, pooled equal to the report's figure, distinct
run seeds, the stable cohort, a total and `brier` with across-run
statistics only, latency observed by the plan, unknown `cost_per_1000`
without comparable units, a reported slice and a slice below its minimum,
round trip and byte determinism); a captured set where one case loses its
bundle in the second run (excluded from the stable cohort, a positive
`k_sd` band), one captured run (unknown band and `sd`), differing replay
runs refused, the `live` source refused, N of 0 and 65 refused; mixed
baseline and candidate runs, unreconciled measures and an unreported
stability metric refused; a candidate set with historical latency and
cost and `interval_width` bands equal to the interval's width; with cost
units declared comparable, known cost per 1000 for a baseline and none for
a candidate.

**Increment 4 (held-out ledger, baselines, reproducibility).**
`crates/rustev-eval/src/holdout.rs` defines `rustev.holdout-ledger/1`
(`HoldoutLedger`, `REPLAY_V1`, canonical identity, no binary64): the
dataset and entries of lineage handle (the contract's `Handle`), plan,
repeat configuration, run report digests (1 to 64), host-supplied
`recorded_at_ms` and `prev`, the chain digest
`tagged_digest("rustev.holdout-ledger/1#entry", canonical entry)`.
`verify` refuses another schema or dataset, more than 10 000 entries, an
entry with no run or more than 64, and a broken chain; `append` verifies,
refuses the bound, and returns a new ledger with the counts by lineage,
plan and dataset (each including the new entry) and the heads before and
after; the input is never modified. `repeat::repeat_held_out` counts a
held-out repeat set against the host's ledger or an explicit new one and
records `held_out` as that count (refused for a split that is not held
out, `not-held-out`; a refused ledger is `holdout-ledger`).
`src/baseline.rs` defines `rustev.baseline/1` (`REPLAY_V1`, record
canonical): name, the repeat report's record digest, every run's
document digests and seed, dataset, split, role and provenance,
evaluator configuration id and version, adapter rules, repeat
configuration, plan, source, N, `recorded_at_ms`, the held-out record,
and every cohort's `pooled`, interval and band. `record` refuses an
invalid name and verifies every binding by assembling the repeat set
again from the supplied runs and configurations, which must reproduce the
report exactly (the held-out count, which only the host's ledger states,
is copied, and must be a count). `reproducibility` compares every
stability metric in every baseline cohort: `not_reproducible` with every
exceedance when `|pooled_run - pooled_baseline|` exceeds the baseline's
band, whatever else is unknown; otherwise `unknown` with the reasons when
a value or band is unknown or the configuration, dataset, split,
evaluator configuration, repeat configuration or plan is not the
baseline's; otherwise `reproducible`.

Tests: `tests/holdout.rs` (counts by lineage, plan and dataset; chained
heads; input unchanged; round trip; broken chains, an edited entry,
another dataset, malformed entries, another schema, unknown fields and an
empty lineage refused; a truncated ledger still verifies, as 3.7.5
states; exactly 10 000 entries admitted and one more refused) and
`tests/reports.rs` (held-out counting through `repeat_held_out` with a
new ledger, a supplied one and another lineage, refusals of a broken
chain and another dataset's ledger, unknown without a ledger; baseline
binding, round trip, name rules, a tampered report and tampered run
seeds refused; reproducibility at a zero band, one ulp beyond it, exactly
at a band of 0.125 and one ulp beyond, an unknown band with and without an
exceedance, another repeat configuration; configuration and split
mismatches combined with an exceedance preserve the definite failure,
and a split mismatch without drift remains unknown).

**Increment 5 (diffs).** `crates/rustev-eval/src/diff.rs` defines
`rustev.eval-diff/1` (`EvalDiff`, `REPLAY_V1`, record canonical) and
`diff`, which compares a baseline with a run's repeat report given the
repeat configuration and both sides' `rustev.eval-cases/1` documents.
A dataset, split, evaluator configuration, adapter rules (which must also
be bound, 3.15.3) or repeat configuration mismatch yields `unknown` with
the reason and no table; case documents that do not bind their runs, a
configuration that does not bind the reports, or case order that differs
between sides are refused. Per cohort and metric the diff states both
pooled values, the delta (unknown when either value is, or when it is not
finite), both intervals, `overlap` and `within_noise` as 3.10.2 states;
changed cases follow 3.10.3 with the 10 000 cap, the total and
`truncated`. `render_markdown` renders `diff.md` from the diff document
alone, escaping Markdown and HTML metacharacters, with numbers in the
contract's canonical binary64 spelling. Tests: `tests/diff.rs` (identical
sides, each mismatch reason, overlap and within-noise at band edges and
one ulp beyond, unknown intervals and bands, top-k changes, the cap at
exactly 10 000 and 10 001, round trip and refusals, and byte-stable JSON
and Markdown goldens under `tests/data/diff/`).

**Needs an owner decision (D-029-1): `brier` per-case measures.** 3.2.3
lists `brier` as a mean, and 3.2.6 requires the measures to reproduce the
report's value bit for bit. Spec 004 computes Brier by adding every class
term of every case to one running sum, so the sum of per-case sums differs
in the last bit for ordinary inputs (three binary cases with true-class
probabilities 0.3, 0.2 and 0.6 already differ, `tests/metrics.rs`), and
spec 029 may not change how spec 004
computes a metric (section 4). Until the owner decides, `brier` has no
per-case measure: it is reported from the run reports only, and its
interval is unknown, never approximated. Options: (a) keep it so (Brier
is not case-decomposable); (b) reconcile Brier within a stated tolerance;
(c) amend spec 004 to sum per case first, which changes Brier's bits in
existing reports.

Engineering choices made by the implementing agent, not by the owner:

- A resample combines its drawn cases in draw order (3.4.1 says each draw
  counts once per time it is drawn, not in which order); binary64 sums
  start from zero, and a case's contributions are summed over runs first,
  as for `pooled`.
- Binary64 values are sorted by the IEEE 754 total order, so zeros of
  opposite sign sort one way on every target; a sample quantile uses
  spec 004's nearest-rank formula over binary64 samples.
- The run seed of 3.4.6 numbers runs from 0 in run order (`run/0` is the
  first run).
- A seed is spelled without leading zeros (`0` itself excepted), so one
  `u64` has one spelling and one configuration identity.
- A band equal to its floor keeps the derived basis; `floor` is recorded
  only when the floor is strictly larger.
- An empty cohort has unknown intervals (`empty cohort`) rather than an
  error, as a slice with no case is reported, not refused (3.6.3).
- A case contributes a denominator of at most `u32::MAX` per run (a
  contribution above it is refused), and integer sums are kept in `u128`,
  so no sum can overflow whatever the number of cases. The quotient is
  the two sums converted to binary64 (round to nearest) and divided.
- A top-k list is read from a `ranking` parameter (entries in order) or a
  `shortlist` parameter (ids in order); a proposal whose parameter is
  absent or of another kind has none, as an abstention does.
- Reconciliation compares known values by bit pattern and treats two
  unknown values as equal whatever their reasons, since spec 004 names
  the first failing case and the measures name each one.
- A case's run record digest is the record digest of the run record the
  report measured; for a candidate, that is the historical run whose raw
  outputs the candidate reused.
- The `all` cohort and each slice include every member case of the split,
  each with its per-run denominators; the stable cohort only decides a
  slice's minimum and is listed for 3.12's comparison of stable cohorts.
- A held-out repeat set without a ledger is accepted by the library with
  its count recorded as unknown; the CLI of 3.13.1 still requires the
  ledger flags.
- A baseline is verified by assembling its repeat set again from the
  supplied runs and configurations rather than field by field, so every
  binding of 3.8.1 is checked at once; the held-out count is the one
  binding only the ledger can check.
- `recorded_at_ms` in ledgers and baselines is a signed 64-bit integer of
  milliseconds, as the host supplies it.
- The reproducibility check compares the baseline's cohorts by name; a
  stability metric missing on either side is unknown, never passed.
- `cost_per_1000/*` follow 3.2.5's stricter rule (unknown unless cost
  units are declared comparable), not spec 004's rule that only refuses
  adding units across backends.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #73 leaves the points below open, and each is
decided by the implementer within this spec, as written:

1. **Where the documents live.** In `rustev-eval`, beside
   `rustev.dataset/1` and `rustev.evaluator-config/2`, which live there;
   no contract change (R-06 keeps only the envelope in the contract).
   Repeat declarations are a separate `rustev.repeat-config/1`, not a new
   evaluator configuration version, so existing configuration identities
   and #76 E1's own configuration changes stay independent.
2. **Interval method.** The case-clustered percentile bootstrap of 3.4,
   with xoshiro256\*\* seeded by SplitMix64 implemented in-crate (no
   generator dependency whose stream could change with a version), seeds
   derived by SHA-256, exact integer ranks, `level` in [0.5, 0.999] and
   1 000 to 100 000 resamples. Wilson intervals are not adopted: one method
   for every case-decomposable metric avoids two different intervals for
   the same figure.
3. **What a repeat is.** `replay`, `captured` and `live` (3.1). Repeating
   deterministic replay verifies identity and yields a zero `k_sd` band by
   construction, with the gate consequences of 3.5.3. Live repeats depend
   on spec 026's implementation and #76 E3.
4. **Combining bands.** The larger of the two sides' bands, so a noisier
   candidate cannot narrow the comparison.
5. **`better_than` margin.** Beyond the band: a win inside noise is not a
   win.
6. **Held-out count.** A host-kept, hash-chained ledger file with a
   host-supplied lineage handle, not a bare prior count: a bare integer
   binds nothing an auditor can check, while each ledger entry binds the
   reports it counts. Rustev cannot infer lineage (every candidate change
   changes `PlanId`), so it also counts per plan and per dataset, which a
   relabeled lineage cannot hide. Truncation remains a stated limit.
7. **Split roles.** Fixed by spec 004's split names (3.7.1), not declared
   per dataset, so held-out status cannot be relabeled.
8. **Slices.** The existing dataset subgroups, selected and sized by the
   repeat configuration; no dataset schema change.
9. **Exit codes.** New code 13 for `not_reproducible`, distinct from a
   gate failure; `gate select` is 0 only when every verdict is known and
   one qualifies, 11 when all are rejected, 12 otherwise.
10. **Offline latency and cost.** Historical observations are labeled and
    never count as a candidate's (3.11.2), following principle XIII.
11. **Cost per 1000.** `cost_per_1000/observed` and `/estimated` as
    separate series (spec 004, 3.5.7); never a mixed total.
12. **Binding equivalent.** The library functions; #76 A1 and A2 expose
    them.
13. **Spec 023 question 3.** Answered by 3.4.10.

---
id: "041-ranking-and-rubric-metrics"
title: "Ranking, constraint-match and rubric metrics; pluggable scorers (amends 004, 014 and 029)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved specs 004, 014 and 029 (R-16), for #76 child
  E1. Under a new `rustev.evaluator-config/3`, evaluation adds ranking
  metrics (precision, recall, nDCG, MRR and hit at k over graded gold
  relevance, with unjudged items excluded and counted, and per-position
  backfill rate), declarative constraint-match metrics declared in a new
  `rustev.task-adapter/2` and bound into evaluation identity, origin and
  evidence-quality rates for the constructs of #76 B1, B2, B5, B6, B7 and
  C2 (specs 036, 032, 037, 038, 035 and 039), and rubric metrics from pluggable scorers. Scorers are composed at
  compile time (a Rust trait, or a TypeScript object through the Node
  binding under R-38); their scores travel as `rustev.scores/1` documents
  bound to `rustev.score-views/1`. Model judges are ordinary plans with a
  `rubric` step whose retained bundles eval reproduces; their metrics are
  model-derived, named under `model/` and never pooled with deterministic
  ones, with judge agreement statistics. Gold records arrive in a new
  `rustev.dataset/2`. Every new metric defines its per-case measure, so
  spec 029's repeat statistics, intervals, slices and selection gates
  apply. Existing documents, identities, plans and CLI outputs keep their
  bytes. Approved (A-38); approval authorizes no live run, provider call
  or spend, and no quality claim (R-37).
amends:
  - "004-evaluation-and-replay"
  - "014-report-integrity"
  - "029-evaluation-repeatability"
extends:
  # Configuration version 3, dataset version 2, gold records, ranking,
  # constraint, origin and rubric metrics, scorers, score views, scores,
  # judge reproduction and the gate preconditions (spec 004, 3.5).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `rustev.task-adapter/2`, `eval --scores`, `views.json` and `eval judge`.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  # The declarative `rustev.task-adapter/2` the CLI reads (3.10.1) lives
  # with version 1 in the shared surface.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  # The `Scorer` host seam and `engine.scoreViews` (3.9), joining under
  # spec 027 3.7.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  # The archive policy lists each new `rustev-eval` source and data file.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # `DatasetCase` gains `gold` (3.1.1); the package tests that build
  # dataset cases name it, with no change to either package.
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: additive }
  - { spec: "008-reference-package-lodging", unit: { kind: directory, path: "packages/rustev-pkg-lodging/" }, nature: additive }
  # Adds 041 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "014-report-integrity"
  - "015-per-case-bundle-failures"
  - "023-real-data-quality-qualification"
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "035-screening-seam"
  - "036-mid-plan-acquisition"
  - "037-generation-steps"
  - "038-structured-extraction"
  - "039-model-backed-rank"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Without a version 3 evaluator configuration, every existing evaluation, gate, selection and CLI invocation produces byte-identical documents, stdout and exit codes, and no plan, dataset, configuration or adapter identity changes."
    anchor: "3-13-compatibility"
  - id: "I-2"
    kind: invariant
    text: "An unjudged item, an unknown attribute, an absent gold record, a missing score and an unscored case are excluded from the metric's denominator and counted by reason; none is treated as non-relevant, failing, zero or correct."
    anchor: "3-2-common-rules"
  - id: "I-3"
    kind: invariant
    text: "A metric produced by a model scorer is named under `model/`, reported after every deterministic metric, and never combined with a deterministic metric in any aggregate, pooled value, interval or ranking; a deterministic metric never carries the `model/` prefix."
    anchor: "3-8-model-judges"
  - id: "I-4"
    kind: invariant
    text: "Every new metric's per-case measure reproduces its report value bit for bit by spec 029's reconciliation, and every value is computed with integer arithmetic, IEEE 754 binary64 addition, multiplication and division in the stated order, and the committed discount table, so values are bit-identical on every target."
    anchor: "3-11-per-case-measures-and-spec-029"
  - id: "R-1"
    kind: requirement
    text: "Scores enter evaluation only as a `rustev.scores/1` document bound to the score views of the same evaluation; a views digest, case order or subject mismatch refuses the evaluation."
    anchor: "3-7-scorers-views-and-scores"
  - id: "R-2"
    kind: requirement
    text: "A gate or selection clause over a metric whose scorer is opaque, or whose adapter measures differ between sides, is unknown, never a pass."
    anchor: "3-12-gates-and-selection"
---

# 041: Ranking, constraint-match and rubric metrics; pluggable scorers (amends 004, 014 and 029)

Approved by the owner (A-38, by the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child E1 (#72). The approved texts of
specs 004, 006, 014, 027 and 029 are not edited; this spec records what
changes, as 014 does for 004. Approval authorizes no live run, provider call
or spend, no judge execution against a live backend, and no quality claim:
synthetic fixtures prove mechanics only, and quality, calibration and judge
validity claims need real labeled data admitted under spec 023 (R-04, R-37).

## 1. Purpose

`rustev-eval` reports accuracy-style rates, log loss, Brier score,
reliability, coverage, abstention, latency quantiles and cost. Recommendation
and generation tasks are judged differently: whether the top k satisfy the
request's hard constraints, how many match a requested attribute, how good
the ordering is, how often text fell back, and how good text is according to
a rubric. This spec adds those measurements as declared, identified,
reconcilable metrics, and a scorer seam for rubric scoring in which a model
judge is identified, model-derived and reported apart from deterministic
metrics.

## 2. Territory

When delivered:

- `crates/rustev-eval/` (owner spec 004): `rustev.evaluator-config/3`,
  `rustev.dataset/2` and its gold records, the measure declarations of
  `rustev.task-adapter/2` as a library type, the ranking, constraint, origin,
  rubric and agreement metrics, the `Scorer` trait, `rustev.score-views/1`,
  `rustev.scores/1`, judge reproduction, and the per-case measures spec 029
  consumes. Test data lives under `crates/rustev-eval/tests/data/metrics/`.
- `crates/rustev-cli/` (owner spec 006): the `eval` additions and `eval
  judge` of 3.10.
- `crates/rustev-surface/` (owner spec 027): the declarative
  `rustev.task-adapter/2` the CLI reads (3.10.1), beside version 1.
- `integrations/rustev-node/` (owner spec 027): the `Scorer` host seam of
  3.9. Spec 027 3.7 lets later seams join through their owning specs; the
  extends edge to that directory was added by an authoring edit of this
  spec once spec 027's crate existed.
- `tools/rustev-release/` (owner spec 031): the archive policy entries for
  each new `rustev-eval` source and embedded data file.
- `packages/rustev-pkg-support-routing/` and `packages/rustev-pkg-lodging/`
  (owner specs 007 and 008): their tests build `DatasetCase` values, which
  gain the `gold` member; neither package's behavior changes.
- `Makefile` (owner spec 001): 041 joins `make verify` at completion.

Specs 036, 037, 038 and 039 (B1, B5, B6 and C2) are in `depends_on` for the
`Selection` (3.4.2, 3.4.4) and origin evidence (3.5) this spec reads. Until
the owning spec is implemented in the build, its origin family is refused
(3.5 item 1) and, with no `Selection`, `backfill@p<p>` excludes every case
(`no_origin`, 3.4.4); neither is reported as zero.

No change to `rustev-contract`, `rustev-core`, `rustev-runtime`, any backend,
any plan, definition or descriptor schema, `rustev.eval-report/1`,
`rustev.eval-detail/1`, or any dependency. Eval's normal dependencies stay
contract, core, `serde` and `serde_json` (spec 004, 2).

### 2.1 Binding principles of #76

- **Proposals, never grants.** Metrics, scores, judge verdicts and agreement
  statistics are evaluation evidence. None is accepted by an executor, none
  is a `Permitted` value, and a judge plan's proposal is a score, never an
  authorization.
- **Derivation classes and lineage.** Every metric states the derivation of
  its measurement (3.2.5): `exact-derived` for deterministic metrics and
  scorers, `model-derived` for model judges. A judge's derivation is checked
  against its reproduced judgments (3.8.3). Every score binds the proposal
  view it scored, and every judge score binds the judge run record it came
  from.
- **Unresolved, never a default; no silent kind conversion.** Unjudged items,
  unknown attributes, absent gold, unscored cases and zero denominators are
  excluded and counted, or unknown, never zero, failing or correct (I-2). A
  rank is never read as a probability, an ordinal level never becomes a
  score except through a declared aggregate (3.6.3), and a bounded score is
  never clamped.
- **Pure core; no async runtime in contract and core.** Core and contract are
  untouched. Eval stays synchronous, performs no I/O and reads no clock;
  asynchronous host scorers run in the binding, outside eval (3.9).
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and still passes.
- **No dynamic plugin loading (D-06, R-38).** A scorer is a Rust value the
  host composes at compile time, or a TypeScript object the host passes to
  the Node binding; a judge is a plan file the host names. Nothing is
  discovered, loaded from a path or registry, or resolved by name at run
  time, and the CLI runs no scorer code.
- **No identity, authority or knowledge store.** Gold records, scores and
  judge bundles are files the host keeps and passes in.
- **Plan identities.** This spec adds no plan construct. Every plan and
  definition golden keeps its bytes and `PlanId`.

## 3. Behavior

### 3.1 Documents and versions

1. **`rustev.dataset/2`** (`REPLAY_V1`, content-derived `DatasetId` as
   today) is `rustev.dataset/1` with one optional member per case, `gold`
   (3.3). Every rule of spec 004 3.5.1 applies unchanged.
   `rustev.dataset/1` keeps its schema, bytes and identities; a version 1
   manifest is a version 2 manifest whose every case has no gold.
2. **`rustev.task-adapter/2`** (`DESCRIPTOR_V1`, owner spec 006 3.8) is
   `rustev.task-adapter/1` plus a required member `measures` (3.4). Its
   label check and correctness rules are unchanged. Its rules digest (spec
   014, 3.1.4) is `tagged_digest("rustev.task-adapter/2", canonical bytes)`
   and so covers `measures`.
3. **`rustev.evaluator-config/3`** (`REPLAY_V1`, no binary64) is
   `rustev.evaluator-config/2` with the adapter reference extended to
   `{ name, version, rules, measures }`, where `measures` is the content
   digest of the canonical bytes of the adapter's measure declarations
   (3.4.1), computed by eval, plus the members `scorers`, `rubric_metrics`,
   `human_scales`, `agreements` and `origin` (3.5 to 3.8). Its canonical digest is the
   `EvaluatorConfigId`, so the report identity covers every declaration
   below. Versions 1 and 2 are unchanged and produce no new metric.
4. **`rustev.score-views/1`** and **`rustev.scores/1`** (3.7), under
   `REPLAY_V1` in record canonical form.
5. Unknown fields and duplicate keys are refused in every new document. All
   new identities are `tagged_digest` of canonical bytes under the
   document's own schema name.

### 3.2 Common rules

1. **Scope.** Every new metric is computed over the comparable cases of the
   split (spec 004 3.5.4), in manifest order. Incomparable and diverged
   cases are excluded as today, and every new metric's detail entry counts
   them by the same reasons.
2. **Abstentions.** A case whose outcome is an abstention has no proposal to
   measure. It is excluded from every new metric and counted `abstained`, so
   each metric reads beside acceptance coverage, as error among accepted
   does.
3. **Exclusion and counting (I-2).** Each metric's `rustev.eval-detail/1`
   `Count` entry gives the numerator and denominator spec 029 reconciles
   (3.11) and a `breakdown` of every excluded case or item by reason, using
   the reason names of this section. A zero denominator makes the metric
   `unknown` with its reason, never zero.
4. **Report placement.** Under a version 3 configuration, the report holds
   every metric versions 1 and 2 report, in the same order and bytes, then
   every deterministic metric of 3.4 to 3.8 in declaration order, then
   every `model/` metric (3.8). Spec 004's declared-metric rule applies: each
   reported name's base (the part before the first `/`) is declared in the
   configuration's `metrics` formulas.
5. **Derivation of a measurement.** A metric's derivation describes how it
   was measured, not the decision measured: ranking, constraint, origin and
   agreement metrics between deterministic sides are `exact-derived`; a
   metric from a model scorer, or an agreement involving one, is
   `model-derived`. The configuration records each declared metric's
   derivation; the report names it through the `model/` prefix.
6. **Splits and slices.** A report covers one split, as today. Per-slice
   values come from spec 029's slices (the dataset's subgroups) through the
   per-case measures of 3.11; a single report does not repeat every metric
   per subgroup.

### 3.3 Gold records

1. A case's `gold`, when present, is
   `{ relevance?, items?, requested?, fields?, rubric? }`:
   - `relevance`: item id to integer grade 0 to 10. Grade 0 is a judged
     non-relevant item; an item absent from the map is **unjudged**.
   - `items`: item id to attribute map, attribute name to gold value.
   - `requested`: request attribute name to gold value.
   - `fields`: extraction field name to gold value or the string `"absent"`
     (the field is expected to be absent).
   - `rubric`: human scale name (3.6.4) to level name.
2. A gold value is a JSON string, a JSON integer, a boolean, or an array of
   1 to 16 strings (a hierarchical path, most general first). Item ids,
   attribute, field and scale names are 1 to 256 bytes. At most 1 000
   entries per map. A malformed gold record refuses the manifest
   (`invalid_input`, naming the case).
3. A proposal value matches a gold value by kind only: `Text` or `Enum`
   with a string (byte equality), `Integer` with an integer, `Bool` with a
   boolean, `Decimal` with a string equal to the decimal's canonical text,
   and a `List` of `Text` with a path (element-wise equality). Any other
   pairing is a **type mismatch**: excluded and counted
   (`type_mismatch`), never a match or a mismatch.
4. A case without `gold`, or without the member a metric needs, is excluded
   from that metric and counted `no_gold`. Synthetic provenance stays
   labeled as such; a gold record is a host's label, not verified truth.

### 3.4 Adapter measures: ranking, constraint and field declarations

1. `measures = { rankings: [ranking], constraints: [constraint], fields:
   [field] }`, each list at most 16 entries, names unique within the list,
   1 to 64 bytes of `[A-Za-z0-9._-]`. Its canonical bytes' content digest is
   the configuration's adapter `measures` member; `evaluate` refuses a
   version 3 configuration whose `measures` differs from the digest of the
   supplied adapter's declarations (the adapter-mismatch error of spec 014
   3.2.2, extended). A library adapter supplies its declarations through a
   new trait `MeasuredAdapter: TaskAdapter`, used only for version 3; eval
   computes the digest itself, so a Rust adapter cannot state one digest
   and apply other declarations.
2. **Ranking declaration.**
   `ranking = { name, param, relevant_at_least: integer 1..10,
   gain: "linear" | "exponential", metrics: [metric-spec],
   backfill_positions: integer 0..100 }`, where `metric-spec` is one of
   `precision@k`, `recall@k`, `ndcg@k`, `hit@k`, `mrr` or `mrr@k`, with k 1
   to 100, unique. The ranked list of a proposal is its parameter `param`:
   a `Ranking` (candidates in position order), a `Shortlist` (its ids in
   order) or a `Selection` of spec 039 (`OutValue::Selection`, the output
   of its `select` step: its entries in position order; a shortfall adds no
   entry and shows in `short@k`). Another type, or no such parameter,
   excludes the case
   (`not_a_ranking`); a list naming one id twice excludes it
   (`invalid_ranking`).
3. **Ranking metrics**, reported as `ranking/<name>/<metric-spec>`. For a
   case, `top_k` is the first `min(k, length)` entries of the list. The
   **condensed** top k removes unjudged items (3.3.1), keeps order and
   renumbers positions from 1; each removed item is counted
   (`unjudged_items`, an item count, in the breakdown). `R` is the number of
   gold items with grade at least `relevant_at_least`; an item is relevant
   when its grade is.
   - `precision@k`: relevant items in the condensed top k divided by its
     length `m`. `m = 0` excludes the case (`no_judged_items`).
   - `recall@k`: relevant items in the condensed top k divided by `R`.
   - `hit@k`: 1 when the condensed top k holds a relevant item, else 0.
   - `mrr` and `mrr@k`: `1 / r`, where `r` is the condensed position of the
     first relevant item in the whole list (`mrr`) or in the top k
     (`mrr@k`), and 0 when there is none.
   - `ndcg@k`: `DCG / IDCG`. `DCG` sums `gain(g_i) * D[i]` over the
     condensed top k in position order; `IDCG` sums `gain(g) * D[i]` over
     the `min(k, J)` highest grades of the case's gold `relevance` map in
     descending order (ties need no order: equal grades give equal gains),
     where `J` is the number of entries in that map, whether or not the
     proposal lists them. `gain` is `g` (linear) or `2^g - 1`
     (exponential), exact integers converted to binary64. `D[i]` is entry i
     of the committed discount table: for i 1 to 100, the binary64 value
     nearest to `1 / log2(i + 1)`, computed by the reference script with
     exact decimal arithmetic and committed as bit patterns (3.11.3).
   - `recall@k`, `hit@k`, `mrr`, `mrr@k` and `ndcg@k` exclude a case with
     `R = 0` (`no_relevant`). `R > 0` implies `IDCG > 0`, since
     `relevant_at_least` is at least 1 and both gains are positive above 0.
   - Every metric at k excludes a case whose condensed top k is empty, and
     `mrr` a case whose condensed whole list is empty (`no_judged_items`),
     so a list of only unjudged items never scores 0. When both apply,
     `no_relevant` is the reason counted.
   - A list shorter than k is not padded: missing positions are not items,
     not unjudged and not non-relevant. Each ranking also reports
     `ranking/<name>/short@k` for every k it declares: cases whose list has
     fewer than k entries divided by the cases with a ranking, which leaves
     out the cases item 2 excludes (`not_a_ranking`, `invalid_ranking`),
     each counted.
4. **Per-position backfill rate (#76 C2).** For each position p from 1 to
   `backfill_positions`, `ranking/<name>/backfill@p<p>` is the number of
   cases whose entry at position p has origin `backfill` divided by the
   cases whose list has a position p. It reads the per-position origin
   (`model` or `backfill`) that spec 039 (#76 C2) records in a `Selection`;
   a `Ranking` or `Shortlist` carries no origin, so its cases are excluded
   from this metric (`no_origin`).
5. **Constraint declaration.**
   `constraint = { name, ranking, k: 1..100, attribute, test, against,
   aggregate }`. `ranking` names a declared ranking whose list is
   checked; its relevance handling is not used, so every item in the top k
   is checked, judged or not.
   `attribute` names an item attribute in gold `items`. `against` is
   `{ requested: name }` (a gold `requested` value), `{ param: name }` (a
   proposal parameter, matched by 3.3.3 kinds) or `{ value: gold-value }`.
   `test` is `equals`, `member_of` (the reference is a path whose elements
   are the allowed strings), or `{ path_prefix: level }` (both values are
   paths and their first `level` elements, 1 to 16, are equal: a
   geographic or hierarchical match at the requested granularity; a path
   shorter than `level` does not match).
6. **Constraint metrics**, reported as `constraint/<name>`. An item whose
   attribute is not in gold `items`, or a reference that is absent, is
   **unknown**: excluded and counted (`unknown_attribute`, `no_reference`).
   An item whose attribute and reference are not kinds the test accepts is
   unknown the same way (`type_mismatch`): for `equals`, a `param`
   reference must pair with the attribute by 3.3.3 and any other reference
   must be the same gold value kind; `member_of` needs a string attribute
   and a path reference; `path_prefix` needs two paths.
   - `aggregate: "share_at_k"`: per case, satisfying items divided by
     checked items in the top k; a case with no checked item is excluded
     (`no_checked_items`); the metric is the mean over cases.
   - `aggregate: "all_at_k"`: per case 1 when every item in the top k is
     checked and satisfies, 0 when any checked item fails; when none fails
     but one is unknown, the case is excluded once, under the first reason
     its unknown items carry in the order `no_reference`, `type_mismatch`,
     `unknown_attribute`. An empty
     list is excluded (`empty_ranking`), never vacuously 1. A constraint the
     plan protects by construction is expected to read 1.0; it is reported
     so a regression shows.
7. **Field declaration** (extraction, #76 B6).
   `field = { name, step, field }` names an extract step and its output
   field; gold is `fields[name]`.

### 3.5 Origin and evidence-quality metrics

1. The configuration's `origin` member lists families to report:
   `text_backstop` (spec 037, #76 B5), `extraction` (spec 038, B6),
   `screening` (spec 035, B7), `corroboration` (spec 032, B2),
   `acquisition_tier` (spec 036, B1). Each reads only evidence that the
   owning spec puts into judgments and run records, with that spec's reason
   vocabulary. Under the #76 coordination rule that evidence lives in the
   shared track versions (`rustev.evidence/2`, `rustev.run/3`,
   `rustev.replay/2`, introduced by the lowest-numbered merged spec and
   added to by the others); this spec reads those versions and adds no
   member to them, and it introduces no contract document version. A family whose owning spec is not
   implemented in the build is refused at configuration check
   (`invalid_input`, `metric-unavailable`, naming it), as spec 029 refuses
   an unavailable source; it is never reported as zero.
2. Metrics, each a count ratio over the denominator named, and each named
   `origin/<family>/...` with the family token of item 1:
   - `origin/text_backstop/backstop_rate` and
     `origin/text_backstop/backstop_rate/<reason>`:
     generated fields with origin backstop (with that reason) divided by
     generated fields of proposals with a generation step;
     `origin/text_backstop/partial_rate` and
     `origin/text_backstop/full_backstop_rate`:
     proposals whose aggregate text origin is partial (backstop) divided by
     proposals with a generation step.
   - `origin/extraction/<name>/accuracy`: present values of the declared
     field matching gold (3.3.3) divided by present values whose gold is a
     value; a present value whose gold is `"absent"` counts as wrong. Gold
     absent is `no_gold`. `origin/extraction/<name>/absence_rate`: absent
     values divided by step results for the field. The detail breaks both
     down by the field's origin (`extracted`, `fallback`).
   - `origin/screening/<screener>/block_rate` and `/unavailable_rate`:
     blocked (unavailable) screened fields divided by screened fields.
   - `origin/corroboration/hint_disagreement_rate`: corroboration outcomes
     `contradicted` divided by outcomes `corroborated` or `contradicted`.
   - `origin/acquisition_tier/<step>/tier/<n>`: decisions whose widening step
     stopped at tier n divided by decisions that ran the step, and
     `origin/acquisition_tier/<step>/exhausted` for those that ran out of
     tiers.
3. A plan without the construct has zero denominators: every such metric is
   `unknown` (`not_in_plan`).

### 3.6 Scorers

```rust
pub trait Scorer: Send + Sync {
    fn descriptor(&self) -> &ScorerDescriptor;
    fn score(&self, case: &CaseView, proposal: &ProposalView) -> ScoreOutcome;
}
```

1. A scorer is the trait above. `ScoreOutcome` is `Score(value)` or `Unscored { reason }`, where a reason
   is 1 to 64 bytes of `[a-z0-9_]` and the reasons of 3.7 and 3.8 are
   reserved for eval.
2. `ScorerDescriptor = { id, version, kind: "deterministic" | "model",
   output, rules, reads_gold }` (3.7.4): `id` 1 to 64 bytes of `[A-Za-z0-9._-]`, `version` 1 to
   64 bytes; `output` is `{ ordinal: { levels: [name] } }` (2 to 16 unique
   level names, lowest first; the value is the level index) or
   `{ bounded: { min: decimal, max: decimal } }` (min below max; the value is
   a finite binary64 in `[min, max]`); `rules` is `{ bound: digest }` (the
   content digest of a canonical document that fully determines the
   scoring, as spec 014 3.1.2 states for adapters) or `"opaque"`. The
   configuration's `scorers` lists every descriptor (at most 16, unique
   ids), so scorer identity enters the `EvaluatorConfigId`. A model scorer
   also carries `judge` (3.8.1).
3. **Rubric metrics.** `rubric_metrics = [ { name, scorer, aggregate } ]`,
   reported as `rubric/<name>` for a deterministic scorer and
   `model/rubric/<name>` for a model scorer. `aggregate` is
   `mean_level_index` (ordinal: the mean level index, a declared reading of
   an ordinal scale as numbers), `share_at_least: level` (ordinal: scored
   cases at or above the level divided by scored cases), or `mean`
   (bounded). An aggregate that does not fit the output kind, or a
   `share_at_least` level that is not one of the scorer's declared levels,
   refuses the configuration (`invalid_input`, `scale-mismatch`). Unscored cases are
   excluded and counted by reason (`unscored:<reason>`).
4. **Human labels.** The configuration may declare `human_scales: [{ name,
   levels }]`; gold `rubric` levels on such a scale are human ratings, used
   only in agreement statistics (3.8.4). A level not on the scale refuses
   the manifest at evaluation (`invalid_input`, naming the case).
5. A deterministic scorer must return the same outcome for the same views.
   Rustev cannot detect a scorer that hides a model or randomness behind
   `deterministic`; spec 029's `replay` repeats re-score each run, and a
   difference refuses the set (`not-reproducible`), which is the evidence
   Rustev can give.

### 3.7 Scorers, views and scores

1. **Views.** For a version 3 configuration with scorers, eval produces
   `rustev.score-views/1`: `{ schema, dataset, split, config, plan, cases }`,
   one entry per comparable proposal case in manifest order, each
   `{ case, case_view, proposal_view, subject }`. `case_view` is
   `{ id, label, gold, subgroups }`; `proposal_view` is `{ action, params,
   derivation, notices }` from the judgment; `subject` is
   `tagged_digest("rustev.proposal-view/1", canonical bytes of
   proposal_view)`. Abstentions and incomparable cases have no entry.
   Views are a pure function of the evaluation's inputs, computed by one
   library function that `eval views`, `eval` and `evaluate` all call, so
   the same inputs give byte-identical views documents and the same record
   digest; 3.7.5 relies on this.
2. **Scores.** `rustev.scores/1` is `{ schema, scorer, views, entries }`:
   the scorer's full descriptor, the views document's record digest, and one
   entry per view entry in the same order, `{ case, subject, outcome,
   judge_run }`, where `outcome` is `{ score: number }` or `{ unscored:
   reason }` and `judge_run` is the judge's run record digest for a model
   scorer and null otherwise.
3. **Scoring in Rust.** `score_views(&dyn Scorer, &ScoreViews) -> Scores`
   calls the scorer once per entry in order, synchronously. The host
   composes its scorers in code (D-06).
4. **Gold access is declared.** A descriptor states `reads_gold: bool`,
   which enters the scorer's identity. A reference-based scorer (one that
   compares text with a reference answer) declares `true`; a scorer that
   judges a proposal on its own declares `false`, and `score_views` and the
   Node binding (3.9) then hand it a case view whose `gold` is null, so it
   cannot score from the labels. A judge's input is built by the host
   (3.8.2), so for a judge `reads_gold` is the host's declaration, which
   Rustev records but cannot enforce.
5. **Evaluation with scores.** `evaluate` under a version 3 configuration
   takes the scores documents. It refuses (`EvalError::Scores`, CLI
   `invalid_input`): a scores document whose descriptor is not declared
   (`scorer-undeclared`) or whose views digest, case list, order or any
   subject differs from the views eval computes for this evaluation
   (`scores-mismatch`), or two documents for one scorer. A declared scorer
   with no document makes each of its metrics `unknown`
   (`scores-missing`). A score outside its output (a level index out of
   range, a non-finite or out-of-bounds value) is not clamped: that case is
   unscored with reason `invalid_score` and counted.

### 3.8 Model judges

1. A model scorer is a **judge**: an ordinary Rustev plan whose definition
   has a semantic step of operation `rubric` (spec 002 3.9, R-05; no new
   step kind) served by a backend with its own artifact identity,
   and whose proposal carries a parameter of enum type whose options are
   exactly the scorer's levels. The descriptor's `judge` member is
   `{ plan: PlanId, artifacts: [ArtifactId], level_param, subject_field }`,
   so the judge plan and its backend identities enter the configuration
   identity. Only ordinal output is allowed for a judge: a model descriptor
   with `bounded` output refuses the configuration (`invalid_input`,
   `judge-output-kind`).
2. **Running a judge** is the host's: it executes the judge plan with the
   runtime (or a binding) once per view entry, supplying the view's
   `subject` as the input field `subject_field`, with capture, and keeps the
   bundles (spec 004 3.1). This spec adds no live path; judge runs are
   ordinary decisions under their own budgets and evidence.
3. **Judge reproduction.** `judge_scores(views, judge plan, bundles,
   resolver)` reproduces each case's judge bundle offline with spec 004's
   reproduction and writes `rustev.scores/1`. Per case: no bundle is
   `judge_bundle_missing`; a bundle of another plan refuses the whole call
   (`judge-plan-mismatch`); a bundle that does not reproduce is
   `judge_not_reproduced`; a supplied `subject_field` value other than the
   view's `subject` is `subject_mismatch`; an escalation or unresolved
   judgment is `judge_unresolved`; otherwise the score is the index of the
   `level_param` option. A reproduced judgment whose derivation is
   `exact-derived` while the descriptor says `model`, or the reverse,
   refuses the call (`judge-derivation-mismatch`). These two refuse the call
   rather than a case on purpose: either shows that the judge, or its
   descriptor, is not the one the configuration declares, which taints
   every score in the document; I-2 governs missing or unusable evidence,
   not a mis-declared judge.
4. **Agreement.** `agreements = [ { name, a, b } ]`, where each side is a
   declared ordinal scorer id or `human:<scale>`, both on identical level
   lists (otherwise `scale-mismatch`). Over cases scored on both sides, with
   `d` the level difference: `agreement/<name>/exact` (`d = 0`, count
   ratio), `/adjacent` (`|d| <= 1`), `/mean_abs_diff` (mean of `|d|`) and
   `/qwk` (quadratic weighted kappa: `1 - sum(w * O) / sum(w * E)` with
   `w = (i - j)^2`, `O` the matrix of observed pair counts and `E` the
   outer product of the two marginal count vectors divided by the case
   count, so both are counts over the same cases; sums in row-major order; unknown
   when `sum(w * E) = 0`). When either side is a model scorer every name
   carries the `model/` prefix. Agreement between two judges, or a judge
   and human labels, on synthetic data says nothing about judge validity.
5. **Separation (I-3).** A `model/` metric is never an input to a
   deterministic metric, never pooled or averaged with one, and spec 029
   pools it only with values of the same metric name (029 3.3.3). No
   configuration member can declare an aggregate over both kinds; one that
   names a model scorer under a name without `model/`, or a deterministic
   one under `model/`, is refused (`invalid_input`, `model-prefix`).

### 3.9 Host scorers through the Node binding

1. The Node binding (spec 027) adds `Scorer` to its host-implemented seams
   (027 3.7's later joiners). A TypeScript scorer is an object with
   `descriptor` (declared data, 027 3.6.1) and an asynchronous
   `score(caseView, proposalView)`. The binding exposes
   `engine.scoreViews(views, scorer)`, which calls it per entry under every
   rule of 027 3.6: bounded queue with the host's declared capacity, the
   remaining deadline and an `AbortSignal` per call, late resolutions
   discarded and counted. A full queue, an exception or rejection, a late
   or cancelled call is `Unscored` with reason `scorer_failed`,
   `scorer_overloaded` or `scorer_deadline`; it is never a score. Rust never
   waits synchronously on JavaScript.
2. The result is the same `rustev.scores/1` document `score_views`
   produces, which the host passes to `engine.evaluate`. A TypeScript scorer
   and an equivalent Rust scorer therefore yield byte-identical scores and
   reports (acceptance).
3. This is explicit composition under R-38. The WebAssembly build exposes
   views, judge reproduction and evaluation; a host scorer seam there is a
   later amendment of 027.

### 3.10 CLI surface (adds to spec 006)

1. The CLI reads `rustev.task-adapter/2` beside version 1 and
   `rustev.dataset/2` beside version 1. Its declarative adapter states
   `bound` rules over the version 2 document (3.1.2).
2. **`rustev eval`** with a version 3 configuration gains `--scores <file>`
   (repeatable, at most 16). `--out` stays the directory of spec 006
   (`report.json`, `detail.json`, `config.json`, `adapter.json`) and
   additionally receives `views.json` when any scorer is declared, all or
   none. Stdout prints every metric as
   the envelope states it, as today. `--scores` with a version 1 or 2
   configuration is a usage error (exit 2) before any file is read.
3. **`rustev eval views`**: the inputs of `eval`, except that `--out`
   names a file, not a directory; writes only `rustev.score-views/1` to
   that file and prints `viewed` with its
   record digest. Scorers run outside the CLI.
4. **`rustev eval judge`**: `--config <file>` (a version 3 evaluator
   configuration), `--views <file>`, `--judge-plan <file>` with
   `--descriptor`, `--rules` and `--calibration` describing the judge plan,
   `--bundles <dir>` (`<dir>/<case id>.json`), the scope flags, `--now-ms`,
   `--scorer <id>` (a scorer of the configuration given by `--config`), and
   `--out <file>` (a file, as for `eval views`). Writes `rustev.scores/1`
   to it; prints `scored` with its
   digest and the count per outcome.
5. Exit codes: no new code. Every refusal of this spec is `invalid_input`
   (exit 4) naming its code; spec 006 3.2 and spec 029 3.14 are unchanged.

### 3.11 Per-case measures and spec 029

1. Every new metric defines its spec 029 3.2.2 measure, so repeat
   statistics, `pooled` values, bootstrap intervals, noise bands, slices,
   diffs, comparisons and selection gates apply to it unchanged:
   - **count ratio** (integer numerator and denominator per case):
     `hit@k`, `short@k`, `backfill@p<p>`, `all_at_k` constraints, every
     origin rate (a case contributes its own counts of fields, outcomes or
     decisions), `share_at_least` rubric metrics, `exact` and `adjacent`
     agreement;
   - **mean** (binary64 numerator, denominator 1 per measured case):
     `precision@k`, `recall@k`, `ndcg@k`, `mrr`, `mrr@k`, `share_at_k`
     constraints, `mean_level_index` and `mean` rubric metrics,
     `mean_abs_diff`;
   - **not case-decomposable**: `qwk`, reported with across-run statistics
     only and an unknown interval (`not-case-decomposable`, 029 3.2.3).
2. Report values are computed by the same functions over the measures in
   manifest order (integers exactly, binary64 left to right, one division
   last), so spec 029's reconciliation (3.2.6) holds bit for bit, and
   `rustev.eval-cases/1` carries these measures.
3. **Determinism (I-4).** No new value uses a transcendental function at run
   time: `D[i]` comes from the committed table
   `crates/rustev-eval/tests/data/metrics/discounts.json` (embedded in the
   crate), generated by `discounts.py` (Python 3 standard library `decimal`,
   60 digits, round half to even to binary64), whose `--check` mode
   regenerates and compares bytes. Ranking per-case values compute `DCG` and
   `IDCG` left to right, then one division.
4. 029's `top_k` repeat declaration and this spec's rankings are
   independent: one records lists for diffs, the other measures them.

### 3.12 Gates and selection

1. **Spec 004 3.5.8 gates**, under version 3 configurations, add two
   preconditions, each making the gate `unknown` with a reason naming the
   side: the adapter `measures` digests differ (already implied by unequal
   configuration identities, and named explicitly), or the gated metric is
   produced by a scorer whose `rules` is `opaque` (or an agreement with
   such a side). This follows spec 014 3.3: an opaque scorer's identity
   names code whose change no digest shows.
2. **Spec 029 selection gates.** A clause (029 3.12.3) whose metric is
   produced by an opaque scorer is unknown, as 029 3.12.4 treats an unknown
   value; `rank_by` over such a metric lists the candidate as `unranked`.
   Directions are declared by the gate as 029 requires; this spec declares
   none. Every other 029 rule applies unchanged, including that unknown
   never becomes qualifies.

### 3.13 Compatibility

1. Without a version 3 configuration: `evaluate`, `evaluate_gate`, 029's
   functions and the CLI read the same inputs and write the same files,
   stdout bytes and exit codes as today. `rustev.eval-report/1`,
   `rustev.eval-detail/1`, `rustev.dataset/1`, `rustev.task-adapter/1`,
   `rustev.evaluator-config/1` and `/2`, and every 029 document keep their
   schemas, bytes and identities. Existing eval, CLI and 029 goldens stay
   byte-identical.
2. A version 3 report's envelope and detail use the existing schemas; new
   metrics are additional entries only, after the existing ones (3.2.4).
3. Spec 004 5.6's record-canonical list gains `rustev.score-views/1` and
   `rustev.scores/1`; `rustev.evaluator-config/3`, `rustev.dataset/2` and
   `rustev.task-adapter/2` are identity-bearing and hold no binary64.
4. Plans and definitions are untouched: every plan golden keeps its bytes
   and `PlanId`, proved by the existing plan goldens. A judge plan is an
   ordinary plan with an ordinary identity.

## 4. Out of scope

Live judge execution or any live dispatch (spec 026, #76 E3); provider
calls or spend; a built-in judge, rubric text or prompt; scorer code loaded
by the CLI; training or fitting a judge; quality, calibration or judge
validity claims from synthetic data (R-04, R-37); per-subgroup metric copies
in single reports (029 slices cover them); defining the evidence of B1, B2,
B5, B6, B7 or C2 (their specs do); significance tests between judges.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A ranking whose top 5 holds 2 unjudged items, 1 relevant of 3 judged | `precision@5` contribution 1/3; `unjudged_items` counts 2. |
| A ranking whose top 5 holds only unjudged items, `R = 2` | Excluded from every `@5` metric, `no_judged_items`; never 0. |
| A case with no gold, or no `relevance` | Excluded from every ranking metric, counted `no_gold`. |
| `R = 0` for a case | Excluded from recall, hit, MRR and nDCG (`no_relevant`); precision still measured. |
| Every case excluded | Metric `unknown` with its reasons, never 0. |
| A list of 3 under `precision@5` | Measured over the 3; counted in `short@5`. |
| A ranking naming one id twice | Case excluded, `invalid_ranking`. |
| Parameter is a `Filtered` value | Case excluded, `not_a_ranking`. |
| `all_at_k` with one unknown attribute and no failure | Case excluded, `unknown_attribute`. |
| `all_at_k` whose reference is absent, no item failing | Case excluded, `no_reference`. |
| `all_at_k` with one unknown attribute and one failure | Case contributes 0. |
| `all_at_k` over an empty list | Case excluded, `empty_ranking`. |
| `path_prefix: 2` against a one-element path | No match. |
| Extraction accuracy: an `Integer` field value against a string gold value | Excluded, `type_mismatch`. |
| Constraint `equals` with `against: { param }` an `Integer` and a string gold attribute | Item unknown, `type_mismatch`. |
| `origin: ["text_backstop"]` before spec 037 (#76 B5) is implemented | `invalid_input`, `metric-unavailable`. |
| A version 3 configuration whose `measures` digest differs from the adapter's | Adapter-mismatch refusal before any case. |
| A scores document over another evaluation's views | `invalid_input`, `scores-mismatch`. |
| A declared scorer with no scores document | Its metrics `unknown`, `scores-missing`. |
| A level index of 7 on a 5-level scale | Case unscored `invalid_score`; not clamped. |
| A judge bundle missing for a case | Case unscored `judge_bundle_missing`. |
| A judge bundle whose subject field differs from the view | Case unscored `subject_mismatch`. |
| A model descriptor with `bounded` output | `invalid_input`, `judge-output-kind`. |
| A deterministic scorer declaring `reads_gold: false` | Its case views carry `gold: null`. |
| `eval judge` without `--config` | Usage error, exit 2. |
| An `exact-derived` judgment under a model descriptor | `invalid_input`, `judge-derivation-mismatch`. |
| One judge bundle of a plan other than `--judge-plan` | `invalid_input`, `judge-plan-mismatch`; no scores document is written. |
| A model scorer's metric named without `model/` | `invalid_input`, `model-prefix`. |
| `mean_level_index` over a bounded scorer | `invalid_input`, `scale-mismatch`. |
| `share_at_least` naming a level the scorer does not declare | `invalid_input`, `scale-mismatch`. |
| Agreement with all cases on one level on both sides | `qwk` `unknown`; `exact` 1. |
| A TypeScript scorer that throws | Case unscored `scorer_failed`; no panic. |
| A TypeScript scorer whose queue is full | Case unscored `scorer_overloaded`. |
| A gate over a metric from an opaque scorer | Gate `unknown`; selection clause unknown, exit 12. |
| `--scores` with a version 2 configuration | Usage error, exit 2, nothing read. |
| The existing `rustev eval` and `rustev gate` on today's inputs | Byte-identical files, stdout and exit codes. |

## Acceptance

- Unit tests for each metric of 3.4 to 3.8 against hand-computed fixtures
  committed under `crates/rustev-eval/tests/data/metrics/`, each with its
  arithmetic written out, including empty, unjudged, unlabeled, `R = 0`,
  short-list, type-mismatch and unknown-attribute cases.
- `discounts.py --check` reproduces `discounts.json` byte for byte, and a
  Rust test checks the embedded table against it.
- A TypeScript scorer through the Node binding (027) produces a
  `rustev.scores/1` document and a report byte-identical to an equivalent
  Rust scorer's, including unscored cases; throwing, late and queue-full
  scorers map as 3.9.1 states. This item depends on spec 027's
  implementation; until then it is reported as externally blocked, never as
  passed.
- A model judge over captured bundles of a judge plan served by a scripted
  stub backend that declares model derivation is reported only under
  `model/`, after every deterministic metric, and changing its scores
  changes no deterministic metric's bytes.
- Judge agreement golden: judge versus human labels, two judges, and the
  `qwk` unknown case.
- Spec 029 integration: a replay repeat set over a version 3 configuration
  reconciles every new metric bit for bit, produces bootstrap intervals for
  the case-decomposable ones and `not-case-decomposable` for `qwk`, and a
  selection gate over `ranking/<name>/ndcg@10` and a `model/rubric/<name>`
  metric qualifies, rejects and is unknown (opaque scorer) as 3.12 states.
- Origin families: each family's metrics are tested once its owning spec's
  evidence is implemented; before that, the `metric-unavailable` refusal is
  tested. An unimplemented family is reported as pending, never as passed.
- Every row of section 5 has a test.
- Compatibility: every existing eval, CLI and 029 golden, and every plan and
  definition golden with its `PlanId`, is byte-identical.
- Seeds in the eval mutation harness (an unjudged item counted as
  non-relevant, an off-by-one discount index, a short list padded with
  non-relevant items, an unknown attribute counted as failing, an invalid
  score clamped, a model metric pooled into a deterministic one, the
  measures digest left out of the configuration identity, an opaque scorer
  passing a gate) are each detected.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item, and no
existing evaluation computes any metric of this spec.

## Implementation record

Planned increments, each a separately reviewed pull request of about
1,000 lines: (1) gold values and their kind matching (3.3.3), the
committed discount table with its generator (3.11.3), and the per-case
ranking measures of 3.4.3 and 3.4.4 as a pure module, not yet wired into
`evaluate`; (2) the documents: `rustev.dataset/2` with gold records,
the measure declarations of `rustev.task-adapter/2` and
`MeasuredAdapter`, and `rustev.evaluator-config/3` with its checks and
identity; (3) evaluation under version 3: ranking, `short@k`,
`backfill@p<p>` and constraint metrics with their detail counts, report
placement and the per-case measures spec 029 reconciles; (4) scorers,
`rustev.score-views/1`, `rustev.scores/1`, `score_views` and rubric
metrics; (5) model judges, judge reproduction, agreement statistics and
the `model/` separation; (6) origin families, each once its owning
spec's evidence is on `main`, and the `metric-unavailable` refusal
before that; (7) gate and selection preconditions (3.12) and the CLI
surface (3.10); (8) the Node `Scorer` seam (3.9); (9) mutation seeds,
the spec 029 integration test, declared verification and closure.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child faithfully
and completely; #72 leaves the points below open, and each is decided within
the binding principles, as written:

1. **Where gold lives.** A new `rustev.dataset/2` with per-case gold, so the
   `DatasetId` each report names covers the labels its metrics used;
   `rustev.dataset/1` is unchanged. A separate gold file would leave the
   report's identity blind to it.
2. **Where measures are declared.** Ranking, constraint and field
   declarations are task semantics and live in `rustev.task-adapter/2`, as
   #72 says for constraints; their digest is bound into
   `rustev.evaluator-config/3` and computed by eval, so a library adapter is
   bound too (amending 014's identity rule). Scorers, rubric metrics,
   agreements and origin families live in the configuration.
3. **Unlabeled items.** Condensed lists: unjudged items are removed and
   counted, never read as non-relevant; IDCG uses gold grades at cutoff k.
   A short list is not padded and is reported by `short@k`.
4. **Macro averaging.** Ranking, `share_at_k` and mean rubric metrics are
   per-case means, so each case weighs the same and each has a spec 029
   measure.
5. **Gain and discount.** Linear or exponential gain over integer grades 0
   to 10, declared; discounts from a committed, exactly generated table,
   for cross-target bit identity.
6. **How scores enter.** As `rustev.scores/1` bound to the evaluation's
   views, so eval stays synchronous and I/O-free, an asynchronous
   TypeScript scorer needs no blocking call, and Rust and TypeScript
   scorers are compared byte for byte.
7. **Model judges.** Ordinary plans with a `rubric` step whose retained
   bundles eval reproduces; the judge's backend identity, plan identity and
   derivation are bound and checked. Judge execution is the host's; no live
   path is added.
8. **The separate section.** Without changing `rustev.eval-report/1`: a
   reserved `model/` prefix, placement after every deterministic metric,
   and a configuration refusal for any mixing (I-3).
9. **Opaque scorers.** Allowed and reported, but gates and selection
   clauses over their metrics are unknown, following spec 014's answer for
   opaque adapters.
10. **Origin metrics before their constructs.** Declared families are
    refused until the owning spec is implemented, never reported as zero;
    their vocabularies are the owning specs'.
11. **Slices.** Spec 029's slices over per-case measures; single reports are
    not repeated per subgroup.
12. **Exit codes.** None added; refusals are `invalid_input`.
13. **Gold access.** Declared per scorer (`reads_gold`) and enforced for
    in-process and Node scorers by withholding gold, so a scorer meant to
    judge a proposal on its own cannot read the labels; reference-based
    scorers declare it.
14. **Shared versions.** Under the #76 coordination rule, origin evidence
    is read from the shared track versions; this spec's own new documents
    are eval-owned (`rustev.dataset/2`, `rustev.task-adapter/2`,
    `rustev.evaluator-config/3`, `rustev.score-views/1`,
    `rustev.scores/1`), which no sibling spec claims. Refusals are named
    codes under `invalid_input`; no category number or exit code is added.
15. **Amending 029.** Only by the opaque-scorer rule of 3.12.2; every new
    metric enters 029 through the measure hook 029 3.2.3 already provides.

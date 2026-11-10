<!-- spec-spine:begin generated id="header" -->
# `041-ranking-and-rubric-metrics` Ranking, constraint-match and rubric metrics; pluggable scorers (amends 004, 014 and 029)

- Path: `specs/041-ranking-and-rubric-metrics/spec.md`
- Summary: An amendment of approved specs 004, 014 and 029 (R-16), for #76 child E1. Under a new `rustev.evaluator-config/3`, evaluation adds ranking metrics (precision, recall, nDCG, MRR and hit at k over graded gold relevance, with unjudged items excluded and counted, and per-position backfill rate), declarative constraint-match metrics declared in a new `rustev.task-adapter/2` and bound into evaluation identity, origin and evidence-quality rates for the constructs of #76 B1, B2, B5, B6, B7 and C2 (specs 036, 032, 037, 038, 035 and 039), and rubric metrics from pluggable scorers. Scorers are composed at compile time (a Rust trait, or a TypeScript object through the Node binding under R-38); their scores travel as `rustev.scores/1` documents bound to `rustev.score-views/1`. Model judges are ordinary plans with a `rubric` step whose retained bundles eval reproduces; their metrics are model-derived, named under `model/` and never pooled with deterministic ones, with judge agreement statistics. Gold records arrive in a new `rustev.dataset/2`. Every new metric defines its per-case measure, so spec 029's repeat statistics, intervals, slices and selection gates apply. Existing documents, identities, plans and CLI outputs keep their bytes. Approved (A-38); implementation is complete. Approval authorizes no live run, provider call or spend, and no quality claim (R-37).
- Reference page schema: `1.0.0`
<!-- spec-spine:end generated id="header" -->

<!-- spec-spine:begin generated id="lifecycle" -->
## Lifecycle

| Field | Value |
| --- | --- |
| status | `approved` |
| implementation | `complete` |
| superseded_by | None declared. |
| retirement_rationale | None declared. |
| created | `2026-10-17` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `041-ranking-and-rubric-metrics#I-1` | `invariant` | Without a version 3 evaluator configuration, every existing evaluation, gate, selection and CLI invocation produces byte-identical documents, stdout and exit codes, and no plan, dataset, configuration or adapter identity changes. | `3-13-compatibility` | no |
| `041-ranking-and-rubric-metrics#I-2` | `invariant` | An unjudged item, an unknown attribute, an absent gold record, a missing score and an unscored case are excluded from the metric's denominator and counted by reason; none is treated as non-relevant, failing, zero or correct. | `3-2-common-rules` | no |
| `041-ranking-and-rubric-metrics#I-3` | `invariant` | A metric produced by a model scorer is named under `model/`, reported after every deterministic metric, and never combined with a deterministic metric in any aggregate, pooled value, interval or ranking; a deterministic metric never carries the `model/` prefix. | `3-8-model-judges` | no |
| `041-ranking-and-rubric-metrics#I-4` | `invariant` | Every new metric's per-case measure reproduces its report value bit for bit by spec 029's reconciliation, and every value is computed with integer arithmetic, IEEE 754 binary64 addition, multiplication and division in the stated order, and the committed discount table, so values are bit-identical on every target. | `3-11-per-case-measures-and-spec-029` | no |
| `041-ranking-and-rubric-metrics#R-1` | `requirement` | Scores enter evaluation only as a `rustev.scores/1` document bound to the score views of the same evaluation; a views digest, case order or subject mismatch refuses the evaluation. | `3-7-scorers-views-and-scores` | no |
| `041-ranking-and-rubric-metrics#R-2` | `requirement` | A gate or selection clause over a metric whose scorer is opaque, or whose adapter measures differ between sides, is unknown, never a pass. | `3-12-gates-and-selection` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
- `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}`
<!-- spec-spine:end generated id="ownership" -->

<!-- spec-spine:begin generated id="signatures" -->
## Signatures

None declared.
<!-- spec-spine:end generated id="signatures" -->

<!-- spec-spine:begin generated id="relations" -->
## Relations

Not supplied.
<!-- spec-spine:end generated id="relations" -->

<!-- spec-spine:begin generated id="operations" -->
## Operations

Not supplied.
<!-- spec-spine:end generated id="operations" -->

<!-- spec-spine:begin generated id="evidence" -->
## Evidence

### Declared inputs

None declared.

### Acceptance commands (declared, not executed)

```console
cargo test -p rustev-eval --locked --test ranking_metrics
python3 -I crates/rustev-eval/tests/data/metrics/discounts.py --check
cargo test -p rustev-eval --locked --test documents_041 --test config_041
cargo test -p rustev-eval --locked --test measured_041 --test agreement_041 --test origin_041
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo test -p rustev-surface --locked
cargo test -p rustev-core --locked --test reference_plans --test identity
cargo test -p rustev-pkg-support-routing -p rustev-pkg-lodging --locked
sh crates/rustev-eval/mutation/seeds.sh "spec 041"
```
<!-- spec-spine:end generated id="evidence" -->

<!-- spec-spine:begin generated id="limitations" -->
## Limitations

None declared.

This page restates declared and structurally resolved records. It does not state that code is correct, that a test ran or passed, or that anything was accepted, released, adopted, or observed.
<!-- spec-spine:end generated id="limitations" -->

<!-- spec-spine:begin generated id="changes" -->
## Changes

| Edge | Specs |
| --- | --- |
| amends | `004-evaluation-and-replay`, `014-report-integrity`, `029-evaluation-repeatability` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `061-host-scored-repeat-sets` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:041-ranking-and-rubric-metrics` | `sha256:bfa5387295a24043f95d15195535573ecb793df9c518283e960e601310ab0e5b` |
| `registry:041-ranking-and-rubric-metrics` | `sha256:bfa5387295a24043f95d15195535573ecb793df9c518283e960e601310ab0e5b` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

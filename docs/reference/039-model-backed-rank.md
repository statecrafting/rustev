<!-- spec-spine:begin generated id="header" -->
# `039-model-backed-rank` Model-backed rank and deterministic backfill (amends 002 and 005)

- Path: `specs/039-model-backed-rank/spec.md`
- Summary: An amendment of approved specs 002 and 005 (R-16), for #76 child C2. Adds a `rank` step kind with three declared modes (listwise, pointwise, pairwise) over a bounded candidate pool, a distinct `ModelRanking` value kind that records partial rankings, dropped ids with reasons, scores scoped to one ranking request and pairwise win counts, optional identity-bearing position-bias controls driven by a named seeded generator, and a `select` step that fills k positions from a model ranking and, when it is short or its backend failed, from a declared exact ordering, with per-position origin and derivation. Pairwise schedules are deterministic and bounded by `max_comparisons`; cycles are resolved by win count, then an exact tie-break, and counted. The constructs join the #76 track versions (`rustev.definition/2`, `rustev.plan/3`, `rustev.evidence/2`); the backend descriptor gains an optional `ranking` member. The rules backend (spec 005) answers pointwise `rank` for the swap test. Bedrock (spec 030) admits its reserved `rank` value under 030 3.12. Jev (specs 012 and 022) keeps R-28: rankings over Jev answers stay composed in plans and feed `select` unchanged. Plans that use none of this keep byte-identical identities. Approved (A-36); implementation is complete; the Bedrock `rank` admission follows spec 030 as its own increment (A-49).
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
| `039-model-backed-rank#I-1` | `invariant` | A `ModelRanking` and a `Selection` are distinct value kinds with no conversion to or from scores, distributions, calibrated probabilities, labels or ordinal levels; no probability threshold, label condition, expectation or `weighted_rank@1` component reads them, and a ranking score is compared only with scores of the same ranking request. | `3-9-the-model-ranking-value` | no |
| `039-model-backed-rank#I-2` | `invariant` | Every position of a selection carries its origin (`model` or `backfill`), its source position and its derivation class; a backfill position comes only from a declared ordering whose static class is exact-derived, never repeats a selected id, and never introduces an id outside the ranked candidate pool. | `3-10-the-select-step-and-deterministic-backfill` | no |
| `039-model-backed-rank#I-3` | `invariant` | A model ranking whose backend failed is never treated as an empty or partial ranking: `select` either fills every position from its backfill with a `fallback` notice (`backfill_all`) or is unresolved with the failure's reason (`propagate`), and evidence reasons (missing, stale, conflicting, invalid) always propagate. | `3-10-the-select-step-and-deterministic-backfill` | no |
| `039-model-backed-rank#I-4` | `invariant` | A pairwise ranking issues its comparisons in the schedule's deterministic order, never more than `max_comparisons`, and the same answers yield the same order, win counts and cycle count on every run. | `3-7-pairwise-mode` | no |
| `039-model-backed-rank#I-5` | `invariant` | Position-bias controls are off unless declared; when declared, their kind, order count and seed enter the plan identity, and every presented order is a pure function of the seed, the step, the instance and the request index through `rustev.permute/1`. | `3-8-position-bias-controls` | no |
| `039-model-backed-rank#I-6` | `invariant` | A definition that uses no construct of this spec, and a descriptor without a `ranking` member, keep byte-identical documents, `DescriptorId`s and `PlanId`s, proven by the existing goldens. | `3-18-compatibility` | no |
| `039-model-backed-rank#R-1` | `requirement` | Listwise answers drop unknown ids, ids outside the pool and duplicates with recorded reasons; a list shorter than the pool is a partial ranking, not an error. | `3-5-listwise-mode` | no |
| `039-model-backed-rank#R-2` | `requirement` | The rules backend answers pointwise `rank` tasks deterministically, so the same plan can be run with the rules backend and with a model backend with no change to the policy, the outcomes or any authority path. | `3-14-rules-backend-pointwise-rank-amends-005` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `005-reference-backends` | `{"kind":"directory","path":"backends/rustev-backend-rules/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test ranking_wire --test ranking_results --test ranking_request --test track_versions
cargo test -p rustev-core --locked --test permute_vectors
cargo test -p rustev-core --locked --test ranking_schedules
cargo test -p rustev-core --locked --test ranking_value --test ranking_select --test ranking_evaluate
cargo test -p rustev-core --locked --test ranking_compile --test identity --test reference_plans
cargo test -p rustev-core --locked --doc
cargo test -p rustev-runtime --locked --test ranking
cargo test -p rustev-eval --locked --test ranking_replay
cargo test -p rustev-backend-rules --locked --test ranking
cargo test -p rustev-jev --locked --test adapter --test runtime
cargo test -p rustev-bedrock --locked --test ranking --test rank_swap
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
| amends | `002-decision-contract-and-pure-core`, `005-reference-backends` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | None declared. |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:039-model-backed-rank` | `sha256:5af67d4b87dee78403e13e29ce2c901cab15532770365eb5092ae6fbfca6da39` |
| `registry:039-model-backed-rank` | `sha256:5af67d4b87dee78403e13e29ce2c901cab15532770365eb5092ae6fbfca6da39` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

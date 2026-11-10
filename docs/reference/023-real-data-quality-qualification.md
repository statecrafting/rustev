<!-- spec-spine:begin generated id="header" -->
# `023-real-data-quality-qualification` Semantic quality and calibration qualification on real labeled data

- Path: `specs/023-real-data-quality-qualification/spec.md`
- Summary: The governed path from "mechanics only" to a scoped empirical quality claim that backlog item RUSTEV-011 and owner decision R-04 require: how an appropriately licensed public dataset or an independently human-labeled dataset is admitted, with provenance, license, privacy basis, labeling method, annotator independence, splits and limitations recorded; how the evaluator configuration is frozen before the final-test split is read; how quality, calibration, abstention and regression are measured against exact and rules baselines under spec 004; and how every resulting claim is scoped to the evaluated population, task and binding identity. Approved (A-22). Admitted datasets and their per-case details stay outside this repository, held by the host (OD-7); approval authorizes the procedure and its refusal checks, not any particular dataset, live call or spend beyond R-29.
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
| created | `2026-09-27` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `023-real-data-quality-qualification#I-1` | `invariant` | No quality, calibration or usefulness claim cites a dataset that lacks an admission record, and synthetic or generated examples are never reported as independent evidence of semantic quality. | `3-1-dataset-admission` | no |
| `023-real-data-quality-qualification#I-2` | `invariant` | The evaluator configuration, gates and baselines are frozen and digested before any final-test case is evaluated; a change after that point starts a new qualification with a new final-test split. | `3-3-freezing-before-the-final-test` | no |
| `023-real-data-quality-qualification#I-3` | `invariant` | Every claim names the dataset id, split, evaluator configuration id, binding identity, call dates and population, and states the limits that bound it; a claim beyond them is rejected. | `3-5-claim-scoping` | no |
| `023-real-data-quality-qualification#R-1` | `requirement` | Every candidate is reported against the exact and rules baselines on the same dataset, split and configuration, with coverage and abstention reported before accuracy. | `3-4-measurement` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
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
cargo test -p rustev-eval --locked --test qualification
cargo clippy -p rustev-eval --all-targets --locked -- -D warnings
sh crates/rustev-eval/mutation/seeds.sh "spec 023"
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
| amends | None declared. |
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
| `index:023-real-data-quality-qualification` | `sha256:71d6c01e87b230553ad4dc79c24385f67d83d7487d2ccd20e451c8d7e8e500eb` |
| `registry:023-real-data-quality-qualification` | `sha256:71d6c01e87b230553ad4dc79c24385f67d83d7487d2ccd20e451c8d7e8e500eb` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

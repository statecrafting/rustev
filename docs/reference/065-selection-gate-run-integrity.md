<!-- spec-spine:begin generated id="header" -->
# `065-selection-gate-run-integrity` Selection gate: run integrity failures void the comparison instead of rejecting (amends 029 and 061)

- Path: `specs/065-selection-gate-run-integrity/spec.md`
- Summary: An amendment of approved specs 029 (3.12) and 061 (3.7), for release 0.6.0 (#538), ratified by the owner on 2026-11-06 (A-84). A run with integrity failures (results the evaluator could not score, cases a backend or source failed on) is not a measurement, yet today it can only lower coverage and reject the candidate, which reads as a verdict about the candidate when the run is the problem. This spec adds an optional integrity precondition to `rustev.selection-gate/1`, `integrity: { max_invalid, max_upstream_failures }`, counted per run on each side from documents the selection already verifies. A breach, or a count that cannot be established, voids the comparison: the candidate is `unknown` naming the run, no coverage minimum or clause is evaluated, and so it can never be rejected or qualify on a void run; a baseline run in breach voids every candidate. Gates without the member keep their bytes, identities and results.
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
| created | `2026-11-06` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `065-selection-gate-run-integrity#I-1` | `invariant` | When a gate declares `integrity`, a candidate any of whose runs, or any baseline run, exceeds a declared bound or has a count that cannot be established is `unknown` naming that run, with every coverage minimum and clause unevaluated; such a candidate is never `rejected`, never `qualifies` and never ranked. | `3-3-a-breach-voids-the-comparison` | no |
| `065-selection-gate-run-integrity#I-2` | `invariant` | An integrity count is computed only from documents the selection has verified; a count that is missing, unknown or cannot be derived (no declared scorer for `max_invalid`, a declared scorer without its scores) voids the comparison and is never taken as zero. | `3-2-counting` | no |
| `065-selection-gate-run-integrity#R-1` | `requirement` | A selection gate without `integrity` has the same canonical bytes and identity as today, and `gate select` over it writes the same `rustev.selection/1` bytes, stdout and exit code. | `3-6-compatibility` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

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
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo test -p rustev-conformance --locked
cargo clippy -p rustev-eval -p rustev-cli -p rustev-conformance --all-targets --locked -- -D warnings
sh crates/rustev-eval/mutation/seeds.sh "spec 065"
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
| amends | `029-evaluation-repeatability`, `061-host-scored-repeat-sets` |
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
| `index:065-selection-gate-run-integrity` | `sha256:c0944450e3173687865bc1a7c5aa8d28b37ec1ab44aeb3355f2053d47097f0eb` |
| `registry:065-selection-gate-run-integrity` | `sha256:c0944450e3173687865bc1a7c5aa8d28b37ec1ab44aeb3355f2053d47097f0eb` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

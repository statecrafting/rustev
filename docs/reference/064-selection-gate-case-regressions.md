<!-- spec-spine:begin generated id="header" -->
# `064-selection-gate-case-regressions` Selection gate: a clause on per-case regressions against the baseline (amends 029 and 061)

- Path: `specs/064-selection-gate-case-regressions/spec.md`
- Summary: An amendment of approved specs 029 (3.12) and 061 (3.7), for release 0.6.0 (#537), ratified by the owner on 2026-11-06 (A-83). Every selection clause today compares `pooled` values of the cohort `all` or of each slice, so a candidate that improves most cases and collapses on a few passes `not_worse_than`. This spec adds one clause to `rustev.selection-gate/1`, `case_regressions`, which counts the dataset cases whose candidate value is worse than their baseline value by at least a declared absolute amount, in the metric's declared direction, and fails when more than a declared number regress. A case's value is 029 3.3.3's `pooled` computation restricted to that case, read from the per-case documents the baseline and each candidate already bind (`rustev.eval-cases/1`, or 061's `rustev.host-cases/1`), so no baseline document changes. The boundary is compared in exact decimal; a missing, partial or unknown case value makes the clause unknown, never a pass; `rustev.selection/1` lists the regressing cases. Gates and selections that do not use the clause keep their bytes and identities.
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
| `064-selection-gate-case-regressions#I-1` | `invariant` | A `case_regressions` clause passes only when every compared case has a known value on both sides and the number of regressing cases is at most `at_most`; a case with a measure on one side only, a measure missing from some run of a side, or an unknown or non-finite value makes the clause unknown unless the definite regressions already exceed `at_most`, and it is never counted as a pass, a non-regression or a regression by default. | `3-4-clause-result` | no |
| `064-selection-gate-case-regressions#I-2` | `invariant` | Whether a case regresses is decided in exact decimal: the baseline and candidate case values are taken as the shortest decimals that round-trip to their binary64 values and `by` as its exact decimal, and a case worse by exactly `by` regresses. | `3-3-regression-of-one-case` | no |
| `064-selection-gate-case-regressions#R-1` | `requirement` | A selection gate that does not use `case_regressions` has the same canonical bytes and identity as today, and `gate select` over it writes the same `rustev.selection/1` bytes, stdout and exit code. | `3-7-compatibility` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
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
cargo test -p rustev-surface --locked
cargo test -p rustev-conformance --locked
cargo clippy -p rustev-eval -p rustev-cli -p rustev-surface --all-targets --locked -- -D warnings
sh crates/rustev-eval/mutation/seeds.sh "spec 064"
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
| `index:064-selection-gate-case-regressions` | `sha256:2c50dee7b88ff00cce05dfecec078b9ddd08941035390f3e08af3f9dbf4bcddb` |
| `registry:064-selection-gate-case-regressions` | `sha256:2c50dee7b88ff00cce05dfecec078b9ddd08941035390f3e08af3f9dbf4bcddb` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

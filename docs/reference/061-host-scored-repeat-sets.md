<!-- spec-spine:begin generated id="header" -->
# `061-host-scored-repeat-sets` Host-scored repeat sets for the selection gate (amends 029 and 041)

- Path: `specs/061-host-scored-repeat-sets/spec.md`
- Summary: A proposed amendment of approved specs 029 and 041 (and of spec 006's command table through 029 3.13), for release 0.4.1. Today `rustev gate select` reads only repeat sets Rustev evaluated itself: every run is a spec 004 evaluation of a Rustev plan over a Rustev dataset manifest. A host whose baseline and candidate runs execute in its own runtime, and whose outputs are scored through `scoreViews` into `rustev.scores/1`, has no supported way in. This spec adds a host run record (`rustev.host-run/1`) carrying host dataset, flow, version and run identities with per-case outcomes, latency and cost; an import command, `rustev eval repeat import`, that verifies the host's views and scores and builds a host repeat set with spec 029's statistics; a host baseline; and selection over host sets. Host documents have their own schema names, so no existing document changes bytes or meaning and a host set can never be compared with a Rustev-evaluated one. Every figure is labeled host-asserted. Ratified (A-77) for release 0.4.1.
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
| created | `2026-11-03` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `061-host-scored-repeat-sets#I-1` | `invariant` | A host repeat set, host baseline or host selection is never combined with, compared against or verified as a Rustev-evaluated repeat set, baseline or selection; any command given both refuses with `invalid_input`, `mixed-provenance`, and writes nothing. | `3-6-provenance-separation` | no |
| `061-host-scored-repeat-sets#I-2` | `invariant` | Every host document states `assertion: "host"`, and every figure in it is reported as computed by Rustev from host-asserted observations; no host figure is presented as a Rustev observation or carries a Rustev `PlanId` or `DatasetId` as its identity. | `3-5-derivation-lineage-and-labels` | no |
| `061-host-scored-repeat-sets#I-3` | `invariant` | A host latency, cost, score or outcome that is missing, unknown or malformed is unknown or refused, never zero, never a default, and never dropped without a count. | `3-1-the-host-run-record` | no |
| `061-host-scored-repeat-sets#R-1` | `requirement` | `rustev gate select` over a host baseline and host candidate sets applies spec 029 3.12's clauses, verdicts, ranking and exit codes 0, 11 and 12 unchanged, with the precondition substitutions of 3.7 and no others. | `3-7-host-baselines-and-selection` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo clippy -p rustev-eval -p rustev-cli --all-targets --locked -- -D warnings
sh crates/rustev-eval/mutation/seeds.sh "spec 061"
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
| amends | `029-evaluation-repeatability`, `041-ranking-and-rubric-metrics` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:061-host-scored-repeat-sets` | `sha256:90332998be3b25981e17bec375f01882e6a32bab3d27057069a25d1b417a33c0` |
| `registry:061-host-scored-repeat-sets` | `sha256:90332998be3b25981e17bec375f01882e6a32bab3d27057069a25d1b417a33c0` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `045-command-surface-release-layer` The command surface in the release family (amends 031)

- Path: `specs/045-command-surface-release-layer/spec.md`
- Summary: A narrow amendment of approved spec 031 (itself amending spec 021). Spec 027 adds `rustev-surface`, the pure command renderer every host calls, and requires the CLI to call it instead of its own copy. The CLI is a published release member, so its new dependency must be published first, in a crate layer of its own: 031 3.1's layer table has no slot between `rustev-eval` and the CLI. This spec adds `rustev-surface` to the release family, states the crate layers as the topological levels the release policy already computes from `cargo metadata`, admits a crate that a release member depends on once the crate exists and passes its checks, and keeps the release-set test's member list as the deliberate, reviewed record of membership. Ratified by the owner on 2026-10-20 (A-42).
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
| created | `2026-10-20` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `045-command-surface-release-layer#I-1` | `invariant` | Every normal or build dependency of a release member that is a workspace crate is itself a member of the same release, in an earlier crate layer. | `3-1-membership` | no |
| `045-command-surface-release-layer#I-2` | `invariant` | A crate's layer is one past the deepest layer of its member dependencies, recomputed from cargo metadata at the candidate commit; the release-set file must equal that recomputation. | `3-2-crate-layers` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo test -p rustev-release --test manifests --locked
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
| amends | `031-public-api-and-release-family` |
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
| `index:045-command-surface-release-layer` | `sha256:cf3ff312a846472c66bffd0437f00d3db6875ec9c4ad0d466c6ce99f20789835` |
| `registry:045-command-surface-release-layer` | `sha256:cf3ff312a846472c66bffd0437f00d3db6875ec9c4ad0d466c6ce99f20789835` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

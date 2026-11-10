<!-- spec-spine:begin generated id="header" -->
# `013-remote-state-on-failed-attempts` Remote state on attempts that end without a stop (amends 003)

- Path: `specs/013-remote-state-on-failed-attempts/spec.md`
- Summary: A separately reviewable amendment of approved spec 003 (R-16), chosen by the owner in R-28 for the gap spec 009 5.1 records: the runtime derives `possibly_continuing` only from cancellation, so an attempt whose request left the process and whose connection then failed is recorded as `finished` although remote work and charges may continue. This adds one field to the attempt report through which an adapter states that remote work may continue, and one derivation rule in the runtime. The `rustev.run/1` schema and every existing record's bytes are unchanged. Approved (A-07) and implemented.
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
| created | `2026-09-24` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `013-remote-state-on-failed-attempts#I-1` | `invariant` | An attempt whose adapter reports that remote work may continue is recorded `possibly_continuing` and its charge is settled as unknown liability, never as finished or refunded. | `3-2-runtime-derivation` | no |
| `013-remote-state-on-failed-attempts#I-2` | `invariant` | Every existing backend reports `finished`, so every run record it produces is byte-identical to the one produced before this amendment. | `3-3-compatibility` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `005-reference-backends` | `{"kind":"directory","path":"backends/rustev-backend-rules/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
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
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-backend-rules --locked
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
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
| amends | `003-runtime-execution-and-evidence` |
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
| `index:013-remote-state-on-failed-attempts` | `sha256:495f0a2f06e2f06d59d846709708027e60e70c8399838b8d209eed45e5e9958c` |
| `registry:013-remote-state-on-failed-attempts` | `sha256:495f0a2f06e2f06d59d846709708027e60e70c8399838b8d209eed45e5e9958c` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

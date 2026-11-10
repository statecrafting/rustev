<!-- spec-spine:begin generated id="header" -->
# `025-wire-witness-replay-input` Wire-witness exchanges as remote-exchange and replay evidence (deferred)

- Path: `specs/025-wire-witness-replay-input/spec.md`
- Summary: Reserves backlog item RUSTEV-013: how a redacted, attempt-bound `wire-witness.exchange/1` record, captured by an independent witness and admitted by its host, may corroborate a Rustev `rustev.remote-exchange/1` record and supply retained response bytes for spec 009's offline mapping check, while preserving requested versus served identity, explicit unknowns, the stricter retention, privacy and cost provenance, and host-owned authorization. Captured traffic that no Rustev attempt produced is never a Rustev decision or replay case. Draft and deferred: depends on wire-witness's exchange record being implemented and on a Rustev remote backend that a witness can observe.
- Reference page schema: `1.0.0`
<!-- spec-spine:end generated id="header" -->

<!-- spec-spine:begin generated id="lifecycle" -->
## Lifecycle

| Field | Value |
| --- | --- |
| status | `draft` |
| implementation | `deferred` |
| superseded_by | None declared. |
| retirement_rationale | None declared. |
| created | `2026-09-27` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `025-wire-witness-replay-input#I-1` | `invariant` | A witness record is joined to a Rustev attempt only by an exact match of request byte digests to exactly one exchange record; any other outcome is recorded as unjoined, never guessed. | `3-2-joining-a-witness-record` | no |
| `025-wire-witness-replay-input#I-2` | `invariant` | Witness evidence never overwrites a Rustev field: requested and served identity, usage and cost from the witness are recorded beside Rustev's as corroborating or conflicting testimony. | `3-3-corroboration-not-substitution` | no |
| `025-wire-witness-replay-input#I-3` | `invariant` | Captured traffic that no Rustev attempt produced never becomes a decision, replay bundle or evaluation case, and no imported record grants authority. | `3-5-what-is-not-imported` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

None declared.

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

No declared acceptance block.
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
| `index:025-wire-witness-replay-input` | `sha256:10fe2407153569084f2a1fe23107f4e0e4d4133e30decbae34a386d3d081527c` |
| `registry:025-wire-witness-replay-input` | `sha256:10fe2407153569084f2a1fe23107f4e0e4d4133e30decbae34a386d3d081527c` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

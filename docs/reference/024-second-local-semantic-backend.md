<!-- spec-spine:begin generated id="header" -->
# `024-second-local-semantic-backend` Second local semantic backend (deferred)

- Path: `specs/024-second-local-semantic-backend/spec.md`
- Summary: Reserves backlog item RUSTEV-012 and the architecture's increment 3 "second semantic backend": a second local, offline `DecisionBackend` added only after the first local backend (spec 011) is complete and comparable evaluation reports show a concrete need it would answer. It declares its actual capabilities, artifact identity, resource use and numerical behavior, and must be swappable with spec 011's backend and the Jev adapter without any authority-path or core contract change. Claims no code, names no model, and adds no dependency.
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
| `024-second-local-semantic-backend#I-1` | `invariant` | Swapping this backend for another changes no plan authority path and requires no change to rustev-contract or rustev-core unless separately approved. | `3-3-swap-evidence` | no |
| `024-second-local-semantic-backend#R-1` | `requirement` | Work under this spec starts only when spec 011 is implementation-complete and a retained comparison report names a need the first backend does not meet. | `3-1-entry-criteria` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

None declared.

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
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
| `index:024-second-local-semantic-backend` | `sha256:b84e5ecee78b7598ecbb9dfb2cba578b0cf46cc7dd674fafd7a9302a022003b6` |
| `registry:024-second-local-semantic-backend` | `sha256:b84e5ecee78b7598ecbb9dfb2cba578b0cf46cc7dd674fafd7a9302a022003b6` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

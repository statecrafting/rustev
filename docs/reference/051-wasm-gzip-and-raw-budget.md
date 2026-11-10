<!-- spec-spine:begin generated id="header" -->
# `051-wasm-gzip-and-raw-budget` The WebAssembly module's gzip and uncompressed budgets for the #76 track (amends 027)

- Path: `specs/051-wasm-gzip-and-raw-budget/spec.md`
- Summary: A narrow amendment of approved spec 027 3.10.4, after specs 046 and 050. Spec 046 kept the gzip budget of 786,432 bytes on purpose so that the next #76 specs adding document types would meet it first, and spec 050 kept it by leaving spec 034's code out of the WebAssembly module. Spec 035's first contract increment then measured 789,931 bytes gzip, 3,499 over, from screening's descriptor, output and required-kind types alone; every remaining #76 contract increment adds document types the module must parse. This spec raises the gzip budget to 1,048,576 bytes (1 MiB) and the uncompressed budget to 3,145,728 bytes (3 MiB), and keeps the instantiation budgets, the procedure, the gate and spec 050's exclusion. Ratified by the owner on 2026-10-24 (A-53).
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
| created | `2026-10-24` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `051-wasm-gzip-and-raw-budget#R-1` | `requirement` | The optimized rustev_wasm_bg.wasm is at most 3,145,728 bytes uncompressed and at most 1,048,576 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job. | `3-budget` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |

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
| amends | `027-node-and-wasm-bindings` |
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
| `index:051-wasm-gzip-and-raw-budget` | `sha256:5c6cda459e18015687d16c72b4df195bb9b0ecd0d88779828b612d115a55fe9a` |
| `registry:051-wasm-gzip-and-raw-budget` | `sha256:5c6cda459e18015687d16c72b4df195bb9b0ecd0d88779828b612d115a55fe9a` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

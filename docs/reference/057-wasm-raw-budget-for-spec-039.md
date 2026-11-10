<!-- spec-spine:begin generated id="header" -->
# `057-wasm-raw-budget-for-spec-039` The WebAssembly module's uncompressed budget for spec 039 (amends 027)

- Path: `specs/057-wasm-raw-budget-for-spec-039/spec.md`
- Summary: A narrow amendment of approved spec 027 3.10.4, after specs 046, 050, 051, 052 and 055. Spec 052 raised the uncompressed budget to 4,718,592 bytes (4.5 MiB) and spec 055 kept it gated. Main measures 4,628,645 bytes; spec 039 increment 6 part b (#325, rank and select evaluation) measures 4,724,936 bytes, 6,344 over. This spec raises the uncompressed budget to 5,242,880 bytes (5 MiB) and keeps everything else of 3.10.4: gzip and Brotli recorded and not gated, the instantiation budgets, the procedure and spec 050's exclusion. Ratified by the owner on 2026-10-28 (A-60).
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
| created | `2026-10-28` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `057-wasm-raw-budget-for-spec-039#R-1` | `requirement` | The optimized rustev_wasm_bg.wasm is at most 5,242,880 bytes uncompressed, measured in CI as spec 027 3.10.6 defines; exceeding it fails the bindings job. | `3-budget` | no |
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
| `index:057-wasm-raw-budget-for-spec-039` | `sha256:bd1fcc8a44370b3dad2a03812dec861b2fb67877292ecff2c931a6729547d5bb` |
| `registry:057-wasm-raw-budget-for-spec-039` | `sha256:bd1fcc8a44370b3dad2a03812dec861b2fb67877292ecff2c931a6729547d5bb` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

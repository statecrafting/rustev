<!-- spec-spine:begin generated id="header" -->
# `052-wasm-budget-for-specs-035-to-037` The WebAssembly module's gzip and uncompressed budgets for specs 035 to 037 (amends 027)

- Path: `specs/052-wasm-budget-for-specs-035-to-037/spec.md`
- Summary: A narrow amendment of approved spec 027 3.10.4, after specs 046, 050 and 051. Spec 051 raised the budgets to 1,048,576 bytes gzip and 3,145,728 bytes uncompressed to cover the #76 track; after spec 036 increment 1 (#243) main measures 1,043,813 bytes gzip, and spec 036 increment 2 (#246) fails at 1,084,956. The open stacks of specs 036 and 037 measure up to 1,219,036 bytes gzip and 3,640,136 bytes uncompressed before spec 035's remaining increments. This spec raises the gzip budget to 1,572,864 bytes (1.5 MiB) and the uncompressed budget to 4,718,592 bytes (4.5 MiB), and keeps the instantiation budgets, the procedure, the gate and spec 050's exclusion. Ratified by the owner on 2026-10-25 (A-54).
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
| created | `2026-10-25` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `052-wasm-budget-for-specs-035-to-037#R-1` | `requirement` | The optimized rustev_wasm_bg.wasm is at most 4,718,592 bytes uncompressed and at most 1,572,864 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job. | `3-budget` | no |
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
| `index:052-wasm-budget-for-specs-035-to-037` | `sha256:f0af359c7017cce966d638f968aba1517c83c880a49d4754ab54ef09e98270cf` |
| `registry:052-wasm-budget-for-specs-035-to-037` | `sha256:f0af359c7017cce966d638f968aba1517c83c880a49d4754ab54ef09e98270cf` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

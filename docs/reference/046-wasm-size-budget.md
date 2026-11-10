<!-- spec-spine:begin generated id="header" -->
# `046-wasm-size-budget` The WebAssembly module's uncompressed size budget (amends 027)

- Path: `specs/046-wasm-size-budget/spec.md`
- Summary: A narrow amendment of approved spec 027 3.10.4. The uncompressed size budget of the optimized `rustev_wasm_bg.wasm` was declared at 2,097,152 bytes before specs 032 and 033 added their document types; the first measurement is 2,215,589 bytes, 5.6% over, while the gzip and instantiation budgets hold. This spec raises the uncompressed budget to 2,621,440 bytes (2.5 MiB) and keeps every other budget, the procedure and the gate unchanged. Ratified by the owner on 2026-10-20 (A-43).
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
| `046-wasm-size-budget#R-1` | `requirement` | The optimized rustev_wasm_bg.wasm is at most 2,621,440 bytes uncompressed and at most 786,432 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job. | `3-budget` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

None declared.

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
| `index:046-wasm-size-budget` | `sha256:dd51dfc15a14bf0f8b6fe64e77e8d1f99a41a982a8b2d4ebc12e5af6d95228bc` |
| `registry:046-wasm-size-budget` | `sha256:dd51dfc15a14bf0f8b6fe64e77e8d1f99a41a982a8b2d4ebc12e5af6d95228bc` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

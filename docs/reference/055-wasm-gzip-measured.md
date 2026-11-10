<!-- spec-spine:begin generated id="header" -->
# `055-wasm-gzip-measured` The WebAssembly module's gzip size is recorded, not gated (amends 027)

- Path: `specs/055-wasm-gzip-measured/spec.md`
- Summary: A narrow amendment of approved spec 027 3.10.4, after specs 046, 050, 051 and 052. As the #76 track added document types, the gzip budget was met four times: specs 046 and 050 kept it, and specs 051 and 052 raised it, and the last time it stopped every open chain at once. This spec keeps measuring and recording the optimized module's gzip size (level 9) on every run, as the Brotli size already is, and removes it from the gated budget. The uncompressed budget of 4,718,592 bytes (spec 052), the instantiation budgets, the procedure and spec 050's exclusion are unchanged. Ratified by the owner on 2026-10-25 (A-57).
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
| `055-wasm-gzip-measured#R-1` | `requirement` | The optimized rustev_wasm_bg.wasm's gzip level 9 size is measured and recorded in the measurement document on every run and gates nothing; its uncompressed size stays gated at 4,718,592 bytes. | `3-measurement` | no |
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
| `index:055-wasm-gzip-measured` | `sha256:9085ee876d001f34c4d59eba36955fee0053956050cf3ff97b25343d64360f02` |
| `registry:055-wasm-gzip-measured` | `sha256:9085ee876d001f34c4d59eba36955fee0053956050cf3ff97b25343d64360f02` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

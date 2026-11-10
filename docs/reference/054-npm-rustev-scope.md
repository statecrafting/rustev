<!-- spec-spine:begin generated id="header" -->
# `054-npm-rustev-scope` The npm packages move to the owner's @rustev scope (amends 027 and 031)

- Path: `specs/054-npm-rustev-scope/spec.md`
- Summary: A narrow amendment of approved specs 027 and 031. Both specs named the npm packages under the `@statecrafting` scope and recorded that whether that scope could be used was not established. The owner publishes Rustev under the npm organization `rustev` (2026-10-25). This spec moves every npm package from `@statecrafting/` to `@rustev/` and changes nothing else: the native package `@rustev/rustev`, its platform packages `@rustev/rustev-&lt;platform&gt;`, and the WebAssembly package `@rustev/rustev-wasm`. No API, export, crate name, container image or GitHub location changes. Ratified by the owner on 2026-10-25 (A-56).
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
| `054-npm-rustev-scope#I-1` | `invariant` | Every npm package of the release family is named under the `@rustev/` scope, and the release tooling refuses a package outside it. | `3-names` | no |
| `054-npm-rustev-scope#R-1` | `requirement` | The native package is `@rustev/rustev`, each platform package is `@rustev/rustev-&lt;platform&gt;` for the platforms of spec 027 3.13.1, and the WebAssembly package is `@rustev/rustev-wasm`; no `@statecrafting/` name remains in a package manifest, the native loader, the release tooling or their tests. | `3-names` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |

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
| amends | `027-node-and-wasm-bindings`, `031-public-api-and-release-family` |
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
| `index:054-npm-rustev-scope` | `sha256:7407260235ec2399e9089c2e04af93f963c3c51ee352e2cb2ba3d81dd08776c4` |
| `registry:054-npm-rustev-scope` | `sha256:7407260235ec2399e9089c2e04af93f963c3c51ee352e2cb2ba3d81dd08776c4` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

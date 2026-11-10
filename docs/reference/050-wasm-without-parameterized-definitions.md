<!-- spec-spine:begin generated id="header" -->
# `050-wasm-without-parameterized-definitions` The WebAssembly module without parameterized definitions (amends 027)

- Path: `specs/050-wasm-without-parameterized-definitions/spec.md`
- Summary: A narrow amendment of approved spec 027. Spec 034's parameterized definitions (rustev.definition/2 templates, profiles, instantiation and rustev.plan/3 instantiation records) put the optimized rustev_wasm_bg.wasm over spec 027's gzip budget, which spec 046 kept on purpose so that such a cost shows: 860,362 bytes against 786,432 at spec 034 increment 3. Spec 034 leaves exposing its functions through WebAssembly to spec 027. This spec keeps the budget and keeps spec 034's code out of the WebAssembly module: rustev-core, rustev-eval and rustev-surface gain an opt-in cargo feature `parameterized` (spec 031 keeps every release crate's `default` empty) that every workspace consumer but rustev-wasm enables, and the WebAssembly binding refuses a definition that uses spec 034's members and a plan that carries an instantiation record with a typed refusal; version 2 definitions and version 3 plans of specs 032 and 033 are unchanged. The Node binding, the CLI and every other surface keep the feature. Ratified by the owner on 2026-10-24 (A-52); the feature made opt-in the same day, before merge, by the owner's decision (A-52).
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
| `050-wasm-without-parameterized-definitions#R-1` | `requirement` | rustev-core, rustev-eval and rustev-surface compile spec 034's template, profile, instantiation and rustev.plan/3 code only under a cargo feature `parameterized`, not enabled by default (spec 031's empty `default`) and forwarded between them; every workspace consumer of them except rustev-wasm enables it, the facade `rustev` included, and rustev-wasm enables it on none of them. | `3-behavior` | no |
| `050-wasm-without-parameterized-definitions#R-2` | `requirement` | Without the feature, a command given a definition that uses any of spec 034's members (`parameters`, `variants`, `only_in`, `enabled` or a parameter reference, spec 034 3.1), a plan that carries an `instantiation` record, or a profile or variant argument refuses it with a typed refusal naming the member or argument and `parameterized`; it never ignores the member or argument. Definitions and plans without them, including version 2 definitions and version 3 plans of specs 032 and 033, are unchanged. | `3-behavior` | no |
| `050-wasm-without-parameterized-definitions#R-3` | `requirement` | With the feature, every surface's behavior and bytes are unchanged; the WebAssembly module stays within spec 046's budgets, and every conformance case other than spec 034's passes on every surface, WebAssembly included. | `3-behavior` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `005-reference-backends` | `{"kind":"directory","path":"backends/rustev-backend-rules/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-aicortex/"}` | no | `resolved` |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-rahi/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"integrations/rustev/"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |

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
| `index:050-wasm-without-parameterized-definitions` | `sha256:935d22f92692c9947284512eecf44f408224533d79be47d2760bfb2ae63e472e` |
| `registry:050-wasm-without-parameterized-definitions` | `sha256:935d22f92692c9947284512eecf44f408224533d79be47d2760bfb2ae63e472e` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

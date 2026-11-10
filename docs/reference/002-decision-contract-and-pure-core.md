<!-- spec-spine:begin generated id="header" -->
# `002-decision-contract-and-pure-core` Decision contract and pure core

- Path: `specs/002-decision-contract-and-pure-core/spec.md`
- Summary: Increment 1: the versioned wire contract (`rustev-contract`) and the pure core (`rustev-core`). Bounded parsing, canonical form and identities, value kinds without silent conversion, fixed-point decimals and millisecond time, input validation with provenance, freshness and conflict, a closed versioned exact-operator registry, decision definitions entered through a typed builder or canonical JSON, the plan compiler with capability matching, declared fallbacks and typed refusals, calibration binding, deterministic selection policy with explicit unresolved handling, and derivation classes with lineage. No I/O, no clock, no async executor, no model.
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
| created | `2026-09-23` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

None declared.
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
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo run -p rustev-boundaries --locked --quiet
cargo build -p rustev-contract --locked
cargo clippy -p rustev-contract -p rustev-core --all-targets --locked -- -D warnings
cargo fmt -p rustev-contract -p rustev-core --check
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
| amends | None declared. |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `003-runtime-execution-and-evidence`, `004-evaluation-and-replay`, `019-dynamic-questions`, `032-evidence-quality`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `044-runtime-optimization-increment-2`, `047-target-independent-transcendentals`, `066-segment-ancestry-predicate` |
| superseded by | None declared. |
| extended by | `003-runtime-execution-and-evidence`, `004-evaluation-and-replay`, `009-remote-adapter-protocol`, `013-remote-state-on-failed-attempts`, `017-general-runtime-optimization`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `026-live-re-execution`, `027-node-and-wasm-bindings`, `028-opentelemetry-evidence-sink`, `031-public-api-and-release-family`, `032-evidence-quality`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `042-decision-service-and-remote-seams`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `047-target-independent-transcendentals`, `050-wasm-without-parameterized-definitions`, `066-segment-ancestry-predicate`, `070-extract-rules-2` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:002-decision-contract-and-pure-core` | `sha256:1f50e3032e1915d83f963a9c4a94889bff351f7bb8985249e6384cc641096d73` |
| `registry:002-decision-contract-and-pure-core` | `sha256:1f50e3032e1915d83f963a9c4a94889bff351f7bb8985249e6384cc641096d73` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

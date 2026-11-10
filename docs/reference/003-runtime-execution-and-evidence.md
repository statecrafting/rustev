<!-- spec-spine:begin generated id="header" -->
# `003-runtime-execution-and-evidence` Runtime execution and evidence emission

- Path: `specs/003-runtime-execution-and-evidence/spec.md`
- Summary: Increment 2: `rustev-runtime` drives the pure core's staged evaluation through backend seams on Tokio (R-03) under explicit bounds: injectable monotonic time and one end-to-end deadline, bounded admission and concurrency, cost budgets reserved before dispatch with honest settlement, bounded retries and runtime fallback declared in an identified execution policy that is part of plan identity, cancellation that never claims more than the backend acknowledged, and evidence delivery under a declared sink policy. Amends spec 002: plan schema 2, the execution-policy and run-record documents, a pre-supply output check, and the backend and sink seams. Batching, duplicate suppression and caches are deferred by name.
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
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
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
cargo test -p rustev-runtime --locked
cargo run -p rustev-boundaries --locked --quiet
cargo build -p rustev-core --locked
sh -c 't=$(cargo tree -p rustev-runtime -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -E "^rustev-" | grep -vxE "rustev-(contract|core|runtime)" | grep -q .; then exit 1; fi'
cargo clippy -p rustev-runtime --all-targets --locked -- -D warnings
cargo fmt -p rustev-runtime --check
sh crates/rustev-runtime/mutation/seeds.sh
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
| amends | `002-decision-contract-and-pure-core` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `004-evaluation-and-replay`, `013-remote-state-on-failed-attempts`, `017-general-runtime-optimization`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `026-live-re-execution`, `032-evidence-quality`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `044-runtime-optimization-increment-2` |
| superseded by | None declared. |
| extended by | `004-evaluation-and-replay`, `013-remote-state-on-failed-attempts`, `017-general-runtime-optimization`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `026-live-re-execution`, `027-node-and-wasm-bindings`, `031-public-api-and-release-family`, `032-evidence-quality`, `033-provenance-rules-and-actionable-outcomes`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `042-decision-service-and-remote-seams`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `050-wasm-without-parameterized-definitions`, `070-extract-rules-2` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:003-runtime-execution-and-evidence` | `sha256:e27994104d668cfb9d4b0023907f14d6f29b60f87ae0d6fd9ddca3e74de08a09` |
| `registry:003-runtime-execution-and-evidence` | `sha256:e27994104d668cfb9d4b0023907f14d6f29b60f87ae0d6fd9ddca3e74de08a09` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

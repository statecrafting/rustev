<!-- spec-spine:begin generated id="header" -->
# `027-node-and-wasm-bindings` Node-API binding and WebAssembly build

- Path: `specs/027-node-and-wasm-bindings/spec.md`
- Summary: Approved (A-23). Two host surfaces over the existing engine: `rustev-node`, a napi-rs Node-API binding through which a TypeScript service compiles plans, runs decisions, replays and evaluates in-process and implements Rustev's seams in TypeScript as explicit composition (R-38); and `rustev-wasm`, a wasm-bindgen build of the pure crates for browsers, edge runtimes and tooling. Documents cross as the contract's canonical JSON, TypeScript types are generated from JSON Schema emitted by `rustev-contract`, command rendering moves into one pure crate shared by every surface, and every surface (CLI, native binding, WebAssembly, later HTTP and gRPC) runs one shared conformance suite. Approval does not authorize npm or crates.io publication, name reservation, a release, a live provider call, spend, or a claim that cross-target floating point agrees beyond what the parity suite measures.
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
| created | `2026-10-16` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `027-node-and-wasm-bindings#I-1` | `invariant` | No binding output is authorization: a binding returns proposals, unresolved outcomes, refusals and evidence only, defines no permission type, and passes the principal handle through as opaque bytes. | `3-15-binding-principles` | no |
| `027-node-and-wasm-bindings#I-2` | `invariant` | Rust never waits synchronously on JavaScript: every host-implemented seam call is asynchronous through a bounded queue with a typed refusal when full, carries its remaining deadline and cancellation, discards and counts a late resolution, and maps a JavaScript exception or rejection to the seam's typed failure without a panic crossing the boundary. | `3-6-host-implemented-seams` | no |
| `027-node-and-wasm-bindings#I-3` | `invariant` | An unresolved outcome, including one caused by an exhausted deadline or budget during a decision, is a value in the result and never an exception; an exception means that no decision ran or that evidence delivery failed, exactly as the Rust API reports it. | `3-4-errors-and-unresolved-results` | no |
| `027-node-and-wasm-bindings#I-4` | `invariant` | Node-API and wasm-bindgen dependency families occur only under integrations; contract, core and the shared surface crate never reach them, an executor or an HTTP stack; and the WebAssembly crate's normal dependency graph contains no runtime, executor, HTTP stack, randomness source or Node-API crate. | `3-14-dependency-boundaries` | no |
| `027-node-and-wasm-bindings#I-5` | `invariant` | Every host surface runs the one shared conformance suite, and a surface's output for a case equals the case's expected bytes under only the comparison the case declares. | `3-12-shared-conformance-suite` | no |
| `027-node-and-wasm-bindings#R-1` | `requirement` | TypeScript document types are generated from JSON Schema emitted by rustev-contract, are identical in both packages, and a drift check fails when committed schema or types differ from regenerated output. | `3-2-typed-boundary-and-generated-types` | no |
| `027-node-and-wasm-bindings#R-2` | `requirement` | The WebAssembly module stays within the declared compressed size and instantiation budgets, measured in CI as section 3.10 defines; exceeding a budget fails the build, and raising one is an amendment of this spec. | `3-10-webassembly-build` | no |
| `027-node-and-wasm-bindings#R-3` | `requirement` | The native package ships prebuilt binaries for the declared platform matrix with a pinned Node-API version and no install-time download, and each target loads and passes a smoke test in CI. | `3-13-packaging` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

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

```console
cargo test -p rustev-conformance --locked
cargo test -p rustev-surface --locked
cargo run -p rustev-boundaries --locked --quiet
cargo test -p rustev-boundaries --locked
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
| amended by | `046-wasm-size-budget`, `047-target-independent-transcendentals`, `049-opentelemetry-through-the-node-binding`, `050-wasm-without-parameterized-definitions`, `051-wasm-gzip-and-raw-budget`, `052-wasm-budget-for-specs-035-to-037`, `054-npm-rustev-scope`, `055-wasm-gzip-measured`, `057-wasm-raw-budget-for-spec-039`, `063-node-binding-parity`, `067-node-live-repeat-runs`, `069-node-host-nonblocking-open` |
| superseded by | None declared. |
| extended by | `011-semantic-backend`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `029-evaluation-repeatability`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `041-ranking-and-rubric-metrics`, `042-decision-service-and-remote-seams`, `044-runtime-optimization-increment-2`, `049-opentelemetry-through-the-node-binding`, `050-wasm-without-parameterized-definitions`, `051-wasm-gzip-and-raw-budget`, `052-wasm-budget-for-specs-035-to-037`, `054-npm-rustev-scope`, `055-wasm-gzip-measured`, `057-wasm-raw-budget-for-spec-039`, `063-node-binding-parity`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity`, `066-segment-ancestry-predicate`, `067-node-live-repeat-runs`, `069-node-host-nonblocking-open`, `070-extract-rules-2` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:027-node-and-wasm-bindings` | `sha256:0d5370e75feb39bfd64a3f955945a1805c2f6b04cb2d59fac5bae6f2493e1b30` |
| `registry:027-node-and-wasm-bindings` | `sha256:0d5370e75feb39bfd64a3f955945a1805c2f6b04cb2d59fac5bae6f2493e1b30` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

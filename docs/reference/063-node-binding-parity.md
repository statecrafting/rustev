<!-- spec-spine:begin generated id="header" -->
# `063-node-binding-parity` Node binding parity: acquisition source, profiles and variants, store seam, capture, repeatability commands (amends 027, 029, 034, 036 and 044)

- Path: `specs/063-node-binding-parity/spec.md`
- Summary: An amendment of approved spec 027, closing the Node exposure that specs 029, 034, 036 and 044 left out of scope (#56, with #62 and #73), for release 0.5.0. `@rustev/rustev` gains a host-implemented acquisition source for 036's `acquire` and `widen` steps; profile, variant and base-profile inputs on `compile`, with `profileCheck` and `planDiff`; a host `RuntimeStore` for 044's persistent tier, with a stated atomicity contract and invalidation; opt-in capture on `decide` that returns a replay bundle, because the evidence a host sink receives cannot replay a decision; and 029's repeatability commands with 061's host import, in process. Each uses 027's seam rules, the Rust API the CLI calls and the shared conformance suite; the WebAssembly package gains none. Ratified by the owner on 2026-11-03 (A-78).
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
| created | `2026-11-03` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `063-node-binding-parity#I-1` | `invariant` | A host-implemented acquisition source or store obeys every rule of spec 027 3.6 and section 3.4 here (asynchronous Promise callbacks through a threadsafe function, bounded queue with a typed refusal, remaining deadline and cancellation passed in, a late or never-settling Promise discarded and counted); an acquisition failure reaches the judgment only as spec 036's unresolved or skipped outcome, and a store failure only as a miss or a refusal under the plan's on_store_failure policy, never as a defaulted value. | `3-4-host-callbacks-promises-deadlines-and-threads` | no |
| `063-node-binding-parity#I-2` | `invariant` | For the same inputs, the binding's compile with a profile or variant, profileCheck, planDiff, repeatability commands and captured replay bundles produce the bytes, PlanId and exit code the CLI or the Rust API produces, under only the comparison each shared conformance case declares. | `3-13-conformance` | no |
| `063-node-binding-parity#I-3` | `invariant` | No binding API accepts a cache key, store scope or generation from the host, or selects a profile or variant from a principal, tenant or request; Rustev derives every key and the host names the profile and variant explicitly. | `2-1-binding-principles` | no |
| `063-node-binding-parity#I-4` | `invariant` | Capture is off unless the caller passes it, retains content only by explicit choice, and its bundle and items reach only the caller's decide result; nothing captured is sent to the evidence sink, telemetry, gauges or an error message, and no principal handle, credential or trace header enters a bundle; embedded and external bundles carry unredacted inputs, model outputs and pages, which spec 028's redaction policy does not cover and the host must protect as personal data. | `3-8-capture-on-decide` | no |
| `063-node-binding-parity#R-1` | `requirement` | The WebAssembly package gains none of these surfaces; its import allowlist, budgets and parameterized-feature exclusion are unchanged. | `3-12-webassembly-position` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

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

```console
cargo test -p rustev-conformance --locked
cargo test -p rustev-surface --locked --all-features
cargo test -p rustev-cli --locked
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
| amends | `027-node-and-wasm-bindings`, `029-evaluation-repeatability`, `034-parameterized-definitions`, `036-mid-plan-acquisition`, `044-runtime-optimization-increment-2` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `067-node-live-repeat-runs` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:063-node-binding-parity` | `sha256:3b9c4b9471213da387aeffe3b9c283d5b299e892a6708729541829eaa88135cf` |
| `registry:063-node-binding-parity` | `sha256:3b9c4b9471213da387aeffe3b9c283d5b299e892a6708729541829eaa88135cf` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

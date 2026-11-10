<!-- spec-spine:begin generated id="header" -->
# `020-remote-protocol-refinements` Remote protocol and evidence refinements

- Path: `specs/020-remote-protocol-refinements/spec.md`
- Summary: A separately reviewable refinement of the approved runtime and remote contracts: `rustev.run/2` references bounded remote exchange evidence, the runtime may dispatch an explicitly compatible general batch, transport phases receive identified ceilings inside one total attempt budget, connection reuse is bounded and isolation-keyed, and a host-operated endpoint may receive an opt-in opaque principal reference. All features default off or preserve version 1 behavior. Approval authorizes implementation only: no migration, qualification, deployment, live call, spend, or publication is authorized or claimed.
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
| created | `2026-09-26` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `020-remote-protocol-refinements#I-1` | `invariant` | Every transport phase ceiling consumes the one remaining attempt budget; no phase, retry, pooled connection or batch grants more time, cost, authority or work than the identified plan permits. | `3-4-phase-specific-time-ceilings` | no |
| `020-remote-protocol-refinements#I-2` | `invariant` | A pooled connection is reusable only within one complete transport isolation key and never carries credentials or principal context across a key boundary. | `3-5-bounded-connection-reuse` | no |
| `020-remote-protocol-refinements#I-3` | `invariant` | A principal reference is optional host-supplied routing context for a host-operated endpoint, never a credential, grant, policy input, provider field or authorization. | `3-6-host-endpoint-principal-reference` | no |
| `020-remote-protocol-refinements#I-4` | `invariant` | A run record references remote evidence without embedding secrets or unbounded exchange bodies, and unavailable referenced evidence is explicit rather than reconstructed. | `3-2-rustev-run-2` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
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
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
cargo test -p rustev-remote-http --locked
cargo test -p rustev-jev --locked
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-contract -p rustev-core -p rustev-runtime -p rustev-eval -p rustev-remote-http -p rustev-jev --all-targets --locked -- -D warnings
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
| amends | `003-runtime-execution-and-evidence`, `004-evaluation-and-replay`, `009-remote-adapter-protocol`, `012-jev-integration` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `044-runtime-optimization-increment-2` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:020-remote-protocol-refinements` | `sha256:d7de0ac937b78f25ec1430a9954022d86f13443835e808b9ae84b0a2dabb46e7` |
| `registry:020-remote-protocol-refinements` | `sha256:d7de0ac937b78f25ec1430a9954022d86f13443835e808b9ae84b0a2dabb46e7` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

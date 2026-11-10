<!-- spec-spine:begin generated id="header" -->
# `042-decision-service-and-remote-seams` Decision service sidecar and remote context and sink protocols (amends 010)

- Path: `specs/042-decision-service-and-remote-seams/spec.md`
- Summary: An amendment of approved spec 010 (R-16), for #76 child A2 (#57). Makes the `rustev-serve` service API concrete as `rustev.serve/1`: plan registration by content identity from plain or signed plan bundles, decisions, cancellation, lookup, server-sent event streaming, offline replay, evaluation and explanation, health, readiness and OpenMetrics, served over HTTP/1.1 and HTTP/2 with JSON and over gRPC from one handler layer, on TCP and Unix domain socket listeners. Defines two new transport-neutral callback protocols with an HTTP binding, `rustev.remote-context/1` (describe, fetch, acquire, cancel and invalidation by push or poll) and `rustev.remote-sink/1` (describe and deliver with receipts), behind bounded queues with deadlines, cancellation and typed refusals when full. Adds no plan construct: every `PlanId`, run record and judgment byte is unchanged. Approved (A-39) under the 2026-10-16 delivery work order; implementation is complete (2026-11-03), after the owner approved `rustev-serve`'s spec 010 section 3.4 binding record (R-44).
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
| created | `2026-10-17` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `042-decision-service-and-remote-seams#I-1` | `invariant` | No service or callback output is authorization: a response, receipt, registration, verified signature or HTTP or gRPC success is only a judgment, unresolved outcome, refusal, integrity fact or evidence disposition, and the principal handle passes through as opaque bytes. | `2-1-binding-principles-of-76` | no |
| `042-decision-service-and-remote-seams#I-2` | `invariant` | For every conformance case, the HTTP/JSON and gRPC transports return byte-identical documents, and their judgments and delivered run records equal the CLI's and the bindings' bytes for the same case. | `3-17-shared-conformance` | no |
| `042-decision-service-and-remote-seams#I-3` | `invariant` | Every callback seam call goes through a bounded queue that refuses with a typed failure when full, carries its remaining deadline and cancellation, and discards and counts a late response; nothing buffers without bound. | `3-12-bounded-callback-seams` | no |
| `042-decision-service-and-remote-seams#I-4` | `invariant` | Shutdown loses no evidence silently: every admitted decision ends delivered, failed with the caller told, or dropped or unflushed and counted with its decision id in the shutdown report. | `3-14-graceful-shutdown` | no |
| `042-decision-service-and-remote-seams#I-5` | `invariant` | This spec adds no plan construct: every plan and definition golden keeps its bytes and PlanId, and run record and judgment bytes are unchanged. | `3-19-compatibility` | no |
| `042-decision-service-and-remote-seams#R-1` | `requirement` | rustev-serve is not implemented until the owner approves its spec 010 section 3.4 binding record, which covers at least the rows of section 2.2. | `2-2-relation-to-spec-010-and-the-binding-record-gate` | no |
| `042-decision-service-and-remote-seams#R-2` | `requirement` | A plan bundle is registered only when its content recompiles to the PlanId in its path and compile report, and its signature satisfies the configured mode; a wrong key, tampered content or an unsigned bundle under a mode that forbids it is refused with its code. | `3-3-plan-registration-and-signed-bundles` | no |
| `042-decision-service-and-remote-seams#R-3` | `requirement` | Every refusal before a decision runs is a typed rustev.serve-refusal/1 with a code from section 3.5; Retry-After is sent only when the runtime states a retry time. | `3-5-admission-refusals-and-limits` | no |
| `042-decision-service-and-remote-seams#R-4` | `requirement` | Request header and body byte limits are enforced before parsing; the document's bounded parser applies after. | `3-5-admission-refusals-and-limits` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/tests/common/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
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
cargo test -p rustev-serve --locked
cargo test -p rustev-contract --locked --test 042_serve_documents --test 042_remote_documents
cargo test -p rustev-runtime --locked --test progress
cargo clippy -p rustev-serve --all-targets --locked -- -D warnings
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
| amends | `010-integrations` |
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
| `index:042-decision-service-and-remote-seams` | `sha256:eb7697d49baf4df6888f75856f299c6053a2e683023251aaa0d5f3e3011cb2f7` |
| `registry:042-decision-service-and-remote-seams` | `sha256:eb7697d49baf4df6888f75856f299c6053a2e683023251aaa0d5f3e3011cb2f7` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

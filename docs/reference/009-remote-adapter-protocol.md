<!-- spec-spine:begin generated id="header" -->
# `009-remote-adapter-protocol` Remote adapter protocol

- Path: `specs/009-remote-adapter-protocol/spec.md`
- Summary: Increment 3 (design roadmap 17.1, ordinal 009): what every decision backend reached across a network must guarantee, and a versioned, transport-neutral protocol (`rustev.remote/1`) for backends served by another process. Covers capability description and negotiation at setup, attempt identity and idempotency, one total time budget per attempt, cancellation that never claims more than the remote side confirmed, validation of responses into `RawOutput` without inventing mass, a closed error taxonomy mapped onto spec 003's failure classes, explicit served-model identity (unknown is a value, never an omission), cost reporting, retained exchange records for audit and replay, and privacy options passed through and recorded. Protocol messages live in `rustev-contract`; every network implementation lives under `integrations/`. Amends no approved behavior. Approved (A-08); implemented.
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
| created | `2026-09-24` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `009-remote-adapter-protocol#I-1` | `invariant` | A remote adapter never grants authority: nothing a remote party returns is authorization, changes a plan, policy, threshold, retry or fallback, or is supplied to the core as anything but a raw output or a classified failure. | `3-1-authority-and-provenance` | no |
| `009-remote-adapter-protocol#I-2` | `invariant` | Served-model identity is always explicit: an exchange record states a served identity or `unknown`, and never copies the requested model into the served field. | `3-7-model-and-served-identity` | no |
| `009-remote-adapter-protocol#I-3` | `invariant` | Retries and fallback happen only as the plan's execution policy declares them: an adapter never resubmits an attempt, never retries internally and never substitutes another model, provider or backend. | `3-3-attempt-identity-and-idempotency` | no |
| `009-remote-adapter-protocol#I-4` | `invariant` | An adapter never manufactures mass: it does not renormalize, fill a missing option, convert a label or a score into a distribution, or pass a provider-reported confidence as a probability. | `3-5-response-validation-into-rawoutput` | no |
| `009-remote-adapter-protocol#I-5` | `invariant` | One attempt has one total time budget, the remaining time at dispatch; no connect, write, read or remote timeout extends beyond it. | `3-4-one-total-budget-and-cancellation` | no |
| `009-remote-adapter-protocol#I-6` | `invariant` | An adapter acknowledges `stopped` only when it can establish that no request byte left the process or the remote side confirmed the stop; every other ended-early attempt is unconfirmed and its charge is liability. | `3-4-one-total-budget-and-cancellation` | no |
| `009-remote-adapter-protocol#I-7` | `invariant` | `rustev-contract` and `rustev-core` carry no vendor, model or provider name and no HTTP dependency; protocol messages are vendor-neutral. | `2-territory` | no |
| `009-remote-adapter-protocol#R-1` | `requirement` | Capabilities are negotiated once, at setup, before any plan is compiled against the adapter; a descriptor that changes afterwards fails attempts as `identity_mismatch` and is never adopted silently. | `3-2-capability-description-and-negotiation` | no |
| `009-remote-adapter-protocol#R-2` | `requirement` | Every remote failure is one code of the closed taxonomy of 3.6, mapped to exactly one spec 003 failure class. | `3-6-error-taxonomy` | no |
| `009-remote-adapter-protocol#R-3` | `requirement` | Every exchange produces a bounded exchange record keyed by attempt id, retained under the host's policy with digest-only content by default. | `3-9-exchange-records-and-replay` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked remote
cargo test -p rustev-remote-http --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 'if grep -rniE "jev|typesafe|vercel|openai|anthropic" crates/rustev-contract/src crates/rustev-core/src; then exit 1; fi'
cargo clippy -p rustev-remote-http --all-targets --locked -- -D warnings
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
| amended by | `020-remote-protocol-refinements`, `035-screening-seam`, `044-runtime-optimization-increment-2` |
| superseded by | None declared. |
| extended by | `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `031-public-api-and-release-family`, `035-screening-seam`, `037-generation-steps`, `039-model-backed-rank`, `042-decision-service-and-remote-seams`, `044-runtime-optimization-increment-2`, `050-wasm-without-parameterized-definitions`, `059-shared-remote-transport-by-dependency` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:009-remote-adapter-protocol` | `sha256:9af5a0b894718dbee917fad1be3badb9a9a913f4d54e250f5b05a21fe15124ef` |
| `registry:009-remote-adapter-protocol` | `sha256:9af5a0b894718dbee917fad1be3badb9a9a913f4d54e250f5b05a21fe15124ef` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

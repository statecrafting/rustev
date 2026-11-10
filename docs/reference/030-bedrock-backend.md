<!-- spec-spine:begin generated id="header" -->
# `030-bedrock-backend` Hosted model platform backend: AWS Bedrock

- Path: `specs/030-bedrock-backend/spec.md`
- Summary: A remote decision backend under `integrations/rustev-bedrock/` for Amazon Bedrock over two wires on the `bedrock-runtime` endpoint, Converse and the OpenAI-compatible Responses API, signed with SigV4 over Rustev's existing hyper and rustls stack. A versioned binding document puts wire, region, model or inference profile, declared destination regions, residency policy, parameters, structured output mode, price table and reserved members for later operations into the artifact identity. A host-supplied `ModelPolicy` (deny wins, no default allowlist) is evaluated at bind time and again at runtime start. Wave 1 serves `classify`, `proposition` and `rubric` as `Label` outputs, because neither wire documents token log-probabilities; `rank`, `generate`, `extract` and Guardrails screening are added by their own specs into members reserved here. Amends spec 001 only by adding a cloud SDK dependency family to the boundary check. Approved (A-26) under the 2026-10-16 delivery work order; implementation is complete, with a recorded live exchange on each wire. The work order authorizes paid Bedrock smoke and probe calls on synthetic or spec 023 admitted data within R-29's total testing cap across all providers (USD 10 since A-75 raised it from USD 5); the owner supplies AWS credentials, whose absence blocks only the live stage. Approval makes no quality, residency-compliance or production claim.
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
| `030-bedrock-backend#I-1` | `invariant` | The Bedrock backend never grants authority: nothing Bedrock returns is authorization, an input fact, a policy parameter, or anything other than a raw output or a classified failure, and every supplied value is model-derived. | `3-1-position-authority-and-provenance` | no |
| `030-bedrock-backend#I-2` | `invariant` | Every member of `rustev.bedrock-binding/1` is always present and enters the artifact identity; a change of wire, region, model, profile kind, destination set, residency policy, parameters, structured output mode, price table or any reserved member is a different backend. | `3-2-the-binding-document` | no |
| `030-bedrock-backend#I-3` | `invariant` | A binding whose declared destination regions or profile kind fall outside its residency policy, or whose model id prefix disagrees with its declared profile kind, is refused at construction; no descriptor exists for it, so no plan compiles against it. | `3-3-residency-and-inference-profiles` | no |
| `030-bedrock-backend#I-4` | `invariant` | Rustev ships no default allowlist: a backend is constructed only with a host-supplied `ModelPolicy`, a binding is admitted only when an allow rule matches and no deny rule matches, and every refusal names the rule or the absence of a match. | `3-4-model-policy` | no |
| `030-bedrock-backend#I-5` | `invariant` | Without the `aws-config` feature the crate reads no environment variable, profile file or instance metadata; credentials come only from the host's credential provider and never enter identity, logs or exchange records. | `3-5-credentials-signing-and-transport` | no |
| `030-bedrock-backend#I-6` | `invariant` | A distribution or logits output is supplied only from token log-probabilities the wire returns for every declared option; a self-reported confidence, an argmax or a free-text score is recorded as provider-reported and never supplied or converted into mass. | `3-7-operations-and-output-kinds` | no |
| `030-bedrock-backend#I-7` | `invariant` | Served identity is explicit: the requested model is never copied into the served field, an echo of the requested id is not a served identity, and the processing region is recorded as unknown unless the response states it. | `3-8-served-identity` | no |
| `030-bedrock-backend#I-8` | `invariant` | Missing usage yields an `unknown` charge, never zero; usage times the bound price table is an `estimated` charge; and every live call is reserved against the persistent testing journal under R-29 before any request byte is sent. | `3-9-cost-and-spend` | no |
| `030-bedrock-backend#R-1` | `requirement` | Every Bedrock failure maps to exactly one code of spec 009's closed taxonomy, with the Bedrock error type retained in the detail. | `3-10-failure-taxonomy` | no |
| `030-bedrock-backend#R-2` | `requirement` | The reachability probe makes several calls under the live flag and the spend journal, and reports reachable only when every call succeeds. | `3-11-reachability-probe` | no |
| `030-bedrock-backend#R-3` | `requirement` | Later specs add `rank`, `generate`, `extract`, Guardrails screening, log-probability outputs and spec 020 policies only by admitting new values of members reserved here; a new member is `rustev.bedrock-binding/2`, and version 1 documents keep their digests. | `3-12-operations-added-by-later-specs` | no |
| `030-bedrock-backend#R-4` | `requirement` | The boundary check names the cloud SDK family and refuses it outside `integrations/` and anywhere in the graph of `rustev-contract` and `rustev-core`. | `3-13-boundary-check-extension` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo test -p rustev-bedrock --locked
cargo test -p rustev-bedrock --locked --features aws-config
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
| amends | `001-boundaries-and-authority` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `059-shared-remote-transport-by-dependency` |
| superseded by | None declared. |
| extended by | `035-screening-seam`, `039-model-backed-rank`, `043-live-re-execution-at-scale`, `050-wasm-without-parameterized-definitions`, `059-shared-remote-transport-by-dependency`, `067-node-live-repeat-runs` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:030-bedrock-backend` | `sha256:717d39a214e8a1b8b449ced74833ad005cfa8f53db9b463697a03e61ad1be81a` |
| `registry:030-bedrock-backend` | `sha256:717d39a214e8a1b8b449ced74833ad005cfa8f53db9b463697a03e61ad1be81a` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

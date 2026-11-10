<!-- spec-spine:begin generated id="header" -->
# `022-jev-production-transport` Production transport and served identity for the Jev adapter (amends 012)

- Path: `specs/022-jev-production-transport/spec.md`
- Summary: The separately reviewable spec 012 amendment that backlog item RUSTEV-010 and spec 012 R-3 require before production use: a Jev binding is production-eligible only when its transport pins a model version and every answered exchange proves the served identity against that pin. Defines the qualification of TypeSafe's direct API with a pinned version, the evidence under which Gateway pinning could be accepted instead, and the transport, TLS, timeout, connection, privacy and cost-reconciliation facts a production binding records. Development evidence with `served: unknown` is never relabeled as production evidence. Approved (A-21) with the additions of tracking issue #76 child C3 (section 3.8). Approval authorizes implementation of the no-network stage; live stages stay inside R-29's testing cap, and no deployment is authorized.
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
| created | `2026-09-27` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `022-jev-production-transport#I-1` | `invariant` | A binding is production-eligible only when its transport pins a model version and the pin is verified per answered exchange; a binding whose served identity can be `unknown` is refused for production at construction. | `3-1-production-eligibility` | no |
| `022-jev-production-transport#I-2` | `invariant` | An answered exchange whose observed served identity differs from the pin, or cannot be observed on a transport that promised it, is `identity_mismatch` and never supplies an output. | `3-2-direct-transport-with-a-pin` | no |
| `022-jev-production-transport#I-3` | `invariant` | Evaluation, calibration and qualification evidence produced under one binding identity is never reported as evidence for another; a change of transport, pin or mapping requires requalification. | `3-6-evidence-and-requalification` | no |
| `022-jev-production-transport#R-1` | `requirement` | Gateway pinning is accepted only after a recorded fact shows the Gateway both honors a requested version and reports the served version per response; documentation alone is not sufficient. | `3-3-gateway-pinning-as-an-alternative` | no |
| `022-jev-production-transport#R-2` | `requirement` | A production binding records its transport facts (endpoint, API version, TLS policy, timeouts inside the one attempt budget, connection behavior, privacy terms and cost source) and a reconciliation procedure between the spend journal and the provider's billing record. | `3-4-transport-facts-of-a-production-binding` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
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
cargo test -p rustev-jev --test production --locked
cargo test -p rustev-jev --test direct_recorded --locked
cargo test -p rustev-jev --locked
sh -c 'if grep -rnE "std::env|env::var|env!\(|option_env!" integrations/rustev-jev/src; then exit 1; fi'
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-jev --all-targets --locked -- -D warnings
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
| amends | `012-jev-integration` |
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
| `index:022-jev-production-transport` | `sha256:e6a284c3e6391283d5e73aa2b93ae7c020d278f3113af0c3558b7020cf9c3f2f` |
| `registry:022-jev-production-transport` | `sha256:e6a284c3e6391283d5e73aa2b93ae7c020d278f3113af0c3558b7020cf9c3f2f` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

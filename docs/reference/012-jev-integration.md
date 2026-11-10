<!-- spec-spine:begin generated id="header" -->
# `012-jev-integration` Jev integration (remote decision backend)

- Path: `specs/012-jev-integration/spec.md`
- Summary: A remote decision backend under `integrations/` that answers Rustev's `proposition`, `classify` and `rubric` steps by calling TypeSafe's Jev model, through the Vercel AI Gateway by default or TypeSafe's own API as an alternative transport, as an ordinary integration. It conforms to Part A of spec 009: explicit served identity (unknown on the Gateway), one total budget, unconfirmed cancellation once sent, closed error codes, estimated cost, requested privacy options, and exchange records. Many Rustev questions over one state go in one request. Provider confidence is recorded, never treated as calibrated. `rank` is not declared. No Jev wire-compatible surface is exposed. Paid inference for smoke and qualification is authorized by R-27 (zero data retention, synthetic and independently labeled data only); testing is capped at USD 10 in total (R-29, raised from USD 5 by A-75) and production at USD 25 per month (R-29), and production requires a pinned model version (R-28). Approved (A-09); implemented.
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
| `012-jev-integration#I-1` | `invariant` | The Jev adapter grants no authority: its outputs are model-derived raw outputs or classified failures, and nothing Jev returns changes a plan, policy, retry, fallback or budget. | `3-1-position-and-names` | no |
| `012-jev-integration#I-2` | `invariant` | Provider-reported confidence, argmax and interpolated score are recorded as provider-reported and are never supplied to the core or labeled calibrated. | `3-4-answers-into-rawoutput` | no |
| `012-jev-integration#I-3` | `invariant` | On the Gateway transport the served model identity is recorded as `unknown`; the requested model id or a routing slug is never recorded as the served identity. | `3-6-identity-routing-and-cost` | no |
| `012-jev-integration#I-4` | `invariant` | The adapter sends each attempt at most once, never retries or switches transport, model or provider on its own; any alternative is a separate adapter instance named as a runtime fallback target in the plan's execution policy. | `3-2-transports` | no |
| `012-jev-integration#I-5` | `invariant` | Rustev exposes no Jev wire-compatible endpoint, and no crate outside `integrations/` names Jev, TypeSafe or Vercel, except the recorded spend-journal header schema `rustev-jev.spend-journal/1`, which spec 043 3.5.1 keeps in `crates/rustev-live` because existing journal files carry it. | `3-1-position-and-names` | no |
| `012-jev-integration#I-6` | `invariant` | The Gateway API key is never copied into a repository, log, fixture, exchange record or other evidence. | `3-2-transports` | no |
| `012-jev-integration#R-1` | `requirement` | The descriptor declares `proposition` (2 options), `classify` (up to 255 options) and `rubric` (up to 10 levels) with output `distribution`, determinism `unspecified`, and no `rank`. | `3-3-questions-and-the-descriptor` | no |
| `012-jev-integration#R-2` | `requirement` | Live calls and paid inference happen only within R-27: smoke and qualification of this spec, key read from the owner's file at runtime and never copied, zero data retention requested, synthetic or independently labeled data only, within R-29's total testing cap, USD 10 since A-75 raised it from USD 5, enforced as a hard stop before dispatch; no quality claim precedes the R-04 dataset. | `3-8-qualification-plan` | no |
| `012-jev-integration#R-3` | `requirement` | Production binds only a transport with a pinned, verified model version: the direct TypeSafe API with a pin, or the Gateway once its pinning is verified and recorded; `served: unknown` is for development only. | `3-6-identity-routing-and-cost` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
- `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}`
- `{"kind":"file","path":"docs/design/discussions/002-user-jev-assessment-and-architecture.md"}`
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
cargo test -p rustev-jev --locked
sh -c 'if grep -rniE "jev|typesafe|vercel" crates tools backends --include=*.rs | sed "s/rustev-jev\.spend-journal\/1//g" | grep -iE "jev|typesafe|vercel"; then exit 1; fi'
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
| amends | None declared. |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `020-remote-protocol-refinements`, `022-jev-production-transport`, `044-runtime-optimization-increment-2`, `059-shared-remote-transport-by-dependency` |
| superseded by | None declared. |
| extended by | `007-support-routing-package`, `008-reference-package-lodging`, `020-remote-protocol-refinements`, `022-jev-production-transport`, `030-bedrock-backend`, `035-screening-seam`, `037-generation-steps`, `039-model-backed-rank`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `050-wasm-without-parameterized-definitions`, `059-shared-remote-transport-by-dependency` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:012-jev-integration` | `sha256:fea3a5d1d2930ad79fea74f0a380921c1b4fd331f5a362b19173c25df6641657` |
| `registry:012-jev-integration` | `sha256:fea3a5d1d2930ad79fea74f0a380921c1b4fd331f5a362b19173c25df6641657` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

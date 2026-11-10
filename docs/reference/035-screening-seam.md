<!-- spec-spine:begin generated id="header" -->
# `035-screening-seam` Untrusted-text screening seam (amends 002, 003 and 009)

- Path: `specs/035-screening-seam/spec.md`
- Summary: An amendment of approved specs 002, 003 and 009, for #76 child B7 (#65). A definition may declare a `screening` block that names untrusted text fields (caller text, third-party text, list item text), the screener backend that judges each one, a consequence on block (`refuse`, `drop_field`, `drop_text`, `drop_item`) and a consequence when the screener is unavailable (`fail_closed`, `skip_generation`, `fail_open`). The block compiles down to ordinary `proposition` steps, one text value per request, plus consequence rules evaluated before the selection policy; a screener is an ordinary backend that declares a `screen` profile (categories, maximum text bytes, text unit size) and uses the general per-capability `isolation: per_item` member this spec introduces. In a definition that declares screening, the compiler refuses a model step that reads unscreened untrusted text. A blocked value under `refuse` yields spec 033's `refused` outcome with its computed addressing. Adds a `screen_unavailable` unresolved reason, an explicit withheld marker in projections, per-value screening evidence with digests only, text-unit budgets, verdict caching through spec 017's cache, the reserved `rustev.screening` metric of spec 028, a per-item isolation rule for spec 009 remote adapters, and the mapping constraints for spec 030's reserved Bedrock screening values. New members go into the shared #76 track versions (`rustev.definition/2`, `rustev.plan/3`, `rustev.evidence/2`, `rustev.judgment/2`, `rustev.run/3`); definitions, descriptors and plans that use no #76 construct keep byte-identical identities. Approved (A-32); implementation is complete.
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
| `035-screening-seam#I-1` | `invariant` | Under every consequence except a declared `fail_open`, no model step request carries screened text whose verdict is not `pass`: blocked or unavailable text is withheld by the marker of 3.8 or the request is not dispatched. | `3-8-withheld-values-in-projections` | no |
| `035-screening-seam#I-2` | `invariant` | A request to a screener whose capability declares `isolation: per_item` carries exactly one text value per call under every batching, coalescing and dispatch setting of specs 009, 017 and 020. | `3-10-isolation-and-batching` | no |
| `035-screening-seam#I-3` | `invariant` | Definitions that use no #76 construct, descriptors without a `screen` profile, and plans compiled from them keep byte-identical canonical documents and identities, and their judgments, evidence and run records keep byte-identical bytes. | `3-18-compatibility-and-identity` | no |
| `035-screening-seam#I-4` | `invariant` | No screening evidence, notice, run record member or metric carries screened text; text is identified only by its tagged digest. | `3-13-evidence` | no |
| `035-screening-seam#R-1` | `requirement` | In a definition that declares screening, the compiler refuses with `unscreened_text` any model step whose projection can carry untrusted text not covered by a screening entry. | `3-5-compilation` | no |
| `035-screening-seam#R-2` | `requirement` | The compiler refuses `fail_open` on any field that accepts `user-supplied` or `authenticated-app-field`, whether or not the definition declares `caller_controlled`, or any other class the definition declares caller-controlled; and a screening refusal is addressed to the caller only when spec 033's computed party is `caller`. | `3-5-compilation` | no |
| `035-screening-seam#R-3` | `requirement` | The first blocked value under a `refuse` entry makes the judgment spec 033's `refused` outcome at once, and no request not yet dispatched for that decision is dispatched afterwards. | `3-7-screening-refusals` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `005-reference-backends` | `{"kind":"directory","path":"backends/rustev-backend-rules/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test screening_wire
cargo test -p rustev-core --locked --test screening_compile
cargo test -p rustev-core --locked --test screening_eval
cargo test -p rustev-runtime --locked --test screening
cargo test -p rustev-remote-http --locked --test screening
cargo test -p rustev-otel --locked
cargo test -p rustev-cli --locked --test screening
sh crates/rustev-eval/mutation/seeds.sh "spec 035"
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
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence`, `009-remote-adapter-protocol` |
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
| `index:035-screening-seam` | `sha256:f1011280a16176153d59b9c7a5244a283917687d8b1fa81949a85009dab37097` |
| `registry:035-screening-seam` | `sha256:f1011280a16176153d59b9c7a5244a283917687d8b1fa81949a85009dab37097` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

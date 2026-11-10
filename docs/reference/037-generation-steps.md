<!-- spec-spine:begin generated id="header" -->
# `037-generation-steps` Generation steps with deterministic backstops and per-field origin (amends 002, 003 and 004)

- Path: `specs/037-generation-steps/spec.md`
- Summary: An amendment of approved specs 002, 003 and 004 (R-16), for #76 child B5 (#63), under owner decisions R-41 (OD-2) and R-42 (OD-4). Defines the bounded model step envelope that later model steps reuse, and its first kind, `generate`: a declared, finite step that runs after selection and asks a backend for schema-bounded text fields per selected item, with a versioned prompt template bound into `PlanId`, a deterministic backstop for every required field written in a logic-less template language implemented in `rustev-core`, optional bounded repair, ownership checks, and per-field origin (`generated`, `repaired`, `backstop{reason}`) in the text result, the evidence, every capture and every cached copy. Generated text is model-derived and can never reach an exact operator, a policy control, an acquisition request or another model step. Generate steps add to the shared #76 versions `rustev.definition/2`, `rustev.plan/3` and `rustev.evidence/2`; plans without any #76 construct keep byte-identical identities and documents. Approved (A-34); implementation is complete; approval authorizes no live provider call, spend, release or real-user data.
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
| `037-generation-steps#I-1` | `invariant` | A definition without a generate step is unaffected by this spec: a version 1 definition compiles to the same plan bytes and PlanId under the same compiler identity, and evaluates to the same judgment, evidence, run record, capture and report bytes, as before. | `3-18-compatibility-and-identity` | no |
| `037-generation-steps#I-2` | `invariant` | No value produced by a generate step is readable by an exact step, a semantic or model step, a policy condition, outcome, parameter or adjustment, an execution policy, an acquisition request, a template parameter or a backstop; every such reference is refused at compile time. | `3-6-isolation-from-decisions` | no |
| `037-generation-steps#I-3` | `invariant` | Every required generated field has a backstop that compiles, reads only exact-derived values, and renders within the field's bounds; its output is exact-derived. | `3-4-backstop-language` | no |
| `037-generation-steps#I-4` | `invariant` | Per-field origin never changes the judgment: the judgment is final before the generation stage starts, a proposal whose text fell back is a successful proposal, and origin is never an Unresolved reason. | `3-10-per-field-origin-and-the-text-result` | no |
| `037-generation-steps#I-5` | `invariant` | A cached copy of generated output is served with its stored origin and with `served_from_cache` naming the original decision; a backstop or repaired field is never served as freshly generated. | `3-14-caching` | no |
| `037-generation-steps#R-1` | `requirement` | An output carrying a member that is not a declared generated field is rejected as an invalid output, and a carried context field or judgment that differs from its source at final verification is restored and reported with an `ownership_restored` notice. | `3-9-ownership` | no |
| `037-generation-steps#R-2` | `requirement` | A streamed field is provisional; a streamed value that is not the field's final value is followed by exactly one `superseded` event, and the final result, evidence and cache entry equal those of the same run without streaming. | `3-13-streaming` | no |
| `037-generation-steps#R-3` | `requirement` | A prompt template is an identified document embedded in the plan; any change to it changes `PlanId` and therefore every request identity and cache key derived from the plan. | `3-3-prompt-templates` | no |
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
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test generation_wire --test track_versions
cargo test -p rustev-core --locked --test template
cargo test -p rustev-core --locked --test generation_compile
cargo test -p rustev-core --locked --test generation_eval
cargo test -p rustev-core --locked --test identity --test reference_plans
cargo test -p rustev-runtime --locked --test generation
cargo test -p rustev-runtime --locked --lib generation_acceptance
cargo test -p rustev-eval --locked --test generation_replay
sh crates/rustev-eval/mutation/seeds.sh "spec 037"
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
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence`, `004-evaluation-and-replay` |
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
| `index:037-generation-steps` | `sha256:b46ea0d7eec9e6d37afe3d03cafd25acadd799731a36943c1938c91bdcc2631b` |
| `registry:037-generation-steps` | `sha256:b46ea0d7eec9e6d37afe3d03cafd25acadd799731a36943c1938c91bdcc2631b` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

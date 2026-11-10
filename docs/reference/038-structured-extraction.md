<!-- spec-spine:begin generated id="header" -->
# `038-structured-extraction` Structured extraction step and the model-derived input pattern (amends 002, 003 and 036)

- Path: `specs/038-structured-extraction/spec.md`
- Summary: An amendment of approved specs 002, 003 and 036 (R-16), for #76 child B6. Two supported ways to turn free text into typed decision inputs, both keeping extracted values model-derived. Pattern A: the host extracts and submits snapshot entries with provenance `model-derived` and an optional producer identity (artifact, template digest, production time), recorded in evidence and lineage. Pattern B: an `extract` step, a model step of spec 037's bounded envelope, asks a backend for a typed record against a declared schema and vocabularies under an identified prompt template; every field is validated and is present or absent with a reason, never coerced, clamped or truncated; each field carries its origin (`extracted`, `fallback`, `absent`) and derivation; a deterministic in-core fallback extractor (`rustev.extract-rules/1`) yields exact-derived fields; per-field distributions exist only from backend log-probabilities and are thresholded only under a bound calibration or a declared `uncalibrated_threshold`. In both patterns an exact operator, policy predicate, proposal parameter, acquisition request or other model step reads a model value only through a declared acceptance rule (an `accept_model_derived` step, or a spec 033 check covering `model-derived`); selection-policy controls never read one. Extraction can drive spec 033's `needs_input` outcome and propose which declared questions it asks. Adds to the shared #76 versions; every version 1 definition keeps its bytes, its plan and its `PlanId`. Approved (A-35); implementation is complete.
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
| `038-structured-extraction#I-1` | `invariant` | Every rustev.definition/1 document compiles to the same rustev.plan/2 bytes and PlanId as before this spec, with the same judgment, evidence and refusals; snapshot 1 documents keep their bytes and SnapshotId. | `3-14-compatibility-and-identity` | no |
| `038-structured-extraction#I-2` | `invariant` | An extracted field is either present with a value that satisfies its declared type, bounds and vocabulary exactly as received, or absent with a reason; no value is coerced, clamped, truncated, defaulted or repaired. | `3-4-field-validation-and-presence` | no |
| `038-structured-extraction#I-3` | `invariant` | A field filled by the model is model-derived; a field filled by the fallback extractor is exact-derived only when every source it read is exact-derived; acceptance never lowers a value's derivation class. | `3-5-origin-derivation-and-lineage` | no |
| `038-structured-extraction#I-4` | `invariant` | A per-field distribution exists only from log-probabilities the backend returned for every declared option, normalized by the core; a self-reported confidence is recorded as provider-reported and never becomes mass. | `3-6-per-field-distributions-and-calibration` | no |
| `038-structured-extraction#I-5` | `invariant` | In a version 2 definition, no exact operator, policy predicate, proposal parameter, acquisition request or other model step reads a model value except through a declared acceptance rule, and no selection-policy control reads one at all; the compiler refuses every other read with unaccepted_model_value. | `3-9-the-model-value-gate` | no |
| `038-structured-extraction#R-1` | `requirement` | The fallback extractor is a pure, bounded, identified program evaluated in rustev-core with identical results on every target, and its identity is part of the PlanId. | `3-7-the-fallback-extractor` | no |
| `038-structured-extraction#R-2` | `requirement` | An accept_model_derived step declares its validation and its consequence on failure, applies them to every model-derived value it reads, passes caller and system values through unchanged, and emits a field_dropped notice with its computed action for every dropped value. | `3-8-acceptance-steps` | no |
| `038-structured-extraction#R-3` | `requirement` | Snapshot producers are accepted only on model-derived entries and are recorded per input and in the lineage of every value that read the input. | `3-2-pattern-a-host-side-extraction` | no |
| `038-structured-extraction#R-4` | `requirement` | Questions of a needs_input outcome are always declared templates; an extraction may only select among a declared set, through an accept step, and never supplies question text. | `3-13-needs-input-from-extraction` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-aicortex/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test extraction_wire --test extraction_documents --test extraction_results --test track_versions
cargo test -p rustev-core --locked --test extraction_validate
cargo test -p rustev-core --locked --test extraction_fallback
cargo test -p rustev-core --locked --test extraction_compile --test extraction_contract
cargo test -p rustev-core --locked --test extraction_evaluate
cargo test -p rustev-core --locked --test identity --test reference_plans --test replay_identity
cargo test -p rustev-runtime --locked --test extraction
cargo test -p rustev-pkg-lodging --locked --test extraction
cargo test -p rustev-eval --locked --test origin_041 extraction_accuracy_and_absence_compare_retained_values_with_gold
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
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence`, `036-mid-plan-acquisition` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `070-extract-rules-2` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:038-structured-extraction` | `sha256:ce3dffa7da5c545e90bddcb7886b1ad5a5ddd0b97a03993ee4291b3e8a045c33` |
| `registry:038-structured-extraction` | `sha256:ce3dffa7da5c545e90bddcb7886b1ad5a5ddd0b97a03993ee4291b3e8a045c33` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

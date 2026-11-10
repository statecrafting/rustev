<!-- spec-spine:begin generated id="header" -->
# `070-extract-rules-2` Extraction rules 2: token extractors, exact-value conditions, declared date formats and rules first (amends 038 and 032)

- Path: `specs/070-extract-rules-2/spec.md`
- Summary: An amendment of approved specs 038 (3.3.3, 3.3.5, 3.5, 3.7, 3.11, 3.12, 3.14) and 032 (3.2.1, 3.9.3), for #572, ratified by the owner on 2026-11-08 (A-90) for release 0.7.0. A host that runs per-sender templates over forwarded booking emails cannot express them in `rustev.extract-rules/1`, so it runs them as a host backend and every templated field becomes `model-derived`. This spec adds the program schema `rustev.extract-rules/2`, a superset of /1: the value extractors `token_after` (a token from a declared ASCII charset after a term), `token_matching` (a token of a small structured shape, never a regular expression), `date` (a closed set of declared formats with month names from a table the program declares) and `vocabulary_after` (a token that is a member of a declared vocabulary, graded `hint` under spec 032); the conditions `equals` and `one_of` over an exact input value; and the extract step's `fill: rules_first`, which evaluates the program before the request is listed, skips the request when every required field is filled, and otherwise asks the backend only for the fields the rules left absent. Everything stays pure, bounded and evaluated in `rustev-core` with no new dependency. A rules match is evidence that this program produced this value from this text, never that the value is true. /1 programs, every existing plan and every `PlanId` keep their bytes.
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
| created | `2026-11-08` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `070-extract-rules-2#I-1` | `invariant` | Every rustev.extract-rules/1 document keeps its bytes, its identity and its meaning; every committed definition, plan, evidence and judgment golden keeps its bytes and PlanId; a /1 document holding any /2 construct is refused at parse, never read as /2. | `3-10-compatibility-and-identity` | no |
| `070-extract-rules-2#I-2` | `invariant` | A value extractor yields exactly the bytes it matched, or nothing: a token longer than its declared bound, outside its charset or shape, or a calendar-invalid date makes the rule not apply; nothing is cut short, case-changed, trimmed, padded beyond the declared date rendering, or defaulted, and the produced value is validated by spec 038 3.4 like any fallback value. | `3-3-value-extractors` | no |
| `070-extract-rules-2#I-3` | `invariant` | An `equals` or `one_of` condition reads only an input whose accepted provenance excludes `model-derived`, compares its core value exactly, and is never evaluated as false because its source is unresolved: the field stays absent with `source_unavailable`. | `3-4-exact-value-conditions` | no |
| `070-extract-rules-2#I-4` | `invariant` | Under `fill: rules_first`, a field the rules filled is never requested from or replaced by the backend; a request skipped because every required field was filled is recorded `skipped{rules_filled}` with no attempt, no reservation and no cost; and compile-time budgets count the request as before. | `3-7-rules-first` | no |
| `070-extract-rules-2#I-5` | `invariant` | Every present value of an extract field that a `vocabulary_after` rule can fill carries grade `hint`, its static ceiling is `hint`, and only a spec 032 corroboration step can raise it; a hard gate or requirement reading it otherwise is refused with category 14 `uncorroborated_hint`. | `3-8-grade-of-vocabulary-capture` | no |
| `070-extract-rules-2#R-1` | `requirement` | The /2 evaluator is pure, bounded and in rustev-core, gives byte-identical results on every target including wasm32, uses no regular expression engine, Unicode table, locale data, floating point or new dependency, and stays within the per-byte work bounds of 3.6. | `3-6-evaluation-bounds-and-determinism` | no |
| `070-extract-rules-2#R-2` | `requirement` | The optimized WebAssembly module stays within spec 057's gated uncompressed budget; an increment of this spec that crosses 5,242,880 bytes raises the gate to 6,291,456 bytes in the same change, as 3.11 amends 027 3.10.4. | `3-11-webassembly-budget` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/types/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/types/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test extract_rules_2 --test extraction_documents --test schemas
cargo test -p rustev-core --locked --test extraction_rules_2 --test extraction_compile --test extraction_fallback
cargo test -p rustev-core --locked --test extraction_rules_2_grade
cargo test -p rustev-core --locked --test extraction_rules_first --test extraction_evaluate
cargo test -p rustev-core --locked --test reference_plans --test packaged_fixtures --test identity
cargo test -p rustev-runtime --locked --test extraction
cargo test -p rustev-conformance --locked --test runtime --test manifest
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
| amends | `032-evidence-quality`, `038-structured-extraction` |
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
| `index:070-extract-rules-2` | `sha256:8c17fbe21537d2851ec1c48e7fe0f0bd279ee908c10b26aeeaeebdc30c189468` |
| `registry:070-extract-rules-2` | `sha256:8c17fbe21537d2851ec1c48e7fe0f0bd279ee908c10b26aeeaeebdc30c189468` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

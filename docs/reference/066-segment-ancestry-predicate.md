<!-- spec-spine:begin generated id="header" -->
# `066-segment-ancestry-predicate` Definition predicate: segment ancestry on a declared separator (amends 002 and 032)

- Path: `specs/066-segment-ancestry-predicate/spec.md`
- Summary: An amendment of approved specs 002 (3.8, 3.9) and 032 (3.1, 3.12), for release 0.6.0 (#509), ratified by the owner on 2026-11-06 (A-85). A partner rule needs "this location slug sits under that city slug", for example `us:ny:new-york:soho` under `us:ny:new-york`; today the host projects the slug's parent into the snapshot and the rule compares with `member_ref`. This spec adds one predicate to the registry `rustev.exact/2`, `under{value, ancestor, separator}`, true when the ancestor's segments are a proper prefix of the value's segments on a separator the definition declares, so `us:ny:new-york-city` is not under `us:ny:new-york`. Comparison is by bytes with no normalization; an operand with an empty segment is `invalid_input`, and an unresolved operand makes the reading step unresolved, never false. Definition 1 refuses it; plans that do not use it keep their bytes and `PlanId`.
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
| created | `2026-11-06` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `066-segment-ancestry-predicate#I-1` | `invariant` | `under` is true only when both operands are valid segmented text on the declared separator and the ancestor's segments are a proper prefix of the value's segments, compared byte for byte; it is never true for a text prefix that ends inside a segment, and no case folding, Unicode normalization or trimming is applied. | `3-2-semantics` | no |
| `066-segment-ancestry-predicate#I-2` | `invariant` | An operand that is unresolved makes the step or condition reading `under` unresolved with that operand's reason (spec 002 3.10.4), and an operand that is empty or has an empty segment is `invalid_input`; neither is ever evaluated as false. | `3-3-unresolved-and-invalid-operands` | no |
| `066-segment-ancestry-predicate#R-1` | `requirement` | Every committed definition, plan, judgment, evidence and run golden keeps its bytes and `PlanId`, and a `rustev.definition/1` using `under` is refused with category 11 `invalid_definition`, requirement `requires_definition_2`. | `3-5-compatibility` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/mutation/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/types/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/types/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test segment_ancestry --test schemas
cargo test -p rustev-core --locked --test segment_ancestry
cargo test -p rustev-core --locked --test acquisition_eval
cargo test -p rustev-core --locked --test reference_plans --test packaged_fixtures --test identity
sh crates/rustev-eval/mutation/seeds.sh "spec 066"
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
| amends | `002-decision-contract-and-pure-core`, `032-evidence-quality` |
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
| `index:066-segment-ancestry-predicate` | `sha256:fb09b11424c7667afd09ab655306e9e42956dfd6c880a0717be0a36148aaa92d` |
| `registry:066-segment-ancestry-predicate` | `sha256:fb09b11424c7667afd09ab655306e9e42956dfd6c880a0717be0a36148aaa92d` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

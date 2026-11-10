<!-- spec-spine:begin generated id="header" -->
# `032-evidence-quality` Evidence quality: grades, presence predicates, typed joins, corroboration and conflict policy (amends 002 and 003)

- Path: `specs/032-evidence-quality/spec.md`
- Summary: An amendment of approved specs 002 and 003 (R-16), for #76 child B2. Adds an ordered evidence grade (`authoritative`, `cached`, `estimated`, `hint`) and value metadata (grade, provenance set, sources, oldest as-of) to every value of a plan that opts in, declared per field by a new context source descriptor (`rustev.source/1`) and lowered, never raised, by the definition; presence predicates so key existence is never presence; typed keyed joins (`join_keyed@1`) and per-field merges (`merge_fields@1`) with no implicit fallback; a corroboration step, the only way a grade rises; and a per-fact conflict policy that makes silent last-writer-wins impossible. Introduces the shared #76 track versions `rustev.definition/2`, `rustev.plan/3`, `rustev.snapshot/2`, `rustev.evidence/2`, `rustev.run/3` and the registry `rustev.exact/2`, to which later #76 specs add optional members; every version 1 definition keeps its bytes, its plan and its `PlanId`. Approved (A-29); implemented in four increments (see the implementation record).
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
| `032-evidence-quality#I-1` | `invariant` | Every rustev.definition/1 document compiles to the same rustev.plan/2 bytes and PlanId as before this spec, evaluates to the same judgment and rustev.evidence/1 bytes, and no existing refusal changes category or order. | `3-12-compatibility` | no |
| `032-evidence-quality#I-2` | `invariant` | A value's grade is never higher than the minimum of its source ceiling, its definition cap, its snapshot claim and the grades of the graded values it was computed from, except through a corroboration step whose supporting checks read values of at least the upgraded grade. | `3-2-evidence-grades-and-value-metadata` | no |
| `032-evidence-quality#I-3` | `invariant` | A map key, list item or record that fails its presence predicate is absent, and reading an absent value is missing_evidence; key existence alone is never presence. | `3-4-presence-predicates` | no |
| `032-evidence-quality#I-4` | `invariant` | merge_fields@1 never takes a base field the field's rule does not name; a field its rule cannot fill is absent, never a fallback value. | `3-6-typed-merge-merge-fields-1` | no |
| `032-evidence-quality#R-1` | `requirement` | The compiler refuses a hard gate, or a grade requirement, that reads a value whose static grade ceiling is below the required grade, with category 13 grade_requirement, or category 14 uncorroborated_hint when the ceiling is hint and no corroboration step raises it. | `3-9-grade-compilation-phase` | no |
| `032-evidence-quality#R-2` | `requirement` | Every disagreement between accepted snapshot entries and every corroboration contradiction is resolved only by a declared rule and recorded in rustev.evidence/2 with each value's digest, source and grade. | `3-8-conflict-policy` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test track_versions
cargo test -p rustev-core --locked --test track_versions --test evidence_quality_compile --test evidence_quality_eval
cargo test -p rustev-core --locked --lib
cargo test -p rustev-runtime --locked --test plan_3
cargo test -p rustev-pkg-lodging --locked --test evidence_quality
cargo test -p rustev-core --locked
cargo test -p rustev-pkg-lodging --locked
sh crates/rustev-core/mutation/seeds.sh
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
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `066-segment-ancestry-predicate`, `070-extract-rules-2` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:032-evidence-quality` | `sha256:ac73ceb52ddaf8566648a48fbb79674f87091d24c3551d5e75b749f16ad5524c` |
| `registry:032-evidence-quality` | `sha256:ac73ceb52ddaf8566648a48fbb79674f87091d24c3551d5e75b749f16ad5524c` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

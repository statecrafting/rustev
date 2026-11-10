<!-- spec-spine:begin generated id="header" -->
# `033-provenance-rules-and-actionable-outcomes` Provenance-conditional rules and actionable outcomes (amends 002 and 006)

- Path: `specs/033-provenance-rules-and-actionable-outcomes/spec.md`
- Summary: An amendment of approved specs 002 and 006, for #76 child B3. A definition may declare which provenance classes the caller controls, input checks whose consequence on failure depends on the provenance of the value tested (refuse, drop the field, relax a named constraint group, downgrade its grade, abstain, escalate, or continue with a notice), exhaustive over the classes the input accepts, and policy conditions over provenance. Every unresolved outcome, refusal and notice of such a plan carries who can act on it, a typed remediation, its causes with provenance, and whether retrying can succeed; who can act is computed from the causes, and an author may only move it away from the caller, never toward it. Adds the `needs_input` outcome with declared, literal questions and a continuation digest, and documents how hosts map outcomes to transport semantics. Adds to the #76 track versions `rustev.definition/2`, `rustev.plan/3` and `rustev.evidence/2`, and introduces `rustev.judgment/2`; plans using no construct of this spec keep their bytes and `PlanId`. Approved (A-30); implemented, with the Node binding's conformance item awaiting spec 027.
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
| `033-provenance-rules-and-actionable-outcomes#I-1` | `invariant` | A definition that uses no construct of this spec compiles to the same plan bytes and PlanId as before this spec, and its decisions produce the same rustev.judgment/1, evidence and run record bytes; no existing refusal changes category or order. | `3-12-compatibility` | no |
| `033-provenance-rules-and-actionable-outcomes#I-2` | `invariant` | An outcome or notice is addressed to the caller only when every cause is an input whose provenance class the definition declares caller-controlled; no declaration, override or remediation can address the caller otherwise, and the compiler refuses any that would. | `3-5-actionable-outcomes` | no |
| `033-provenance-rules-and-actionable-outcomes#I-3` | `invariant` | A check over an input that accepts several provenance classes covers each accepted class with a branch or a default; otherwise the definition is refused naming the uncovered classes. | `3-2-checks` | no |
| `033-provenance-rules-and-actionable-outcomes#I-4` | `invariant` | A dropped field is absent, read as missing_evidence by every dependent; no consequence substitutes a value, and no check result is a default. | `3-3-consequences` | no |
| `033-provenance-rules-and-actionable-outcomes#R-1` | `requirement` | Every question of a needs_input outcome is a declared literal template, byte-identical to its declaration; no model output or input value enters a question's text. | `3-7-the-needs-input-outcome` | no |
| `033-provenance-rules-and-actionable-outcomes#R-2` | `requirement` | transient is computed from the fixed table of 3.6 and is true only for backend_unavailable, deadline_exceeded, and invalid_backend_output from a backend whose determinism is not bitwise. | `3-6-remediations-and-transience` | no |
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
cargo test -p rustev-contract --locked --test actionability_wire
cargo test -p rustev-core --locked --test actionability_versions --test actionability_compile
cargo test -p rustev-core --locked --test actionability_eval --test actionability_rows
cargo test -p rustev-core --locked --test actionability_origin
cargo test -p rustev-runtime --locked --test actionability_origin
cargo test -p rustev-cli --locked --test actionability
cargo test -p rustev-core --locked
cargo test -p rustev-cli --locked
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
| amends | `002-decision-contract-and-pure-core`, `006-cli-surface` |
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
| `index:033-provenance-rules-and-actionable-outcomes` | `sha256:29bb2d7153771ef1fd2138135191460e382bba0d4633a15b02a4e2b5ed7f9c52` |
| `registry:033-provenance-rules-and-actionable-outcomes` | `sha256:29bb2d7153771ef1fd2138135191460e382bba0d4633a15b02a4e2b5ed7f9c52` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

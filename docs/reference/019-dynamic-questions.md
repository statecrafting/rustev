<!-- spec-spine:begin generated id="header" -->
# `019-dynamic-questions` Dynamic questions as a narrow evaluated class

- Path: `specs/019-dynamic-questions/spec.md`
- Summary: A separate, explicitly invoked class for one bounded proposition, classification, or rubric question over caller-selected context. Dynamic questions use a dedicated backend capability, structured data-only inputs, fixed request-time labels or levels, one attempt, hard cost and time limits, explicit abstention, model-derived lineage, and class-specific evaluation. They do not alter a registered plan, supply a value into a running plan, create calibration or probability by implication, invoke tools, or grant authority.
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
| created | `2026-09-26` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `019-dynamic-questions#I-1` | `invariant` | A dynamic question is a separate request and outcome class; it cannot mutate a registered plan, satisfy a pending plan request, or enter a plan evaluation without a later explicit, separately governed snapshot input. | `3-1-separate-class-and-api` | no |
| `019-dynamic-questions#I-2` | `invariant` | Question, labels, levels, and context are bounded structured data, never instructions to Rustev, a tool invocation, an authorization, or a source of new capability. | `3-3-data-only-and-injection-resistant` | no |
| `019-dynamic-questions#I-3` | `invariant` | A backend answers a dynamic question only when it separately declares the exact dynamic operation, output kind, bounds, artifact identity, and data-handling posture; ordinary registered-plan capability is insufficient. | `3-4-dynamic-capability-disclosure` | no |
| `019-dynamic-questions#I-4` | `invariant` | Every dynamic request has one attempt under a hard declared cost cap and deadline; unknown exposure, missing capability, invalid output, ambiguity, or insufficient support yields refusal or abstention, never a default label. | `3-5-execution-budgets-and-abstention` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test dynamic_wire
cargo test -p rustev-core --locked --test dynamic
cargo test -p rustev-core --locked --doc dynamic
cargo test -p rustev-runtime --locked --test dynamic
cargo test -p rustev-eval --locked --test dynamic
cargo test -p rustev-fp-corpus --locked --test corpus
cargo run -p rustev-boundaries --locked --quiet
sh -c '! grep -nE "std::(fs|net|process|env|time)|tokio|async fn" crates/rustev-contract/src/dynamic.rs crates/rustev-core/src/dynamic.rs'
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
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
| `index:019-dynamic-questions` | `sha256:6eb409ed4bbb5b69d8ebe8d0142ff87c51a1aaf7ba86421d867ba125fce3dc1f` |
| `registry:019-dynamic-questions` | `sha256:6eb409ed4bbb5b69d8ebe8d0142ff87c51a1aaf7ba86421d867ba125fce3dc1f` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

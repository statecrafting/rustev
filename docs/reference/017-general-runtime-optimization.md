<!-- spec-spine:begin generated id="header" -->
# `017-general-runtime-optimization` General runtime batching, duplicate suppression, and caches

- Path: `specs/017-general-runtime-optimization/spec.md`
- Summary: A separately reviewable amendment to runtime execution for general runtime-planned batching, concurrent duplicate suppression, bounded memory caches, and opt-in persistent caches. Optimized execution must be judgment equivalent to independent uncached execution, isolate authorized scopes, preserve each waiter's deadline and cancellation, attribute one charge and every liability exactly once, invalidate on correction, revocation and erasure, and record every shared-call or cache effect. Existing adapter-scoped batching remains a special case, not evidence that this general contract is implemented or qualified.
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
| `017-general-runtime-optimization#I-1` | `invariant` | For the same plan, snapshot, evaluation time and validated backend values, optimization produces the same judgment and core evidence as independent execution; it changes only runtime observations explicitly recorded by this spec. | `3-1-equivalence-and-plan-identity` | no |
| `017-general-runtime-optimization#I-2` | `invariant` | No batch, shared call or cache entry crosses an authorized scope, tenant, principal, artifact, question, preprocessing, policy or other output-affecting identity boundary. | `3-2-complete-request-equivalence` | no |
| `017-general-runtime-optimization#I-3` | `invariant` | Correction, revocation or erasure invalidates every affected cache entry and in-flight result; a result arriving after invalidation is neither stored nor served to a later request. | `3-7-invalidation-erasure-and-races` | no |
| `017-general-runtime-optimization#I-4` | `invariant` | One external attempt has one charge owner and one liability record; sharing never duplicates, hides, refunds or reallocates an unknown charge by inference. | `3-5-budgets-cost-and-liability` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo clippy -p rustev-contract -p rustev-core -p rustev-runtime -p rustev-eval -p rustev-cli --all-targets --locked -- -D warnings
cargo test -p rustev-runtime --test optimization_benchmark --locked -- --ignored
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
| amends | `003-runtime-execution-and-evidence` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `044-runtime-optimization-increment-2` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:017-general-runtime-optimization` | `sha256:40d3d2870f2be1da3bc18b88d4a747bf4dff476db59065475cf474984ed88795` |
| `registry:017-general-runtime-optimization` | `sha256:40d3d2870f2be1da3bc18b88d4a747bf4dff476db59065475cf474984ed88795` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

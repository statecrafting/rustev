<!-- spec-spine:begin generated id="header" -->
# `040-decision-explanation` Decision explanation API and `rustev explain` (amends 004 and 006)

- Path: `specs/040-decision-explanation/spec.md`
- Summary: An amendment of approved specs 004 and 006 (R-16), for #76 child D2. A pure function over recorded evidence (a delivered `rustev.run/1` record, or a `rustev.replay/1` bundle, with the plan document when supplied) answers seven structured questions: why an item was selected, why one was excluded, why one item precedes another, why the outcome is what it is, where text came from, which models answered at what cost, and where two decisions first diverge. Answers are written as a versioned `rustev.explanation/1` document in which every fact cites the evidence path it was copied from and carries the derivation class and lineage its source records; a fact the evidence does not hold, or holds only as a digest, or that a supplied redaction policy withholds, is `not_available` with a typed reason, never a guess. Nothing is re-executed, compiled or dispatched and no model is called. Adds a deterministic Markdown rendering, `rustev explain` in the CLI, and the binding surface for the Node and WebAssembly packages and the service. No existing document, identity or CLI output changes. Approved (A-37); implementation is pending.
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
| `040-decision-explanation#I-1` | `invariant` | Explaining performs no re-execution: it starts no evaluation, compiles or recompiles no plan, supplies no value, constructs or calls no backend, calls no model, performs no I/O and reads no clock; the CLI and the bindings read files and resolve bundle items before the call. | `3-4-the-explainer` | no |
| `040-decision-explanation#I-2` | `invariant` | Every available fact in an explanation is a copy of, or a comparison stated over, values at the evidence paths it cites, and every cited path resolves in the supplied inputs; a fact the inputs do not hold is not_available with a typed reason, never a default, an inference or a recomputed value. | `3-5-facts-availability-derivation-and-lineage` | no |
| `040-decision-explanation#I-3` | `invariant` | Every fact carries the derivation class and lineage its source records, or an upper bound marked as such; a statement computed over cited values is exact-derived only when every compared value is exact-derived, and is never less derived than a value it reads. | `3-5-facts-availability-derivation-and-lineage` | no |
| `040-decision-explanation#I-4` | `invariant` | Under a supplied redaction policy, no value whose effective action is other than allow appears in the explanation or its rendering, raw or transformed; it is not_available with reason redacted and the policy identity. | `3-13-redaction` | no |
| `040-decision-explanation#I-5` | `invariant` | No existing document, schema, canonical byte, plan identity, CLI output or exit code changes; every existing plan, definition, run, replay and CLI golden stays byte-identical. | `3-18-compatibility` | no |
| `040-decision-explanation#R-1` | `requirement` | diff names as its first divergence the earliest difference in the fixed comparison order of 3.11, and for two replayed records that differ in one backend output names that step and instance with reason backend-output-changed. | `3-11-diff` | no |
| `040-decision-explanation#R-2` | `requirement` | The same evidence, query and policy produce byte-identical explanation documents and renderings through the library, the CLI, the native binding and the WebAssembly package. | `3-15-library-surface-bindings-and-service` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
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
cargo test -p rustev-eval --locked --test explain --test explain_models --test explain_selected --test explain_excluded --test explain_order --test explain_diff --test explain_render
cargo test -p rustev-cli --locked --test explain
cargo test -p rustev-surface --locked --test explain
cargo test -p rustev-conformance --locked
sh crates/rustev-eval/mutation/seeds.sh "spec 040"
sh crates/rustev-cli/mutation/seeds.sh explain040
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
| amends | `004-evaluation-and-replay`, `006-cli-surface` |
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
| `index:040-decision-explanation` | `sha256:55528fafdf682d51029f9e2aebda379a888ab23182e5f84fbe4082001a29320c` |
| `registry:040-decision-explanation` | `sha256:55528fafdf682d51029f9e2aebda379a888ab23182e5f84fbe4082001a29320c` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

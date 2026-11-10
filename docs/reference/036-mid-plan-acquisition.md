<!-- spec-spine:begin generated id="header" -->
# `036-mid-plan-acquisition` Mid-plan context acquisition and tiered widening (amends 002, 003 and 004)

- Path: `specs/036-mid-plan-acquisition/spec.md`
- Summary: An amendment of approved specs 002, 003 and 004 (R-16), for #76 child B1 under owner decision R-40 (OD-3 of #76). A plan may declare bounded acquisition steps that ask a host-implemented acquisition source for more context between steps, with requests built only from exact values and literals, and a tiered widening construct that tries a finite, declared list of increasingly broad requests (optionally across sources) until a stop condition holds, never relaxing protected constraints. Paging is bounded by pages, items and a stop condition. Adds acquisition operations to spec 032's source descriptor (`rustev.source/1`), acquisition request and page documents, the `compose_key@1` operator, a typed `skipped` outcome, compile-time call, item, byte, deadline and cost budgets, and the in-process `AcquisitionSource` seam a later remote protocol (#76 A2) carries. Every page is recorded and captured by request identity; offline replay never dispatches and an uncaptured request is `missing_evidence`. Adds to the #76 track versions introduced by spec 032 (source 1, definition 2, plan 3, evidence 2, run 3, exact 2), introduces the track version `rustev.replay/2`, and admits every plan 3 decision to capture and replay, lifting spec 032's interim `capture_unsupported` refusal. Plans that use no #76 construct keep byte-identical documents and `PlanId`s. Approved (A-33); implementation is complete.
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
| `036-mid-plan-acquisition#I-1` | `invariant` | A definition that uses no construct of this spec is a `rustev.definition/1` document and compiles to a `rustev.plan/2` document byte-identical to today's, with the same `PlanId`; its run, evidence and replay documents keep their existing schemas and bytes. | `3-16-compatibility` | no |
| `036-mid-plan-acquisition#I-2` | `invariant` | Every acquisition request, guard, stop count and dedupe input is computed only from values whose static derivation class is exact-derived, or from literals; anything else is refused at compile time. | `3-9-compilation-refusals-and-budget` | no |
| `036-mid-plan-acquisition#I-3` | `invariant` | Offline replay and candidate comparison never call an acquisition source; an acquisition request with no captured page is supplied as `missing_evidence` and the case is never reported as reproduced. | `3-15-capture-and-offline-replay` | no |
| `036-mid-plan-acquisition#I-4` | `invariant` | A skipped, empty, failed or truncated acquisition is never presented as an empty or default value: each has its own typed outcome, visible to the policy and recorded in evidence. | `3-8-skipped-values-and-policy-handling` | no |
| `036-mid-plan-acquisition#R-1` | `requirement` | A widening never sends a request in which a field of a protected constraint group differs from the base request; a definition that could do so is refused with invalid_acquisition. | `3-5-the-widen-construct` | no |
| `036-mid-plan-acquisition#R-2` | `requirement` | The worst-case number of acquisition calls, retained items, response bytes, deadline share and, under a hard cost policy, cost units of a plan is computed at compile time and a plan exceeding a declared ceiling is refused with category 7. | `3-9-compilation-refusals-and-budget` | no |
| `036-mid-plan-acquisition#R-3` | `requirement` | Given the same plan, snapshot, evaluation time and supplied pages and outputs, the core produces byte-identical judgments and evidence regardless of the order in which independent acquisitions complete. | `3-11-staged-evaluation-in-the-core` | no |
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
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test acquisition_wire --test acquisition_steps --test acquisition_records --test acquisition_cache
cargo test -p rustev-core --locked --test acquisition_structure --test acquisition_compile
cargo test -p rustev-core --locked --test compose_key --test acquisition_eval
cargo test -p rustev-core --locked --lib expr::
cargo test -p rustev-runtime --locked --test acquisition
cargo test -p rustev-eval --locked --test acquisition_replay
cargo test -p rustev-cli --locked --test plan
cargo test -p rustev-pkg-lodging --locked --test acquisition --test evidence_quality
sh crates/rustev-eval/mutation/seeds.sh "spec 036"
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
| amended by | `038-structured-extraction`, `063-node-binding-parity` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:036-mid-plan-acquisition` | `sha256:f60fb0290402905af30318b2938d4026cbf59b8e64e83bf4796f348d00251fe0` |
| `registry:036-mid-plan-acquisition` | `sha256:f60fb0290402905af30318b2938d4026cbf59b8e64e83bf4796f348d00251fe0` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `001-boundaries-and-authority` Ecosystem boundaries, proposals versus authority, and product principles

- Path: `specs/001-boundaries-and-authority/spec.md`
- Summary: What Rustev owns and refuses to own within the ecosystem (spec-spine, Aicortex, Rahi, statecraft-cli, applications), the rule that Rustev produces proposals and never grants, the provenance classes of Rustev outputs, the dependency layout that keeps contracts transport-neutral and the core free of integrations, and the product principles that are the constitution's principles VI onward. Owns the workspace manifest, the Makefile and the boundary check that enforces the dependency rules.
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
| created | `2026-09-23` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"anchor":"vi-onward-the-principles-of-the-system-you-are-specifying","file":"standards/spec/constitution.md","kind":"section"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"scripts/landing/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"file","path":"Makefile"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}` | no | `resolved` |

### References (non-owning)

None declared.
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
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 'for d in crates tools; do [ -d "$d" ] || continue; if grep -rnE "(struct|enum|trait|type)[[:space:]]+(Permitted|Grant)([^A-Za-z0-9_]|$)" "$d"; then exit 1; fi; done'
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
| amends | None declared. |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `011-semantic-backend`, `018-statecraft-profile-14`, `028-opentelemetry-evidence-sink`, `030-bedrock-backend`, `031-public-api-and-release-family`, `049-opentelemetry-through-the-node-binding`, `059-shared-remote-transport-by-dependency` |
| superseded by | None declared. |
| extended by | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence`, `004-evaluation-and-replay`, `005-reference-backends`, `006-cli-surface`, `007-support-routing-package`, `008-reference-package-lodging`, `009-remote-adapter-protocol`, `010-integrations`, `011-semantic-backend`, `012-jev-integration`, `013-remote-state-on-failed-attempts`, `014-report-integrity`, `015-per-case-bundle-failures`, `017-general-runtime-optimization`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `022-jev-production-transport`, `026-live-re-execution`, `027-node-and-wasm-bindings`, `028-opentelemetry-evidence-sink`, `029-evaluation-repeatability`, `030-bedrock-backend`, `031-public-api-and-release-family`, `032-evidence-quality`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `041-ranking-and-rubric-metrics`, `042-decision-service-and-remote-seams`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `045-command-surface-release-layer`, `047-target-independent-transcendentals`, `049-opentelemetry-through-the-node-binding`, `050-wasm-without-parameterized-definitions`, `059-shared-remote-transport-by-dependency`, `060-the-service-image-in-the-release`, `061-host-scored-repeat-sets`, `063-node-binding-parity`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity`, `066-segment-ancestry-predicate`, `067-node-live-repeat-runs`, `068-qualification-requires-npm-provenance`, `069-node-host-nonblocking-open`, `070-extract-rules-2`, `071-generated-spec-reference` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:001-boundaries-and-authority` | `sha256:eac3bef4c7300502607d426a0395257554a78485fc0d3d3cc7a76da38159712d` |
| `registry:001-boundaries-and-authority` | `sha256:a72b036437b5261e08b4697cb8986cf1aeb401a588dcfafd52167eb5f72f1687` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

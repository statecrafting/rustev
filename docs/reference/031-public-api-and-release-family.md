<!-- spec-spine:begin generated id="header" -->
# `031-public-api-and-release-family` Public API facade, compatibility policy and release family (amends 021)

- Path: `specs/031-public-api-and-release-family/spec.md`
- Summary: Amends spec 021 so the first public release becomes a release family: the existing eight crates plus `rustev-pkg-lodging`, a facade crate `rustev` with no default features and one optional feature per adoptable slice, further integration crates as each lands, native and WebAssembly npm packages, a multi-arch service container image and prebuilt CLI binaries. Fixes one compatibility policy for every public surface (Rust API, documents, protocols, npm packages, service API, PlanId, MSRV), a supply chain of CycloneDX SBOMs, `cargo deny`, reproducible package checks, signed tags, signed artifacts with stated SLSA build provenance, npm provenance where the registry accepts it, a changelog generated from the governed corpus, and `cargo semver-checks` against the previous release. Narrowly amends spec 001 3.4.4, spec 010 I-2 and spec 008 for the facade and lodging. Approved (A-27) under R-39. Approval authorizes implementation and candidate evidence; publication of the first release is authorized only by the conditional owner act quoted in section 3.11, and no other release, yank, repair version, workflow change without the owner's environment approval, or availability claim is authorized.
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
| created | `2026-10-16` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `031-public-api-and-release-family#I-1` | `invariant` | The facade `rustev` defines no type, trait, conversion or authorization of its own; it only re-exports member crates, has no default features, and with no features enabled its normal dependency graph is `rustev-contract` and `rustev-core` alone. | `3-2-the-facade-crate-rustev` | no |
| `031-public-api-and-release-family#I-2` | `invariant` | Only the facade may depend on a crate under `integrations/`, only as an optional dependency enabled by one named facade feature, and no workspace crate depends on the facade. | `3-3-boundary-amendment-for-the-facade` | no |
| `031-public-api-and-release-family#I-3` | `invariant` | A facade feature exists in a release only when its crate is a member of that release's publish set; a feature whose crate is not released is absent, never a stub. | `3-4-feature-map-and-staged-features` | no |
| `031-public-api-and-release-family#I-4` | `invariant` | A plan identity that changes for an unchanged definition and bound plan inputs is a breaking release; a release states whether any plan identity changed, and plans that use no new construct keep byte-identical PlanIds, proven by goldens. | `3-7-plan-identity-stability` | no |
| `031-public-api-and-release-family#I-5` | `invariant` | Every pre-publication check of spec 021 is kept for every crate, and every new artifact kind adds its own reproducibility, signature, provenance, SBOM and fresh-consumer checks before it may be published. | `3-9-supply-chain` | no |
| `031-public-api-and-release-family#I-6` | `invariant` | Publication remains the owner's act: the first release is authorized only conditionally by the quoted 2026-10-16 work order after every pre-publication check passes; any other release, repair version or yank needs a fresh owner instruction. | `3-11-publication-authority` | no |
| `031-public-api-and-release-family#R-1` | `requirement` | Each documented partial-adoption subset is a CI job that builds a fresh consumer of the facade with exactly that feature set and proves that the omitted crates are absent from its graph. | `3-5-partial-adoption-subsets` | no |
| `031-public-api-and-release-family#R-2` | `requirement` | A superseded document or protocol version stays readable for at least two subsequent minor releases or twelve months after its successor ships, whichever is longer, and the release notes state the end date. | `3-6-compatibility-policy` | no |
| `031-public-api-and-release-family#R-3` | `requirement` | `cargo semver-checks` runs in CI against the previous released family version, and a breaking change without the matching version increment fails. | `3-8-api-compatibility-checks` | no |
| `031-public-api-and-release-family#R-4` | `requirement` | The changelog of a release is generated deterministically from the governed corpus through spec-spine queries and the implementing commits, never written freehand. | `3-10-changelog-from-implementation-records` | no |
| `031-public-api-and-release-family#R-5` | `requirement` | Release and CI workflows are added only through the owner-approved `statecraft-review-exception` path; managed files change only through `.statecraft/environment.json` parameters and `statecraft-cli init plan\|apply`. | `3-12-ci-and-release-workflows` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"file","path":".cargo/config.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |
| `extends` | `005-reference-backends` | `{"kind":"file","path":"backends/rustev-backend-rules/Cargo.toml"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"file","path":"crates/rustev-cli/Cargo.toml"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-contract/Cargo.toml"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-core/Cargo.toml"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"file","path":"crates/rustev-eval/Cargo.toml"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"file","path":"crates/rustev-runtime/Cargo.toml"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"file","path":"integrations/rustev-otel/Cargo.toml"}` | no | `resolved` |
| `extends` | `028-opentelemetry-evidence-sink` | `{"kind":"file","path":"integrations/rustev-otel/LICENSE"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"file","path":"integrations/rustev-remote-http/Cargo.toml"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"file","path":"packages/rustev-pkg-lodging/Cargo.toml"}` | no | `resolved` |
| `extends` | `008-reference-package-lodging` | `{"kind":"file","path":"packages/rustev-pkg-lodging/LICENSE"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"file","path":"packages/rustev-pkg-support-routing/Cargo.toml"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
- `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}`
- `{"kind":"file","path":"rust-toolchain.toml"}`
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
cargo test -p rustev-release --locked
cargo test -p rustev --locked
cargo test -p rustev-boundaries --locked
cargo clippy -p rustev-release -p rustev -p rustev-boundaries --all-targets --locked -- -D warnings
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
| amends | `001-boundaries-and-authority`, `008-reference-package-lodging`, `010-integrations`, `021-packaging-and-first-release` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `045-command-surface-release-layer`, `053-signed-release-candidates`, `054-npm-rustev-scope`, `056-release-0-1-1-and-consumer-qualification`, `058-one-tagged-run-releases-the-family`, `060-the-service-image-in-the-release`, `068-qualification-requires-npm-provenance` |
| superseded by | None declared. |
| extended by | `010-integrations`, `011-semantic-backend`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `026-live-re-execution`, `027-node-and-wasm-bindings`, `028-opentelemetry-evidence-sink`, `029-evaluation-repeatability`, `030-bedrock-backend`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `041-ranking-and-rubric-metrics`, `042-decision-service-and-remote-seams`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `047-target-independent-transcendentals`, `050-wasm-without-parameterized-definitions`, `053-signed-release-candidates`, `054-npm-rustev-scope`, `056-release-0-1-1-and-consumer-qualification`, `058-one-tagged-run-releases-the-family`, `060-the-service-image-in-the-release`, `061-host-scored-repeat-sets`, `063-node-binding-parity`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity`, `067-node-live-repeat-runs`, `068-qualification-requires-npm-provenance`, `070-extract-rules-2` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:031-public-api-and-release-family` | `sha256:7ffb156badb7912b6e880831f608259678d3c692ad2c5c9da42b3f949150491d` |
| `registry:031-public-api-and-release-family` | `sha256:7ffb156badb7912b6e880831f608259678d3c692ad2c5c9da42b3f949150491d` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

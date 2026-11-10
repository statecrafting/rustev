<!-- spec-spine:begin generated id="header" -->
# `008-reference-package-lodging` Reference package: lodging recommendation

- Path: `specs/008-reference-package-lodging/spec.md`
- Summary: The second reference domain package reserved by the architecture roadmap: `packages/rustev-pkg-lodging`, containing a bounded lodging-recommendation definition, canonical documents, synthetic mechanics fixtures, and evaluation material. The package preserves exact eligibility, explicit trust treatment, bounded semantic fan-out, abstention, lineage, offline replay, partial adoption, and backend-swap comparison without claiming semantic quality, external readiness, or authority to book lodging.
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

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"packages/rustev-pkg-lodging/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
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
cargo test -p rustev-pkg-lodging --locked
cargo test -p rustev-jev --test lodging_swap --locked
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-pkg-lodging --all-targets --locked -- -D warnings
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
| amended by | `031-public-api-and-release-family` |
| superseded by | None declared. |
| extended by | `031-public-api-and-release-family`, `032-evidence-quality`, `036-mid-plan-acquisition`, `038-structured-extraction`, `041-ranking-and-rubric-metrics`, `050-wasm-without-parameterized-definitions` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:008-reference-package-lodging` | `sha256:b204168e5eee7aeed0d188cd30de250df07f7765df83f3cab8e4e3a27469afa5` |
| `registry:008-reference-package-lodging` | `sha256:b204168e5eee7aeed0d188cd30de250df07f7765df83f3cab8e4e3a27469afa5` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `007-support-routing-package` Reference package: support routing

- Path: `specs/007-support-routing-package/spec.md`
- Summary: The first domain package of increment 3 (design section 16.1, roadmap ordinal 007): `packages/rustev-pkg-support-routing`, a library that publishes the support-routing decision definition, its golden canonical documents, a declarative task adapter and evaluator configuration, and a synthetic labeled dataset, depending only on `rustev-contract` and `rustev-core`. It demonstrates that a domain is a package and not a core change, and that swapping the semantic backend (the rules backend's synthetic head against the Jev integration, spec 012) produces comparable reports with no change in any authority path. Approved (A-12, 2026-09-25) and amended by 016 (A-13, 2026-09-26); implemented.
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
| created | `2026-09-25` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
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
cargo test -p rustev-pkg-support-routing --locked
cargo test -p rustev-jev --test support_routing_swap --locked
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-pkg-support-routing --all-targets --locked -- -D warnings
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
| amended by | `016-support-routing-swap-threshold`, `021-packaging-and-first-release`, `056-release-0-1-1-and-consumer-qualification` |
| superseded by | None declared. |
| extended by | `021-packaging-and-first-release`, `031-public-api-and-release-family`, `036-mid-plan-acquisition`, `038-structured-extraction`, `041-ranking-and-rubric-metrics`, `050-wasm-without-parameterized-definitions`, `056-release-0-1-1-and-consumer-qualification` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:007-support-routing-package` | `sha256:f22da493acc92fab67606c5999e657d048f130871c8f5b6588bb85b81f0bfb60` |
| `registry:007-support-routing-package` | `sha256:f22da493acc92fab67606c5999e657d048f130871c8f5b6588bb85b81f0bfb60` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

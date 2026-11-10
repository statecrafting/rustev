<!-- spec-spine:begin generated id="header" -->
# `010-integrations` Optional ecosystem integrations and serving boundary

- Path: `specs/010-integrations/spec.md`
- Summary: A shared boundary contract for three independently optional integration crates: `rustev-aicortex` adapts attributed context and observation proposals, `rustev-rahi` adapts evidence delivery and optional operational hosting, and `rustev-serve` exposes bounded registered-plan execution over an application-operated service boundary. Every integration preserves explicit identity, provenance, failure and unresolved effects; none grants authority, owns domain judgment, or makes Rustev depend on an ecosystem service. Concrete external API bindings and readiness claims require later evidence before implementation.
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
| `010-integrations#I-1` | `invariant` | No integration output is authorization: Aicortex context remains attributed input, Rahi receipts establish only their declared acceptance or durability, and a serving response is only a Rustev judgment, unresolved outcome, or failure. | `3-1-common-boundary` | no |
| `010-integrations#I-2` | `invariant` | Each integration is independently optional and removable; contract, core, runtime, evaluation, backends and packages build without all three, and no crate depends on an integration crate. | `3-2-dependencies-and-partial-adoption` | no |
| `010-integrations#I-3` | `invariant` | Every external object, request, response, receipt and failure used by an integration retains its producer identity or an explicit unknown; an adapter never substitutes a requested identity for an observed identity. | `3-3-identity-provenance-and-failure` | no |
| `010-integrations#I-4` | `invariant` | Aicortex, Rahi, HTTP and async dependencies remain under integrations; rustev-contract and rustev-core gain no async runtime, HTTP or ecosystem dependency. | `3-2-dependencies-and-partial-adoption` | no |
| `010-integrations#R-1` | `requirement` | Before an integration is implemented, an owner-reviewed binding record identifies the exact external contract, immutable dependency versions, compatibility range, failure mapping, security and privacy posture, resource limits, and retained qualification evidence. | `3-4-binding-records-before-implementation` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-aicortex/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-rahi/"}` | no | `resolved` |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
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
cargo test -p rustev-rahi --locked
cargo clippy -p rustev-rahi --all-targets --locked -- -D warnings
cargo build -p rustev-rahi --locked
cargo test -p rustev-aicortex --locked
cargo clippy -p rustev-aicortex --all-targets --locked -- -D warnings
cargo build -p rustev-aicortex --locked
sh -c 'for p in rustev-contract rustev-core rustev-runtime rustev-eval rustev-surface rustev-cli; do t=$(cargo tree -p "$p" -e normal,build --locked --prefix none) || exit 1; if printf "%s\n" "$t" | grep -E "^(rahi|aicortex)"; then exit 1; fi; done'
cargo build -p rustev-serve --no-default-features --locked
cargo test -p rustev-serve --locked
cargo clippy -p rustev-serve --all-targets --locked -- -D warnings
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 'if grep -rnE "(struct|enum|trait|type)[[:space:]]+(Permitted|Grant)([^A-Za-z0-9_]|$)" integrations/rustev-rahi integrations/rustev-aicortex integrations/rustev-serve; then exit 1; fi'
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
| amended by | `031-public-api-and-release-family`, `042-decision-service-and-remote-seams`, `049-opentelemetry-through-the-node-binding`, `059-shared-remote-transport-by-dependency` |
| superseded by | None declared. |
| extended by | `011-semantic-backend`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `042-decision-service-and-remote-seams`, `050-wasm-without-parameterized-definitions`, `059-shared-remote-transport-by-dependency` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:010-integrations` | `sha256:c8dacf4baa0de0f5f8b491d402dd99599249f33f91b5e43049efe0137c99efbd` |
| `registry:010-integrations` | `sha256:c8dacf4baa0de0f5f8b491d402dd99599249f33f91b5e43049efe0137c99efbd` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

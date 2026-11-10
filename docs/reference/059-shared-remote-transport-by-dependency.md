<!-- spec-spine:begin generated id="header" -->
# `059-shared-remote-transport-by-dependency` Shared remote transport by dependency, not by source link (amends 001, 010, 012, 030 and 049)

- Path: `specs/059-shared-remote-transport-by-dependency/spec.md`
- Summary: An amendment of approved specs 001 (3.4.4), 010 (I-2 and the `rustev-serve` note), 012, 030 and 049 (I-3). `rustev-jev`, `rustev-bedrock` and `rustev-serve` compile spec 009's Part A machinery from `integrations/rustev-remote-http/src/` through `#[path]`, because 001 3.4.4 forbids a crate depending on an integration crate. A crate whose sources sit outside its own directory cannot be packaged, so spec 031 3.4 keeps `rustev-jev` (and, for the same reason, `rustev-bedrock`) out of the release family until a reviewed change removes the source link, and leaves open how. This spec chooses the boundary amendment: those three crates may depend on `rustev-remote-http`, only as a normal dependency, and re-export or use its modules instead of compiling copies. No behavior, record, document or public path of any crate changes. Admission of any crate to the release family stays with a later change.
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
| created | `2026-10-29` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `059-shared-remote-transport-by-dependency#I-1` | `invariant` | Apart from the facade's exception of spec 031 3.3 and rustev-node's edge to rustev-otel of spec 049 3.1, the only dependencies of a crate on an integration crate are the normal dependencies of rustev-jev, rustev-bedrock and rustev-serve on rustev-remote-http; make boundaries refuses every other edge, including these three as build or dev dependencies, any of them on another integration, and any other crate on rustev-remote-http. | `3-1-the-boundary-exception` | no |
| `059-shared-remote-transport-by-dependency#I-2` | `invariant` | No source file of rustev-jev, rustev-bedrock or rustev-serve is compiled from outside that crate's own directory; each uses spec 009's Part A machinery through its dependency on rustev-remote-http. | `3-2-the-source-links-become-a-dependency` | no |
| `059-shared-remote-transport-by-dependency#R-1` | `requirement` | The change alters no behavior: every existing test, conformance case, golden, PlanId, recorded fixture and exchange record of the three crates and of rustev-remote-http is unchanged, and the public module paths of rustev-jev and rustev-bedrock still resolve. | `3-3-what-does-not-change` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"file","path":"integrations/rustev-remote-http/Cargo.toml"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
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
sh -c 'if grep -rnE "#\[path[^]]*\.\./\.\./" integrations/rustev-jev/src integrations/rustev-bedrock/src integrations/rustev-serve/src; then exit 1; fi'
cargo test -p rustev-remote-http --locked --all-features
cargo test -p rustev-jev --locked --all-features
cargo test -p rustev-bedrock --locked --all-features
cargo test -p rustev-serve --locked --all-features
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
| amends | `001-boundaries-and-authority`, `010-integrations`, `012-jev-integration`, `030-bedrock-backend`, `049-opentelemetry-through-the-node-binding` |
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
| `index:059-shared-remote-transport-by-dependency` | `sha256:5ac57da3a44f4a7f53dee55f640fc6c937b7ff6537e7ecaac497ee6eb8a6c3dc` |
| `registry:059-shared-remote-transport-by-dependency` | `sha256:5ac57da3a44f4a7f53dee55f640fc6c937b7ff6537e7ecaac497ee6eb8a6c3dc` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

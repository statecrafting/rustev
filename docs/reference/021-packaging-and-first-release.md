<!-- spec-spine:begin generated id="header" -->
# `021-packaging-and-first-release` Packaging and first public release

- Path: `specs/021-packaging-and-first-release/spec.md`
- Summary: Defines the intended first public Rustev crate family, its lockstep pre-1.0 version policy, registry contents and features, internal dependency pins, license and dependency audit, reproducible package checks, signed release identity, publication journal, and fresh registry-only consumer proof. Candidate construction, owner publication authorization, registry upload, public availability, and consumer qualification are distinct lifecycle states. Approval authorizes candidate implementation only: no crate is publishable yet and no release, tag, upload, name reservation, or availability claim is authorized or made.
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
| `021-packaging-and-first-release#I-1` | `invariant` | A passing package check, a signed candidate, an approved release spec, an uploaded crate, a visible registry version and a qualified fresh consumer are distinct states and none is reported as another. | `3-1-lifecycle-and-authority` | no |
| `021-packaging-and-first-release#I-2` | `invariant` | Every published Rustev dependency resolves from the public registry at an exact family version; no released package or qualification uses a path, Git, patch, alternate registry or unpublished workspace crate. | `3-4-manifests-and-dependency-closure` | no |
| `021-packaging-and-first-release#I-3` | `invariant` | A release identity is one exact commit, signed tag, artifact manifest and immutable registry checksum set; an ambiguous or partial publication is never rerun as though nothing happened. | `3-8-signed-release-identity-and-artifacts` | no |
| `021-packaging-and-first-release#I-4` | `invariant` | Publication requires a fresh explicit owner instruction naming the version, exact candidate commit, registry and package set after all candidate evidence passes. | `3-9-publication-procedure-and-failure` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |
| `extends` | `005-reference-backends` | `{"kind":"file","path":"backends/rustev-backend-rules/Cargo.toml"}` | no | `resolved` |
| `extends` | `005-reference-backends` | `{"kind":"file","path":"backends/rustev-backend-rules/LICENSE"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"file","path":"crates/rustev-cli/Cargo.toml"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"file","path":"crates/rustev-cli/LICENSE"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-contract/Cargo.toml"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-contract/LICENSE"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-core/Cargo.toml"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"file","path":"crates/rustev-core/LICENSE"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"file","path":"crates/rustev-eval/Cargo.toml"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"file","path":"crates/rustev-eval/LICENSE"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"file","path":"crates/rustev-runtime/Cargo.toml"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"file","path":"crates/rustev-runtime/LICENSE"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"file","path":"integrations/rustev-remote-http/Cargo.toml"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"file","path":"integrations/rustev-remote-http/LICENSE"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"file","path":"packages/rustev-pkg-support-routing/Cargo.toml"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"file","path":"packages/rustev-pkg-support-routing/LICENSE"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"LICENSE"}`
- `{"kind":"file","path":"README.md"}`
- `{"kind":"file","path":"docs/backlog.md"}`
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
cargo test -p rustev-release --locked --test manifests --test family
cargo test -p rustev-release --locked --test archives --test embedded_files
cargo test -p rustev-release --locked --test audit --test repro --test candidate
cargo test -p rustev-release --locked --test lifecycle --test tagged --test cratesio
cargo test -p rustev-release --locked --test qualify_harness
cargo clippy -p rustev-release --all-targets --locked -- -D warnings
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
| amends | `006-cli-surface`, `007-support-routing-package` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `031-public-api-and-release-family`, `058-one-tagged-run-releases-the-family` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:021-packaging-and-first-release` | `sha256:22719de0981a758396b7b066e83da9c4ec45fe2026c1d85284395196824a9b13` |
| `registry:021-packaging-and-first-release` | `sha256:22719de0981a758396b7b066e83da9c4ec45fe2026c1d85284395196824a9b13` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

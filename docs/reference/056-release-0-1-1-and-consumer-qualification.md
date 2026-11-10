<!-- spec-spine:begin generated id="header" -->
# `056-release-0-1-1-and-consumer-qualification` The next family release: a CLI version command, declared consumer examples and the fresh-consumer qualification (amends 006, 007 and 031)

- Path: `specs/056-release-0-1-1-and-consumer-qualification/spec.md`
- Summary: A narrow amendment of approved specs 006, 007 and 031, written after `v0.1.0` was published to crates.io, npm and GitHub on 2026-10-26 but could not be qualified (021 3.10, 031 3.9.7). The CLI has no version command; the support-routing package declares no runnable offline example (its SYNTHETIC rules program is a test fixture of the rules backend); no harness runs the registry-only consumer across the platform matrix; and the npm packages carry no provenance. This spec adds `rustev version`, ships the package's SYNTHETIC example as package data, adds the qualification harness and its workflow, and defines how the owner authorizes a release after `0.1.0`, so that the next family version is published through the existing pipeline and qualified. Ratified by the owner on 2026-10-26 (A-58). Revised the same day (A-59): the work is folded into the next family version (`0.2.0`); there is no separate `0.1.1`.
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
| created | `2026-10-26` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `056-release-0-1-1-and-consumer-qualification#I-1` | `invariant` | A release after `0.1.0` is published only under an owner instruction whose bytes carry the owner's detached SSH signature in the `rustev-release` namespace, verified against `tools/rustev-release/allowed_signers`; an unsigned instruction for such a version is refused before any journal entry or upload. | `3-4-authority-for-a-later-release` | no |
| `056-release-0-1-1-and-consumer-qualification#R-1` | `requirement` | `rustev version` and `rustev --version` print `rustev &lt;version&gt;` and a newline to standard output, where `&lt;version&gt;` is the `rustev-cli` package version, and exit 0; both take no other argument. | `3-1-the-version-command` | no |
| `056-release-0-1-1-and-consumer-qualification#R-2` | `requirement` | `rustev-pkg-support-routing` ships, as package data reachable from its public API, its SYNTHETIC rules program, the calibration bound to it, one SYNTHETIC snapshot and the outcome expected of it, so that a consumer with only registry packages reproduces the example offline. | `3-2-the-declared-example` | no |
| `056-release-0-1-1-and-consumer-qualification#R-3` | `requirement` | The qualification harness runs every step of 021 3.10 and of 031 3.9.7 for each artifact kind the signed manifest names (crates, npm, CLI binaries; 3.9.7 (c) only when the manifest names an image) against the public registries only, from an empty Cargo home and npm cache with no registry credential, and writes per step its command, output, lockfile or install record, checksums and environment identity; a step it cannot run is reported as not run with its reason, never as passing. | `3-3-the-qualification-harness` | no |
| `056-release-0-1-1-and-consumer-qualification#R-4` | `requirement` | The release lifecycle reaches `qualified` only through `rustev-release qualify`, which records the digest of each required evidence file of the harness and refuses when one is missing, failing or `not_run`; a step is `excluded`, and not required, only when the signed manifest names no artifact of its kind. | `3-3-the-qualification-harness` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `007-support-routing-package` | `{"kind":"directory","path":"packages/rustev-pkg-support-routing/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |

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

No declared acceptance block.
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
| amends | `006-cli-surface`, `007-support-routing-package`, `031-public-api-and-release-family` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `058-one-tagged-run-releases-the-family`, `060-the-service-image-in-the-release`, `068-qualification-requires-npm-provenance` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:056-release-0-1-1-and-consumer-qualification` | `sha256:b6067b24667e36969582cbcb23393e28c64fd9aa420a9d8bdc2cca6f7d96380a` |
| `registry:056-release-0-1-1-and-consumer-qualification` | `sha256:b6067b24667e36969582cbcb23393e28c64fd9aa420a9d8bdc2cca6f7d96380a` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `068-qualification-requires-npm-provenance` Qualification requires npm provenance after 0.5.0 (amends 031, 056 and 058)

- Path: `specs/068-qualification-requires-npm-provenance/spec.md`
- Summary: An amendment of approved specs 031 (3.9.6, 3.9.7 (b)), 056 (3.3) and 058 (3.4.2), for #559, ratified by the owner on 2026-11-08 (A-88). Owner decision A-87 (#555) makes every npm package of a release after 0.5.0 carry npm provenance and refuses an unattested version at publication, but the fresh-consumer qualification still follows A-74: its npm step accepts a verified registry signature in place of provenance, so a later version without provenance would still be recorded `qualified`. This spec makes the npm step require, for every release version after 0.5.0, an SLSA build-provenance attestation on each installed `@rustev/*` package, verified by npm and bound to this repository's release workflow at the release tag. Versions 0.1.0 to 0.5.0 keep A-74's rule, so the default-branch harness can still requalify them. It records, without specifying, the removal of A-87's `NPM_TOKEN` fallback once npm fixes npm/cli#9969 and one release has published through trusted publishing alone.
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
| created | `2026-11-07` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `068-qualification-requires-npm-provenance#I-1` | `invariant` | For a release version greater than 0.5.0, the npm step passes only when every installed `@rustev/*` package's version document names an SLSA build-provenance attestation, `npm audit signatures --include-attestations` reports no invalid or missing signature or attestation for it, and the attestation's subject and build definition bind it to the package's manifest integrity and to `.github/workflows/release.yml` of this repository at the tag `vX.Y.Z`; a missing, unverifiable, unreadable or differently bound attestation fails the step and is never treated as not required. | `3-2-the-provenance-rule` | no |
| `068-qualification-requires-npm-provenance#I-2` | `invariant` | Which rule applies is decided by comparing the release version with 0.5.0 inside the harness that runs, never by reading the release's own tree, so qualifying an earlier tag with the default-branch harness cannot loosen the rule for a version after 0.5.0. | `3-1-which-rule-applies` | no |
| `068-qualification-requires-npm-provenance#R-1` | `requirement` | Versions 0.1.0 to 0.5.0 are qualified by A-74's npm rule unchanged (a verified registry signature and an integrity equal to the signed manifest), and the step evidence of every npm step records, per package, which rule applied and its outcome. | `3-3-grandfathered-versions-and-evidence` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

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
cargo test -p rustev-release --locked --test qualify_harness
cargo test -p rustev-release --locked --test family
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
| amends | `031-public-api-and-release-family`, `056-release-0-1-1-and-consumer-qualification`, `058-one-tagged-run-releases-the-family` |
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
| `index:068-qualification-requires-npm-provenance` | `sha256:2e937440974393aca4058a5c6df769b782e6f1b687054e9644a022f0123483cf` |
| `registry:068-qualification-requires-npm-provenance` | `sha256:2e937440974393aca4058a5c6df769b782e6f1b687054e9644a022f0123483cf` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `053-signed-release-candidates` Signed release candidates published only as GitHub pre-releases (amends 031)

- Path: `specs/053-signed-release-candidates/spec.md`
- Summary: A narrow amendment of approved spec 031 (3.9, 3.11, 3.12). It adds a release candidate: an owner-signed annotated tag `vX.Y.Z-rc.N` on a commit of `main`, built and checked by the release workflow exactly as a release is, and published only as a GitHub pre-release of this repository. A candidate never reaches crates.io, the npm registry or the container registry, never consumes spec 031 3.11's first-release authority, and is the identity a downstream consumer such as traveler (`statecrafting/tm`) pins with a Cargo git dependency. An integration rehearsal branch, `release/next`, is where each candidate commit is rehearsed before the owner signs it. Ratified by the owner on 2026-10-25 (A-55).
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
| created | `2026-10-25` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `053-signed-release-candidates#I-1` | `invariant` | A release candidate is never published to crates.io, the npm registry or a container registry: `rustev-release publish` and the release workflow's `npm` job refuse a candidate tag or version before any upload. | `3-4-what-a-candidate-never-does` | no |
| `053-signed-release-candidates#I-2` | `invariant` | A candidate run accepts only an annotated `vX.Y.Z-rc.N` tag that verifies against `tools/rustev-release/allowed_signers`, whose `X.Y.Z` is the release set's version, whose N is a positive integer without leading zeros, and whose commit is reachable from `main`; any other tag is refused before a build. | `3-2-the-candidate-tag` | no |
| `053-signed-release-candidates#R-1` | `requirement` | A candidate run performs every build and check of a release run (spec 031 3.9), requires the dispatched manifest's SHA-256 to equal the tag's `release-manifest-sha256:`, binds its artifacts to that manifest, and only then creates one GitHub pre-release of this repository holding exactly those artifacts, their checksums, SBOMs, the manifest and its signature. | `3-3-the-candidate-run` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
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
| amends | `031-public-api-and-release-family` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `058-one-tagged-run-releases-the-family` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:053-signed-release-candidates` | `sha256:2aaa2cbc695c8b8a386897f86b9f94dc3c88eabaaf9388ca0a935ea21fdf4512` |
| `registry:053-signed-release-candidates` | `sha256:2aaa2cbc695c8b8a386897f86b9f94dc3c88eabaaf9388ca0a935ea21fdf4512` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `060-the-service-image-in-the-release` The service image in the release (amends 031, 056 and 058)

- Path: `specs/060-the-service-image-in-the-release/spec.md`
- Summary: An amendment of approved specs 031 (3.1 layer 7, 3.9), 056 (3.3) and 058 (3.3.4, 3.5.2), ratified (A-76) for the release after 0.4.0. Spec 042 3.16's `rustev-serve` image is built, checked, compared, described and scanned in every release run, and is not published: 058 3.3.4 gives no job `packages: write`, and the owner decided on 2026-11-02 that 0.4.0 ships without the image. This spec admits the image as a release member. One job, `publish-image`, holds `packages: write` and runs only for a release tag, never for a candidate or a dry run. It pushes the run's verified OCI archives by digest to `ghcr.io/statecrafting/rustev-serve`, joins them in one index tagged with the family version, and attests the SLSA build provenance and each platform's SBOM. The manifest names the index and platform digests, the vulnerability scan gates through an owner-approved, expiry-bounded exception file, and qualification runs 031 3.9.7 (c). Spec 053's candidate refusals are unchanged. A release also keeps its qualification evidence as `qualification-` assets, which the release comparison ignores (3.7).
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
| created | `2026-11-02` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `060-the-service-image-in-the-release#I-1` | `invariant` | Only the `publish-image` job holds `packages: write`, and it pushes nothing unless the run is for a release tag `vX.Y.Z` that passed 058 3.1.1, the release set names the image as a member, and every image archive equals the bytes the run built, compared and scanned; a candidate tag or a dry run pushes nothing. | `3-3-the-push` | no |
| `060-the-service-image-in-the-release#I-2` | `invariant` | A tag `X.Y.Z` already present in the image repository is skipped when it names exactly the run's index digest and fails the run otherwise; no tag is moved, deleted or overwritten, and no `latest` tag is written. | `3-3-the-push` | no |
| `060-the-service-image-in-the-release#I-3` | `invariant` | No image digest, attestation, signature or registry state produced by the release is an authorization accepted by any action executor; they are release evidence only (spec 001, 031 3.13, 058 I-3). | `3-6-what-stays` | no |
| `060-the-service-image-in-the-release#R-1` | `requirement` | When the image is a member, any vulnerability finding of the pinned scanner over a platform's SBOM fails the run unless an unexpired entry of `tools/rustev-release/image-exceptions.json`, naming the owner's decision row, covers its vulnerability id and package; an expired or unused entry also fails. | `3-2-the-scan-gate` | no |
| `060-the-service-image-in-the-release#R-2` | `requirement` | The release manifest names the image index digest, each platform manifest digest, each platform's SBOM and the SLSA build level claimed; `verify-release` checks the registry against them, and qualification pulls the image by digest on amd64 and arm64, verifies its attestations and serves one synthetic decision (031 3.9.7 (c)). | `3-4-manifest-verification-and-qualification` | no |
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
cargo test -p rustev-release --locked --test image_scan
cargo test -p rustev-release --locked --test image --test image_registry
cargo test -p rustev-release --locked --test release_workflow --test candidate
cargo test -p rustev-release --locked --test tagged
cargo test -p rustev-release --locked --test family
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
| `index:060-the-service-image-in-the-release` | `sha256:96e8fbc6772938f1e8d6b34e3c44cafb8e0cc3deb9200908bb640549c48222ed` |
| `registry:060-the-service-image-in-the-release` | `sha256:96e8fbc6772938f1e8d6b34e3c44cafb8e0cc3deb9200908bb640549c48222ed` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

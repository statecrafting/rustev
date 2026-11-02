---
id: "060-the-service-image-in-the-release"
title: "The service image in the release (amends 031 and 058)"
status: draft
implementation: pending
created: "2026-11-02"
summary: >
  A proposed amendment of approved specs 031 (3.1 layer 7, 3.9) and 058
  (3.3.4), for the release after 0.4.0. Spec 042 3.16's `rustev-serve`
  image is built, checked, compared, described and scanned in every
  release run, and is not published: 058 3.3.4 gives no job
  `packages: write`, and the owner decided on 2026-11-02 that 0.4.0
  ships without the image. This spec admits the image as a release
  member. One job, `publish-image`, holds `packages: write` and runs only
  for a release tag, never for a candidate or a dry run. It pushes the
  run's verified OCI archives by digest to
  `ghcr.io/statecrafting/rustev-serve`, joins them in one index tagged
  with the family version, and attests the SLSA build provenance and
  each platform's SBOM. The manifest names the index and platform
  digests, the vulnerability scan gates through an owner-approved,
  expiry-bounded exception file, and qualification runs 031 3.9.7 (c).
  Spec 053's candidate refusals are unchanged.
amends:
  - "031-public-api-and-release-family"
  - "058-one-tagged-run-releases-the-family"
extends:
  # The release tooling this spec changes (031's unit).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: amending }
depends_on:
  - "010-integrations"
  - "031-public-api-and-release-family"
  - "042-decision-service-and-remote-seams"
  - "053-signed-release-candidates"
  - "056-release-0-1-1-and-consumer-qualification"
  - "058-one-tagged-run-releases-the-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Only the `publish-image` job holds `packages: write`, and it pushes nothing unless the run is for a release tag `vX.Y.Z` that passed 058 3.1.1, the release set names the image as a member, and every image archive equals the bytes the run built, compared and scanned; a candidate tag or a dry run pushes nothing."
    anchor: "3-3-the-push"
  - id: "I-2"
    kind: invariant
    text: "A tag `X.Y.Z` already present in the image repository is skipped when it names exactly the run's index digest and fails the run otherwise; no tag is moved, deleted or overwritten, and no `latest` tag is written."
    anchor: "3-3-the-push"
  - id: "I-3"
    kind: invariant
    text: "No image digest, attestation, signature or registry state produced by the release is an authorization accepted by any action executor; they are release evidence only (spec 001, 031 3.13, 058 I-3)."
    anchor: "3-6-what-stays"
  - id: "R-1"
    kind: requirement
    text: "When the image is a member, any vulnerability finding of the pinned scanner over a platform's SBOM fails the run unless an unexpired entry of `tools/rustev-release/image-exceptions.json`, naming the owner's decision row, covers its vulnerability id and package; an expired or unused entry also fails."
    anchor: "3-2-the-scan-gate"
  - id: "R-2"
    kind: requirement
    text: "The release manifest names the image index digest, each platform manifest digest, each platform's SBOM and the SLSA build level claimed; `verify-release` checks the registry against them, and qualification pulls the image by digest on amd64 and arm64, verifies its attestations and serves one synthetic decision (031 3.9.7 (c))."
    anchor: "3-4-manifest-verification-and-qualification"
---

# 060: The service image in the release (amends 031 and 058)

Draft: a proposal for the release after 0.4.0, never a claim about code.
Ratification is the owner's act. The approved texts of 031 and 058 are
not edited; this spec records what it replaces in them.

## 1. Purpose

Spec 042 3.16 defines the `rustev-serve` image, and spec 031 makes it a
planned release member (3.1, layer 7) with its supply chain (3.9). Since
2026-11-02 the release workflow builds each platform twice on native
runners, requires identical OCI archives, runs spec 042's `check.sh`,
writes CycloneDX SBOMs and scans the base layers (031's record of that
date; 042 increment 9). Three things keep the image out of a release:

1. 058 3.3.4: no job holds `packages: write`, so nothing can push.
2. 031 3.9.2: scan findings follow an owner-approved, expiry-bounded
   exception rule that exists for crates (`advisory-exceptions.json`,
   tied to `cargo deny`) but not for image findings. The pinned
   `distroless/cc-debian12` base reported 61 findings on 2026-11-02 in
   libc6, libssl3 and the gcc-12 runtime. A newer base digest clears
   the fixed libssl3 ones; the rest are marked "won't fix" upstream.
3. No manifest entry, verification or qualification step names an image
   (056 3.3 runs 3.9.7 (c) only when the manifest names one).

## 2. Territory

`.github/workflows/release.yml` and `.github/workflows/qualify.yml`
(owned in text by 031 3.12, 056 3.3 and 058), the release tooling under
`tools/rustev-release/`, and the release set. Changing either workflow
needs the owner's approval of the `statecraft-review-exception`
Environment once; ratifying this spec is not that approval. Refreshing
the runtime base digest is a change to spec 010's `rustev-serve` binding
record and to 042's image definition, reviewed on its own. Out of scope:
the image definition (042 3.16.1), candidates (053, unchanged), any other
registry, and any change of decision behavior.

## 3. Behavior

### 3.1 Membership

1. The image is a release member when spec 042 is implemented at the
   tagged commit and the release set lists it under `image` with its
   repository `ghcr.io/statecrafting/rustev-serve` and the platforms
   `linux/amd64` and `linux/arm64`; until then it stays under `absent`
   with its reason. The `rustev-serve` crate's own membership is separate
   (031 3.1).
2. The image's tag is the family version `X.Y.Z` (031 3.6.1). No other
   tag is written.

### 3.2 The scan gate

1. `tools/rustev-release/image-exceptions.json`
   (`rustev.image-exceptions/1`) lists entries of vulnerability id,
   package name, the owner's decision row, an expiry date at most 90 days
   after the row's date, and a reason. It starts empty.
2. With the image a member, `scan-image` fails on any finding of the
   pinned grype over a platform's SBOM that no unexpired entry covers,
   and on an entry that is expired or matches no finding. "Won't fix"
   upstream is not an exception by itself.
3. The scanner's database is the one current at the run, recorded with
   its build date in the evidence; it is not pinned, so a rerun may find
   more.

### 3.3 The push

1. `publish-image` needs `approve`, `image-checks` and the crate and npm
   layers' publication (031 3.1: layer 7 follows layer 6). It runs only
   for a release tag; for a candidate or a dry run it is skipped and
   names why. It is the only job with `packages: write`, beside the
   `id-token: write` and `attestations: write` that attestation needs.
2. It pushes each platform's verified OCI archive by digest with a
   pinned copy tool, never rebuilding, then creates one OCI index naming
   the two platform manifests in a fixed order and pushes it under the
   tag `X.Y.Z`. The index bytes are a pure function of the platform
   digests, so the release and rebuild produce the same index digest.
3. A tag already present is skipped when it names exactly the run's
   index digest and fails the run otherwise (058 3.5). Nothing is moved,
   deleted or overwritten.
4. It attests the index digest with SLSA v1.0 build provenance and each
   platform digest with its SBOM (`actions/attest-build-provenance` and
   `actions/attest-sbom`, pinned by commit, `push-to-registry: true`).
   These keyless Sigstore attestations are the image's signature under
   031 3.9.5; Build Level 2 is claimed.

### 3.4 Manifest, verification and qualification

1. `rustev.release-manifest/2` names, at layer 7, the image repository,
   tag, index digest, each platform manifest digest and its SBOM file,
   and the build level. `manifest` refuses an image member without
   them, and refuses a candidate manifest naming an image (053 3.4.2,
   unchanged).
2. `verify-release` resolves the tag and requires the manifest's index
   digest and platform digests, and verifies each attestation against
   the attestation store.
3. Qualification (056 3.3) runs 031 3.9.7 (c) on an amd64 and an arm64
   runner: pull by index digest, `gh attestation verify` for provenance
   and SBOM, then start the image on a read-only root with a test
   configuration on a Unix socket and serve one synthetic decision of the
   lodging reference plan, comparing its document with the golden.

### 3.5 Failure and resume

058 3.5 applies: a rerun skips what is public with the run's digests and
fails on any other; an image push that fails after some platform
manifests are pushed leaves untagged manifests, which are harmless and
are pushed again by digest on the rerun.

### 3.6 What stays

058 I-3 holds for the image. Candidates never push (053 3.4.2). The
image definition, its base and `check.sh` are 042's.

## 4. Out of scope

Other registries, a `latest` or floating tag, image signing with a
long-lived key, Build Level 3, and deployment.

## 5. Observable negative cases

| Case | Result |
|---|---|
| A candidate or dry-run run reaches `publish-image` | Skipped with its reason; nothing pushed. |
| An image finding with no exception, image a member | `image-checks` fails; nothing published. |
| An expired or unused image exception | `image-checks` fails. |
| Tag `X.Y.Z` names another index digest | `publish-image` fails; nothing moved. |
| The two builds of a platform differ | `image-checks` fails (031 3.9.3). |
| A manifest names an image without its digests or SBOMs | `manifest` refuses. |
| The registry's index or platform digest differs from the manifest | `verify-release` fails. |

## Acceptance

- `image-exceptions.json` and its check, with tests for an uncovered
  finding, an expired entry and an unused entry.
- `publish-image` pushes and tags only for a release tag; workflow tests
  or a dry run show the skip for a candidate and a dry run.
- The manifest, `verify-release` and qualification cover the image as
  3.4 states, with tests for each refusal of section 5.
- A release whose run pushes, attests, verifies and qualifies the image.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None. Draft.

## Decisions at approval

Open for the owner:

1. Whether the keyless attestations of 3.3.4 satisfy 031 3.9.5's
   "signature" for the image, or a separate `cosign sign` is wanted.
2. The base refresh before the first image release: take the current
   `distroless/cc-debian12:nonroot` digest through spec 010's binding
   record, and decide the exceptions for the remaining "won't fix"
   findings.
3. The 90-day bound on an image exception.

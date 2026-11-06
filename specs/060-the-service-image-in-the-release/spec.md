---
id: "060-the-service-image-in-the-release"
title: "The service image in the release (amends 031, 056 and 058)"
status: approved
implementation: in-progress
created: "2026-11-02"
summary: >
  An amendment of approved specs 031 (3.1 layer 7, 3.9), 056 (3.3) and
  058 (3.3.4, 3.5.2), ratified (A-76) for the release after 0.4.0. Spec 042 3.16's `rustev-serve`
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
  Spec 053's candidate refusals are unchanged. A release also keeps its
  qualification evidence as `qualification-` assets, which the release
  comparison ignores (3.7).
amends:
  - "031-public-api-and-release-family"
  - "056-release-0-1-1-and-consumer-qualification"
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

# 060: The service image in the release (amends 031, 056 and 058)

Approved by the owner on 2026-11-03 (A-76) for the release after 0.4.0;
3.7 may land before it. The approved texts of 031, 056 and 058 are not
edited; this spec records what it replaces in them.

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
   configuration on a Unix socket, register a small exact-only synthetic
   plan unsigned, and serve one decision of it, comparing its judgment
   with the golden. (Amended 2026-11-03 on the owner's decision of that
   day: the text first named the lodging reference plan; the standalone
   image installs no semantic backends, so it cannot decide that plan, and
   the remote-backend path is covered by spec 042's tests.)

### 3.5 Failure and resume

058 3.5 applies: a rerun skips what is public with the run's digests and
fails on any other; an image push that fails after some platform
manifests are pushed leaves untagged manifests, which are harmless and
are pushed again by digest on the rerun.

### 3.6 What stays

058 I-3 holds for the image. Candidates never push (053 3.4.2). The
image definition, its base and `check.sh` are 042's.

### 3.7 Qualification evidence on the release

1. Spec 021 3.10 requires the fresh consumer's evidence to be retained.
   A release keeps its qualification record and each platform's evidence
   bundle as assets of its GitHub release, named
   `qualification-vX.Y.Z.json` and `qualification-vX.Y.Z-<platform>.tar.gz`,
   as the owner had done by hand for `v0.3.0` on 2026-11-03 (spec 021's
   record).
2. Those assets are not release assets of 058 3.3.3. `publish_release`
   leaves every present asset whose name starts with `qualification-` out
   of its comparison (058 3.5.2), and refuses an asset set of its own
   holding such a name. `verify_assets` reads only the names the manifest
   implies, so an attached record never fails it. A rerun of a tagged run
   whose release holds qualification assets then succeeds as before.
3. Once 3.7.2 is implemented, `qualify.yml` attaches the record and the
   bundles of a run that recorded `qualified` to the tag's release, from
   one job that alone holds `contents: write`; it uploads only names not
   yet present and never replaces an asset. Until then they are attached
   by hand on the owner's instruction. The workflow change needs the
   owner's approval of the `statecraft-review-exception` Environment.

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
| A present release holds the run's assets and `qualification-` assets | `publish_release` reports it present (3.7.2). |
| A present release holds any other extra or differing asset | `publish_release` fails; nothing replaced. |
| A run's own asset set names a `qualification-` file | `publish_release` refuses before any upload. |

## Acceptance

- `image-exceptions.json` and its check, with tests for an uncovered
  finding, an expired entry and an unused entry.
- `publish-image` pushes and tags only for a release tag; workflow tests
  or a dry run show the skip for a candidate and a dry run.
- The manifest, `verify-release` and qualification cover the image as
  3.4 states, with tests for each refusal of section 5.
- A release whose run pushes, attests, verifies and qualifies the image.
- `publish_release` tests for each row of section 5 on `qualification-`
  assets (3.7.2), and a `qualify.yml` change that attaches them (3.7.3).
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

**Increment 1, 2026-11-03 (3.7.2).** `tagged::publish_release` leaves a
present release's `qualification-` assets out of its comparison and
refuses a run's own asset named with that prefix
(`QUALIFICATION_ASSET_PREFIX`); `verify_assets` is unchanged. The test
`qualification_assets_beside_a_release_are_ignored_never_published` in
`tools/rustev-release/tests/tagged.rs` covers the three rows of section 5
on those assets. A rerun of `v0.3.0`'s tagged run, whose release now
holds its `qualification-` assets, would find the release present again.
3.7.3 (`qualify.yml`) follows in its own pull request; the image (3.1 to
3.5) follows for the release after 0.4.0.

**Increment 4a, 2026-11-03 (3.1.1, 3.3.2, 3.4.1, the manifest's image).**
The release set takes an optional `image` entry (`image::ImageMember`:
`repository`, `platforms`); `check_set` refuses one whose repository is
not `ghcr.io/statecrafting/rustev-serve`, whose platforms are not
`linux/amd64`, `linux/arm64` in that order, or that stands beside an
`absent` entry of kind `image`. The committed set is unchanged: the image
stays absent until the membership change (section 3.1). The manifest
(`rustev.release-manifest/2`) takes an optional `image`
(`image::ImageRelease`): repository, tag (the family version), layer 7,
the index digest, each platform's manifest media type, digest, size,
optional variant and SBOM file, and SLSA build level 2. It is serialized
only when present, so every earlier manifest keeps its bytes and digest.
`image::platform_manifest` reads a verified OCI archive (a ustar reader,
no new dependency): `index.json` must name exactly one image manifest
whose blob hashes to its digest and size, and whose config names the
platform. `image::index_bytes` is the OCI index of the two descriptors in
the fixed order, as compact JSON in field order (the platform object as
`os`, `architecture`, then `variant`, the OCI image-spec order), so the
index digest is a pure function of the platform digests (3.3.2) and is
known when the manifest is attested, before anything is pushed. With the
image a member of a release run given its artifacts, `collect_family`
requires `image/<archive>` with its `.sha256` sidecar and
`sbom/<archive>.cdx.json` for each platform, names the image, and makes
the two SBOMs `sbom` artifacts at layer 8 (release assets in
`SHA256SUMS`); the archives are checked, never release assets. A member
without them is refused. `family_artifacts::validate` refuses a manifest
image lacking a platform digest or SBOM, out of order, at another tag,
repository or build level, or whose index digest is not the index of its
platform descriptors, and refuses any candidate manifest naming an image
(053 3.4.2); an image as a plain artifact stays refused. The tests in
`tools/rustev-release/tests/image.rs` cover section 5's manifest row and
the candidate refusal. `verify-release` against the registry follows in
its own pull request; the workflow that hands `manifest` the image files
follows with `publish-image`.

**Increment 2a, 2026-11-03 (3.7.3, tooling).** `rustev-release
attach-qualification <tag> <assets-dir>` (`tagged::attach_qualification`
through `prerelease::AssetUploads`) attaches a qualified run's evidence to
the tag's release. The directory must hold exactly
`qualification-vX.Y.Z.json` and one `qualification-vX.Y.Z-<platform>.tar.gz`
per platform of the release matrix (`qualification_asset_names`); the
record must be `rustev.qualification-record/1` for that tag and version in
state `qualified`, every bundle a gzip stream, and the release present and
not a pre-release. All of that is checked before the first upload. A name
already present is never uploaded again: with the same bytes it is
reported present, with other bytes (an earlier qualification of the tag)
it is kept and the command prints a warning. `gh release upload` runs
without `--clobber`, so a present name is refused there too. A failed
upload fails the command, and a rerun uploads only the missing names. The
tests `qualification_evidence_is_attached_once_and_never_replaced` and
`qualification_evidence_is_refused_before_any_upload` in
`tools/rustev-release/tests/tagged.rs` cover the resume, the kept record
and each refusal. The `qualify.yml` change that runs the command, with
the workflow tests that check its permissions, follows in its own pull
request.

**Increment 3, 2026-11-03 (3.2, the scan gate).**
`tools/rustev-release/image-exceptions.json`
(`rustev.image-exceptions/1`) is committed empty. An entry names the
vulnerability `id` and `package` as grype reports them, the owner's
`decision` row, the last day it holds (`expires`) and a `reason`; there is
no separate decision date, because the row's date is read from
`docs/decisions/00-founding-decisions.md` (embedded at build time) and
`expires` may be at most 90 days after it. `rustev-release image-scan
<today> <platform>=<grype.json>...` (`image_scan::check`) fails on a
finding that no unexpired entry for its id and package covers, on an
expired entry, on an entry that matches no finding, and on an entry with
no decision row, more than 90 days, a malformed date, an empty field or a
duplicate; `<today>` must be today's UTC date, as for `exceptions`. It
prints each report's scanner database build date
(`descriptor.db.status.built`); the JSON reports are already part of the
`image-evidence` artifact (3.2.3). `artifacts.sh scan-image` now runs it
after the scan in place of its own count: while the release set lists the
image as absent a failing verdict is recorded and does not gate, as
before; once the image is a member it fails `image-checks`. The tests in
`tools/rustev-release/tests/image_scan.rs` cover the uncovered finding,
the expired entry and the unused entry of section 5, and the other
refusals. No exception is added here: the exceptions for the remaining
"won't fix" findings are decided with the base refresh (decision 2).

**Increment 4b, 2026-11-03 (3.3.3, 3.4.2, the registry).**
`image_registry::ImageRegistry` reads a manifest or index by tag or
digest; tests drive it with a fake, and `Ghcr` reads `ghcr.io`'s registry
API through the system `curl`, exchanging `GH_TOKEN` (when set) for a pull
token on curl's standard input, never its command line, and accepting only
an `X.Y.Z` tag or a `sha256:` digest as a reference. `verify` (`rustev-release
verify-image <release-dir>`) requires the tag to name the manifest's index
digest, the index bytes to be the index of the manifest's platform
descriptors, and each platform manifest to be present by its digest with
its size; a failed lookup fails, never passes. A manifest naming no image
passes with nothing to check. `tag_state` (`rustev-release image-tag
<release-dir> <index-out>`) writes the index bytes to push and prints
`absent` or `present` (the run's index), and fails when the tag names
another digest (3.3.3); neither command pushes. The tests in
`tools/rustev-release/tests/image_registry.rs` cover section 5's rows for
the tag naming another index and the registry's index or platform digest
differing from the manifest. The `Ghcr` adapter has not yet met the live
registry; the workflow change of `publish-image` and `verify-release`
runs it first in a release run. Attestation verification against the
store (`gh attestation verify oci://...`) belongs to that workflow change.
The workflow tests now admit `packages: write` on a job named
`publish-image` only, with exactly `contents: read`, `packages: write`,
`id-token: write` and `attestations: write`
(`only_github_release_writes_contents_and_nothing_writes_packages`,
`workflow_write_permissions_are_confined`), and
`publish_image_runs_only_for_a_member_release_tag_after_npm` requires,
once that job exists, that it follow `approve`, `image-checks`,
`manifest`, `publish-crates` and `publish-npm`, that `github-release`
follow it, that it run only when `tag` decided a push, and that it write
no `latest` tag. Each passes on the current workflow, which has no such
job. For a check by hand, `rustev-release image-plan <amd64.oci.tar>
<arm64.oci.tar> <tag> <out-dir>` writes the image entry (`image.json`) and
the index bytes (`index.json`) the manifest job would derive, and
`verify-image` also reads such an `image.json`, with an optional
repository that replaces the image's for the registry read (a scratch
package); the release workflow passes neither. On the archives of the
dry run 41628699504, `image-plan` read both buildx OCI archives and gave
the platform digests their own `index.json` names
(`sha256:af46746d...` amd64, `sha256:321fdd73...` arm64).

**Increment 6a, 2026-11-03 (3.4.3 as amended, harness).** 3.4.3 now
decides an exact-only synthetic plan instead of the lodging reference plan
(the owner's amendment of that day, under Decisions at approval).
`qualification::required` reads a named image from the manifest's `image`
(no artifact is of kind `image`), and `required_on` requires the image
step on `linux-x64-gnu` and `linux-arm64-gnu` only
(`qualification::IMAGE_PLATFORMS`); elsewhere it must be excluded with a
reason. The harness's `image` step (`tools/rustev-release/qualify/harness.py`)
pulls the image by its index digest and requires that digest, verifies
the index's provenance and the platform manifest's CycloneDX SBOM
attestations with `gh attestation verify oci://...` (signer workflow
release.yml, source ref the tag), runs spec 042's `check.sh`, takes the
plan ID from the release's verified CLI (`plan compile` of
`qualify/image/definition.json`), starts the image read-only with no
network, `qualify/image/serve.json` mounted read-only and its socket in a
private directory owned by 65532, registers the plan unsigned, decides
`qualify/image/decide.json` once and requires `judged` and a judgment
equal to `qualify/image/judgment.json` with the plan ID filled in. The
configuration drops evidence with a count (`drop_counted`), since the
image has no sink. Locally, on the Debian 13 image built for
`linux/arm64` from this tree, the configuration, registration and
decision answered `judged` with a judgment equal to the golden (the
service's socket was on a Docker volume there, because Docker Desktop's
host mounts cannot hold a Unix socket; GitHub's Linux runners bind-mount
it as the harness does). `a_named_image_is_qualified_on_amd64_and_arm64_linux_only`
in `tests/image.rs` covers the per-platform rule. `qualify.yml`'s change
(the registry login and `packages: read`) follows in its own pull request.

**Increment 2b, 2026-11-03 (3.7.3, workflow).** `qualify.yml`'s `record`
job, once `rustev-release qualify` has written a record, packs each
platform's `qualify-<platform>` artifact unchanged into
`qualification-vX.Y.Z-<platform>.tar.gz` (fixed order, times and owners),
copies the record to `qualification-vX.Y.Z.json` and uploads them as the
artifact `qualification-assets`. A new job `attach`, run only when `record`
recorded `qualified`, is the workflow's one job with `contents: write`: it
checks out the workflow's own commit (so a dispatch can attach evidence of
a tag released before this change) and runs `attach-qualification`, which
uploads only names not yet present. release.yml's `qualify` call now grants
`contents: write` as the ceiling of qualify.yml's jobs; only `attach` takes
it (`qualify` and `record` declare their read permissions explicitly, so
the wider ceiling never reaches them), and `github-release` stays the only job of release.yml itself that
writes contents. A release made before spec 058 records no lifecycle here,
so nothing is attached for it. 058 3.3.3 names `github-release` as the
only job with `contents: write`; the owner accepted on 2026-11-03 the
reading that the `contents: write` this change adds applies only to
qualify.yml's `attach` job, only on tag runs through release.yml (and on a
hand dispatch of qualify.yml for a published tag), and only adds missing
`qualification-` assets to an existing release, never replacing one.
The workflow tests change with it: `qualify_reads_and_only_attach_may_write_contents`
in `tools/rustev-release/tests/release_workflow.rs` requires `qualify.yml`'s
top-level permissions to read only and only its `attach` job to hold
`contents: write`, and lets release.yml's `qualify` call grant it;
`workflow_write_permissions_are_confined` in `tests/candidate.rs` skips
comment lines and admits that call beside `github-release`.

**Increment 6b, 2026-11-03 (3.4.3, workflow).** `qualify.yml`'s `qualify`
job also holds `packages: read`, and on the Linux gnu runners logs in to
ghcr.io with the job's token before the harness runs, so the image step
(increment 6a) can pull the image by digest and `gh attestation verify
oci://...` can read its attestations while the package is still private
(the owner makes it public after its first push). release.yml's `qualify`
call grants `packages: read` as that job's ceiling. No job gains a write
permission.

**Increment 4c, 2026-11-03 (3.3, 3.4.2, workflow).** release.yml's `tag`
job decides, after 058 3.1.1, whether the manifest names the image
(`image_member`: the release set names it and the tag is not a
candidate) and whether the run pushes it (`image`: also a pushed release
tag), and otherwise names why in a notice (absent from the release set, a
candidate, or a dry run). `manifest` now also waits for `image-checks`
and, with the image a member, takes the two `image-release-*` archives
with their sidecars and the two image SBOMs from `image-evidence`, so
`run-manifest` names the image (increment 4a); `image-checks` gates the
manifest only then. The new `publish-image` job follows `manifest`,
`approve`, `image-checks` and `publish-npm`, requires each of them (and
`manifest`) to have succeeded, runs only when `tag` decided a push, and is the only job with `packages: write`, beside `contents: read`,
`id-token: write` and `attestations: write`. It installs oras 1.3.6 by its
release tarball's SHA-256, checks each archive against its sidecar, and
runs `rustev-release image-tag`: a tag naming another index fails the run,
a tag already naming the run's index skips the push (058 3.5), and an
absent tag is pushed: each platform by digest from its OCI archive (`oras
cp --from-oci-layout <archive>@<digest>`, no tag), then the manifest's
index bytes under `X.Y.Z` (`oras manifest push`), after which the tag must
name the manifest's index. It then attests the index digest with
`actions/attest-build-provenance` v4.2.2 and each platform digest with its
SBOM with `actions/attest-sbom` v4.1.0 (commit
`c604332985a26aa8cf1bdc465b92731239ec6b9e`), both `push-to-registry:
true`; a rerun attests again, which only adds. `github-release` follows
`publish-image` whenever the run pushes the image. `verify-release` gains
`packages: read` and, when the run pushed the image, runs `rustev-release
verify-image` and `gh attestation verify oci://...` for the index's
provenance and each platform's CycloneDX SBOM against the attestation
store. Nothing changes while the release set lists the image as absent:
`image_member` and `image` are false, `publish-image` is skipped, and the
manifest names no image. Neither oras step has run against ghcr.io yet;
the first release with the image runs them. A new organization package on
ghcr.io starts private; the owner decided on 2026-11-03 to make
`ghcr.io/statecrafting/rustev-serve` public by hand after its first push,
so anonymous consumers (and 3.4.3's qualification) can pull it.

**Increment 3b, 2026-11-03 (3.2.1, the exceptions).** The owner moved the
base to `gcr.io/distroless/cc-debian13:nonroot` (decision 2; spec 010's
binding record) and ruled on its findings on 2026-11-03: "accept all 44"
(A-80). `tools/rustev-release/image-exceptions.json` lists the 44
(vulnerability, package) pairs grype 0.120.1 reported on both platforms of
that base, each naming A-80 and expiring on 2027-02-01, 90 days after the
row. 43 are not reached, each entry saying why: the binary links only
libc, libm and libgcc_s (TLS is statically linked rustls), imports none
of the affected glibc functions, is neither setuid nor setgid, and
imports no C++ symbol, and the gcc-14 findings are in libstdc++ code;
CVE-2026-8674 in libc6 is reachable
only through the container's `resolv.conf` or `LOCALDOMAIN`, which the
operator controls, and the `rustev-serve` README says so. The same day the
owner amended 3.4.3 ("amend 3.4.3 as recommended", A-79). Both rows are in
`docs/decisions/00-founding-decisions.md`.
`the_committed_exceptions_name_owner_rows_and_cover_their_findings` in
`tests/image_scan.rs` requires every entry's decision row to exist, every
entry to cover its finding through 2027-02-01, every entry to expire the
day after, and an empty scan to leave all 44 unused. While the image is
absent from the release set (and on the cc-debian12 base, until the base
move merges) the scan's verdict is recorded without gating.

**Increment 5, 2026-11-05 (3.1, membership).**
`tools/rustev-release/release-set.json` names the image under `image`
(`ghcr.io/statecrafting/rustev-serve`, `linux/amd64` and `linux/arm64`)
and drops its `absent` entry. This is the switch that makes the next
release name, scan-gate, push, attest, verify and qualify the image. The
test fixtures that are not about the image (`tests/family.rs`,
`tests/tagged.rs`) set `image` to none, and `tests/npm.rs` and
`tests/image.rs` no longer read an image absence from the committed set.
It merged after 0.4.1 was cut, so 0.5.0 is the first release to carry
the image. Its conditions hold: the base is
`gcr.io/distroless/cc-debian13:nonroot` with the 44 accepted exceptions
(increment 3b, A-80), 3.4.3's image qualification is in `qualify.yml`,
and spec 042 is complete (#477), so 3.1.1's condition on it holds.

**Increment 5 follow-up, 2026-11-06 (3.1, the bans policy once a member).**
With the image a member, `artifacts.sh scan-image` gates on the bans
check of `tools/rustev-release/deny.toml` (031 3.9.2), which the
`v0.5.0-rc.1` run found failing: five wildcard path dependencies in
`integrations/rustev-serve/Cargo.toml` and two transitive duplicates.
The dependencies now name the workspace entries (`workspace = true`,
each with its exact family version), in a pull request of their own
because a manifest is a policy file. The duplicates cannot be unified
from this tree, so the release `deny.toml` skips them with their reasons,
as it already skips three: `base64` 0.22.1 (tonic 0.14 pins it; sigstore's
`pem` 4 uses 0.23) and `untrusted` 0.7.1 (`aws-lc-rs` 1.18 under
`sigstore-crypto` pins it; `ring` 0.17 uses 0.9). Advisories, licenses
and sources are unchanged.

## Decisions at approval

Ratified by the owner on 2026-11-03 (A-76: "I ratify 060"), as proposed:

1. The keyless attestations of 3.3.4 are the image's signature under 031
   3.9.5; no separate `cosign sign`.
2. The base refresh is decided before the first image release, through
   spec 010's binding record, with the exceptions for the remaining
   "won't fix" findings as owner decision rows (3.2.1).
3. An image exception expires at most 90 days after its decision row
   (3.2.1).
4. 3.7 records the owner's decision of 2026-11-03 that a release keeps
   its qualification evidence as `qualification-` assets and that the
   release tooling ignores them.

Owner amendment of 2026-11-03, after ratification: 3.4.3's decision is
one of a small exact-only synthetic plan registered unsigned, compared
with a new golden, in place of the lodging reference plan. The standalone
`rustev-serve` image installs no semantic backends (its binary serves
exact plans; ecosystem backends are host supplied or remote), so the
lodging plan's semantic steps cannot be decided in the image alone, and
the remote-backend path is covered by spec 042's tests (3.17). Decision
row A-79 records this amendment.

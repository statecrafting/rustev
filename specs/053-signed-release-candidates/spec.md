---
id: "053-signed-release-candidates"
title: "Signed release candidates published only as GitHub pre-releases (amends 031)"
status: approved
implementation: pending
created: "2026-10-25"
summary: >
  A narrow amendment of approved spec 031 (3.9, 3.11, 3.12). It adds a release candidate: an owner-signed
  annotated tag `vX.Y.Z-rc.N` on a commit of `main`, built and checked by the
  release workflow exactly as a release is, and published only as a GitHub
  pre-release of this repository. A candidate never reaches crates.io, the
  npm registry or the container registry, never consumes spec 031 3.11's
  first-release authority, and is the identity a downstream consumer such as
  traveler (`statecrafting/tm`) pins with a Cargo git dependency. An
  integration rehearsal branch, `release/next`, is where each candidate
  commit is rehearsed before the owner signs it. Ratified by the owner on
  2026-10-25 (A-55).
amends:
  - "031-public-api-and-release-family"
# `extends` is spec-spine's ownership edge for a unit another spec
# establishes; the registry compiles it (`spec-spine registry show`), and
# specs 031 and 051 claim their amended units the same way.
extends:
  # The tag rules, version check and publish refusals (3.2, 3.4).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: amending }
depends_on:
  - "021-packaging-and-first-release"
  - "031-public-api-and-release-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
# Anchors are spec-spine's heading slugs (`### 3.2 The candidate tag` is
# `3-2-the-candidate-tag`, as spec 031's `3-2-the-facade-crate-rustev`);
# `spec-spine registry obligation 053-signed-release-candidates#I-2`
# resolves each to its section digest.
obligations:
  - id: "I-1"
    kind: invariant
    text: "A release candidate is never published to crates.io, the npm registry or a container registry: `rustev-release publish` and the release workflow's `npm` job refuse a candidate tag or version before any upload."
    anchor: "3-4-what-a-candidate-never-does"
  - id: "I-2"
    kind: invariant
    text: "A candidate run accepts only an annotated `vX.Y.Z-rc.N` tag that verifies against `tools/rustev-release/allowed_signers`, whose `X.Y.Z` is the release set's version, whose N is a positive integer without leading zeros, and whose commit is reachable from `main`; any other tag is refused before a build."
    anchor: "3-2-the-candidate-tag"
  - id: "R-1"
    kind: requirement
    text: "A candidate run performs every build and check of a release run (spec 031 3.9), requires the dispatched manifest's SHA-256 to equal the tag's `release-manifest-sha256:`, binds its artifacts to that manifest, and only then creates one GitHub pre-release of this repository holding exactly those artifacts, their checksums, SBOMs, the manifest and its signature."
    anchor: "3-3-the-candidate-run"
---

# 053: Signed release candidates published only as GitHub pre-releases (amends 031)

Approved: ratified by the owner on 2026-10-25 (decision A-55).
The approved texts of specs 021 and 031 are not edited; this spec records
what it adds to 031 3.9, 3.11 and 3.12.

## 1. Purpose

Spec 031's release workflow has two modes. A rehearsal builds and checks
every artifact at a commit and attests and publishes nothing; a release
builds from the owner's signed `vX.Y.Z` tag and, after the owner's acts of
3.11, publishes to the registries. Nothing between them gives a downstream
consumer a stable, owner-signed identity to integrate against before a
release.

Traveler (`statecrafting/tm`) is the first such consumer. Its program
decisions consume Rustev "at signed revision" and allow no path override in
qualification evidence. The owner chose (2026-10-25) that candidates are
git tags, that the candidate run is a real release-workflow run in this
repository, and that this amendment be drafted. Registry pre-release
versions were rejected: a crates.io or npm version is permanent and public,
and publication is the owner's act under spec 021.

## 2. Territory

The release workflow of spec 031 3.12 (`.github/workflows/release.yml`) and
the release tooling under `tools/rustev-release/`. The workflow file is a
non-managed workflow; changing it needs the owner's approval of the
`statecraft-review-exception` Environment once, as 031 3.12 states, and
ratifying this spec is not that approval.

## 3. Release candidates

### 3.1 The integration rehearsal branch

1. `release/next` is a branch of this repository that only ever points at a
   commit of `main`; it is moved forward, never committed to.
2. To prepare a candidate, `release/next` is moved to the candidate commit
   and the release workflow is dispatched on it as a rehearsal (031 3.11:
   no tag, attests nothing, publishes nothing). Its artifacts are the bytes
   the candidate must reproduce (031 increment 9 part 1a).
3. `rustev-release manifest` builds the candidate's manifest
   (`rustev.release-manifest/2`) from that rehearsal, with the candidate's
   version (3.2.3) and a limitation stating it is a candidate.

### 3.2 The candidate tag

1. The owner signs one annotated tag `vX.Y.Z-rc.N` on the rehearsed commit,
   with the same identity and `allowed_signers` record as a release tag
   (021 3.8). Its message names the candidate manifest's SHA-256 as
   `release-manifest-sha256:`, as a release tag's does.
2. `X.Y.Z` is the release set's `version` (`release-set.json`). `N` starts
   at 1 and increases by one for each candidate of that version; it has no
   leading zeros.
3. The candidate's version is `X.Y.Z-rc.N`. It names the CLI archives and
   is the `version` of each npm tarball's `package.json`. The crates in the
   tree keep `X.Y.Z`: they are consumed from the tag through Cargo git
   dependencies and are never packaged for a registry by a candidate.
4. The tag's commit is reachable from `main`. A tag that is lightweight,
   unsigned, signed by another key, of another version, not on `main`, or
   named in any other form is refused before any build.
5. A candidate tag is never moved or deleted. A corrected candidate is the
   next `N`.

### 3.3 The candidate run

1. The release workflow is dispatched on the candidate tag with
   `tag=vX.Y.Z-rc.N` and the candidate manifest. The `tag` job first
   requires the manifest's SHA-256 to equal the tag message's
   `release-manifest-sha256:` (031 3.11.2), as for a release, and refuses a
   run without a manifest. It then runs every job of a
   release run (031 3.9: bindings, CLI, rebuild, archive, pack,
   reproducibility, SBOM, audit) and the `bind` job, which requires every
   artifact to equal the manifest's bytes.
2. Its artifacts are attested with SLSA build provenance bound to the
   workflow, commit and candidate tag (031 3.9.5).
3. Only after every check and the binding pass, one job creates the GitHub
   pre-release `vX.Y.Z-rc.N` of this repository, marked as a pre-release
   and never as the latest release, holding the CLI archives, the npm
   tarballs, `SHA256SUMS`, every SBOM, the manifest and its detached
   signature. That job alone has `contents: write`.
4. A failed candidate run creates no pre-release. A rerun of the same tag
   either creates it or finds it present with the same assets; it never
   replaces an asset.

### 3.4 What a candidate never does

1. It is not publication under spec 021 or 031 3.11 and never consumes the
   first-release authority 031 3.11 quotes.
2. `rustev-release publish` refuses a candidate tag or version. The `npm`
   job refuses a candidate tag even with `publish-npm: true`. No candidate
   is pushed to a container registry: `rustev-release manifest` refuses a
   candidate manifest that names a container image artifact, and no job of
   a candidate run holds `packages: write`.
3. A candidate claims no availability, support window or compatibility
   promise of 031 3.6; its release notes say it is a candidate.

### 3.5 Consumption

A consumer pins a candidate with a Cargo git dependency on this repository
and the candidate tag, for example `rustev = { git =
"https://github.com/statecrafting/rustev", tag = "v0.1.0-rc.1" }`; its
`Cargo.lock` then records the exact commit. npm consumers install a
tarball from the pre-release by URL and check it against `SHA256SUMS`.
Whether a consumer's evidence may cite a candidate is the consumer's rule,
not this spec's.

## 4. Out of scope

Registry pre-release versions; any change to 031 3.11's first-release
authority; the final `vX.Y.Z` release procedure; the container image;
traveler's own integration and CI.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A lightweight or unsigned `v0.1.0-rc.1` tag is dispatched | The `tag` job fails before any build, naming 021 3.8. |
| `v0.1.0-rc.1` signed by a key not in `tools/rustev-release/allowed_signers` | `git verify-tag` fails in the `tag` job before any build. |
| `v0.2.0-rc.1` while the release set's version is `0.1.0` | Refused: the version differs from `release-set.json`. |
| `v0.1.0-rc.01`, `v0.1.0-rc.0` or `v0.1.0-beta.1` | Refused: not a candidate tag. |
| The tag's commit is not reachable from `main` | Refused before any build. |
| A candidate dispatched with `publish-npm: true` | The `npm` job refuses; nothing is uploaded. |
| A candidate manifest names a container image artifact | `rustev-release manifest` refuses it; no manifest is written. |
| A job of the release workflow requests `packages: write` | The tooling's workflow test fails: no candidate run may push an image. |
| `rustev-release publish` given a candidate | Refused before any upload, journaling nothing. |
| The dispatched manifest's SHA-256 differs from the tag's `release-manifest-sha256:`, or no manifest is given | The `tag` job fails before any build. |
| An artifact differs from the candidate manifest | `bind` fails and no pre-release is created. |
| The pre-release exists with a different asset | The run fails and replaces nothing. |
| A rerun finds the pre-release present with identical assets | The run succeeds and uploads nothing. |

## Acceptance

- The tooling tests under `tools/rustev-release/` break each rule of 3.2
  once, and the refusals of 3.4.2 (crates, npm and container image) once
  each.
- The pre-release step's tests cover a first run (created), a rerun with
  identical assets (succeeds, uploads nothing) and a rerun with a differing
  asset (fails, replaces nothing).
- A candidate run on `release/next`'s rehearsed commit, tagged
  `v0.1.0-rc.1` by the owner, produces a GitHub pre-release whose assets
  equal the candidate manifest's bytes.
- `make gate` passes.

## Verification

The `tools/rustev-release/` tests run in CI as spec 031's do. The candidate
run itself is evidence recorded in this spec's implementation record (run
id, tag, pre-release URL), because it needs the owner's signed tag.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
release tooling under `tools/rustev-release/`; (2) the release workflow's
candidate run; then the owner's signed candidate run and closure.

**Increment 1 (2026-10-25): the release tooling.** `candidate.rs` holds
the tag rules of 3.2 (`vX.Y.Z-rc.N`, `X.Y.Z` the release set's version,
N positive without leading zeros) and the publication refusal of 3.4.2,
which `publish`, `hosted-release` and the crate lifecycle apply before
any upload or journal write. `artifacts.sh dispatch` refuses a candidate
that is lightweight, unverified against `allowed_signers`, not reachable
from `main`, or dispatched with `publish-npm`; a rehearsal may name the
candidate it rehearses (3.1.2). `tag-manifest` binds the manifest to the
tag's `release-manifest-sha256:` (3.3.1) and `npm-guard` refuses a
candidate tag (3.4.2). A candidate's npm tarballs and CLI archives carry
`X.Y.Z-rc.N` (3.2.3); its manifest names no crate packages, adds the
candidate limitation (3.4.3) and refuses a container image before any
file is written. `prerelease` creates the pre-release once and never
replaces an asset (3.3.3, 3.3.4). The tests in
`tools/rustev-release/tests/candidate.rs`, `signing.rs`, `artifacts.rs`
and `family.rs` cover the tooling rows of section 5, and a workflow test
refuses any job requesting the packages write permission.

## Decisions at approval

Ratified as proposed (A-55):

1. The owner's signed candidate tag is the authority for the pre-release;
   the pre-release job waits for no Environment, because a pre-release is
   not registry publication.
2. `release/next` is protected so that only the owner moves it, with no
   required checks, since it only names commits of `main`.
3. The candidate's npm version `X.Y.Z-rc.N` changes tarball bytes against
   a later `X.Y.Z` release, so a candidate's artifacts are never reused as
   the release's.

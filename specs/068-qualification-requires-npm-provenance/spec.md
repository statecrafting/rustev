---
id: "068-qualification-requires-npm-provenance"
title: "Qualification requires npm provenance after 0.5.0 (amends 031, 056 and 058)"
status: approved
implementation: pending
created: "2026-11-07"
summary: >
  An amendment of approved specs 031 (3.9.6, 3.9.7 (b)), 056 (3.3)
  and 058 (3.4.2), for #559, ratified by the owner on 2026-11-08 (A-88). Owner decision A-87 (#555) makes every npm
  package of a release after 0.5.0 carry npm provenance and refuses an
  unattested version at publication, but the fresh-consumer
  qualification still follows A-74: its npm step accepts a verified
  registry signature in place of provenance, so a later version without
  provenance would still be recorded `qualified`. This spec makes the
  npm step require, for every release version after 0.5.0, an SLSA
  build-provenance attestation on each installed `@rustev/*` package,
  verified by npm and bound to this repository's release workflow at the
  release tag. Versions 0.1.0 to 0.5.0 keep A-74's rule, so the
  default-branch harness can still requalify them. It records, without
  specifying, the removal of A-87's `NPM_TOKEN` fallback once npm fixes
  npm/cli#9969 and one release has published through trusted publishing
  alone.
amends:
  - "031-public-api-and-release-family"
  - "056-release-0-1-1-and-consumer-qualification"
  - "058-one-tagged-run-releases-the-family"
extends:
  # The qualification harness, its README and tests, and the step-evidence
  # check of `rustev-release qualify` (3.3.3).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: amending }
depends_on:
  - "031-public-api-and-release-family"
  - "056-release-0-1-1-and-consumer-qualification"
  - "058-one-tagged-run-releases-the-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "For a release version greater than 0.5.0, the npm step passes only when every installed `@rustev/*` package's version document names an SLSA build-provenance attestation, `npm audit signatures --include-attestations` reports no invalid or missing signature or attestation for it, and the attestation's subject and build definition bind it to the package's manifest integrity and to `.github/workflows/release.yml` of this repository at the tag `vX.Y.Z`; a missing, unverifiable, unreadable or differently bound attestation fails the step and is never treated as not required."
    anchor: "3-2-the-provenance-rule"
  - id: "I-2"
    kind: invariant
    text: "Which rule applies is decided by comparing the release version with 0.5.0 inside the harness that runs, never by reading the release's own tree, so qualifying an earlier tag with the default-branch harness cannot loosen the rule for a version after 0.5.0."
    anchor: "3-1-which-rule-applies"
  - id: "R-1"
    kind: requirement
    text: "Versions 0.1.0 to 0.5.0 are qualified by A-74's npm rule unchanged (a verified registry signature and an integrity equal to the signed manifest), and the step evidence of every npm step records, per package, which rule applied and its outcome."
    anchor: "3-3-grandfathered-versions-and-evidence"
---

# 068: Qualification requires npm provenance after 0.5.0 (amends 031, 056 and 058)

Approved: ratified by the owner on 2026-11-08 (A-88) for the release
after 0.6.0; the implementation is pending. The approved texts of 031,
056 and 058 are not edited; this spec records what it changes in them.

## 1. Purpose

Owner decision A-74 (2026-11-02) accepted npm packages without npm
provenance while npm refused this repository's trusted-publishing exchange
(npm/cli#9969), and changed both publication and qualification to match:
the owner could publish the run's verified tarballs by hand, and the
qualification's npm step required a verified registry signature instead
of provenance (056 3.3, 031 3.9.6).

Owner decision A-87 (2026-11-07, #555) reverses the publication half for
every release after 0.5.0. `publish-npm` publishes through OIDC trusted
publishing and, only when npm refuses the exchange, through a granular
token held by the `npm-publish` Environment; `--provenance` signs with
the job's OIDC token either way, and a public version without an SLSA
build-provenance attestation fails the run (058 3.3.2, 3.5.2 as amended).

The qualification half was not reversed. On `main` at 35ab7e59:

- `tools/rustev-release/qualify/harness.py` `Harness.npm` (lines 292 to
  330) runs `npm audit signatures --json --include-attestations`, reads
  only its `invalid` and `missing` lists, and fails a package only when
  it is listed there ("npm provenance is not required (056 3.3, 031
  3.9.6, A-74)"). npm verifies an attestation that exists but does not
  report one that is absent, so a package without provenance passes.
- `tools/rustev-release/qualify/README.md` (lines 14 to 17) says the same.
- 058 3.4.2 runs the harness "exactly as 056 3.3 specifies"; #555 amends
  058 3.3 and 3.5, not 3.4.

Publication's check sees only the run's own uploads. Qualification is the
later check against public state, and it is what records a release
`qualified` (021 3.10). Without this amendment a release after 0.5.0
whose npm packages lack provenance, for example a version published
outside the run before the run reached it, would be refused by the run
but could still be recorded `qualified` by a later qualification run
(056 3.5). This spec closes that path.

## 2. Territory

`tools/rustev-release/qualify/harness.py` (the npm step),
`tools/rustev-release/qualify/README.md`, and the harness tests in
`tools/rustev-release/tests/qualify_harness.rs`; and (3.3.3) `tools/rustev-release/src/qualification.rs`
and its tests, which read the step evidence. All of these sit in 031's
unit `tools/rustev-release/`, which 058 amends.

Edges: `tools/rustev-release/` (031, amending), declared at
ratification; at completion `Makefile` and `scripts/acceptance/` (001,
additive).

Unchanged: `.github/workflows/qualify.yml` and `.github/workflows/release.yml`
(no workflow edit, so no review-exception approval is needed for this
spec's own change); `qualify.sh`'s interface; the crates, CLI and image
steps; publication (058 3.3 and 3.5 as amended by A-87); the release set;
every published version's state on npm.

### 2.1 Binding principles

- **Proposals, never grants.** A provenance attestation and a
  `qualified` lifecycle are release evidence only (058 I-3); neither is
  an authorization any action executor accepts.
- **Unresolved, never a default.** An attestation that cannot be fetched,
  parsed or verified fails the step; it is never read as "not required"
  or as passing (I-1).
- **Evidence over assertion.** The step records, per package, which rule
  applied and what it found (R-1), so a `qualified` verdict states what
  was verified.

## 3. Behavior

### 3.1 Which rule applies

1. `qualify.sh` accepts only a stable version `X.Y.Z` (056 3.3). The
   harness compares it numerically, component by component, with
   `0.5.0`.
2. A version greater than `0.5.0` is qualified by the provenance rule
   (3.2). A version from `0.1.0` to `0.5.0` is qualified by A-74's rule
   (3.3).
3. The boundary `0.5.0` is a constant of the harness that runs. It is
   never read from `RUSTEV_QUALIFY_RELEASE_TREE` or from the release's
   manifest, so a default-branch harness qualifying an earlier tag (056
   3.5) applies the current rule, and an earlier release's tree cannot
   lower it (I-2).

### 3.2 The provenance rule

For each installed `@rustev/*` package (the two required packages and any
`@rustev/*` dependency they resolve), in addition to every check the
step makes today (public registry source, version, integrity equal to the
signed manifest):

1. **Named.** The package's version document
   (`https://registry.npmjs.org/<name>/<version>`) names an attestation:
   `dist.attestations.url` is a non-empty string and
   `dist.attestations.provenance.predicateType` starts with
   `https://slsa.dev/provenance/`. This is the same predicate as
   `npm_has_provenance` in `tools/rustev-release/src/hosted.rs` (#555);
   a shared fixture keeps the two equal (Acceptance).
2. **Verified.** `npm audit signatures --json --include-attestations`
   exits zero and lists the package in neither `invalid` nor `missing`.
   With 3.2.1 this means npm verified the attestation's signature and
   its subject against the tarball, since npm verifies every attestation
   the version document names.
3. **Bound.** The harness fetches the attestation bundle from
   `dist.attestations.url`, selects the SLSA provenance statement, and
   requires:
   - its subject named `pkg:npm/<name>@<version>` with a `sha512` digest
     equal to the manifest's integrity for the package;
   - `buildDefinition.externalParameters.workflow.repository` equal to
     `https://github.com/statecrafting/rustev`;
   - `buildDefinition.externalParameters.workflow.path` equal to
     `.github/workflows/release.yml`;
   - `buildDefinition.externalParameters.workflow.ref` equal to
     `refs/tags/vX.Y.Z` for the version under qualification.
   The harness reads the statement only to check this binding; the
   signature is npm's verification of 3.2.2 (decision 2).
4. Any failure of 3.2.1 to 3.2.3, including a network error, a non-200
   response, an unparsable bundle or a missing member, fails the npm
   step with a message naming the package and the check (I-1).

### 3.3 Grandfathered versions and evidence

1. For `0.1.0` to `0.5.0` the npm step is A-74's, unchanged: a verified
   registry signature (the package is in neither `invalid` nor `missing`)
   and an integrity equal to the signed manifest. A package of such a
   version that does carry an attestation is still verified by npm
   (3.2.2), so an invalid one fails the step; its absence does not.
2. The npm step's evidence gains, per installed `@rustev/*` package, a
   member `npm_provenance` with value `verified` (3.2 passed) or
   `not_required` (3.3.1 applied). A failing step records no value for
   the package that failed.
3. `rustev-release qualify` refuses an npm step file for a version after `0.5.0` unless
   every `@rustev/*` package it lists records `verified`, so a step
   produced by a harness older than this spec cannot qualify such a
   version.

### 3.4 What the amended specs now say

1. **031 3.9.6.** "Provenance is not a condition of publication or of
   qualification (A-74)" holds for `0.1.0` to `0.5.0` only. For every
   later version provenance is a condition of publication (A-87, 058
   3.3.2) and of qualification (this spec).
2. **031 3.9.7 (b).** "verifying provenance" is restored for every
   version after `0.5.0`, in the sense of 3.2.
3. **056 3.3**, the npm bullet: "npm provenance is not required (031
   3.9.6, A-74)" holds for `0.1.0` to `0.5.0`; later versions follow 3.2.
   056's section 4 row on the npm step gains the rows of section 5 below.
4. **058 3.4.2.** Unchanged in text; it runs the harness as 056 3.3
   specifies, which now includes 3.2 for a version after `0.5.0`.

### 3.5 Recorded follow-up: ending the publish fallback

Not specified here; recorded so it is not lost (#559 (b)). A-87 keeps a
granular `NPM_TOKEN` in the `npm-publish` Environment only because npm
refuses the trusted-publishing exchange (npm/cli#9969). When both hold:

- npm/cli#9969 is closed as fixed, and
- one release after the fix published every npm package through trusted
  publishing alone (the run's `publish-npm` log shows no fallback),

a separate amendment of 058 3.3.2 and 3.3.4 removes the fallback step
and the `NPM_TOKEN` environment of `publish-npm` in
`.github/workflows/release.yml`, restores the workflow test that no job
reads a registry secret (`tools/rustev-release/tests/release_workflow.rs`),
and the owner deletes the secret and revokes the token. The provenance
rule of 3.2 does not change: provenance is the same either way, which is
why this spec does not wait for it.

## 4. Out of scope

Republishing, deprecating or attesting any published version (npm
versions are immutable, 058 I-2); requiring provenance for `0.1.0` to
`0.5.0`; a provenance policy for crates.io, whose crates carry no npm
attestation; the CLI and image steps, which already verify their
attestations (031 3.9.5); the token fallback's removal (3.5).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Version `0.6.0`, a `@rustev/*` package whose version document has no `dist.attestations` | npm step fails naming the package; not `qualified`. |
| Version `0.6.0`, an attestation whose `predicateType` is not SLSA provenance | npm step fails. |
| Version `0.6.0`, the attestation listed in `invalid` by `npm audit signatures` | npm step fails. |
| Version `0.6.0`, the attestation built by another repository, another workflow, or the tag of another version | npm step fails naming the binding. |
| Version `0.6.0`, the attestation subject's digest differs from the manifest's integrity | npm step fails. |
| Version `0.6.0`, the bundle URL answers non-200 or unparsable JSON | npm step fails; never `not_required`. |
| Version `0.5.0` (or `0.3.0`), packages without provenance and with verified registry signatures | npm step passes; each package records `not_required`. |
| Version `0.5.0`, a package with an attestation npm lists as invalid | npm step fails. |
| `RUSTEV_QUALIFY_RELEASE_TREE` names a `0.5.0` tree while qualifying `0.6.0` | The provenance rule still applies. |
| `rustev-release qualify` over an npm step file for `0.6.0` without `npm_provenance: verified` per package | Refused. |

## Acceptance

- Harness tests in `tools/rustev-release/tests/qualify_harness.rs`, with
  the registry, the audit output and the bundle served from fixtures (no
  network), cover every row of section 5 and the boundary itself
  (`0.5.0` grandfathered, `0.5.1` and `0.6.0` not).
- One fixture version document is shared with `hosted.rs`'s
  `npm_has_provenance` test (#555), and both sides agree on it, so
  publication and qualification cannot drift on what "has provenance"
  means.
- `rustev-release` tests cover the step-file
  refusal and acceptance.
- The README states the two rules and the boundary.
- A real requalification of the first release after 0.5.0 by
  `qualify.yml` passes with `verified` for every package, and one of
  `v0.5.0` passes with `not_required`; their run ids are recorded here.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

## Decisions at approval

Decided in this text, with the reason beside each: the rule is chosen by
version inside the running harness (3.1.3), because 056 3.5 lets the
default-branch harness qualify earlier tags; `0.1.0` to `0.5.0` are
grandfathered (3.3.1), because A-87 supersedes A-74 only after 0.5.0 and
those versions cannot be republished; the "named" predicate is #555's
(3.2.1), so publication and qualification agree; the token fallback's
removal is recorded, not specified (3.5), because it waits on upstream.

Ratified by the owner on 2026-11-08 (A-88: "I agree with all your
recommendations"), adopting option A (ratified now, merged after the
`v0.6.0` tag, implemented in the release after 0.6.0), with every
question the draft left open answered as recommended:

1. **The boundary** is a numeric comparison with `0.5.0`: every version
   greater than it requires provenance, with no list of grandfathered
   versions (3.1).
2. **The attestation is bound** to this repository's
   `.github/workflows/release.yml` at `refs/tags/vX.Y.Z` and to the
   manifest integrity, in addition to npm's verification (3.2.3).
3. **Enforced twice:** in the harness and again in `rustev-release
   qualify`, which refuses an npm step file for a version after `0.5.0`
   unless every `@rustev/*` package records `verified` (3.3.3).
4. **No distinction** between packages published through trusted
   publishing and through A-87's fallback token; which path was used is
   evidence for 3.5, read from the release run's log.
5. **Target:** the release after 0.6.0; this spec does not block 0.6.0,
   whose npm packages #555 already holds to provenance at publication.

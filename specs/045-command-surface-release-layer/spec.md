---
id: "045-command-surface-release-layer"
title: "The command surface in the release family (amends 031)"
status: draft
implementation: pending
created: "2026-10-20"
summary: >
  A narrow amendment of approved spec 031 (itself amending spec 021). Spec
  027 adds `rustev-surface`, the pure command renderer every host calls,
  and requires the CLI to call it instead of its own copy. The CLI is a
  published release member, so its new dependency must be published first,
  in a crate layer of its own: 031 3.1's layer table has no slot between
  `rustev-eval` and the CLI. This spec adds `rustev-surface` to the release
  family, states the crate layers as the topological levels the release
  policy already computes from `cargo metadata`, admits a crate that a
  release member depends on once the crate exists and passes its checks,
  and keeps the release-set test's member list as the deliberate, reviewed
  record of membership. Draft: a proposal for the owner's ratification; it
  claims nothing about code.
amends:
  - "031-public-api-and-release-family"
depends_on:
  - "021-packaging-and-first-release"
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every normal or build dependency of a release member that is a workspace crate is itself a member of the same release, in an earlier crate layer."
    anchor: "3-1-membership"
  - id: "I-2"
    kind: invariant
    text: "A crate's layer is one past the deepest layer of its member dependencies, recomputed from cargo metadata at the candidate commit; the release-set file must equal that recomputation."
    anchor: "3-2-crate-layers"
---

# 045: The command surface in the release family (amends 031)

Draft: a proposal for the owner's ratification, not a claim about code.
The approved texts of specs 021, 027 and 031 are not edited; this spec
records what changes in 031. Implementation is the release-set change of
spec 027's increment 3, which waits on this spec's approval.

## 1. Purpose

Spec 027 (approved) adds `crates/rustev-surface`, the pure, bytes-in,
bytes-out command surface shared by the CLI, the native binding and the
WebAssembly package, and requires the CLI to call it for every rendering
(027 section 2). Its "Publishable to crates.io" column leaves the decision
to spec 021 and #76 child A3, which is spec 031.

The CLI is a member of the first release family (021 section 2, 031 3.1),
and a published crate can depend only on published crates. So
`rustev-surface` must be in the same release, uploaded and observed before
the CLI. 031 3.1's layer table names the CLI in crate layer 4 ("crates
depending on layer 3") and has no slot for a crate between layer 3, where
`rustev-eval` sits, and the CLI. The release policy already computes each
crate's layer from its dependencies; the table and the membership rule are
what need amending.

## 2. Territory

- 031 3.1 (release family members and layer order): amended as section 3
  states.
- `tools/rustev-release/release-set.json`, `archive-policy.json` and the
  release-set test (owned by spec 031): changed by spec 027's increment 3
  under this spec, not by this spec.

Nothing else changes: the policy code, the compatibility policy (031 3.3),
the supply-chain checks (031 3.9), the publication authority (031 3.11) and
the lifecycle (021) are unchanged.

## 3. Behavior

### 3.1 Membership

1. `rustev-surface` (spec 027) is a crate member of the release family,
   `publish = ["crates-io"]`, with the family's lockstep version and exact
   internal pins (021 3.2), its archive allowlist (021 3.5) and every check
   of 031 3.9 for a crate.
2. 031 3.1 admits a planned member "only when its own spec is implemented
   at the candidate commit". A crate that a release member depends on is
   instead admitted as soon as the crate exists at the candidate commit and
   passes every check for its kind, whatever the implementation status of
   its owning spec: the dependent member cannot be released without it. It
   publishes only its own source; nothing it does not contain (for
   `rustev-surface`, the bindings, the conformance suite and their CI) is
   claimed by its presence in a release.
3. This amends 031 3.1's sentence "Adding a member never requires editing
   a check to make it pass" to read: "Adding a member never requires
   editing a policy check (manifests, archives, audit, reproducibility) to
   make it pass; the release-set test's member list is the reviewed record
   of membership and changes with the release-set file." The release-set
   test keeps an explicit member list: adding or removing a member edits it
   in the same change as the release-set file, under the spec that changes
   membership, and the policy checks stay generic.

### 3.2 Crate layers

1. A crate's layer is one past the deepest layer of its normal and build
   dependencies among the release's members; a crate with none is layer 1.
   This is what the release policy computes from `cargo metadata` today
   (021 section 2); the release-set file must equal it, and a difference
   fails the candidate.
2. `rustev-surface` is in layer 4 because it depends on `rustev-eval`
   (layer 3): offline replay, evaluation, gates and calibration are
   `rustev-eval`'s (027 section 2 lists it among the surface's only
   dependencies, with `rustev-contract` and `rustev-core`). The CLI
   depends on `rustev-surface`, so it is in layer 5.
3. 031 3.1's layer table is read with its crate layers recomputed this way.
   For the members of this amendment the crate layers are: 1
   `rustev-contract`; 2 `rustev-core`; 3 `rustev-backend-rules`,
   `rustev-eval`, `rustev-pkg-support-routing`, `rustev-remote-http`,
   `rustev-runtime` (and `rustev-pkg-lodging` when 031 adds it); 4
   `rustev-surface`; 5 `rustev-cli`. Crates added later take the layer their
   dependencies give them.
4. The facade `rustev` stays the last crate layer (031 3.2), and the npm,
   image and hosted-release layers of 031 3.1 follow the last crate layer in
   their stated order; their numbers shift with the crate layers and nothing
   else about them changes.
5. Uploads follow the layers; within a layer, packages are ordered by name
   and are not an atomic transaction (unchanged).

## 4. Out of scope

Publishing anything (031 3.11 still governs the first publication);
`rustev-pkg-lodging`'s admission (031's own); the facade's features; npm
packages; any change to `rustev-surface`'s behavior, which spec 027 owns.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A release member depends on a workspace crate outside the release set | The manifest policy fails the candidate, naming both. |
| The release-set file places a crate in another layer than its dependencies give | The manifest policy fails, naming the declared and expected layers. |
| The release-set file's members differ from the release-set test's list | The test fails; membership changes only deliberately, in both. |
| `rustev-surface` fails an archive, audit or reproducibility check | The candidate fails, and with it the CLI's release. |

## Acceptance

- The release-set file lists `rustev-surface` in layer 4 and `rustev-cli` in
  layer 5, with the upload order following the layers; the manifest policy
  passes on the workspace and refuses each case of section 5 in its tests.
- `rustev-surface` has an archive allowlist and passes the archive, audit
  and reproducibility checks of the candidate.
- `make gate` passes.

## Verification

Pending with implementation: spec 027's increment 3 applies this
amendment's release-set change and records it there.

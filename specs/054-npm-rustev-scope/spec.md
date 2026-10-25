---
id: "054-npm-rustev-scope"
title: "The npm packages move to the owner's @rustev scope (amends 027 and 031)"
status: approved
implementation: complete
created: "2026-10-25"
summary: >
  A narrow amendment of approved specs 027 and 031. Both specs named the npm packages under the
  `@statecrafting` scope and recorded that whether that scope could be used
  was not established. The owner publishes Rustev under the npm
  organization `rustev` (2026-10-25). This spec moves every npm package
  from `@statecrafting/` to `@rustev/` and changes nothing else: the native
  package `@rustev/rustev`, its platform packages `@rustev/rustev-<platform>`,
  and the WebAssembly package `@rustev/rustev-wasm`. No API, export, crate
  name, container image or GitHub location changes. Ratified by the owner
  on 2026-10-25 (A-56).
amends:
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
extends:
  # Package names and the native loader's platform package lookup (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: amending }
  # The platform package check (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
  # The release set, npm order, tarball names and semver check (3.2).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: amending }
depends_on:
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every npm package of the release family is named under the `@rustev/` scope, and the release tooling refuses a package outside it."
    anchor: "3-names"
  - id: "R-1"
    kind: requirement
    text: "The native package is `@rustev/rustev`, each platform package is `@rustev/rustev-<platform>` for the platforms of spec 027 3.13.1, and the WebAssembly package is `@rustev/rustev-wasm`; no `@statecrafting/` name remains in a package manifest, the native loader, the release tooling or their tests."
    anchor: "3-names"
---

# 054: The npm packages move to the owner's @rustev scope (amends 027 and 031)

Approved: ratified by the owner on 2026-10-25 (decision A-56).
The approved texts of specs 027 and 031 are not edited; this spec records
the names that replace theirs. Spec 049 mentions `@statecrafting/rustev`
only in its motivation and sets no package name, so it is not amended.

## 1. Purpose

Spec 027's first decision at approval named the native package
`@statecrafting/rustev`, its platform packages
`@statecrafting/rustev-<platform>` and the WebAssembly package
`@statecrafting/rustev-wasm`, and recorded that whether the `@statecrafting`
organization exists on npm, and who controls it, could not be determined.
Spec 031's first decision aligned with it, with the same open question.
Neither name has been published.

The owner publishes Rustev under the npm organization `rustev` (2026-10-25).
On 2026-10-25 the organization existed and held no packages
(`registry.npmjs.org/-/org/rustev/package` returned `{}`, and a registry
search for scope `rustev` returned none). This spec moves the packages to
that scope before the first publication, while a rename costs nothing on
the registry.

## 2. Territory

The npm names only: the package manifests and native loader of
`integrations/rustev-node/` and `integrations/rustev-wasm/`, the platform
package check of `tools/rustev-conformance/`, and the release set, npm
publication order, tarball names and semver check of
`tools/rustev-release/`. No workflow file names a package, so implementing
this spec changes no file under `.github/workflows/`.

## 3. Names

1. The scope `@statecrafting/` is replaced by `@rustev/` in every npm
   package name, and nothing else in a name changes:

   | Spec 027 and 031 | This spec |
   |---|---|
   | `@statecrafting/rustev` | `@rustev/rustev` |
   | `@statecrafting/rustev-<platform>` | `@rustev/rustev-<platform>` |
   | `@statecrafting/rustev-wasm` | `@rustev/rustev-wasm` |

2. A packed tarball's file name follows npm's rule for a scoped package
   (`rustev-rustev-<version>.tgz` for `@rustev/rustev`), replacing the
   `statecrafting-rustev-` prefix.
3. Spec 031's npm publication order is unchanged: the platform packages by
   name, then `@rustev/rustev`, then `@rustev/rustev-wasm`. Its refusal of a
   package outside the scope now names `@rustev/`.
4. Unchanged: every export, type and behavior of both packages; the
   platform set; the `private` guard spec 027 keeps until publication; npm
   provenance and trusted publishing (031 3.9.6); crate names; the image
   `ghcr.io/statecrafting/rustev-serve`; and the GitHub repository.
5. Availability is still rechecked immediately before each upload
   (spec 021 3.9); the observation in section 1 is not a reservation.

## 4. Out of scope

Publication itself, which stays the owner's act under specs 021 and 031
3.11; renaming any crate or the container image; the unscoped names
`rustev`, `rustev-node` and `rustev-wasm`.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A package manifest still named under `@statecrafting/` | The release tooling's manifest check refuses it before packing. |
| A package outside `@rustev/` in the npm publication order | Refused, naming the `@rustev/` scope. |
| The native loader on a supported platform | It loads `@rustev/rustev-<platform>`; a missing one is the typed error naming that package, as spec 027 requires. |

## Acceptance

- No `@statecrafting/` npm name remains under `integrations/`, `tools/`
  or in the native loader, and `tools/rustev-release/`'s tests check the
  `@rustev/` scope and the new tarball names.
- `make bindings` passes, including the platform package check and the
  conformance suite's native run.
- `make code` and `make gate` pass.

## Verification

The `bindings` CI job runs `make bindings`; the `tools/rustev-release/`
tests and `semver.sh` run in CI as spec 031's do.

## Decisions at approval

Ratified as proposed (A-56):

1. **Name shape.** Only the scope changes: `@rustev/rustev`,
   `@rustev/rustev-<platform>` and `@rustev/rustev-wasm`, so every naming
   rule of 027 and 031 holds as written. The shorter `@rustev/node`,
   `@rustev/node-<platform>` and `@rustev/wasm` were not chosen.
2. **The `@statecrafting` names** are neither reserved nor published, and
   nothing in this repository refers to them after this change.

## Implementation record

**2026-10-25.** Every npm name under `integrations/rustev-node/`,
`integrations/rustev-wasm/`, `tools/rustev-conformance/js/` and
`tools/rustev-release/` moves from `@statecrafting/` to `@rustev/`, and
the packed tarball prefix from `statecrafting-rustev-` to `rustev-rustev-`;
the SBOM's package URL follows (`pkg:npm/%40rustev/rustev@0.1.0`). The two
places in `tools/rustev-release/src/` that took the scope's length as a
literal (`&name[15..]`) now strip `npm::SCOPE`, so the scope is written
once. Locally: `cargo test -p rustev-release` passes (143 tests), the Node
binding's tests pass (42), and the platform package check passes for all
seven packages; this change's `bindings` run is the first full
`make bindings` against the new names. Every acceptance item holds at this
change, so `implementation` is `complete`.

---
id: "031-public-api-and-release-family"
title: "Public API facade, compatibility policy and release family (amends 021)"
status: approved
implementation: pending
created: "2026-10-16"
summary: >
  Amends spec 021 so the first public release becomes a release family: the
  existing eight crates plus `rustev-pkg-lodging`, a facade crate `rustev`
  with no default features and one optional feature per adoptable slice,
  further integration crates as each lands, native and WebAssembly npm
  packages, a multi-arch service container image and prebuilt CLI binaries.
  Fixes one compatibility policy for every public surface (Rust API,
  documents, protocols, npm packages, service API, PlanId, MSRV), a supply
  chain of CycloneDX SBOMs, `cargo deny`, reproducible package checks,
  signed tags, signed artifacts with stated SLSA build provenance and npm
  provenance, a changelog generated from the governed corpus, and
  `cargo semver-checks` against the previous release. Narrowly amends spec
  001 3.4.4, spec 010 I-2 and spec 008 for the facade and lodging. Approved
  (A-27) under R-39. Approval authorizes implementation and candidate
  evidence; publication of the first release is authorized only by the
  conditional owner act quoted in section 3.11, and no other release, yank,
  repair version, workflow change without the owner's environment approval,
  or availability claim is authorized.
amends:
  - "001-boundaries-and-authority"
  - "008-reference-package-lodging"
  - "010-integrations"
  - "021-packaging-and-first-release"
establishes:
  - { kind: directory, path: "integrations/rustev/" }
  # Written by spec 021 increment 1 (021 extends it; 2.2 below).
  - { kind: directory, path: "tools/rustev-release/" }
  # Linker settings that make the Windows artifacts reproducible (3.9.3).
  - { kind: file, path: ".cargo/config.toml" }
extends:
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-contract/Cargo.toml" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-core/Cargo.toml" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: file, path: "crates/rustev-runtime/Cargo.toml" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: file, path: "crates/rustev-eval/Cargo.toml" }, nature: amending }
  - { spec: "005-reference-backends", unit: { kind: file, path: "backends/rustev-backend-rules/Cargo.toml" }, nature: amending }
  - { spec: "006-cli-surface", unit: { kind: file, path: "crates/rustev-cli/Cargo.toml" }, nature: amending }
  - { spec: "007-support-routing-package", unit: { kind: file, path: "packages/rustev-pkg-support-routing/Cargo.toml" }, nature: amending }
  - { spec: "008-reference-package-lodging", unit: { kind: file, path: "packages/rustev-pkg-lodging/Cargo.toml" }, nature: amending }
  # The license file `cargo package` puts in lodging's archive (021 3.5).
  - { spec: "008-reference-package-lodging", unit: { kind: file, path: "packages/rustev-pkg-lodging/LICENSE" }, nature: additive }
  - { spec: "009-remote-adapter-protocol", unit: { kind: file, path: "integrations/rustev-remote-http/Cargo.toml" }, nature: amending }
  # Admits `rustev-otel` to the family (3.1; spec 028 section 2).
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: file, path: "integrations/rustev-otel/Cargo.toml" }, nature: amending }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: file, path: "integrations/rustev-otel/LICENSE" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "007-support-routing-package"
  - "008-reference-package-lodging"
  - "009-remote-adapter-protocol"
  - "010-integrations"
  - "012-jev-integration"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "rust-toolchain.toml" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "The facade `rustev` defines no type, trait, conversion or authorization of its own; it only re-exports member crates, has no default features, and with no features enabled its normal dependency graph is `rustev-contract` and `rustev-core` alone."
    anchor: "3-2-the-facade-crate-rustev"
  - id: "I-2"
    kind: invariant
    text: "Only the facade may depend on a crate under `integrations/`, only as an optional dependency enabled by one named facade feature, and no workspace crate depends on the facade."
    anchor: "3-3-boundary-amendment-for-the-facade"
  - id: "I-3"
    kind: invariant
    text: "A facade feature exists in a release only when its crate is a member of that release's publish set; a feature whose crate is not released is absent, never a stub."
    anchor: "3-4-feature-map-and-staged-features"
  - id: "I-4"
    kind: invariant
    text: "A plan identity that changes for an unchanged definition and bound plan inputs is a breaking release; a release states whether any plan identity changed, and plans that use no new construct keep byte-identical PlanIds, proven by goldens."
    anchor: "3-7-plan-identity-stability"
  - id: "I-5"
    kind: invariant
    text: "Every pre-publication check of spec 021 is kept for every crate, and every new artifact kind adds its own reproducibility, signature, provenance, SBOM and fresh-consumer checks before it may be published."
    anchor: "3-9-supply-chain"
  - id: "I-6"
    kind: invariant
    text: "Publication remains the owner's act: the first release is authorized only conditionally by the quoted 2026-10-16 work order after every pre-publication check passes; any other release, repair version or yank needs a fresh owner instruction."
    anchor: "3-11-publication-authority"
  - id: "R-1"
    kind: requirement
    text: "Each documented partial-adoption subset is a CI job that builds a fresh consumer of the facade with exactly that feature set and proves that the omitted crates are absent from its graph."
    anchor: "3-5-partial-adoption-subsets"
  - id: "R-2"
    kind: requirement
    text: "A superseded document or protocol version stays readable for at least two subsequent minor releases or twelve months after its successor ships, whichever is longer, and the release notes state the end date."
    anchor: "3-6-compatibility-policy"
  - id: "R-3"
    kind: requirement
    text: "`cargo semver-checks` runs in CI against the previous released family version, and a breaking change without the matching version increment fails."
    anchor: "3-8-api-compatibility-checks"
  - id: "R-4"
    kind: requirement
    text: "The changelog of a release is generated deterministically from the governed corpus through spec-spine queries and the implementing commits, never written freehand."
    anchor: "3-10-changelog-from-implementation-records"
  - id: "R-5"
    kind: requirement
    text: "Release and CI workflows are added only through the owner-approved `statecraft-review-exception` path; managed files change only through `.statecraft/environment.json` parameters and `statecraft-cli init plan|apply`."
    anchor: "3-12-ci-and-release-workflows"
---

# 031: Public API facade, compatibility policy and release family (amends 021)

Approved by the owner (A-27, 2026-10-16, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation is
pending. It records owner decision OD-6 of tracking issue #76 as R-39 and
implements #76 child A3 (#58). Approval authorizes the facade crate, the
release tooling, manifest changes and candidate evidence. It does not by
itself publish, tag, upload, reserve a name, create a hosted release, change
a workflow, use a credential, or claim availability; section 3.11 states the
single conditional publication act that exists and its conditions.

## 1. Purpose

Bindings, the service and Rust hosts need one stable surface to wrap, and
consumers need registry artifacts rather than Git pins (`publish = false`
today). This spec:

1. adds a facade crate `rustev` that re-exports the engine and, behind
   features, each optional slice;
2. extends spec 021's release set, layer order and pre-publication evidence
   to the facade, `rustev-pkg-lodging`, new integration crates, npm packages,
   the service container image and prebuilt CLI binaries (OD-6, R-39);
3. fixes one compatibility policy for every public surface; and
4. adds the supply chain, changelog and API-compatibility checks a release
   family needs.

It amends spec 021 (release scope, features, lifecycle, artifacts,
publication), spec 001 3.4.4 and spec 010 I-2 (one narrow exception for the
facade), and spec 008 (lodging is publishable). It changes no decision,
runtime, evaluation, backend, protocol or authority semantics.

## 2. Territory

### 2.1 Current state (observed 2026-10-16)

- Workspace `Cargo.toml`: `edition = "2024"`, `rust-version = "1.85"`,
  `license = "Apache-2.0"`. `rust-toolchain.toml` pins the development and
  CI toolchain to rustc 1.98.1. No CI job builds at 1.85 today, so the
  declared MSRV is a declaration, not verified evidence.
- Every manifest says `publish = false` and version `0.1.0`. No crate declares
  a Cargo feature. No crate named `rustev` exists; the CLI package
  `rustev-cli` builds a binary named `rustev`.
- Existing members: `crates/` (contract, core, runtime, eval, cli),
  `backends/rustev-backend-rules`, `integrations/rustev-remote-http`,
  `integrations/rustev-jev`, `packages/rustev-pkg-support-routing`,
  `packages/rustev-pkg-lodging`, `tools/rustev-boundaries`.
- Internal normal dependencies: core on contract; runtime, eval, rules
  backend, remote HTTP and both packages on contract and core; CLI on
  contract, core, runtime, eval and rules backend.
- `rustev-jev` compiles spec 009's machinery in from sibling source files
  because spec 001 3.4.4 forbids depending on an integration crate; spec 021
  excludes it from the first release for that reason.
- Spec 008 lists registry publication of the lodging package as out of scope.

Registry observations, 2026-10-16 20:42 UTC, anonymous HTTPS queries:

| Registry | Names queried | Result |
|---|---|---|
| crates.io (`/api/v1/crates/<name>`; control `serde` returned 200) | `rustev`, `rustev-contract`, `rustev-core`, `rustev-runtime`, `rustev-eval`, `rustev-backend-rules`, `rustev-remote-http`, `rustev-pkg-support-routing`, `rustev-pkg-lodging`, `rustev-cli`, `rustev-bedrock`, `rustev-otel`, `rustev-serve`, `rustev-jev`, `rustev-node`, `rustev-wasm`, `rustev-backend-embed-linear` | 404 for every name: not registered. `cargo search rustev` lists no `rustev` crate (nearest: `rusteval`). |
| npm (`registry.npmjs.org`; control `react` returned 200) | `rustev`, `@statecrafting/rustev`, `@statecrafting/rustev-wasm`, `@statecrafting/rustev-linux-x64-gnu` (20:49 UTC), `@rustev/node`, `@rustev/wasm`, `rustev-node`, `rustev-wasm` | 404 for every name. Whether a scope (`@statecrafting`, `@rustev`) is held cannot be determined anonymously (the org page answers 403 for any org). |

An observation is not a reservation. Availability is rechecked immediately
before each upload (spec 021 3.9).

### 2.2 What changes

- New crate `integrations/rustev/` (the facade, crate name `rustev`), owned
  by this spec. It lives under `integrations/` because its optional features
  reach HTTP stacks, cloud SDK crates and telemetry exporters (Decisions at
  approval, item 4).
- New tool crate `tools/rustev-release/`, owned by this spec: the
  machine-checked release-set file, archive allowlists, the `cargo deny`
  policy, SBOM and provenance checks, the dry-run lifecycle state machine,
  the fake-registry harness and the changelog generator. It is repository
  tooling, never published. Spec 021's candidate machinery lives here too.
- Manifests of every family member: metadata, exact internal pins and the
  `publish` restriction of spec 021 3.4, as each joins the release.
- `tools/rustev-boundaries/`: the facade exception of 3.3 and its tests.
- Workspace `Cargo.toml`, `Cargo.lock` and `Makefile` (additive targets
  `adoption`, `semver` and `release-dry-run`).

### 2.3 What does not change

Contract, core, runtime, evaluation, backend and package behavior, wire
bytes, document versions, plan identities, the CLI surface, and every rule of
spec 001 other than 3.4.4. Crates that do not yet exist (bedrock, otel,
serve, local semantic backend, node, wasm) are specified by their own specs;
this spec only fixes how each joins the family when it lands.

## 3. Behavior

### 3.1 Release family and layer order

This section replaces spec 021 section 2's single-layer crate table with a
family. The family version is lockstep across every artifact kind: crates,
npm packages, the container image tag and CLI binaries of one release all
carry the same `X.Y.Z`, and the first family version remains `0.1.0`.

Family members:

| Kind | Member | Status on 2026-10-16 |
|---|---|---|
| crate | the eight packages of spec 021 section 2 | exist |
| crate | `rustev-pkg-lodging` (spec 008) | exists; added by this amendment |
| crate | `rustev` (facade, 3.2) | planned here |
| crate | `rustev-bedrock` (#66), `rustev-otel` (#70), `rustev-serve` (spec 010, #57) | planned by their own specs |
| crate | the local semantic backend crate (spec 011, #69) | planned by spec 011 |
| crate | the TypeSafe transport crate (`rustev-jev`, specs 012 and 022) | exists, not publishable (3.4) |
| npm | the native Node-API package and its per-platform packages, and the WebAssembly package (#56) | planned by their own spec |
| image | the `rustev-serve` multi-arch container image (#57) | planned |
| binary | `rustev` CLI binaries for the native matrix of #56 | built from `rustev-cli` |

The machine-checked release-set file at the candidate commit fixes the exact
members of a release. A planned member is in a release only when its own spec
is implemented at the candidate commit and it passes every check of 3.9 for
its kind; otherwise the release-set file lists it as absent with the reason,
and the release notes repeat that. Adding a member never requires editing a
check to make it pass.

Layer order (topological; a layer starts only after every package of the
earlier layers is observed on its registry with the expected checksum, as
spec 021 section 2 requires):

| Layer | Contents |
|---|---|
| 1 | `rustev-contract` |
| 2 | `rustev-core` |
| 3 | crates depending only on layers 1 and 2: `rustev-runtime`, `rustev-eval`, `rustev-backend-rules`, `rustev-remote-http`, `rustev-pkg-support-routing`, `rustev-pkg-lodging`, and any new member with the same closure |
| 4 | crates depending on layer 3: `rustev-cli`, and new integration or backend crates whose closure reaches layer 3 (for example on `rustev-runtime`) |
| 5 | `rustev` (the facade), always the last crate, because every optional dependency must already be public |
| 6 | npm: each per-platform native package, then the native main package naming them, and the WebAssembly package |
| 7 | the container image (pushed by digest) and the CLI binaries (attached to the hosted release) |
| 8 | the hosted release with the signed manifest, checksums, SBOMs, provenance, changelog and journal summary |

Crate layers 3 and 4 are recomputed from `cargo metadata` at the candidate
commit and must equal the release-set file; a difference fails the
candidate. Packages within a layer are ordered by name and are not an atomic
transaction (unchanged from 021).

This replaces spec 021 section 2's exclusion of lodging, serving, prebuilt
CLI binaries and containers; spec 021 3.1's "eight-package set" reads as the
release-set file's member set. Still excluded: `rustev-boundaries` and `rustev-release` (repository
tooling); Aicortex and Rahi integrations until their own release amendment;
models, weights, provider credentials, real datasets, production
configuration and generated governance shards. The Rust crates behind the
npm packages (`rustev-node`, `rustev-wasm`) are `publish = false` on
crates.io; they ship only as npm packages.

### 3.2 The facade crate `rustev`

`integrations/rustev/` is a library crate named `rustev`, `#![forbid(unsafe_code)]`,
with no binary target (the `rustev` binary stays in package `rustev-cli`; the
two are reported distinctly, as 021 3.4 requires).

1. **Always present:** `rustev::contract` (re-export of `rustev-contract`)
   and `rustev::decision_core` (re-export of `rustev-core`). The core is not
   re-exported as `rustev::core`, because a crate-root item named `core`
   shadows the built-in `core` crate and breaks `core::` paths inside the
   facade and in macro expansions (observed 2026-10-16 with a minimal crate).
2. **Behind features:** each feature enables exactly one optional dependency
   and one module re-exporting it, named after the feature with `-` replaced
   by `_` (`rustev::runtime`, `rustev::eval`, `rustev::rules`,
   `rustev::remote_http`, `rustev::bedrock`, `rustev::typesafe`,
   `rustev::otel`, `rustev::serve`, `rustev::local_embed`). A feature may
   enable another facade feature only when its crate's own normal
   dependencies already require that crate (for example `serve` enabling
   `runtime`); the implied set is listed in the facade's documentation and
   tested.
3. **No default features.** `default = []`. With no features the facade's
   normal graph is contract and core only: no I/O, no clock, no async
   executor, no HTTP stack. This keeps spec 021 3.3's rule that a default
   cannot silently select a network, provider, runtime, backend or
   integration. Spec 021 3.3's "the first release declares no public Cargo
   features" is amended for the facade alone; every other family crate still
   declares none unless its own spec adds one as a minor-version change.
4. **Nothing of its own.** The facade defines no type, trait, function,
   conversion, `From` implementation, prelude glob of its own, or
   authorization value. It re-exports items unchanged, so value kinds,
   derivation classes, lineage and `Unresolved` outcomes are exactly those of
   the member crates. A test asserts the facade's public items are only the
   re-export modules.
5. **Documentation** lists each feature, the crate behind it, the subset of
   3.5 it serves, and that a feature absent from this version is absent
   because its crate is not in the release.

### 3.3 Boundary amendment for the facade

Spec 001 3.4.4 ("No crate depends on a crate under `integrations/`") and the
last clause of spec 010 I-2 ("no crate depends on an integration crate") are
amended to read, in effect: no crate depends on a crate under
`integrations/`, except that the facade `rustev` at `integrations/rustev/`
may depend on one, only as an `optional = true` normal dependency enabled by
exactly one facade feature of 3.4. Every other part of 3.4 and 010 is
unchanged. Because the facade is itself under `integrations/` and no crate
may depend on it, every crate that reaches an HTTP stack, cloud SDK crate,
Node-API, `wasm-bindgen` or telemetry exporter, directly or through a facade
feature, is under `integrations/`: 3.4.2 and #76's principle hold literally.
The facade still declares no direct dependency on any forbidden family; it
reaches them only through the integration crates it re-exports.

`tools/rustev-boundaries` enforces:

1. a dependency on an integration crate by any package other than `rustev`,
   or by `rustev` without `optional = true`, or enabled by other than one
   named facade feature, is a 3.4.4 violation;
2. a dependency on `rustev` by any workspace member (normal, build or dev) is
   a violation;
3. the facade's `default` feature list is empty; and
4. the facade's resolved normal graph with no features reaches no async
   executor, HTTP stack or ecosystem family (the transitive rule already
   applied to contract, core and packages).

Each rule has a violating synthetic-metadata fixture, following the
existing tests. Integration crates keep their own independence: they still
may not depend on one another (the exception names the facade alone), so
`rustev-jev`'s source-linking remains until a reviewed change resolves it.

### 3.4 Feature map and staged features

| Feature | Crate | Owning spec | Present today |
|---|---|---|---|
| `runtime` | `rustev-runtime` | 003 | yes |
| `eval` | `rustev-eval` | 004 | yes |
| `rules` | `rustev-backend-rules` | 005 | yes |
| `remote-http` | `rustev-remote-http` | 009 | yes |
| `bedrock` | `rustev-bedrock` | #66 (C1) | no: the crate does not exist |
| `typesafe` | the TypeSafe transport crate (`rustev-jev`) | 012, 022 (#68) | no: not publishable (below) |
| `otel` | `rustev-otel` | #70 (D1) | no: the crate does not exist |
| `serve` | `rustev-serve` | 010, #57 (A2) | no: the crate does not exist |
| `local-embed` | the local semantic backend crate | 011 (#69, C4) | no: the crate does not exist |

The facade gains each feature in the change that admits its crate to the
family, never earlier. A crate is admissible only when its archive is
self-contained: a crate whose sources include files outside its own
directory (as `rustev-jev` does today) cannot be packaged by
`cargo package` and is not admitted. The `typesafe` feature therefore waits
for a reviewed change that removes that source-linking (a further boundary
amendment or moving the shared machinery into an admissible crate); this
spec does not decide which. Adding a feature is a minor-version change under
spec 021 3.2.

### 3.5 Partial-adoption subsets

Each row of section 3.3 ("Partial adoption") of
`docs/design/001-decision-engine-architecture.md` and each feature is a
documented subset. For each subset, a CI job builds a fresh consumer
project outside the workspace that depends on the facade with
`default-features = false` and exactly the subset's features, runs its
tests, and proves with `cargo tree -e normal` that every crate
of an omitted feature is absent from the graph.

| Subset | Features |
|---|---|
| Documents and pure compilation | none |
| Embedded decisions with a host's own store and identity | `runtime`, `rules` (plus a package crate as a separate dependency) |
| Evaluation harness only | `eval` |
| Remote backend over HTTP | `runtime`, `remote-http` |
| Hosted service | `serve` (when present) |
| One job per remaining present feature alone | that feature |

Before publication the consumer depends on the facade by path inside CI;
after publication the same projects run against the registry under 3.9's
fresh-consumer rules. A subset whose feature is absent is listed as skipped
with the reason, not reported as passing.

### 3.6 Compatibility policy

Applies to every public surface: the Rust API of every family crate, the
documents, the protocols, the npm packages and the service API.

1. **Documents are versioned by name** (`rustev.plan/2`). An incompatible
   shape is a new version name; an existing version's bytes and meaning
   never change.
2. **Support window.** A superseded document or protocol version stays
   readable for at least two subsequent minor releases or twelve months
   after its successor first ships in a release, whichever is longer. The
   release notes of the release that supersedes it state the end date, and
   every later release notes repeat it until it passes. Dropping a version
   before its end date is a breaking change.
3. **Protocols** (`rustev.remote/1`, `rustev.remote/2`,
   `rustev.remote-context/1`, `rustev.remote-sink/1`, `rustev.serve/1`, and
   any later one) follow the same naming and support-window rule; a version
   that is not yet specified is not claimed by a release.
4. **Rust API versions** follow spec 021 3.2: before 1.0 a breaking change
   increments the minor number of the whole family (Cargo's breaking
   position), a compatible change the patch number; from 1.0 the major and
   minor numbers respectively. "Major version" in this spec means that
   breaking position.
5. **npm packages** carry the family version and follow the same rule; a
   change to the generated TypeScript declarations that removes or narrows
   an export is breaking.
6. **Service API** (`rustev.serve/1` and its HTTP surface) follows rules 1 to
   3; the container image tag is the family version.
7. **MSRV policy.** Each release notes state its MSRV and edition (today
   1.85 and edition 2024). Raising the MSRV is at most a minor-version
   change, happens at most once per calendar quarter, and is its own
   reviewed change. The candidate matrix builds and tests at the declared
   MSRV (spec 021 3.7); until a CI job does so, the MSRV is unverified and
   the release cannot be candidate-verified. The development pin
   (`rust-toolchain.toml`, today 1.98.1) is separate from the MSRV.

### 3.7 Plan identity stability

Every release states, in its notes and release manifest, whether any
`PlanId` changes relative to the previous release, with the list of affected
reference plans. A change of plan identity for an unchanged definition and
unchanged bound plan inputs is a breaking ("major") release under 3.6.4.
Plans that use no construct introduced since the previous release keep
byte-identical `PlanId`s; a golden suite over the existing reference plans
compares identities compiled through the member crates and through the
facade with every feature combination of 3.5, and fails on any difference.

### 3.8 API compatibility checks

`cargo semver-checks` (pinned version) runs in CI for every published family
crate against the previous released family version on crates.io. A
detected break without the matching version increment fails. Until a first
release exists the job reports `no-baseline`, which is recorded as not
applicable and never as a passing check. The npm declaration surface and the
service API schema are diffed against the previous release by an equivalent
pinned check with the same rule.

### 3.9 Supply chain

Spec 021 3.5 to 3.8 apply unchanged to every crate, including the facade and
lodging. Each artifact kind adds:

1. **SBOM.** A CycloneDX SBOM (pinned generator, pinned spec version) for
   every artifact: each `.crate`, each npm tarball, each CLI binary and each
   image platform. SBOMs are hosted release artifacts; the image SBOM is also
   attached to the image as a signed attestation.
2. **Audit.** `cargo deny` (pinned version, policy in
   `tools/rustev-release/`) checks licenses, advisories, bans and sources for
   the whole family graph, implementing spec 021 3.6. npm dependencies are
   audited by an equivalent pinned check; the image's base layers by the
   SBOM plus a pinned vulnerability scan whose findings follow the same
   owner-approved, expiry-bounded exception rule.
3. **Reproducible package check.** Spec 021 3.7's two independent clean
   builds and digest comparison extend to npm tarballs, each CLI binary per
   target and each image platform (with a fixed source date and base images
   pinned by digest). A mismatch blocks the release and is explained, never
   normalized away.
4. **Signed tags** exactly as spec 021 3.8 (owner signing identity).
5. **Signed artifacts with build provenance.** Every artifact built by the
   release workflow receives a keyless signature and an in-toto SLSA
   provenance attestation bound to the workflow, commit and tag. The level
   claimed is SLSA v1.0 Build Level 2 (signed provenance from a hosted build
   platform). Level 3 is claimed for an artifact only when its provenance is
   produced by an isolated reusable build workflow and verified; the
   release manifest records the level per artifact. These attestations are
   in addition to the owner's detached signatures over the manifest and
   `SHA256SUMS` required by spec 021 3.8.
6. **npm provenance.** Every npm package is published with npm provenance
   attestations from the release workflow; a package without verifiable
   provenance is not published.
7. **Fresh-consumer qualification** (spec 021 3.10, all six steps kept)
   adds: (a) a fresh consumer resolves `rustev =X.Y.Z` from the registry with
   no path, Git or patch override and builds each subset of 3.5; (b) a
   fresh Node project installs both npm packages from the public registry on
   each supported platform of #56 and runs their smoke tests, verifying
   provenance; (c) the image is pulled by digest on amd64 and arm64, its
   signature, provenance and SBOM attestation verified, and a bounded
   synthetic decision served; (d) each CLI binary is downloaded from the
   hosted release, its checksum, signature and provenance verified, and
   version, help, plan check and one offline synthetic example run.

### 3.10 Changelog from implementation records

The changelog of `vX.Y.Z` is generated by `tools/rustev-release` from the
governed corpus through `spec-spine` subcommands (never by parsing
`.statecraft/derived/` directly), comparing the previous release tag with
the candidate commit: every spec whose implementation state or approved
text changed, with its id, title, approval row, amendment edges and
implementing commits, plus every compatibility fact of 3.6 and 3.7
(superseded versions with end dates, MSRV, PlanId changes). Two runs on the
same inputs produce identical bytes. A reviewed limitations section may be
added by hand; no generated entry is edited. The changelog is a hosted
release artifact and its digest is in the release manifest.

### 3.11 Publication authority

Spec 021 I-4 and 3.9 require a fresh explicit owner instruction naming the
version, exact candidate commit, registry and package set. For the first
release only, the 2026-10-16 delivery work order is that instruction, in
these words: "You may cut the first release under spec 021 as amended by #58
... This order is the owner publication act spec 021 requires, conditional on
every pre-publication check in 021 passing first."

Because that act precedes the candidate, this amendment binds it as follows:

1. It authorizes exactly one release, family version `0.1.0`, of the members
   the release-set file fixes at a candidate-verified commit, to crates.io,
   the npm public registry and the GitHub container registry of the
   repository's organization.
2. It takes effect only after every check of spec 021 and of 3.9 passes at
   that exact commit and the signed release manifest naming commit, tree,
   version, registries and artifact set is appended to the publication
   journal before the first upload. That manifest supplies the commit and set
   that the instruction could not name in advance.
3. Every other rule of spec 021 3.9 holds: exact verified archives only,
   journal before each attempt, no blind retry, `partially published` on
   ambiguity, stop on refusal.
4. Any continuation after a partial publication, a repair version, a yank, a
   second release, or a change of member set after the manifest is journaled
   needs a fresh owner instruction as spec 021 I-4 states.

Registry credentials are used only by the publication step, preferably
through registry trusted publishing tied to the release workflow, and are
never read during specification, implementation, review or candidate checks.

### 3.12 CI and release workflows

The subset jobs of 3.5, the semver check of 3.8 and the release workflow of
3.9 are workflow changes. They are added as non-managed workflow files (as
`.github/workflows/verify.yml` and `boundaries.yml` are, owned by spec 001),
and any new required job is registered by editing
`project.setup.parameters` (`ci.extra_required_jobs`) in
`.statecraft/environment.json` and re-rendering with
`statecraft-cli init plan|apply`. Managed files
(`.github/workflows/statecraft-*.yml`, `scripts/statecraft/*`,
`.statecraft/setup/*`, `.github/CODEOWNERS`) are never edited by hand. Every
such change needs the owner's approval of the `statecraft-review-exception`
Environment; approval of this spec is not that approval.

### 3.13 Binding principles of #76

- **Proposals, never grants.** The facade re-exports and defines nothing;
  no release artifact, signature, attestation or registry state is an
  authorization, and no `Permitted` type exists.
- **Derivation classes and lineage; `Unresolved` never a default; no silent
  kind conversion.** Re-exported items are unchanged; the facade adds no
  conversion (3.2.4).
- **Core purity; no async runtime in contract and core.** Unchanged; the
  facade with no features is contract and core only and is checked
  transitively (3.3.4).
- **New dependency classes only under `integrations/`.** No dependency
  class is added to any crate; supply-chain tools are pinned repository
  tools, not Cargo dependencies. The single exception of 3.3 lets the facade
  name integration crates optionally. The facade is itself under
  `integrations/` and nothing depends on it, so no crate outside
  `integrations/` reaches an HTTP stack, cloud SDK crate, Node-API,
  `wasm-bindgen` or telemetry exporter, directly or transitively; the
  principle holds literally, and `make boundaries` is extended for the
  exception.
- **No dynamic plugin loading.** Features are compile-time Cargo features.
- **Rustev owns no identity, authority or knowledge store.** Signing
  identities are the owner's and the platform's release identities, not a
  Rustev feature.

## 4. Out of scope

- Implementing the planned crates, npm packages or service (their own specs).
- Aicortex and Rahi in the release family.
- Moving or splitting existing crates, renaming existing packages.
- Any release after `0.1.0`, any yank, and any registry or hosting choice
  beyond 3.11.1, each a fresh owner act.
- A production-readiness, provider-quality or availability claim beyond the
  fresh-consumer evidence of 3.9.7.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `rustev-cli` adds a dependency on `rustev-remote-http` | `make boundaries` fails naming 3.4.4. |
| The facade depends on an integration crate without `optional = true` | `make boundaries` fails. |
| A workspace crate depends on `rustev` | `make boundaries` fails. |
| The facade declares a non-empty `default` feature | `make boundaries` fails. |
| The facade defines a struct, trait or `From` impl | The public-items test fails. |
| A `bedrock` feature appears before `rustev-bedrock` is in the release set | Release-set check fails. |
| `rustev-jev` is admitted while it source-links sibling files | Packaging check fails; not admitted. |
| A subset build pulls an omitted feature's crate | The subset job fails. |
| An existing reference plan's `PlanId` differs through the facade | Golden fails. |
| `cargo semver-checks` finds a break with only a patch bump | CI fails. |
| No previous release exists | `semver` reports `no-baseline`, recorded as not applicable, not as passed. |
| Two image builds produce different platform digests | Release blocked and explained. |
| An npm package lacks provenance | Not published; release not `released`. |
| An artifact lacks an SBOM | Candidate fails. |
| Publication starts before every check passes, or before the manifest is journaled | Not authorized by 3.11; the operator stops. |
| A second version or a yank is attempted under the work order | Not authorized; needs a fresh owner instruction. |
| The release notes drop a superseded version before its end date | Breaking-change check fails the candidate. |
| A release workflow is added by editing a managed file | Refused; only the parameter and `init plan|apply` path with the Environment approval. |

## Acceptance

- A fresh consumer resolves the facade from the registry with no path or Git
  overrides and builds each partial-adoption subset (3.5, 3.9.7a).
- A fresh Node project installs both npm packages on each supported platform
  and runs their smoke tests (3.9.7b).
- `cargo semver-checks` (or equivalent for npm and the service API) runs in
  CI against the previous release (3.8).
- Signed tags and verified artifacts; SBOMs attached; build provenance with
  the SLSA level stated per artifact; npm provenance (3.9).
- The release-set file fixes members, layers, absences with reasons, and the
  feature map; its check matches `cargo metadata`.
- Boundary-tool fixtures cover each rule of 3.3; the facade public-items
  test and the PlanId golden of 3.7 pass.
- An MSRV CI job builds and tests the family at the declared MSRV.
- The changelog generator is deterministic over fixed inputs (3.10).
- The dry-run state machine and fake-registry harness of spec 021 cover the
  added layers and artifact kinds, including 3.11's conditions, without
  credentials or network writes.
- Every acceptance item of spec 021 still passes for the extended family.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval authorizes the facade, release tooling,
manifest changes, CI changes through 3.12's path, and candidate evidence.
Upload, tag push and hosted release creation happen only under 3.11.

## Implementation record

**2026-10-19, written by spec 021.** `tools/rustev-release/` exists: spec
021 increment 1 wrote the release-set file and the manifest policy there,
as 2.2 provides, so this spec's unit is established rather than planned and
spec 021 extends it. Nothing of this spec is implemented by that change;
its implementation remains pending.

**Increment 1, 2026-10-21 (the facade, its boundary rules and the two new
members).** `integrations/rustev/` is the facade crate `rustev` (3.2):
`#![forbid(unsafe_code)]`, no binary, `rustev::contract` and
`rustev::decision_core` always, and the features present today (3.4)
`runtime`, `eval`, `rules` and `remote-http`, each enabling exactly one
optional dependency and one module named after it; `default = []`, no
feature implies another, and the documentation lists each feature with its
module, crate and subset (3.2.5). `tests/public_items.rs` fails when the
source holds anything but those re-exports, when a feature enables other
than its one `dep:`, when `default` is not empty, or when a feature is
undocumented. `tests/plan_ids.rs` compiles both reference plans through
`rustev::decision_core` from the conformance suite's pinned `plan check`
fixtures and gets the plan identities the CLI goldens pin (3.7), with no
feature and with every feature. `tools/rustev-boundaries` enforces 3.3:
the facade may depend on an integration crate only as an optional normal
dependency enabled by exactly one feature (`FacadeIntegration`), no
workspace crate depends on the facade in any dependency kind
(`DependsOnFacade`), its default features are empty (`FacadeDefault`), it
declares no ecosystem, HTTP or binding family itself (`FacadeFamily`), and
its graph with no features is checked transitively as the pure crates'
are; another integration crate depending on an integration still fails.
Each rule has a synthetic-metadata fixture. The release policy
(`tools/rustev-release`) admits `rustev-pkg-lodging` (layer 3, uploading
after the CLI it dev-depends on, as support-routing does) and the facade
(the last crate layer, alone, uploading last; 3.1): the facade alone may
declare features, each exactly one optional normal dependency on a release
member, so a feature for a crate outside the release fails (I-3), and the
set's shape refuses a facade sharing its layer or uploading early; both
have tests. `rustev-pkg-lodging` takes the family metadata, `publish =
["crates-io"]`, the archive `include` list and workspace dependencies
(amending spec 008's out-of-scope publication), and both new members join
the reviewed archive list. Subsets, MSRV and semver checks, SBOMs, the
changelog, the extended dry run and the release workflow come in later
increments.

**Increment 2, 2026-10-21 (subsets, MSRV and API compatibility as
repository checks).** `tools/rustev-release/adoption.sh` (`make adoption`)
builds each partial-adoption subset of 3.5 as a fresh consumer project
outside the workspace (its own empty `[workspace]`, the workspace's
lockfile copied in): the facade by path with `default-features = false`
and exactly the subset's features, plus the package crate for embedded
decisions; a test that names every module the subset takes and parses a
definition through the facade must run and pass; and `cargo tree -e
normal` must contain each chosen crate and none of the omitted ones. The
subsets are documents and pure compilation (no feature), embedded
decisions (`runtime`, `rules`, with `rustev-pkg-support-routing`), the
evaluation harness (`eval`), the remote backend over HTTP (`runtime`,
`remote-http`), and each remaining present feature alone (`runtime`,
`rules`, `remote-http`); `serve`, `bedrock`, `typesafe`, `otel` and
`local-embed` are reported as skipped with the reason their crate is not
in this release, never as passing. `tools/rustev-release/msrv.sh`
(`make msrv`) builds and tests every release member at the MSRV the
release policy reads from the members' `rust-version`.
`tools/rustev-release/semver.sh` (`make semver`; it needs `curl`, Node 20
or later to read the index's JSON lines, and the pinned checker, each
refused by name when missing) runs `cargo semver-checks` (exact pin 0.51.0) for each member against the newest
earlier non-yanked version in the crates.io sparse index; with none it
writes `no-baseline`, not applicable and not passed. For the npm packages
it reports `no-baseline` while neither is published and refuses a verdict
once one is, until the declaration diff exists; the service API has no
crate in this release. The CI jobs that will run these are to be
registered by a later workflow change, together with spec 027's
`bindings` job; this increment changes no workflow.

**Increment 3, 2026-10-21 (the changelog and the crates' SBOMs).** `rustev-release changelog
<out> [<previous tag>]` (`make changelog`, `PREVIOUS=<tag>`) generates the
release changelog (3.10) at a clean commit: the specs come from
`spec-spine registry list --json` at HEAD and, for a previous release, at
that tag's tree (extracted with `git archive`), never from
`.statecraft/derived/` directly; a spec is listed when it is new, its
status or implementation state changed, or its approved text changed
(every section digest but the implementation record's), with its id,
title, the decision rows that ratified or approved it, its amendment
edges and the commits since the previous release that touched it. The
compatibility facts come first: MSRV and edition from the members'
manifests, each reference plan's `PlanId` from its `plan check` golden
(compared with the previous release's, a change stated as breaking under
3.6.4), and superseded versions with their end dates (none so far). The
text is a pure function of those inputs (`changelog::render`), with unit
tests for what is listed, ordering independence, a changed `PlanId`, an
implementation-record-only change and the decision rows; two runs at one
commit produce identical bytes, and the command prints the digest the
release manifest will carry. For the first release it lists all 47
specs.
`make release-candidate` now also writes, from run a's clean tree at the
source date, a CycloneDX SBOM per crate member (`cargo-cyclonedx` 0.5.9
exactly, CycloneDX 1.5, every target's dependencies), and the changelog
of that commit; the release manifest's evidence map carries the digest of
each `sbom-<member>.cdx.json` and of `changelog.md`, and a missing one
fails the candidate (3.9.1). The SBOMs of the npm tarballs, binaries and
image are their artifact kinds' own, with those kinds.

**Increment 4, 2026-10-21 (the release checks in CI).** The CI jobs of
increment 2 are `.github/workflows/release-checks.yml`, a reusable workflow
this spec owns (3.12), registered as the required job `release-checks` by
adding it to `ci.extra_required_jobs` in `project.setup.parameters` and
re-rendering with the setup profile's revision-17 renderer, in the same
change as spec 027's `bindings` job. `adoption` is one job per subset of
3.5 (`documents`, `embedded`, `evaluation`, `remote-http`, `runtime`,
`rules`, `remote-http-alone`), each running `adoption.sh <subset>`, and the
`documents` job also reports the five subsets whose crate is not in this
release as skipped; `msrv` installs the toolchain the members'
`rust-version` declares and runs `make msrv` (3.6.7); `semver` installs the
pinned `cargo-semver-checks` 0.51.0, runs `make semver` (3.8) and uploads
its report, where `no-baseline` is recorded as not applicable, never as a
pass. Until this change merges, the MSRV stays unverified (3.6.7).

**Increment 5, 2026-10-21 (the npm members and the absences in the
release set).** Spec 027 is complete (R-47), so its npm packages join the
release family as 3.1 admits a planned member. `release-set.json` lists
them under `npm` in the npm layer's upload order (3.1's layer 6, which follows the last crate layer, spec 045 3.2.4): the seven per-platform
packages `@statecrafting/rustev-<platform>` by name, then the native main
package `@statecrafting/rustev`, then `@statecrafting/rustev-wasm`, each
with the directory its `package.json` is in. Under `absent` it lists, each
with its reason, what 3.1 plans but this release does not ship: the
prebuilt `rustev` CLI binaries (the release workflow that builds and
attaches them, 3.9.5 and 3.12, does not exist yet; the CLI ships as the
`rustev-cli` crate), the `ghcr.io/statecrafting/rustev-serve` image and
its crate `rustev-serve` (which does not exist yet; spec 010 is in
progress), and the planned crates `rustev-bedrock` (#66) and
`rustev-backend-embed-linear` (spec 011). `rustev-otel` (spec 028) exists
and is listed under `excluded` with its reason, as every workspace
package outside the family is, so it is not repeated as absent. `tools/rustev-release/src/npm.rs` checks each member's
`package.json` (`rustev-release manifests` runs it with the crates'
manifest policy): its name, the family version, the license, the
repository URL and directory, a description and a `files` list, `os` and
`cpu` for a platform package, no runtime dependency, and, for the native
main package, `optionalDependencies` naming exactly the release's platform
packages at exactly the family version; the set's shape refuses a platform
package after the main package, platform packages out of name order, the
WebAssembly package not last, a package outside the `@statecrafting/`
scope or built from another directory, and an absence with an unknown
kind, no reason, a duplicate, or the name of a member. Every package keeps
`"private": true` in the repository, as spec 027 requires until
publication, and the check requires it; the release's packing step is to
remove exactly that field (3.12). `tests/npm.rs` passes on the repository,
fixes the member and absence lists, and breaks each rule once. Packing,
the npm tarballs' SBOMs and reproducibility (3.9.1, 3.9.3), npm provenance
(3.9.6) and the release workflow come next; this increment changes no
workflow and publishes nothing.

**Increment 6, 2026-10-21 (the release workflow).**
`.github/workflows/release.yml`, a non-managed workflow this spec owns
(3.12), runs only when dispatched by hand on the release tag (`gh
workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z`); it is not a
required job. Its `tag` job refuses a tag other than the release-set
file's, a lightweight tag, a tag whose signature does not verify against
`tools/rustev-release/allowed_signers`, and a dispatch on any other ref,
so the workflow and spec 027's bindings workflow it calls are the tag's
own files. `bindings` runs that workflow at the tag (every test, the
budgets and the corpus on the seven platforms) and supplies each
platform's addon. `cli` builds `rustev` on the seven platforms of spec
027 3.13.1 in the same pinned images at the commit's source date,
checks `--help` and that `plan check` of the lodging reference plan
prints its golden bytes, archives it with the license and attests each
archive. `pack` runs `rustev-release manifests` (increment 5), builds the
WebAssembly package as `make bindings` does, stages each addon, removes
exactly the `private` guard from each `package.json` and packs the nine
npm members in the release set's order, and attests each tarball. The
attestations are `actions/attest-build-provenance` 4.2.2: keyless Sigstore
signatures over in-toto SLSA v1.0 provenance bound to this workflow, the
commit and the tag, the Build Level 2 of 3.9.5. `npm` publishes the
tarballs with npm provenance (3.9.6), in the release set's order, through
npm trusted publishing with npm 11.21.0 pinned by its registry SHA-512 integrity (OIDC; no token is stored or read and no `.npmrc` expects one), only when dispatched
with `publish-npm: true` and after the owner approves the `npm-publish`
Environment; nothing in this increment dispatches it, configures that
Environment or the trusted publisher, or publishes. Crates stay with
`rustev-release publish` (spec 021).

Open after this increment: the CLI has no version command (spec 006), so
3.9.7d's version step cannot run as written; the macOS and Windows CLI
archives are built with `bsdtar`, without a fixed entry order or times,
so they are not yet reproducible (3.9.3), while the Linux archives are;
two independent builds and the digest comparison of 3.9.3 for the npm
tarballs and binaries, the npm audit of 3.9.2, the SBOMs of the
tarballs and binaries (3.9.1), the hosted release with the manifest,
checksums, SBOMs, provenance and changelog (layer 8), and the image,
which is absent from this release (increment 5).

**Owner decision A-48 (2026-10-22): dependencies.** Specs 011, 021 and
022 are no longer declared dependencies of this spec. A crate whose spec is
not complete is listed under `absent` in the release set with its reason
(increment 5) and has no facade feature (I-3), so this spec can be
implemented and completed while those specs remain in progress. Spec 021's
publication acts follow this spec's tooling rather than precede it. Spec
010 remains a dependency.

**2026-10-23, dependency repair found by the release audit (3.9.2).** A
rehearsal of the release workflow's audit (the crates compiled into the
CLI binaries and npm tarballs, `cargo deny` 0.20.2 with the release
policy) reported RUSTSEC-2026-0386 (unsound: `tokio::io::unix::AsyncFd`'s
safe constructors) against tokio 1.53.1, reached through `rustev-runtime`
and `rustev-cli`. As spec 021 3.6 requires, the repair is its own reviewed
change: the lock takes tokio 1.53.3, the fixed release, and the workspace
requirement is raised to that floor so no registry consumer resolves an
affected version. No Rustev crate constructs an `AsyncFd`; no API or
behavior changes.

**Increment 7 part 1, 2026-10-23 (the artifact evidence commands).**
`rustev-release` gains the commands the release workflow's reproducibility,
SBOM and audit steps call (3.9.1 to 3.9.3); none of them publishes.
`compare-artifacts <a> <b>` compares two artifact sets (raw binaries,
addons, archives, npm tarballs), naming each differing file, archive entry
or container and never normalizing one away, and prints its report
(`rustev.artifact-reproducibility/1`); an empty set or an unreadable
archive is a problem, not a pass. `npm-audit <dir>` fails a tarball with
any dependency, peer or bundled dependency, install script, `node_modules`
entry or optional dependency other than a release platform package at the
family version. `sbom-check <artifacts> <sboms>` fails an artifact without
its CycloneDX SBOM. `main-sbom <tarball> <epoch>` writes the deterministic
SBOM of the native main package, which ships JavaScript and declarations
only, naming its platform packages. Archive extraction compares every path
when an artifact has no crate root. Part 2 wires these into the release
workflow.

**Increment 7 part 2, 2026-10-23 (reproducible, audited workflow
artifacts with SBOMs).** The release workflow builds every npm tarball and CLI binary
twice and requires identical bytes (3.9.3): a `rebuild` job on its own
runners repeats each platform's CLI build and addon build in the same
images and environments (the CLI as the `cli` job builds it, at the
commit's source date; the addon exactly as spec 027's bindings workflow
builds it), the CLI archives of both builds are written on Linux with GNU
tar in a fixed order, owner, mode and time and `gzip -n`, the nine npm
tarballs are packed once from the bindings workflow's addons and once from
the rebuild's (each with its own WebAssembly build), and `rustev-release
compare-artifacts` compares the two sets (raw binaries, addons, archives,
tarballs), naming each differing file, archive entry or container and
never normalizing one away; its report (`rustev.artifact-reproducibility/1`)
is a workflow artifact. Each archive and tarball gets a CycloneDX 1.5 SBOM
(3.9.1): `cargo-cyclonedx` 0.5.9 at the source date for the crate compiled
into it and its target (`rustev-cli`, `rustev-node`, `rustev-wasm`), and
for the native main package, which ships JavaScript and declarations only,
a deterministic document naming its platform packages; `sbom-check` fails
an artifact without one (section 5). The audit (3.9.2) is `npm-audit`,
which fails a tarball with any dependency, peer or bundled dependency,
install script, `node_modules` entry or optional dependency other than a
release platform package at the family version, so the npm closure is the
family alone, and `cargo deny` 0.20.2 with the release policy over the
crates compiled into the binaries and tarballs. The steps are
`tools/rustev-release/artifacts.sh`. A rehearsal mode (`rehearsal: true`)
runs all of it at a branch's commit without a tag, attesting and
publishing nothing (3.11); it is how this increment was exercised.
Rehearsal at this change's first head: every Linux and macOS binary,
addon, archive and tarball reproduced; the Windows binary and addon did
not, because the MSVC linker stamps the link time and a random PDB GUID,
so `.cargo/config.toml` passes `/Brepro` for `x86_64-pc-windows-msvc`;
and the audit found RUSTSEC-2026-0386 in tokio 1.53.1, repaired by its own
change (the 2026-10-23 dependency repair entry). The bsdtar archives and the "not yet reproducible" note of
increment 6 are gone.

**Increment 8, 2026-10-23 (`rustev-otel` joins the family).** Spec 028 is
complete, so its crate joins the release as 3.1 admits a planned member,
and the facade gains `otel` (3.4): `otel = ["dep:rustev-otel"]`, module
`rustev::otel`, documented in the feature table, with a public-items test
under the feature. `rustev-otel` takes the family metadata, `publish =
["crates-io"]`, an `include` list of its sources and the linked `LICENSE`,
and the workspace dependency `{ path, version = "=0.1.0" }`; it sits in the
crate layer `cargo metadata` computes (beside `rustev-surface`, since it
depends on `rustev-runtime`) and uploads after `rustev-backend-rules`, its
development dependency. Its own features (`otlp`, `otlp-grpc`,
`otlp-http`, spec 028 3.2.3) are the case 3.2.3 allows ("unless its own
spec adds one"): the release set records each such allowance under
`member_features` with the spec that adds it, and the policy requires a
non-facade member's declared features to equal its allowance exactly with
an empty `default`; a member without an allowance still declares none.
Tests break each rule once. The archive policy lists its exact file list
and bounds; `adoption.sh` and `release-checks.yml` run the `otel` subset
(the facade with `otel` alone; the OpenTelemetry crates, which are
optional in `rustev-otel`, and every other feature's crate absent).
`rustev-serve` stays excluded until spec 042 admits it.

**Increment 10, 2026-10-23 (the npm declaration check of 3.8).**
`tools/rustev-release/npm-api-diff.mjs` compares two versions of an npm
package's declaration surface with the TypeScript compiler API, pinned at
the 5.9.3 the conformance tooling's lockfile holds (refused by name
otherwise): every export, reached through the `types` and `exports`
entry points and the package's relative re-exports, with each member of
its interfaces, classes, enums and aliases and each signature, in one
program holding both versions. A removed export or member, an added
required member or parameter, an optional one made required, an output
type widened, an input type narrowed or a writable property's type
changed either way (judged by the checker's assignability), or a change the checker cannot judge, is breaking
(3.6.5); additions and widened inputs or narrowed outputs are compatible.
Its report (`rustev.npm-api-diff/1`) is deterministic. The version rule is
3.6.4's: a breaking finding passes only when the version increments the
breaking position over the baseline. `semver.sh` now takes, for each npm
package with a published earlier version, the newest non-deprecated one
below the release, downloads its tarball, verifies it against the
registry's `sha512` integrity before unpacking (regular files only, no
links or paths outside the root), and runs the check; with no earlier
version it still reports `no-baseline`, not applicable and not passed, and
a registry or download failure is no verdict. The WebAssembly package's
declarations are generated by its build (`make bindings`), so its check
needs that build first and gives no verdict without it. The `semver` job
of `release-checks.yml` installs the pinned TypeScript and runs the
check's tests (`node --test`): removed and narrowed exports, members and
parameters, widened returns, compatible additions, a re-export through a
second file, determinism, the version rule, and the native package against
itself. The service API has no member in this release (`rustev-serve` is
excluded until spec 042 admits it), so it has nothing to diff.

## Decisions at approval

1. **Crate and package names** (left to the implementer by #58): decided
   within this spec. Crate `rustev` for the facade; existing crate names
   unchanged; new crates keep the names of their issues (`rustev-bedrock`,
   `rustev-otel`, `rustev-serve`). npm, aligned with spec 027: recommended
   `@statecrafting/rustev` (native, with per-platform
   `@statecrafting/rustev-<platform>` packages) and
   `@statecrafting/rustev-wasm` under the owner's `@statecrafting` scope,
   falling back to unscoped `rustev-node` and `rustev-wasm` if the scope
   cannot be used; all were
   unregistered on 2026-10-16 (2.1). Image `ghcr.io/statecrafting/rustev-serve`.
   Availability is rechecked before upload.
2. **Facade module for the core**: `rustev::decision_core`, decided within
   this spec (3.2.1).
3. **Default features**: none, settled by spec 021 3.3 which this amendment
   keeps.
4. **Facade location `integrations/rustev/` and the 3.4.4 exception**: #58
   names the path `crates/rustev`. It is changed to `integrations/rustev/`
   (crate name `rustev` unchanged) because #76 lists "only `integrations/*`
   depend on HTTP stacks, cloud SDK crates, Node-API, `wasm-bindgen` or
   telemetry exporters" as a principle not relaxable without the owner, and a
   facade under `crates/` reaching an HTTP stack transitively through its
   features would be read as relaxing it. Under `integrations/` the principle
   holds literally. This changes a path, not a requirement of #58. The
   narrow 3.4.4 and 010 I-2 exception of 3.3 is still needed, because
   integration crates may not depend on one another, and it is the narrowest
   that allows the issue's integration features.
5. **`typesafe` and source-linked crates**: the feature waits until the crate
   is self-contained; how that is achieved is left to a later reviewed
   change (3.4).
6. **SLSA level**: Build Level 2 claimed, Level 3 only per artifact with
   verified isolated provenance, decided within this spec.
7. **First-release publication**: settled by the work order as quoted in
   3.11, under the conditions there.
8. **`rustev-node` and `rustev-wasm` on crates.io**: not published; npm only,
   decided within this spec.

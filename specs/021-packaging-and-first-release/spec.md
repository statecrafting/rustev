---
id: "021-packaging-and-first-release"
title: "Packaging and first public release"
status: approved
implementation: pending
created: "2026-09-26"
summary: >
  Defines the intended first public Rustev crate family, its lockstep pre-1.0
  version policy, registry contents and features, internal dependency pins,
  license and dependency audit, reproducible package checks, signed release
  identity, publication journal, and fresh registry-only consumer proof.
  Candidate construction, owner publication authorization, registry upload,
  public availability, and consumer qualification are distinct lifecycle
  states. Approval authorizes candidate implementation only: no crate is
  publishable yet and no release, tag, upload, name reservation, or
  availability claim is authorized or made.
amends:
  - "006-cli-surface"
  - "007-support-routing-package"
extends:
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-contract/Cargo.toml" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-core/Cargo.toml" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: file, path: "crates/rustev-runtime/Cargo.toml" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: file, path: "crates/rustev-eval/Cargo.toml" }, nature: amending }
  - { spec: "005-reference-backends", unit: { kind: file, path: "backends/rustev-backend-rules/Cargo.toml" }, nature: amending }
  - { spec: "006-cli-surface", unit: { kind: file, path: "crates/rustev-cli/Cargo.toml" }, nature: amending }
  - { spec: "007-support-routing-package", unit: { kind: file, path: "packages/rustev-pkg-support-routing/Cargo.toml" }, nature: amending }
  - { spec: "009-remote-adapter-protocol", unit: { kind: file, path: "integrations/rustev-remote-http/Cargo.toml" }, nature: amending }
  # Each archive's license text (3.3): a link to the root LICENSE.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-contract/LICENSE" }, nature: additive }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: file, path: "crates/rustev-core/LICENSE" }, nature: additive }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: file, path: "crates/rustev-runtime/LICENSE" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: file, path: "crates/rustev-eval/LICENSE" }, nature: additive }
  - { spec: "005-reference-backends", unit: { kind: file, path: "backends/rustev-backend-rules/LICENSE" }, nature: additive }
  - { spec: "006-cli-surface", unit: { kind: file, path: "crates/rustev-cli/LICENSE" }, nature: additive }
  - { spec: "007-support-routing-package", unit: { kind: file, path: "packages/rustev-pkg-support-routing/LICENSE" }, nature: additive }
  - { spec: "009-remote-adapter-protocol", unit: { kind: file, path: "integrations/rustev-remote-http/LICENSE" }, nature: additive }
  # The release tool spec 031 plans; this spec's candidate machinery lives
  # there (031, 2.2).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "007-support-routing-package"
  - "009-remote-adapter-protocol"
references:
  - { unit: { kind: file, path: "README.md" }, role: context }
  - { unit: { kind: file, path: "LICENSE" }, role: context }
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A passing package check, a signed candidate, an approved release spec, an uploaded crate, a visible registry version and a qualified fresh consumer are distinct states and none is reported as another."
    anchor: "3-1-lifecycle-and-authority"
  - id: "I-2"
    kind: invariant
    text: "Every published Rustev dependency resolves from the public registry at an exact family version; no released package or qualification uses a path, Git, patch, alternate registry or unpublished workspace crate."
    anchor: "3-4-manifests-and-dependency-closure"
  - id: "I-3"
    kind: invariant
    text: "A release identity is one exact commit, signed tag, artifact manifest and immutable registry checksum set; an ambiguous or partial publication is never rerun as though nothing happened."
    anchor: "3-8-signed-release-identity-and-artifacts"
  - id: "I-4"
    kind: invariant
    text: "Publication requires a fresh explicit owner instruction naming the version, exact candidate commit, registry and package set after all candidate evidence passes."
    anchor: "3-9-publication-procedure-and-failure"
---

# 021: Packaging and first public release

Approved (A-20, 2026-09-26) as its own reviewable change before any code
(R-16); candidate implementation is pending. Approval does not reserve a name,
create or push a tag, publish a crate, create a hosted release, use a registry
credential, or claim that a consumer can obtain Rustev. The current `0.1.0`
and `publish = false` manifests remain unreleased source identities.

## 1. Purpose

Make the first release a reproducible, reviewable product-family operation
rather than a sequence of local `cargo publish` commands. Define exactly what
ships, why each crate is in the family, how the graph is packaged without
workspace paths, which evidence precedes an owner publication act, and what
fresh evidence is required before saying the release is accessible.

This spec amends the CLI and support-package exclusions of publication in
specs 006 and 007. It changes no decision, runtime, evaluation, backend,
protocol, or authority semantics.

## 2. First-release scope

The intended first family version is `0.1.0`. Approval fixes
that candidate version, not publish it. The publish set and order are:

| Layer | Package | Public purpose |
|---|---|---|
| 1 | `rustev-contract` | Versioned documents, bounded parsing, canonical form and identities. |
| 2 | `rustev-core` | Pure compilation, values, exact operations and judgment. |
| 3 | `rustev-runtime` | Bounded execution and run evidence. |
| 3 | `rustev-eval` | Offline replay, evaluation, gates and calibration fitting. |
| 3 | `rustev-backend-rules` | Deterministic identified reference backend. |
| 3 | `rustev-remote-http` | Neutral remote protocol HTTP client, server and conformance machinery. |
| 3 | `rustev-pkg-support-routing` | Synthetic reference decision package and evaluation material. |
| 4 | `rustev-cli` | The `rustev` command-line host. |

Layers are topological. A layer is uploaded only after every required earlier
package and exact version are observable from the public registry with their
expected checksums. Packages in one layer may be ordered deterministically by
name, but are not treated as an atomic registry transaction.

The first release excludes:

- `rustev-jev`: it source-links sibling integration files and has provider and
  qualification boundaries not satisfied by a standalone registry archive;
- `rustev-boundaries`: repository governance tooling, not a product crate;
- separately implemented future packages and integrations, including lodging, Aicortex,
  Rahi and serving; and
- prebuilt CLI binaries, containers, models, weights, provider credentials,
  real datasets, production configuration and generated governance shards.

Exclusion is not a quality judgment. Adding a package or binary artifact needs
a later reviewed release amendment and its own package and consumer evidence.

## 3. Contract

### 3.1 Lifecycle and authority

The release advances through named states:

1. **specified**: this spec is owner-approved;
2. **implemented**: manifests, metadata, checks and documentation implement the
   approved contract, with repository gates passing;
3. **candidate-verified**: an exact clean commit produced reproducible package,
   audit, signature-input and dry-run evidence;
4. **publication-authorized**: the owner issued a fresh instruction naming the
   exact candidate commit, version, public registry and eight-package set;
5. **partially published** or **published**: immutable registry uploads are
   observed and checksummed package by package;
6. **released**: the signed tag and hosted release point at the authorized
   commit and expose the complete artifact and journal set; and
7. **qualified**: a fresh unauthenticated registry-only consumer passed every
   required public-surface probe against the observed checksums.

Only `qualified` supports the claim that the first release is publicly
accessible. `published` without consumer proof is not `qualified`.
`candidate-verified` and approval of this spec are not publication authority.
Ratification, merge, tag push, registry upload and hosted-release creation are
separate acts. Registry credentials are never inspected during specification,
ordinary implementation, review, or candidate checks.

### 3.2 Version policy

The eight first-release packages use one lockstep family version. Every
internal normal, build and development dependency names the same exact version
with `=X.Y.Z`; a local path may accompany it only while building the workspace
before publication. Registry packages must resolve using the version alone.

Before 1.0:

- a compatible correction to any shipped member increments the patch version
  of the whole family;
- a public API, Cargo feature, wire compatibility, MSRV, package membership,
  or other consumer-contract change increments the minor version of the whole
  family; and
- a yanked, compromised, incomplete or incorrectly packaged version is never
  overwritten or reused. Its repair receives a new family version.

Wire document versions, plan identities, artifact identities and decision
package versions remain their own contracts. A Cargo version bump neither
silently migrates their bytes nor proves their compatibility.

The release tag is exactly `vX.Y.Z`. Branch names, commit abbreviations,
moving aliases, registry index timestamps and hosted-release numeric ids are
not release identities.

### 3.3 Public metadata and features

Each publishable manifest provides the shared edition, MSRV, Apache-2.0
license expression, repository, homepage, documentation target, README,
description, keywords and categories permitted by the registry. Names,
descriptions and links are checked for accuracy at the candidate commit.
Each archive includes the complete Apache-2.0 license and the release README.

The first release declares no public Cargo features. Optional product slices
remain separate crates; default features cannot silently select a network,
provider, runtime, backend or integration. Adding or changing a feature is a
minor-version contract change under 3.2.

The public README states the exact released scope and limitations. It does not
describe draft specs as implemented, a synthetic report as quality evidence,
an adapter as a qualified provider, a proposal as authorization, or a registry
candidate as an accessible release.

### 3.4 Manifests and dependency closure

Implementation replaces `publish = false` only for the eight packages in
section 2 and restricts publication to the intended public registry. Every
workspace dependency in their normal, build and development graphs has an
exact registry version. `cargo metadata` over the package graph must show no
Git source, alternate registry, patch replacement, unpublished member, or
dependency on `integrations/*` from a forbidden layer.

`Cargo.lock` is generated by Cargo and committed where the workspace and CLI
policy require it. A published library does not rely on the workspace lockfile
to constrain its consumer. The candidate resolves from a freshly fetched
public index under the declared MSRV and current supported stable toolchain.

The `rustev-cli` binary is installed from package `rustev-cli`; the package and
binary names are reported distinctly. The excluded Jev crate cannot be pulled
into any archive through source paths, tests, examples, dev dependencies or
workspace inheritance.

### 3.5 Registry contents

For each crate, `cargo package --list` is captured and compared with a reviewed
allowlist. An archive contains only the manifest normalized by Cargo, source,
public tests and examples needed by that crate, selected bounded fixtures, the
README and license. The support-routing package includes its declared
synthetic definition, calibration and evaluation fixtures because its public
API embeds them.

Archives exclude `.git`, `.statecraft`, `.tooling`, `target`, evidence and
scratch directories, credentials, environment files, provider responses,
real-user data, local paths, editor state, unreleased sibling sources and
unrelated design or discussion material. Generated Cargo metadata is labeled
as generated. Every file, archive and unpacked byte total is bounded in the
candidate policy and measured before signing; exceeding a reviewed bound is a
failure, not an automatic bound increase.

The candidate performs a prohibited-content and secret-pattern scan over the
exact unpacked archives. A scanner pass is evidence only for its ruleset and
version. It is not proof that arbitrary secrets cannot exist.

### 3.6 Dependency, license, and advisory audit

The exact resolved normal and build dependency graph is recorded with package
name, version, source, checksum and SPDX license expression. Development
dependencies are recorded separately. A repository-pinned audit tool and
policy refuse unknown sources, missing license data, denied licenses,
unreviewed duplicate major versions, and advisories not covered by a named,
owner-approved, expiry-bounded exception.

License files and attribution required by every included dependency and
fixture are present in the archives or release artifacts. The audit records
which obligations were checked; it does not make a legal conclusion. A
dependency or fixture with uncertain redistribution terms blocks the affected
package. The process never changes dependency versions merely to make an audit
green without a separately reviewed source change and ordinary verification.

### 3.7 Reproducible package verification

Candidate verification starts from the exact clean commit in two independent
clean worktrees under the required worktree root. It uses recorded Rust, Cargo,
target, spec-spine and audit-tool identities and a fixed source-date input.
For each package, both runs execute ordinary repository gates and
`cargo package --locked` without `--allow-dirty` or `--no-verify`, capture the
file list, unpack the archive, and compare canonical file digests and final
`.crate` SHA-256. A mismatch blocks the release and is explained, never
normalized away after the fact.

The unpacked package is then tested outside the workspace with no workspace
manifest, path override, Git override, patch section or unpublished source.
The candidate matrix includes the declared MSRV and current supported stable
Rust on each owner-approved target. Untested targets are unsupported, not
inferred from a successful cross-compile.

All spec acceptance, `make gate`, `make code`, `make verify`, documentation
tests and package checks pass at the candidate commit. Expected draft-only
governance warnings remain lifecycle facts and must be resolved by owner policy
before a release candidate can be declared green; they are not waived by this
spec.

### 3.8 Signed release identity and artifacts

The candidate evidence produces a canonical release manifest containing:

- version, exact full commit id and source tree id;
- ordered package set and dependency layers;
- each `.crate` filename, byte length and SHA-256;
- expected registry checksum, equal to the exact archive SHA-256, while actual
  observation remains in the append-only publication journal;
- toolchain, target, audit policy and source-date identities;
- gate and package-evidence digests;
- dependency, license and advisory report digests; and
- every limitation, unsupported target and unresolved item.

The owner-authorized commit receives one signed annotated `vX.Y.Z` tag. The
tag message names the release-manifest digest and package set. `git verify-tag`
must validate against the documented owner signing identity. The canonical
manifest and `SHA256SUMS` receive detached signatures from that same documented
release identity. Verification instructions and the public key or allowed-key
record are release artifacts. No private key or credential enters Git, logs,
CI artifacts or the repository evidence directory.

Hosted release artifacts are the signed manifest, signatures, checksums,
dependency and license report, software bill of materials, release notes and
the exact `.crate` archives whose digests the manifest names. No prebuilt
binary is implied. Platform-generated source archives are convenience copies;
the signed Git tag and recorded tree are the source identity.

### 3.9 Publication procedure and failure

After candidate verification, work stops for the owner publication instruction
defined by I-4. Immediately before each upload, the operator verifies name and
version availability, candidate commit, tag signature, archive digest, registry
identity, account identity and current journal. The command uploads the exact
already-verified archive, never a rebuilt archive.

Each attempt is appended before execution to an outside-Git publication
journal with package, version, archive digest and intended registry. Its result
then records command status, registry response, observed index identity,
checksum and time. Secrets and tokens are redacted. A successful or ambiguous
attempt is never repeated blindly. The operator queries public state first.

If an upload fails before acceptance, the journal records the refusal and the
operator stops. If acceptance is ambiguous, or any earlier layer is visible
while a later package fails, lifecycle becomes `partially published`; no tag,
version or history is deleted or reused. The owner chooses whether to continue
the exact authorized set, yank an affected crate, or prepare a new version.
Yanking is not deletion and requires its own explicit owner instruction.

The hosted release is created only after all eight registry checksums match.
Release notes list the exact public scope, install commands, MSRV, supported
targets, authority boundary, known limitations and excluded crates. They do
not claim production readiness, provider quality or compatibility beyond the
evidence.

### 3.10 Fresh registry-only consumer qualification

Qualification waits for public index and artifact availability, then starts in
a new temporary directory with a new empty Cargo home and no registry token.
It consumes the public registry by exact `=X.Y.Z` versions. It forbids path and
Git dependencies, `[patch]`, alternate registries, local source replacement,
workspace inheritance and a primed artifact cache.

At minimum, the consumer:

1. fetches all eight exact packages and records registry checksums;
2. builds and tests a small library using contract, core, runtime and the rules
   backend to compile and execute a bounded synthetic decision;
3. loads the support-routing package and reproduces its declared synthetic
   offline example without network access;
4. compiles a remote HTTP client and loopback server example without calling a
   provider;
5. installs `rustev-cli =X.Y.Z` with `--locked` and runs version, help, plan
   check and one bounded offline synthetic example; and
6. reruns the supported toolchain and target matrix from registry sources.

The consumer records source URLs, checksums, lockfiles, command output and
environment identity. Anonymous failure, index lag, a checksum mismatch, an
undeclared network need or resolution to a different version blocks
`qualified`. The proof is fresh for that registry state and matrix; it is not
a permanent availability guarantee.

## 4. Observable negative cases

| Case | Expected |
|---|---|
| Local `cargo package` succeeds before owner authorization | Candidate evidence only; no upload, tag push or release claim. |
| A packaged manifest retains only a path for an internal dependency | Package check fails; no publication. |
| A package resolves an internal crate at `0.1.1` during a `0.1.0` release | Exact-family closure fails. |
| The Jev sibling source is present in any archive | The affected archive and release candidate fail. |
| Two clean builds produce different `.crate` digests | Reproducibility fails and the release stops. |
| One registry upload returns an ambiguous result | Query public state, record ambiguity, stop; never blind retry. |
| Four crates are public and the fifth fails | State is `partially published`, never released or qualified. |
| A consumer succeeds through a workspace path or warm Cargo cache | The proof is invalid and must be repeated cleanly. |
| A hosted release exists but anonymous exact-version fetch fails | Not qualified and not claimed publicly accessible. |
| An advisory exception has expired | Audit fails until the owner renews or the dependency change passes review. |

## Acceptance

- A machine-checked release-set file or equivalent test fixes the eight names,
  version, layers, exclusions and exact internal dependency policy.
- Manifest tests prove complete metadata, no public features, intended registry
  restriction, package versus binary naming, and absence of excluded closure.
- Each exact archive passes allowlist, byte bounds, prohibited-content scan,
  license and advisory policy, independent unpacked verification and the
  two-build reproducibility comparison.
- The candidate passes the declared MSRV, stable and target matrix plus all
  repository gates without changing or waiving an expected failure.
- A dry-run state machine covers every lifecycle transition and section 4 row
  without credentials, network writes, tag creation or publication.
- Signature fixtures prove valid commit, tree, tag, manifest, checksum and key
  binding, and reject substitution of any one member.
- A local fake-registry harness tests topological upload, propagation waits,
  refusal, ambiguity, partial publication and immutable-version recovery.
- After actual publication under fresh owner authority, the anonymous
  registry-only consumer in 3.10 passes and its immutable evidence is retained.
- `make gate`, `make code`, `make verify` and every declared spec acceptance
  pass at the exact candidate commit.

## Verification

Pending with implementation and release. Approval authorizes work on
manifests, checks and candidate evidence only. Commands that create or push a
tag, upload to a registry, create a hosted release, yank a version, access a
credential or make a public availability claim remain outside that authority
until the owner gives the exact instruction required by 3.9.

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it, and no increment
uploads, tags, signs with the owner identity or reads a registry credential.
Plan, in dependency order:

1. Release set and manifest policy (2, 3.2 to 3.4).
2. Archive policy: per-package allowlists, byte bounds and the
   prohibited-content scan (3.5).
3. Unpacked verification outside the workspace and the two-build
   reproducibility comparison (3.7).
4. Dependency, license and advisory audit with a pinned tool and policy
   (3.6).
5. The canonical release manifest, `SHA256SUMS` and the signature binding
   fixtures (3.8).
6. The dry-run lifecycle state machine, the publication journal and the
   fake-registry harness (3.1, 3.9, section 4).
7. Candidate evidence at an exact commit (3.7), then the owner's
   publication instruction (I-4), publication, and the registry-only
   consumer (3.10).

**Increment 1 (release set and manifest policy).** `tools/rustev-release/`
(the release tool spec 031 owns and says holds this spec's candidate
machinery, 031 2.2; `publish = false`, an exclusion) holds `release-set.json`
(`rustev.release-set/1`: version `0.1.0`, tag `v0.1.0`, crates.io, the four
layers of section 2, the upload order, the `rustev` binary of package
`rustev-cli`, and the exclusions with reasons) and `rustev-release
manifests`, which judges `cargo metadata` against it. The eight manifests
inherit the family version, the shared edition, MSRV, license, repository,
homepage and README, name their docs.rs target, keywords and categories,
declare `publish = ["crates-io"]` and no feature, and take every internal
dependency from `[workspace.dependencies]` as `{ path, version = "=0.1.0" }`;
each package directory links the root `LICENSE`, so the archive carries the
complete license text. Layers are computed over normal and build
dependencies, as section 2's table is. Upload order is topological over
every dependency kind, because the registry refuses an upload whose
development dependency is not yet public: within layer 3 it runs runtime,
rules backend, eval, remote HTTP, and `rustev-pkg-support-routing` uploads
after `rustev-cli`, whose library its tests drive (spec 007, `rustev eval`
over the dataset). That ordering still uploads every package only after
each package it requires (3.9). `tools/rustev-release/tests/manifests.rs`
passes on the workspace and breaks each rule once: a path-only or
non-exact internal requirement (section 4 row 2 and 3), a dependency of any
kind on an excluded crate, publication unrestricted or by an exclusion,
each metadata field, a feature, a Git, alternate-registry or patched path
source, the binary name, a layer and an upload order.

**2026-10-19, dependency repair found by the audit (3.6).** The pinned
cargo-deny 0.20.2 reports RUSTSEC-2026-0346, 0347, 0348 and 0349 against
rustls 0.23.45, which `rustev-remote-http` reaches. As 3.6 requires, the
repair is its own reviewed change rather than part of the audit increment:
the workspace requirement for rustls moves to `0.23.46` (the fixed
release), so a registry consumer cannot resolve a vulnerable version, and
`Cargo.lock` follows. No API or behavior of spec 009 changes.

**Increment 2 (archive policy).** Each manifest's `include` limits its
archive to `src/**/*.rs`, `LICENSE` and what the package embeds
(`rustev-pkg-support-routing`'s `data/**/*.json`, which its public API
embeds; `rustev-cli`'s `tests/golden/support.record.json`, which a unit
test embeds); Cargo adds the README and its generated `Cargo.toml`,
`Cargo.lock` and `.cargo_vcs_info.json`. Integration tests stay in the
workspace: they share harnesses and fixtures across sibling crates, so no
archive could hold them whole. `tools/rustev-release/archive-policy.json`
(`rustev.archive-policy/1`) is the reviewed allowlist: per package the
exact file list, the archive and unpacked byte bounds, a per-file bound,
and which files Cargo generated. `rustev-release archives <dir>` unpacks
each `.crate` `cargo package` wrote, compares it with the list, measures
the bounds, runs the prohibited-content scan `rustev.archive-scan/1`
(private-key headers, cloud, registry, source-host and model-provider
token shapes, local home-directory paths; prohibited path components such
as `.git`, `.statecraft`, `target`, `.env*`, keys and credentials) and
prints the evidence: per file its size, SHA-256 and whether Cargo
generated it, and the `.crate` SHA-256. A scan pass is evidence for that
ruleset only (3.5). `tests/archives.rs` compares every list with `cargo
package --list` and breaks each rule once; the bounds are fixed in the
policy, measured from the first packaging, and a check never raises them.

**2026-10-19, MSRV correction (3.7).** The declared MSRV was 1.85, which
no check had exercised (spec 031 2.1 records the same). Built at the
declared version for the candidate matrix, `rustev-core` fails: it uses
`let` chains, stable since 1.88. `cargo +1.88.0 check --locked` of the
eight members passes and 1.87 cannot accept `let` chains, so the workspace
declares `rust-version = "1.88"`, the true minimum, before any release
(no published version is affected). This is its own reviewed change, as
spec 031 3.6.7 asks of an MSRV change; the candidate matrix of increment 7
builds and tests the unpacked archives on it. With the MSRV at 1.88,
clippy's `collapsible_if` proposes `let` chains and its
`manual_is_multiple_of` applies; `cargo clippy --fix` collapsed the nested
`if`s it named (12 files) and rewrote one `% 10 == 0` as `is_multiple_of`,
recorded in each owning spec, with no behavior change.

**Increment 3 (unpacked verification and reproducibility).** `make
release-candidate` runs `tools/rustev-release/candidate.sh` on the clean
commit at HEAD (a dirty tree is refused): two detached worktrees under the
worktree root each set every tracked file's time to the commit time (the
fixed source-date input) and run `cargo package --locked` for the eight
packages, without `--allow-dirty` or `--no-verify`; `rustev-release
compare` then names every difference between the two runs' archives, by
file or, when every file is identical, as container metadata, and never
normalizes one away; `rustev-release archives` records the archive
evidence of increment 2; and `rustev-release unpacked` tests each unpacked
archive outside any workspace (a work directory inside one is refused),
offline, with `--locked`, against a directory source holding the exact
candidate archives (each with its `.crate` SHA-256 as package checksum)
and the workspace's locked third-party crates from `cargo vendor`: no
workspace manifest, path or Git override, `[patch]` or network. The
toolchain is the pinned one or one named for the matrix (3.7). Evidence
lands in the ignored `target/release-candidate/<commit>/`, with
`SHA256SUMS` of run a. `tests/repro.rs` covers identical runs, a changed
file, a file in one run only, identical files in differing containers
(section 4 row 5), a missing archive, the vendored checksum and the
offline source configuration.

**Increment 4 (dependency, license and advisory audit).** The audit tool
is cargo-deny, pinned at 0.20.2 (`make release-candidate` refuses another
version); its policy is `tools/rustev-release/deny.toml`, run over the
eight members with development dependencies excluded: unknown registries
and Git sources fail; licenses must be one of a reviewed allowlist
(Apache-2.0, its LLVM exception, BSD-3-Clause, CDLA-Permissive-2.0, ISC,
MIT, Unicode-3.0, Unlicense), so missing license data fails too; every
vulnerability, unmaintained, unsound or yanked finding fails; a second
version of a crate fails unless listed with its reason (today only
`windows-sys` 0.52, which ring 0.17 pins). An advisory is ignored only
when `advisory-exceptions.json` (`rustev.advisory-exceptions/1`) names it
with a reason, the owner's approval and an expiry date, and
`rustev-release exceptions <date>` fails an ignore without one, an
expired or undated exception (section 4 row 10), or an approved one not
applied. `rustev-release deps` writes the dependency report
(`rustev.dependency-report/1`): every package of the members' normal and
build closure with version, source, lockfile checksum and SPDX expression,
and what development dependencies add, apart. The archives redistribute no
dependency source (increment 2), so license obligations of dependencies
attach to future binary artifacts, not to these archives; the report
records which licenses were checked and draws no legal conclusion. The
first run found RUSTSEC-2026-0346 to 0349 in rustls 0.23.45; their repair
is the separate change recorded above. `tests/audit.rs` covers the report
over the real workspace and each exception rule.

**Increment 5 (release manifest and signature binding).**
`rustev-release manifest <evidence-dir>` writes, for the clean commit at
HEAD, the canonical `release-manifest.json` (`rustev.release-manifest/1`:
compact JSON in field order, so equal inputs give equal bytes) with the
version, tag, full commit and tree ids, registry, layers and upload order,
each archive's file name, length and SHA-256 (the checksum the registry is
expected to record; the observed one belongs to the journal), the rustc,
Cargo, host and audit-tool identities, the source date, the SHA-256 of
every evidence file of the candidate run, and the stated limitations; and
`SHA256SUMS` over the archives. `make release-candidate` ends with it.
Signing is the owner's act and nothing here signs or reads a private key.
The documented owner identity is the owner's Git SSH signing key,
committed as `tools/rustev-release/allowed_signers` (public key only);
file signatures use the SSH namespace `rustev-release`, and the tag is the
owner's signed annotated `v0.1.0` whose message names the manifest digest
and the package set. `rustev-release verify-release <dir> [<repo>]` checks
every binding: both detached signatures under that identity, the archives
against the manifest and `SHA256SUMS`, the tag's type, signature, commit
and message, and the commit's tree. `tests/signing.rs` builds each fixture
from a throwaway key generated at test time and rejects the substitution
of each member: an archive, the manifest's signer, the manifest's content,
the checksums (even re-signed), the tag's commit, the manifest's tree, the
tag's message, an unsigned or lightweight tag, and a non-canonical
manifest.

**Increment 6 (lifecycle, journal and fake registry).**
`tools/rustev-release/src/lifecycle.rs` holds the lifecycle of 3.1 as a
state machine over its named states: each event applies only in its state
and otherwise is refused with the state unchanged, so no state is skipped
or reported as another (I-1); candidate evidence must be for the
manifest's commit, and the owner's instruction must name exactly its
version, commit, registry and package set (I-4); only `qualified` answers
the public-accessibility question. The publication journal is append-only
JSON lines (attempt before each upload, its result after, every query of
public state, and why a run stopped); an upload is refused after an
accepted result, or after an ambiguous one until a recorded query found
the package absent (3.9). The publisher uploads only the verified
archives, by checksum, in the manifest's order; skips a package already
public with the same checksum and stops when a version is public with
other bytes (a spent version needs a new family version); waits, after
each acceptance, until the package is visible with its expected checksum
before the next; and stops on a refusal, an ambiguity (after querying and
recording public state) or a package that never appears.
`tests/lifecycle.rs` drives it against `FakeRegistry`, a scripted
in-memory registry: every transition in order, refused out-of-order
events, local packaging before authorization (section 4 row 1), a wrong
instruction, propagation waits and the journal's order, a package that
never appears, a refusal, four public and the fifth failing (row 7,
`partially published`, never released), an ambiguous upload in both
outcomes and its recovery (row 6), an ambiguous journal entry blocking a
retry, a rerun skipping public packages, a rebuilt archive and a spent
version, and a release without consumer proof (rows 8 and 9). No
credential, network write or tag is involved; the adapter for the real
registry belongs to the publication step.

Open for the owner: spec 031 3.1, which amends this section, orders each
layer by name and starts a layer only after every package of the earlier
layers is public. With today's development dependencies that order cannot
be uploaded (the rules backend's tests use the runtime, and the
support-routing package's tests use `rustev-cli`). This spec's text allows
the order above; reconciling 031 3.1 (an ordering amendment, or removing
those development dependencies from published manifests) is the owner's
choice before 031 is implemented.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Are the eight packages the desired first public family, or should the
   remote adapter and synthetic reference package wait for a later release?
2. Which public registry account or organization should own the names, and are
   all eight names available at candidate time?
3. Which targets beyond the CI host must the first release support and qualify?
4. Which pinned audit tool, license allowlist, advisory policy and artifact
   signing identity should the implementation adopt?
5. Should public API documentation be hosted only by the registry service or
   also built and attached under a separately governed documentation policy?

---
id: "056-release-0-1-1-and-consumer-qualification"
title: "The next family release: a CLI version command, declared consumer examples and the fresh-consumer qualification (amends 006, 007 and 031)"
status: approved
implementation: in-progress
created: "2026-10-26"
summary: >
  A narrow amendment of approved specs 006, 007 and 031, written after
  `v0.1.0` was published to crates.io, npm and GitHub on 2026-10-26 but could
  not be qualified (021 3.10, 031 3.9.7). The CLI has no version command;
  the support-routing package declares no runnable offline example (its
  SYNTHETIC rules program is a test fixture of the rules backend); no
  harness runs the registry-only consumer across the platform matrix; and
  the npm packages carry no provenance. This spec adds `rustev version`,
  ships the package's SYNTHETIC example as package data, adds the
  qualification harness and its workflow, and defines how the owner
  authorizes a release after `0.1.0`, so that the next family version is
  published through the existing pipeline with npm provenance and qualified. Ratified by the
  owner on 2026-10-26 (A-58). Revised the same day (A-59): the work is
  folded into the next family version (`0.2.0`); there is no separate
  `0.1.1`.
amends:
  - "006-cli-surface"
  - "007-support-routing-package"
  - "031-public-api-and-release-family"
extends:
  # The version command (3.1).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The declared SYNTHETIC example (3.2).
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: amending }
  # The qualification harness, the release authority and the version (3.3, 3.4).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: amending }
depends_on:
  - "006-cli-surface"
  - "007-support-routing-package"
  - "021-packaging-and-first-release"
  - "031-public-api-and-release-family"
  - "053-signed-release-candidates"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "`rustev version` and `rustev --version` print `rustev <version>` and a newline to standard output, where `<version>` is the `rustev-cli` package version, and exit 0; both take no other argument."
    anchor: "3-1-the-version-command"
  - id: "R-2"
    kind: requirement
    text: "`rustev-pkg-support-routing` ships, as package data reachable from its public API, its SYNTHETIC rules program, the calibration bound to it, one SYNTHETIC snapshot and the outcome expected of it, so that a consumer with only registry packages reproduces the example offline."
    anchor: "3-2-the-declared-example"
  - id: "R-3"
    kind: requirement
    text: "The qualification harness runs every step of 021 3.10 and of 031 3.9.7 for each artifact kind the signed manifest names (crates, npm, CLI binaries; 3.9.7 (c) only when the manifest names an image) against the public registries only, from an empty Cargo home and npm cache with no registry credential, and writes per step its command, output, lockfile or install record, checksums and environment identity; a step it cannot run is reported as not run with its reason, never as passing."
    anchor: "3-3-the-qualification-harness"
  - id: "R-4"
    kind: requirement
    text: "The release lifecycle reaches `qualified` only through `rustev-release qualify`, which records the digest of each required evidence file of the harness and refuses when one is missing, failing or `not_run`; a step is `excluded`, and not required, only when the signed manifest names no artifact of its kind."
    anchor: "3-3-the-qualification-harness"
  - id: "I-1"
    kind: invariant
    text: "A release after `0.1.0` is published only under an owner instruction whose bytes carry the owner's detached SSH signature in the `rustev-release` namespace, verified against `tools/rustev-release/allowed_signers`; an unsigned instruction for such a version is refused before any journal entry or upload."
    anchor: "3-4-authority-for-a-later-release"
---

# 056: The next family release, a CLI version command, declared consumer examples and the fresh-consumer qualification (amends 006, 007 and 031)

Approved: ratified by the owner on 2026-10-26 (decision A-58). The
approved texts of 006, 007 and 031 are not edited; this spec records what
it adds to them. Spec 021 is not amended: its fresh-consumer steps (3.10)
and version policy (3.2) are implemented here as written.

Revised on 2026-10-26 by the owner's decision A-59 to fold this work into
the next family version (`0.2.0`) instead of a separate `0.1.1`: the title,
summary, territory, 3.5 and the version named in sections 3 to 4,
Acceptance and Verification now say "the next family version". The
specification identifier keeps its original name. This revises 056's own
text, ratified by its owner; no other spec is amended, so no `amends`
edge applies.

## 1. Purpose

`v0.1.0` (commit `9dbf8e0`) was published on 2026-10-26: twelve crates on
crates.io, nine `@rustev/*` packages on npm, and the hosted GitHub release.
Spec 021 3.10 and spec 031 3.9.7 then require a fresh registry-only
consumer before the lifecycle reaches `qualified`. Run locally on macOS
arm64 the same day, the consumer passed step 1 (all twelve crates resolved
at `=0.1.0` with the manifest's checksums), the eight partial-adoption
subsets of 031 3.5 on 1.98.1 and the MSRV 1.88, the npm install with
registry signatures, and the hosted CLI's checksum and SLSA provenance.
Four things blocked qualification:

1. The CLI has no version command: `rustev version` and `rustev --version`
   are refused as unknown commands, and 021 3.10 step 5 runs `version`.
2. The support-routing package declares no runnable offline example
   (step 3). Its plan check refuses `no_capable_backend` without a
   backend, and the SYNTHETIC rules program its own tests use is a test
   fixture of `rustev-backend-rules`, outside every archive.
3. No harness runs the consumer across the platform matrix of 031 3.9.7,
   and `qualify` in `tools/rustev-release/src/family_lifecycle.rs` has no
   caller.
4. The `0.1.0` npm packages carry no npm provenance (031 3.9.6): trusted
   publishing could not apply to packages that did not yet exist, and the
   owner chose a local first publish. Trusted publishers are now attached.

Every published version is immutable, so the fix is a new family version.

## 2. Territory

In scope: the CLI's version command; package data and an accessor in
`rustev-pkg-support-routing`; the qualification harness and the
`qualify` command in `tools/rustev-release/`; a qualification workflow;
the authority check for a release after `0.1.0`; and the next family
version. Out of scope: any change of decision behavior, contract, plan
identity or API beyond the version command and the package accessor; the
container image (still excluded); any other version change.

## 3. Behavior

### 3.1 The version command

`rustev version` and `rustev --version` print `rustev <version>` and a
newline to standard output and exit 0, where `<version>` is the
`rustev-cli` package version (`CARGO_PKG_VERSION`). Either form with any
further argument is a usage error. `rustev --help` lists the command. No
other command changes.

### 3.2 The declared example

`rustev-pkg-support-routing` adds to its package data, each SYNTHETIC and
labeled so (R-04 of spec 007): the support-routing rules program now held
at `backends/rustev-backend-rules/tests/fixtures/support-routing.rules.json`
(a copy whose bytes a workspace test keeps equal to the fixture), the
calibration bound to that program, one snapshot, and the outcome expected
when the package's definition is compiled with that program and
calibration and evaluated on that snapshot at `EVALUATION_TIME_MS`. A
public constant or function exposes each, as `DEFINITION` is exposed
today. The archive policy's file list adds them by review.

### 3.3 The qualification harness

`tools/rustev-release/qualify.sh <version>` is the consumer of 021 3.10
and 031 3.9.7. It starts in a new temporary directory outside any
workspace, with an empty Cargo home and npm cache, no registry token, and
no path, Git, patch, alternate-registry or source-replacement
configuration, and runs:

- **crates** (021 3.10 steps 1 to 6, 031 3.9.7 (a)): fetch all release-set
  crates at `=<version>` and compare each lockfile checksum with the
  signed manifest; a small library using contract, core, runtime and the
  rules backend that compiles and evaluates a bounded synthetic decision;
  the declared example of 3.2 reproduced offline (network denied for the
  test process where the platform allows it); a remote HTTP client and
  loopback server compiled from `rustev-remote-http`'s public API without
  calling a provider; `cargo install rustev-cli --version =<version>
  --locked` and its version, help, plan check and the declared example;
  each partial-adoption subset of 031 3.5 against the registry. Steps run
  on the pinned toolchain and on the MSRV.
- **npm** (031 3.9.7 (b)): a fresh Node project installs `@rustev/rustev`
  and `@rustev/rustev-wasm` at `<version>`, runs `npm audit signatures`
  and requires a verified provenance attestation for every installed
  `@rustev/*` package, and runs a smoke test of each (version, plan check
  of the declared example, and the WebAssembly module's load).
- **CLI** (031 3.9.7 (d)): the host's CLI archive is downloaded from the
  hosted release, its checksum compared with its `.sha256` file and the
  signed manifest, its attestation verified (`gh attestation verify`,
  bound to `release.yml` at the tag), and its version, help, plan check
  and declared example run.

- **image** (031 3.9.7 (c)): run only when the signed manifest names a
  container image. The next family version names none (the image stays excluded, section
  2), so the harness writes the step as `excluded` with that reason.

The required steps are derived from the signed manifest, never from the
harness's own list: a kind the manifest names is required, and a kind it
does not name is `excluded`. Each step writes `<step>.json`
(`rustev.qualification-step/1`: command, exit status, output digest,
lockfile or install record, checksums, and the host, OS, architecture,
toolchain and Node identities), and a step that cannot run on the host
writes its reason as `not_run`, which is never `excluded`.
`.github/workflows/qualify.yml`, dispatched after publication with the
tag, runs the harness on each platform of the release set's CLI and npm
matrix and uploads the step files. `rustev-release qualify <release-dir>
<evidence-dir> <instruction> <journal>` requires every step for every
platform, refuses a failing or `not_run` step and an `excluded` step whose
kind the manifest names, and records their digests through `qualify` (the
`Qualification` journal entry); only then is the lifecycle `qualified`.
A qualification is evidence for the registry state and matrix it ran on,
never a permanent availability claim (021 3.10).

`qualify.yml` is a non-managed workflow file added as 031 3.12 says, like
`release.yml` and `release-checks.yml`, and this spec owns it the way
spec 031 owns those two: in its text (031 3.12; `AGENTS.md` names
`release-checks.yml` as spec 031's), not through an `extends` entry. No
spec's frontmatter names a workflow file as a unit, because spec-spine
counts `.github/workflows/**/*` among the repository's global hashed
inputs (`extra_hashed_inputs` in `spec-spine.toml`). `qualify.yml` is
dispatched after publication and is not a required check, so it is not
added to `ci.extra_required_jobs`.

### 3.4 Authority for a later release

031 3.11.1 authorizes `0.1.0` only, and the publisher refuses any other
version. A release after `0.1.0` is authorized by the owner's publication
instruction (`Instruction`) together with its detached SSH signature
(`<instruction>.sig`, namespace `rustev-release`, the owner identity of
021 3.8). `rustev-release publish` and `hosted-release` verify that
signature against `tools/rustev-release/allowed_signers` before any
journal entry or upload, and refuse an unsigned or wrongly signed
instruction. `0.1.0` keeps its 3.11.1 authority. Everything else in 021
3.9 and 031 3.11 (exact version, commit, registry, package and artifact
set; journal; layer order; owner approval of the `npm-publish`
Environment) is unchanged.

### 3.5 The next family version

This work ships in the next family version after `0.1.0`; there is no
separate `0.1.1` release. Every release-set crate and npm package moves
to that version in lockstep (021 3.2), including members whose bytes do
not otherwise change. The release
follows spec 053 (rehearsal on `release/next`, candidate evidence, owner
signature and tag) and 031 3.9.6: the npm job publishes through trusted
publishing with provenance, which also validates the trusted publishers
attached after `0.1.0`.

## 4. Observable negative cases

| Case | Expected |
|---|---|
| `rustev version extra` | Usage error, nonzero exit. |
| The package's rules copy differs from the backend fixture | The workspace test fails. |
| A consumer resolves a crate through a path, Git source or warm cache | The harness refuses the run as not fresh. |
| An npm package installs without a verified provenance attestation | The npm step fails; not `qualified`. |
| A platform's step is `not_run` | `rustev-release qualify` refuses. |
| An instruction for a version after `0.1.0` without a valid owner signature | `publish` refuses before any journal entry or upload. |
| An instruction for `0.1.0` today | Still governed by 031 3.11.1; no signature is required. |

## Acceptance

- CLI tests cover both version forms, the extra-argument refusal and help.
- A package test keeps the rules copy equal to the backend fixture and
  reproduces the declared example's expected outcome in the workspace.
- `rustev-release` tests cover the instruction signature (valid, unsigned,
  wrong key, altered bytes) and `qualify` (complete evidence, a missing,
  failing and `not_run` step).
- The harness runs locally against `0.1.0` and reports the version
  command and npm provenance as failing, and steps it cannot yet run as
  `not_run`, never as passing.
- After the next family version is published under a signed instruction, the workflow's
  matrix passes and `rustev-release qualify` records the lifecycle as
  `qualified`.

## Verification

Pending. Ratification authorizes the code and workflow changes and the
next family version bump. Adding `.github/workflows/qualify.yml` needs the
owner's approval of the `statecraft-review-exception` Environment once, as
every change under `.github/workflows/` does (`AGENTS.md`). Publishing
that version additionally needs the owner's signed instruction of 3.4.

## Implementation record

**Part b1 (2026-10-27): later-release authority and `qualify`.** An
instruction for a version after `0.1.0` is authorized only with
`<instruction>.sig`, an SSH signature over its exact bytes (namespace
`rustev-release`, the owner identity, `allowed_signers`), verified by
`publish` and `hosted-release` before any journal entry or upload; an
unsigned, wrongly signed or altered instruction is refused, and `0.1.0`
keeps its 031 3.11.1 authority (`tools/rustev-release/tests/signing.rs`).
`rustev.qualification-step/1` and `rustev-release qualify` are in
`tools/rustev-release/src/qualification.rs`: required kinds come from the
signed manifest, every required kind needs a passed step for each
platform of the CLI and npm matrix, and a missing, failing or `not_run`
step, or an `excluded` step for a named kind, is refused before the
journal (`tools/rustev-release/tests/family.rs`). The harness
(`qualify.sh`, part b2) and the workflow (part c) follow.

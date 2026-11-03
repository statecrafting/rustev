---
id: "062-cli-npm-bin"
title: "The CLI as an npm bin of @rustev/rustev (amends 027, 031, 056 and 058)"
status: draft
implementation: pending
created: "2026-11-03"
summary: >
  A proposed amendment of approved specs 027 (3.13), 031 (3.1, 3.9), 056
  (3.3) and 058 (3.2), for release 0.4.1. A Node consumer that gates its
  flows with `rustev gate select` today installs `@rustev/rustev` from npm
  and, separately, downloads a checksum-pinned CLI archive from the GitHub
  release, and must keep the two pins in step. This spec ships the CLI
  inside the npm family: each per-platform package `@rustev/rustev-<platform>`
  carries the release's `rustev` binary for its platform beside the
  Node-API addon, byte-identical to that platform's CLI archive member,
  and `@rustev/rustev` declares a `rustev` bin, a small JavaScript
  launcher that resolves the installed platform package and executes its
  binary with arguments, standard streams, exit code and signals passed
  through unchanged. One npm pin then names the binding and the CLI. No
  npm name is added, no install script runs, the CLI archives stay on the
  release, and the command surface of spec 006 does not change. Draft:
  nothing is implemented until ratified.
amends:
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "056-release-0-1-1-and-consumer-qualification"
  - "058-one-tagged-run-releases-the-family"
depends_on:
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "053-signed-release-candidates"
  - "054-npm-rustev-scope"
  - "056-release-0-1-1-and-consumer-qualification"
  - "058-one-tagged-run-releases-the-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "The CLI binary in each platform package is byte-identical to the executable member of the same release's CLI archive for that platform; the run fails before publication on any difference, and no step builds, strips, signs or rewrites the binary for npm."
    anchor: "3-3-same-bytes-as-the-archive"
  - id: "I-2"
    kind: invariant
    text: "The launcher passes arguments, standard input, output and error through unchanged, writes nothing to standard output, and ends with the binary's exit code, or re-raises the binary's terminating signal; it never maps, adds or suppresses a status of spec 006 3.2."
    anchor: "3-2-the-launcher"
  - id: "I-3"
    kind: invariant
    text: "No npm package of the family gains an install script, an install-time or run-time download, a new dependency or a new package name; the launcher executes only the binary of the platform package at exactly its own version."
    anchor: "3-1-where-the-binary-lives"
  - id: "R-1"
    kind: requirement
    text: "Qualification's npm step runs, on every platform of the npm matrix, `npx --no-install rustev version`, `--help` and the declared example's plan check through the installed bin, and requires the reported version and the check output equal to the CLI step's."
    anchor: "3-5-qualification"
---

# 062: The CLI as an npm bin of @rustev/rustev (amends 027, 031, 056 and 058)

Draft: a proposal for release 0.4.1, never a claim about code.
Ratification is the owner's act. The approved texts of 027, 031, 056 and
058 are not edited; this spec records what it adds to them.

## 1. Purpose

A partner API consumes Rustev from Node. Its service loads the binding
`@rustev/rustev`; its CI gates each flow with `rustev gate select` (spec
029 3.13.6), which only the CLI exposes. So its CI pins two things: the
npm version, and a CLI archive downloaded from the GitHub release with its
SHA-256 pinned beside it. Every release upgrade moves both, and nothing
checks that they agree. The consumer asked for either the CLI as an npm
bin or `gateSelect` in the binding, and named the bin as enough.

The bin is the smaller change: no new API, no new conformance surface,
one pin. The binary already exists per platform, is already reproducible,
attested and qualified (031 3.9.3, 3.9.5, 3.9.7 (d)); this spec moves the
same bytes into the npm packages that already select by platform.

## 2. Territory

The native package layout `integrations/rustev-node/` (027's unit: the
main package's `bin`, the launcher, the platform packages' `files`), the
platform package check `tools/rustev-conformance/js/platform-packages.mjs`
(027's unit), and the release tooling `tools/rustev-release/` (031's
unit: `pack`, `npm-audit`, `sbom`, `verify-release` and the
qualification harness). The release and qualification workflows
(`.github/workflows/release.yml`, `qualify.yml`, owned in text by 031
3.12, 056 3.3 and 058) change job dependencies only; changing them needs
the owner's approval of the `statecraft-review-exception` Environment
once, and ratifying this spec is not that approval.

This draft declares no `extends` or `establishes` edge, so it claims no
path while it is a proposal. Ratification adds `extends` edges, each
`nature: amending`, to `integrations/rustev-node/` and
`tools/rustev-conformance/` (027) and `tools/rustev-release/` (031).

Out of scope: the CLI's commands, flags, outputs and exit codes (spec
006 and its amendments, unchanged); a `gateSelect` binding function; the
WebAssembly package; the CLI archives' layout; the glibc floor of the gnu
artifacts (3.6); and any change of decision behavior.

## 3. Behavior

### 3.1 Where the binary lives

1. Each platform package `@rustev/rustev-<platform>` of 027 3.13.1 (names
   of 054) holds, beside its addon `rustev.<platform>.node`, the CLI
   binary `rustev` (`rustev.exe` for `win32-x64-msvc`) at the package
   root. Its `files` lists exactly the two; its `main` stays the addon;
   it declares no `bin`, so no package manager links a second `rustev`.
2. No new npm package is added. The platform packages already carry the
   `os`, `cpu` and `libc` selection the binary needs, are already
   `optionalDependencies` of `@rustev/rustev` at exactly the family
   version, and already have trusted publishers and a first publication.
   Separate `@rustev/cli-<platform>` packages were considered and not
   proposed: seven new names, each needing the owner's first publication
   by hand (as 0.1.0 showed), seven more release-set members, and a
   second platform selection that could disagree with the binding's.
3. Cost: a binding-only consumer downloads the binary too. Today a
   platform tarball is 12 to 14 MB and a CLI archive 10 to 12 MB
   (v0.3.0 assets), so a platform tarball grows to roughly 23 to 27 MB.
   `npm-audit` refuses a platform tarball over 48 MiB, and the release
   notes state each tarball's size.
4. I-3: no `preinstall`, `install`, `postinstall` or `prepare` script
   (027 3.13.2 and the existing audit), no download at install or run
   time, and no dependency beyond the existing `optionalDependencies`.
   The binary's executable bit is set in the tarball (mode `0755`) on
   every non-Windows platform; `npm-audit` checks it.

### 3.2 The launcher

1. `@rustev/rustev` declares `"bin": { "rustev": "bin/rustev.js" }` and
   packs `bin/rustev.js`, a CommonJS script with a `#!/usr/bin/env node`
   line and no dependency outside Node's built-ins. npm generates the
   Windows `.cmd` and `.ps1` shims for it; the script itself is the same
   on every platform.
2. It computes the platform name with the same function the addon loader
   uses (027 3.13.2), moved into one module both require, so the binding
   and the bin can never select different packages. It resolves
   `@rustev/rustev-<platform>/package.json` from its own location and
   requires that package's `version` to equal its own; it then executes
   `rustev` (or `rustev.exe`) in that directory.
3. It fails, writing one line to standard error and exiting 1 (a code no
   status of 006 3.2 uses), when: the platform is outside the matrix; the
   platform package is not installed (naming the package, and noting that
   `--omit=optional` or a mismatched `libc` leaves it out); the versions
   differ (naming both); or the binary is missing or cannot be executed
   (naming the path and the operating system's error).
4. I-2: it starts the binary with `child_process.spawn`, no shell,
   inherited standard streams and the unchanged argument list and
   environment. It forwards `SIGTERM` and `SIGHUP` to the child and
   ignores `SIGINT` while the child runs, because a terminal delivers
   `SIGINT` to the whole process group; so 006's interrupt behavior (exit
   130, nothing written) holds through the bin. When the child exits
   with a code, the launcher exits with it; when the child ends by a
   signal, the launcher removes its handlers and re-raises the signal on
   itself. Gate exit codes 0, 11 and 12 therefore reach CI unchanged.
5. The launcher has no local-build path. 027's local addon beside
   `index.js` is a development convenience; a local CLI is
   `target/release/rustev`.
6. Invocation: consumers run the installed pin with `npx --no-install
   rustev ...`, `npm exec --no -- rustev ...` or a package script. A bare
   `npx rustev` in a project without the install would ask npm for the
   unscoped name `rustev`, which is not Rustev's (it was unclaimed on
   2026-11-03); the package's README and the release notes say so.

### 3.3 Same bytes as the archive

1. I-1: `pack` takes the run's raw CLI binaries (the input of
   `archive-cli`, 031 3.9.3) beside the addons, copies each into its
   platform package, and packs. In the release run they come from the
   `cli` job, in the rebuild from the `rebuild` job, so 058 3.2.2's byte
   comparison of every tarball pair covers the binary as it already
   covers the archives. 058 3.2.1 reads, as amended: `pack` needs `cli`.
2. A new check, run by `npm-audit` with the archives present, extracts
   `package/rustev[.exe]` from each platform tarball and the executable
   from the same platform's archive and requires equal SHA-256. A
   mismatch fails the run before `manifest`.
3. Before packing, `pack` runs each host-native binary's `version` and
   requires `rustev X.Y.Z`, the release set's version. For a candidate
   (053 3.2.3) the packages carry `X.Y.Z-rc.N` and the binary still
   reports `X.Y.Z`, as the candidate's CLI archive does today.
4. The release manifest keeps `rustev.release-manifest/2`: it already
   names every tarball's integrity and every archive's SHA-256, so the
   binding of 3.3.2 is re-derivable from it. `verify-release` re-runs
   that check on the downloaded GitHub release assets.
5. Attestation (031 3.9.5) is per tarball and per archive, unchanged; a
   platform tarball's attestation now covers the binary inside it, and
   the archive's attestation covers the same bytes independently.

### 3.4 SBOM and audit

1. 031 3.9.1: a platform tarball's SBOM covers both graphs it ships, the
   addon's (`rustev-node`) and the binary's (`rustev-cli`) for that
   target, as one CycloneDX 1.5 document with the tarball as the root
   component and each graph's components beneath it, merged
   deterministically from the two `cargo cyclonedx` outputs. The
   archive's SBOM is unchanged.
2. 031 3.9.2: `npm-audit` additionally requires that `bin`, in every
   form, names packed files (the entry-point rule extended to `bin`); that
   only `@rustev/rustev` declares a `bin`, and only `rustev`; that a
   platform tarball holds exactly `package.json`, the addon and the
   binary; and the mode and size rules of 3.1. `cargo deny` already
   audits the `rustev-cli` graph, so no audit policy changes.

### 3.5 Qualification

R-1: 056 3.3's npm step, after its install and signature checks, runs in
the same fresh project on each platform of the npm matrix:
`npx --no-install rustev version` (requiring `rustev X.Y.Z`), `--help`,
and the declared example's `plan check`, requiring output bytes equal to
the CLI step's on that host. It also requires the launcher's exit code
to pass through, running one command expected to exit with a non-zero
006 status (a usage error, 2). The CLI step (031 3.9.7 (d)) is unchanged:
the archives stay release assets and stay qualified.

### 3.6 The glibc floor

The gnu binaries are built in `rust:1.98.1-bookworm` (glibc 2.36), as the
gnu addons are, so the bin inherits the addon's floor: any binary linked
against glibc 2.34 or later references `__libc_start_main@GLIBC_2.34`, so
neither runs on glibc 2.31 without a newer C library supplied. A host
that loads the addon through a bookworm sysroot needs the same for the
binary, which the launcher starts as a separate process; it can run
`node_modules/@rustev/rustev-<platform>/rustev` through that sysroot's
dynamic loader directly, at a path this spec fixes. Implementation
records the highest `GLIBC_` symbol version each gnu binary and addon
requires, and the release notes state it. The musl binaries are static
and need no C library, but npm installs only the gnu package on a glibc
host. Lowering the floor is not proposed here (Decisions at approval, 2).

### 3.7 What stays

The CLI archives, their checksums, SBOMs and attestations on the GitHub
release (031 3.1 layer 7); the binding's API, loader behavior and typed
errors; the family's names, order and lockstep version (031 3.1, 054);
058's trigger, publication and resume; and spec 001's rule: no artifact,
signature or attestation of the release is an authorization accepted by
any action executor.

## 4. Out of scope

A `gateSelect` (or any CLI command) in the binding; the CLI in the
WebAssembly package; separate CLI npm packages; Homebrew, Scoop or other
installers; a lower glibc floor; and replacing the CLI archives.

## 5. Observable negative cases

| Case | Result |
|---|---|
| A platform tarball's binary differs from its archive member | The run fails before `manifest`. |
| A platform tarball lacks the binary, has another file, or its mode is not `0755` | `npm-audit` fails. |
| A platform package declares a `bin` | `npm-audit` fails. |
| `bin` names a file not packed | `npm-audit` fails. |
| A platform tarball over 48 MiB | `npm-audit` fails. |
| The packed binary reports a version other than `X.Y.Z` | `pack` fails. |
| The platform package is not installed (`--omit=optional`) | `rustev` exits 1 naming the package. |
| The platform package's version differs from `@rustev/rustev`'s | `rustev` exits 1 naming both. |
| The binary exits 11 or 12 | `rustev` exits 11 or 12. |
| The binary is interrupted | `rustev` exits as the binary did (130), with nothing written by the launcher. |
| `npx rustev` without the install | npm resolves the unscoped name; not Rustev (3.2.6). |

## Acceptance

- The platform packages carry the binary as 3.1 states; `@rustev/rustev`
  declares the bin; one platform-name module serves the loader and the
  launcher.
- Launcher tests (`node:test`, with a stub executable in a fake platform
  package) for resolution, each failure of 3.2.3, argument and stream
  pass-through, exit codes 0, 2, 11 and 12, and signal re-raise.
- `platform-packages.mjs` checks the new `files` and `bin` fields, stages
  the host's CLI with its addon, and `fresh` runs `npx --no-install rustev
  version` on the tarball install; `make bindings` passes.
- `pack`, `npm-audit`, `sbom` and `verify-release` implement 3.3 and 3.4,
  with tests for each `npm-audit` and `pack` row of section 5.
- The qualification harness implements 3.5.
- A release whose run passes 3.3's checks and whose qualification passes
  3.5 on every platform of the npm matrix.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None. Draft.

## Decisions at approval

Decided in this text, with the reason beside each: the binary in the
existing platform packages, not new packages (3.1.2); a JavaScript
launcher in the main package, not a platform package `bin` (3.1.1,
because pnpm and Yarn link only direct dependencies' bins); exit code 1
for launcher failures (3.2.3); no manifest schema change (3.3.4). Open
for the owner, each with a recommendation:

1. **Binding-only consumers pay the download.** Recommended: accept for
   0.4.1. If the size matters later, separate `@rustev/cli-<platform>`
   packages can follow as their own spec with the owner's first
   publication.
2. **The glibc floor.** The bin, like the addon, needs glibc 2.34 or
   later on gnu hosts (3.6). Options: (a) keep it, document the binary's
   fixed path for hosts that run it through a newer sysroot's loader; (b)
   put the static musl binary in the gnu packages, which runs on any
   glibc but differs from the gnu archive's bytes (I-1 would then bind it
   to the musl archive of the same architecture); (c) a separate spec
   that builds every gnu artifact against an older sysroot and states a
   floor. Recommended: (a) for 0.4.1, (c) as its own proposal if the
   partner's 2.31 runner is to be supported without a shim.
3. **The 48 MiB tarball bound** of 3.1.3. Recommended: accept; it leaves
   about twice the measured headroom.
4. **The unscoped npm name `rustev`.** It is unclaimed, and a bare `npx
   rustev` without the install would resolve it. Recommended: document
   `npx --no-install` (3.2.6) and do not publish a placeholder; reserving
   it is a new name and the owner's act if wanted.
5. **Release target.** Recommended: 0.4.1 as a patch release, since the
   change adds a bin and a file to existing packages and removes nothing
   (031 3.6.5).

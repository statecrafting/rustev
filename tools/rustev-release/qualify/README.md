# Fresh consumer qualification

Run from any directory:

```sh
sh /path/to/rustev/tools/rustev-release/qualify.sh 0.2.0 /release/release-manifest.json /evidence
```

The manifest's adjacent `.sig` is verified against the repository's owner
signer before network steps. Consumers use public registry versions and package
data only. Python 3.11 or later handles TOML and JSON; Cargo, rustup, npm, Node,
GitHub CLI and ssh-keygen are needed for the relevant live steps. The pinned Rust
toolchain and manifest MSRV must already be installed. No toolchain is installed
implicitly. npm must support `audit signatures --json --include-attestations`.
A registry signature alone does not satisfy the per-package provenance check.

The shell creates and removes a new temporary directory. Its resolved ancestors
must contain neither Cargo.toml nor package.json. Visible Cargo source, patch,
registry, path and compiler override configuration is refused. Registry environment
overrides and credentials are removed, and homes and caches start empty. Each
consumer lockfile is checked before compiling: every dependency source must be
crates.io, and every release crate checksum and version must match the manifest.
The Cargo target is local to the fresh external consumer, never the repository's
shared build cache. No workspace lockfile or dependency path is used in registry
adoption mode. The subset list remains owned by adoption.sh.

The evidence directory retains per-kind logs, SHA-256 output digests, lockfiles,
install/audit records and exported example documents. It contains only the host's
four step JSON files; collect other hosts' evidence separately for `qualify`.
The declared example's pinned and MSRV consumer tests run with Cargo offline;
process network denial uses sandbox-exec or a network namespace when available,
and logs its absence. The partial-adoption subsets run adoption.sh in registry
mode: they resolve from the registry, then build with `--locked` once the
lockfile has been checked against the signed manifest.
The remote client/server example is compiled but never calls a provider.

`RUSTEV_QUALIFY_DRY=1` performs no signature verification or network steps. Required
kinds become `not_run` with reason `dry run`, absent kinds become `excluded`, and
the process exits nonzero. These records deliberately fail the qualification
validator. Missing required tools/toolchains and unsupported hosts also produce
`not_run`. Verification or execution errors produce `failed` with captured output.
An image named by a future manifest remains `not_run`: its consumer is not defined
by this release. An absent image uses the mandated exclusion reason.

The example template references the part-a API names `EXAMPLE_RULES`,
`EXAMPLE_CALIBRATION`, `EXAMPLE_SNAPSHOT` and `EXAMPLE_JUDGMENT`. They are not
available on the part-b1 base, so live template execution requires the release
containing part a. The harness never substitutes workspace test fixtures.

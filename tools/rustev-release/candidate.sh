#!/bin/sh
# Spec 021, 3.5 and 3.7: candidate package evidence for the exact clean
# commit at HEAD. Two independent clean worktrees package the release set
# with a fixed source date; the archives are compared byte for byte, checked
# against the archive policy, and each unpacked archive is tested outside
# any workspace. Evidence lands in <target>/release-candidate/<commit>/.
#
# Read-only towards every registry: nothing is uploaded, tagged or signed,
# and no credential is read. Passing is candidate evidence only (I-1).
set -eu

# The checker is a host tool: build it for the host, whatever target the
# environment names, so its path is `<target>/debug/rustev-release`.
unset CARGO_BUILD_TARGET
root=$(git rev-parse --show-toplevel)
cd "$root"
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "021 3.7: the candidate is an exact clean commit; the tree is dirty" >&2
  exit 1
fi
commit=$(git rev-parse HEAD)
short=$(printf %s "$commit" | cut -c1-12)
epoch=$(git log -1 --format=%ct "$commit")
# UTC with its designator, so `touch -d` never reads it as local time.
stamp=$(date -u -d "@$epoch" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -r "$epoch" +%Y-%m-%dT%H:%M:%SZ)
if [ -z "$stamp" ]; then
  echo "021 3.7: could not format the commit time $epoch as a source date" >&2
  exit 1
fi
# Build the checker from this commit first: worktrees under the worktree
# root share one target directory (its `.cargo/config.toml` sets
# `target-dir`), so a binary another worktree built must never answer here.
cargo clean --quiet -p rustev-release
cargo build --quiet --locked -p rustev-release
target=$(cargo run --quiet --locked -p rustev-release -- field target)
release="$target/debug/rustev-release"
# Every later value comes from the binary just built, never a rebuild.
field() { "$release" field "$1"; }
out="$target/release-candidate/$commit"
packages=$(field packages)
if [ -z "$packages" ]; then
  echo "021 2: the release set names no package" >&2
  exit 1
fi
version=$(field version)
# Worktrees live under the repository's worktree root (never beside the checkout).
worktrees="${RUSTEV_WORKTREES:-${HOME:?set HOME or RUSTEV_WORKTREES to locate the worktree root}/.statecraft/worktrees/rustev}"
rm -rf "$out"
mkdir -p "$out/a" "$out/b"
work=""
# Whatever fails, registered candidate worktrees and the work directory go.
cleanup() {
  for r in a b; do
    if [ -d "$worktrees/candidate-$r-$commit" ]; then
      git worktree remove --force "$worktrees/candidate-$r-$commit" || true
    fi
  done
  if [ -n "$work" ]; then rm -rf "$work"; fi
}
trap cleanup EXIT

# The SBOM paths are read from cargo metadata with Node (as `make bindings`
# requires it too).
command -v node >/dev/null 2>&1 || { echo "031 3.9.1: the candidate needs node on PATH" >&2; exit 1; }
# 031 3.9.1: the pinned SBOM generator.
cyclonedx_pin=0.5.9
# The tool prints its name doubled when run as a cargo subcommand.
if [ "$(cargo cyclonedx --version 2>/dev/null)" != "cargo-cyclonedx-cyclonedx $cyclonedx_pin" ]; then
  echo "031 3.9.1: the SBOMs need cargo-cyclonedx $cyclonedx_pin (cargo install cargo-cyclonedx --version $cyclonedx_pin --locked)" >&2
  exit 1
fi
for run in a b; do
  wt="$worktrees/candidate-$run-$commit"
  git worktree add --detach --quiet "$wt" "$commit"
  (
    cd "$wt"
    # The fixed source-date input: every tracked file carries the commit time.
    # `touch -d` takes this ISO 8601 UTC form on GNU coreutils and on BSD
    # (macOS) touch alike.
    if [ -z "$(git ls-files | head -n 1)" ]; then
      echo "021 3.7: the candidate commit tracks no file" >&2
      exit 1
    fi
    git ls-files -z | xargs -0 touch -h -d "$stamp"
    flags=""
    for p in $packages; do flags="$flags -p $p"; done
    # shellcheck disable=SC2086
    SOURCE_DATE_EPOCH=$epoch cargo package --locked $flags >"$out/$run.package.log" 2>&1
    wt_target=$("$release" field target)
    for p in $packages; do cp "$wt_target/package/$p-$version.crate" "$out/$run/"; done
    # Spec 031 3.9.1: a CycloneDX SBOM per member (pinned generator and
    # specification version), from run a's clean tree at the source date.
    if [ "$run" = a ]; then
      # cargo-cyclonedx 0.5.9 has no `--workspace` flag (it refuses one):
      # run at the root of this virtual workspace it writes `<name>.cdx.json`
      # beside every member's manifest, and each member's file is required
      # below.
      if ! SOURCE_DATE_EPOCH=$epoch cargo cyclonedx --format json --spec-version 1.5 --target all \
        >"$out/sbom.log" 2>&1; then
        echo "031 3.9.1: cargo cyclonedx failed:" >&2
        cat "$out/sbom.log" >&2
        exit 1
      fi
      # The workspace's metadata, read once; a failure stops the candidate
      # with cargo's own message.
      if ! meta=$(cargo metadata --format-version 1 --no-deps --locked); then
        echo "031 3.9.1: cargo metadata failed" >&2
        exit 1
      fi
      for p in $packages; do
        # Each member's SBOM lands beside its own manifest: the path comes
        # from cargo metadata (read once, above), never from a search.
        dir=$(printf '%s' "$meta" | P="$p" node -e '
          let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
            const m = JSON.parse(s).packages.find((x) => x.name === process.env.P);
            // A member cargo metadata does not list is an error, never an
            // empty path.
            if (!m) process.exit(1);
            process.stdout.write(require("path").dirname(m.manifest_path));
          });') || { echo "031 3.9.1: cargo metadata lists no package $p" >&2; exit 1; }
        f="$dir/$p.cdx.json"
        if [ -z "$dir" ] || [ ! -f "$f" ]; then
          echo "031 3.9.1: no SBOM was generated for $p (expected $f)" >&2
          exit 1
        fi
        cp "$f" "$out/sbom-$p.cdx.json"
      done
    fi
  )
  git worktree remove --force "$wt"
done

# No pipelines: each step's own exit status stops the script (set -e).
# Each step's output, stdout and stderr, is its evidence file; a failing
# step prints that file and stops the script.
# Each step's standard output and standard error both go to its evidence
# file, so a tool that reports on stderr (cargo deny) leaves its diagnostics.
step() {
  name=$1
  shift
  if ! "$@" >"$out/$name" 2>&1; then
    echo "021: candidate step $name failed:" >&2
    cat "$out/$name" >&2
    exit 1
  fi
}
step compare.txt "$release" compare "$out/a" "$out/b"
step archives.json "$release" archives "$out/a"
work=$(mktemp -d "${TMPDIR:-/tmp}/rustev-candidate-$short.XXXXXX")
step unpacked.txt "$release" unpacked "$out/a" "$work"
# 3.7: the candidate matrix also tests every unpacked archive at the
# declared MSRV, from a fresh work directory.
# `set -e` stops on a failing `field`; an empty answer is refused too.
msrv=$(field msrv)
if [ -z "$msrv" ]; then
  echo "021 3.7: the release set declares no MSRV" >&2
  exit 1
fi
rm -rf "$work" && work=$(mktemp -d "${TMPDIR:-/tmp}/rustev-candidate-$short.XXXXXX")
step unpacked-msrv.txt "$release" unpacked "$out/a" "$work" "$msrv"
# 3.6: the pinned audit tool and policy, the exceptions rule and the report.
deny_pin=0.20.2
if [ "$(cargo deny --version)" != "cargo-deny $deny_pin" ]; then
  echo "021 3.6: the audit needs cargo-deny $deny_pin (cargo install cargo-deny --version $deny_pin --locked)" >&2
  exit 1
fi
excluded=""
for e in $(field excluded); do excluded="$excluded --exclude $e"; done
# `--locked` is a cargo-deny global flag (before the subcommand), like
# `--manifest-path`.
# shellcheck disable=SC2086
step deny.txt cargo deny --manifest-path Cargo.toml --config tools/rustev-release/deny.toml \
  --workspace $excluded --locked check
step exceptions.txt "$release" exceptions "$(date -u +%Y-%m-%d)"
step dependencies.json "$release" deps
# 031 3.10: the changelog generated from the governed corpus at this commit
# (PREVIOUS names the previous release's tag, when there is one).
# Unquoted on purpose: with PREVIOUS unset or empty, `${PREVIOUS:-}` expands
# to no argument at all (not to ""), so the first-release form runs.
# shellcheck disable=SC2086
step changelog.txt "$release" changelog "$out/changelog.md" ${PREVIOUS:-}
# 3.8: the canonical manifest and SHA256SUMS of run a. Signing them and the
# tag is the owner's act; nothing here signs.
# 031 3.11.2: downloaded rehearsal bytes are optional candidate inputs.
if [ -n "${RUSTEV_RELEASE_ARTIFACTS:-}" ]; then
  step manifest.txt "$release" manifest "$out" "$RUSTEV_RELEASE_ARTIFACTS"
else
  step manifest.txt "$release" manifest "$out"
fi
cat "$out/compare.txt" "$out/unpacked.txt" "$out/unpacked-msrv.txt" "$out/exceptions.txt"
# `step` stops the script on a failing audit, so reaching here means it
# passed; every diagnostic stays in deny.txt, part of the candidate evidence.
echo "cargo deny: passed ($out/deny.txt)"
echo "candidate evidence for $commit in $out"

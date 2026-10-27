#!/bin/sh
# Spec 031 3.5 and R-1: each partial-adoption subset is a fresh consumer
# project outside the workspace that depends on the facade with
# `default-features = false` and exactly the subset's features, runs its
# tests, and proves with `cargo tree -e normal` that every crate of an
# omitted feature is absent from its graph (and every crate of a chosen one
# present).
#
#   sh tools/rustev-release/adoption.sh [<subset> ...]   (default: all)
#
# Before publication the consumer depends on the facade by path; 3.9.7a
# repeats these projects against the registry after it. A subset whose
# feature's crate is not in this release is reported as skipped with the
# reason, never as passing.
set -eu

script=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
root=$(CDPATH='' cd -- "$script/../.." && pwd -P)
version=${RUSTEV_ADOPTION_VERSION:-}
if [ -n "$version" ]; then
  case "$version" in *[!0-9.]*) exit 2 ;; esac
  printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || exit 2
  : "${RUSTEV_ADOPTION_MANIFEST:?registry adoption requires the signed manifest}"
fi
# The consumer's edition is the workspace's own.
edition=$(sed -n 's/^edition = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
[ -n "$edition" ] || { echo "adoption: the workspace declares no edition" >&2; exit 1; }
# `name|features|package|chosen crates|omitted crates`; the package is a
# separate dependency of the consumer, as 3.5 says for embedded decisions.
# Crate names are Cargo package names as Cargo.lock spells them:
# `opentelemetry_sdk` is published with an underscore, unlike its siblings.
subsets='
documents|||rustev-contract rustev-core|rustev-runtime rustev-eval rustev-backend-rules rustev-remote-http tokio hyper rustev-otel
embedded|runtime rules|rustev-pkg-support-routing|rustev-runtime rustev-backend-rules rustev-pkg-support-routing tokio|rustev-eval rustev-remote-http hyper rustev-otel
evaluation|eval||rustev-eval|rustev-runtime rustev-backend-rules rustev-remote-http tokio hyper rustev-otel
remote-http|runtime remote-http||rustev-runtime rustev-remote-http hyper tokio|rustev-eval rustev-backend-rules rustev-otel
runtime|runtime||rustev-runtime tokio|rustev-eval rustev-backend-rules rustev-remote-http hyper rustev-otel
rules|rules||rustev-backend-rules|rustev-runtime rustev-eval rustev-remote-http tokio hyper rustev-otel
remote-http-alone|remote-http||rustev-remote-http hyper tokio|rustev-runtime rustev-eval rustev-backend-rules rustev-otel
otel|otel||rustev-otel rustev-runtime tokio|rustev-eval rustev-backend-rules rustev-remote-http hyper opentelemetry opentelemetry_sdk opentelemetry-otlp
'
# Subsets whose crate is not in this release (031 3.4).
skipped='
serve|the crate rustev-serve is not in this release (spec 010, #57)
bedrock|the crate rustev-bedrock is not in this release (#66)
typesafe|rustev-jev source-links sibling files and is not admitted (031 3.4)
local-embed|the local semantic backend crate is not in this release (spec 011)
'
want=${*:-all}
selected() { [ "$want" = all ] || printf ' %s ' "$want" | grep -q " $1 "; }
# A requested name that is no subset, present or skipped, is an error, not
# a silent no-op.
if [ "$want" != all ]; then
  known=$(printf '%s\n%s\n' "$subsets" "$skipped" | cut -d'|' -f1 | grep .)
  for w in $want; do
    printf '%s\n' "$known" | grep -qx "$w" || { echo "adoption: no subset named '$w'" >&2; exit 2; }
  done
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/rustev-adoption.XXXXXX")
trap 'rm -rf "$work"' EXIT
# `IFS='|'` prefixes `read` alone (a prefix assignment to a regular
# built-in is not kept), so the `for` loops below split the lists on spaces.
echo "$subsets" | while IFS='|' read -r name features package chosen omitted; do
  [ -n "$name" ] || continue
  selected "$name" || continue
  dir="$work/$name"
  mkdir -p "$dir/src" "$dir/tests"
  feats=$(printf '%s' "$features" | awk '{for(i=1;i<=NF;i++) printf "%s\"%s\"", (i>1?", ":""), $i}')
  {
    printf '[package]\nname = "rustev-adoption-%s"\nversion = "0.0.0"\nedition = "%s"\npublish = false\n\n' "$name" "$edition"
    # An empty workspace table: the consumer is its own workspace, never
    # a member of Rustev's.
    printf '[workspace]\n\n[dependencies]\n'
    if [ -n "$version" ]; then
      printf 'rustev = { version = "=%s", default-features = false, features = [%s] }\n' "$version" "$feats"
      if [ -n "$package" ]; then
        printf '%s = "=%s"\n' "$package" "$version"
      fi
    else
      printf 'rustev = { path = "%s/integrations/rustev", default-features = false, features = [%s] }\n' "$root" "$feats"
      if [ -n "$package" ]; then
        printf '%s = { path = "%s/packages/%s" }\n' "$package" "$root" "$package"
      fi
    fi
  } > "$dir/Cargo.toml"
  printf '//! A consumer of the facade: subset %s.\n' "$name" > "$dir/src/lib.rs"
  # The test names every module the subset takes, so a missing one fails
  # to compile, and parses a definition through the facade.
  {
    printf '#[test]\nfn the_subset_builds_and_parses_a_definition() {\n'
    # Proof the test ran, independent of the harness's output format.
    printf '    std::fs::write(std::env::var("RUSTEV_ADOPTION_MARK").unwrap(), b"ran").unwrap();\n'
    # `Document` is the trait whose `parse` the test calls below.
    printf '    use rustev::contract::Document;\n'
    # `contract` and `decision_core` are the facade's always-present modules
    # (031 3.2.1), behind no feature, so every subset names them.
    printf '    let _ = rustev::decision_core::compile;\n'
    for f in $features; do
      printf '    #[allow(unused_imports)]\n    use rustev::%s as _;\n' "$(printf '%s' "$f" | tr - _)"
    done
    if [ -n "$package" ]; then
      printf '    let def = rustev::contract::definition::Definition::parse(%s::DEFINITION);\n' "$(printf '%s' "$package" | tr - _)"
      printf '    assert!(def.is_ok(), "{:?}", def.err());\n'
    else
      printf '    assert!(rustev::contract::definition::Definition::parse(b"{}").is_err());\n'
    fi
    printf '}\n'
  } > "$dir/tests/subset.rs"
  # The workspace's lockfile and pinned toolchain, so the consumer resolves
  # the reviewed versions and builds with the compiler the workspace uses.
  if [ -z "$version" ]; then
    cp "$root/Cargo.lock" "$dir/Cargo.lock"
    cp "$root/rust-toolchain.toml" "$dir/rust-toolchain.toml"
  else
    (cd "$dir" && cargo generate-lockfile)
    python3 "$script/qualify/harness.py" lock "$dir/Cargo.lock" "$RUSTEV_ADOPTION_MANIFEST"
  fi
  echo "== subset $name: features [${features}]${package:+, package $package}"
  # The subset's own test must run and pass: a build alone is not enough.
  # The test writes a mark file, so its having run is checked directly.
  mark="$dir/ran"
  # In registry mode the lockfile was checked against the signed manifest
  # before anything built, so the test runs `--locked` to keep it as checked.
  if ! result=$(cd "$dir" && RUSTEV_ADOPTION_MARK="$mark" cargo test --quiet ${version:+--locked} --test subset 2>&1); then
    printf '%s\n' "$result" >&2
    echo "subset $name: tests failed" >&2
    exit 1
  fi
  [ -f "$mark" ] || {
    printf '%s\n' "$result" >&2
    echo "subset $name: its test did not run" >&2
    exit 1
  }
  # `--locked` cannot hold here: the copied lockfile names the workspace's
  # packages, which the consumer's own root replaces. What it guarantees
  # instead is checked: every registry package the consumer resolved is at
  # exactly the version the workspace's lockfile pins.
  if [ -z "$version" ]; then
    drift=$(awk '
      FNR == 1 { file++ }
      /^name = / { n = $3 }
      /^version = / { v = $3 }
      /^source = "registry/ { if (file == 1) pinned[n] = pinned[n] " " v; else used[n " " v] = 1 }
      END { for (k in used) { split(k, a, " "); if (index(pinned[a[1]] " ", " " a[2] " ") == 0) print k } }
    ' "$root/Cargo.lock" "$dir/Cargo.lock")
    if [ -n "$drift" ]; then
      echo "subset $name: resolved versions the workspace does not pin: $drift" >&2
      exit 1
    fi
  else
    if [ -n "${RUSTEV_ADOPTION_EVIDENCE:-}" ]; then
      mkdir -p "$RUSTEV_ADOPTION_EVIDENCE"
      cp "$dir/Cargo.lock" "$RUSTEV_ADOPTION_EVIDENCE/$name.Cargo.lock"
    fi
  fi
  tree=$(cd "$dir" && cargo tree -e normal --prefix none --format '{p}' | awk '{print $1}' | sort -u)
  for c in $chosen; do
    printf '%s\n' "$tree" | grep -qx "$c" || { echo "subset $name: $c is missing from its graph" >&2; exit 1; }
  done
  for c in $omitted; do
    if printf '%s\n' "$tree" | grep -qx "$c"; then
      echo "subset $name: the omitted $c is in its graph" >&2
      exit 1
    fi
  done
  echo "subset $name: pass ($(printf '%s\n' "$tree" | wc -l | tr -d ' ') crates in the normal graph)"
done

echo "$skipped" | while IFS='|' read -r name reason; do
  [ -n "$name" ] || continue
  selected "$name" || continue
  echo "subset $name: skipped, not passed: $reason"
done

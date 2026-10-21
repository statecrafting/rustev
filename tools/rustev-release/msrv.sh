#!/bin/sh
# Spec 031 3.6.7 and spec 021 3.7: build and test every release member at
# the declared MSRV (the workspace's `rust-version`), in the workspace, with
# the locked dependencies. The candidate repeats this on the unpacked
# archives.
#
#   sh tools/rustev-release/msrv.sh
set -eu

root=$(git rev-parse --show-toplevel)
cd "$root"
unset CARGO_BUILD_TARGET
cargo build --quiet --locked -p rustev-release
target=$(cargo run --quiet --locked -p rustev-release -- field target)
release="$target/debug/rustev-release"
msrv=$("$release" field msrv)
packages=""
for p in $("$release" field packages); do packages="$packages -p $p"; done
# An empty member list would test the whole workspace instead: refused.
if [ -z "$packages" ] || [ -z "$msrv" ]; then
  echo "031 3.6.7: the release set names no package or no MSRV" >&2
  exit 1
fi
echo "msrv: testing$packages on $msrv"
# shellcheck disable=SC2086 # one -p per member
cargo "+$msrv" test --locked $packages
echo "msrv: every release member builds and passes its tests on $msrv"

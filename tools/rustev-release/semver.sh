#!/bin/sh
# Spec 031 3.8 and R-3: `cargo semver-checks` (exact pin) for every
# published family crate against the previous released family version on
# crates.io. With no previous release a crate reports `no-baseline`, which
# is recorded as not applicable and never as a passing check.
#
#   sh tools/rustev-release/semver.sh [<report file>]
#
# The report defaults to `semver.txt` in the cargo target directory.
#
# Reads the public sparse index only; never uploads or reads a credential.
# Needs git, cargo, curl, Node (to read the index's JSON lines; 20 or later,
# as `make bindings` does) and the pinned cargo-semver-checks.
set -eu

PIN=0.51.0
# Every tool this script runs, checked before anything else, so a missing
# one is named rather than surfacing as a later, unrelated failure.
for tool in git cargo curl node; do
  command -v "$tool" >/dev/null 2>&1 || { echo "031 3.8: semver.sh needs $tool on PATH" >&2; exit 1; }
done
node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 20 ? 0 : 1)' ||
  { echo "031 3.8: semver.sh needs Node 20 or later, found $(node --version)" >&2; exit 1; }
# The pinned checker, required whether or not a baseline exists, so a
# missing or wrong tool is found on the first run, not the first release.
have=$(cargo semver-checks --version 2>/dev/null || true)
if [ "$have" != "cargo-semver-checks $PIN" ]; then
  echo "031 3.8: need cargo-semver-checks $PIN (cargo install cargo-semver-checks --version $PIN --locked), found '${have}'" >&2
  exit 1
fi
root=$(git rev-parse --show-toplevel)
cd "$root"
unset CARGO_BUILD_TARGET
cargo build --quiet --locked -p rustev-release
target=$(cargo run --quiet --locked -p rustev-release -- field target)
release="$target/debug/rustev-release"
report=${1:-$target/semver.txt}
version=$("$release" field version)
# The release policy admits only `X.Y.Z` family versions (021 3.2), which
# the comparison below assumes; anything else is refused here too.
dots=$(printf '%s' "$version" | tr -cd . | wc -c | tr -d ' ')
case $version in
  *[!0-9.]* | .* | *. | *..*) dots=0 ;;
esac
if [ "$dots" -ne 2 ]; then
  echo "031 3.8: family version '$version' is not X.Y.Z" >&2
  exit 1
fi

# The sparse index path of a crate name (crates.io's layout).
index_path() {
  n=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
  case ${#n} in
    1) printf '1/%s' "$n" ;;
    2) printf '2/%s' "$n" ;;
    3) printf '3/%s/%s' "$(printf '%s' "$n" | cut -c1)" "$n" ;;
    *) printf '%s/%s/%s' "$(printf '%s' "$n" | cut -c1-2)" "$(printf '%s' "$n" | cut -c3-4)" "$n" ;;
  esac
}

: > "$report"
# One scratch directory for every index response, removed however the
# script ends.
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
checked=0
for name in $("$release" field packages); do
  body="$scratch/$name.index"
  code=$(curl -sS --max-time 60 -o "$body" -w '%{http_code}' -A 'rustev-release (https://github.com/statecrafting/rustev)' \
    "https://index.crates.io/$(index_path "$name")")
  case $code in
    404)
      echo "031 3.8: $name: no-baseline: no release of $name exists on crates.io; not applicable, not passed" | tee -a "$report"
      continue
      ;;
    200) ;;
    *)
      echo "031 3.8: $name: the index answered HTTP $code; no verdict" >&2
      exit 1
      ;;
  esac
  # The newest non-yanked version below this release's. The file and
  # version reach the script as environment variables, not arguments.
  baseline=$(INDEX_FILE="$body" CURRENT="$version" node -e '
    const lines = require("fs").readFileSync(process.env.INDEX_FILE, "utf8").split("\n").filter(Boolean);
    const current = process.env.CURRENT;
    const cmp = (a, b) => { const x = a.split(".").map(Number), y = b.split(".").map(Number);
      for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]; return 0; };
    const vs = lines.map((l) => JSON.parse(l)).filter((e) => !e.yanked && /^\d+\.\d+\.\d+$/.test(e.vers) && cmp(e.vers, current) < 0).map((e) => e.vers).sort(cmp);
    process.stdout.write(vs.pop() ?? "");
  ') || {
    # An unreadable index response is no verdict, never `no-baseline`.
    echo "031 3.8: $name: could not read its index response; no verdict" >&2
    exit 1
  }
  if [ -z "$baseline" ]; then
    echo "031 3.8: $name: no-baseline: no earlier released version than $version; not applicable, not passed" | tee -a "$report"
    continue
  fi
  cargo semver-checks check-release --package "$name" --baseline-version "$baseline"
  echo "031 3.8: $name: checked against $baseline with cargo-semver-checks $PIN: pass" | tee -a "$report"
  checked=$((checked + 1))
done
# Spec 031 3.8: npm baselines are authenticated before extraction and compared
# with the same breaking-position version rule as the Rust family.
node tools/rustev-release/npm-api-diff.mjs --check-typescript
for pkg in @statecrafting/rustev @statecrafting/rustev-wasm; do
  case $pkg in
    @statecrafting/rustev) package_dir=integrations/rustev-node ;;
    @statecrafting/rustev-wasm) package_dir=integrations/rustev-wasm ;;
  esac
  body="$scratch/npm-index.json"
  if ! code=$(curl -sS --max-time 60 -o "$body" -w '%{http_code}' -A 'rustev-release (https://github.com/statecrafting/rustev)' \
    "https://registry.npmjs.org/$(printf '%s' "$pkg" | sed 's|/|%2f|')"); then
    echo "031 3.8: $pkg: npm registry request failed; no verdict" >&2
    exit 1
  fi
  case $code in
    404)
      echo "031 3.8: $pkg: no-baseline: no release of $pkg exists on npm; not applicable, not passed" | tee -a "$report"
      continue
      ;;
    200) ;;
    *)
      echo "031 3.8: $pkg: the npm registry answered HTTP $code; no verdict" >&2
      exit 1
      ;;
  esac
  baseline_file="$scratch/npm-baseline.json"
  node tools/rustev-release/npm-api-diff.mjs --baseline "$body" "$version" "$pkg" > "$baseline_file" || {
    echo "031 3.8: $pkg: could not read npm baseline; no verdict" >&2
    exit 1
  }
  if [ ! -s "$baseline_file" ]; then
    echo "031 3.8: $pkg: no-baseline: no earlier non-deprecated version than $version; not applicable, not passed" | tee -a "$report"
    continue
  fi
  baseline=$(node -e 'process.stdout.write(require(process.argv[1]).version)' "$baseline_file")
  tarball=$(node -e 'process.stdout.write(require(process.argv[1]).dist.tarball)' "$baseline_file")
  if ! curl -fLsS --proto '=https' --proto-redir '=https' --max-time 60 -o "$scratch/npm.tgz" "$tarball"; then
    echo "031 3.8: $pkg: npm tarball download failed; no verdict" >&2
    exit 1
  fi
  old_dir="$scratch/$(basename "$package_dir")-$baseline"
  node tools/rustev-release/npm-api-diff.mjs --unpack "$scratch/npm.tgz" "$baseline_file" "$old_dir" || {
    echo "031 3.8: $pkg: npm tarball verification or extraction failed; no verdict" >&2
    exit 1
  }
  diff_report="$scratch/npm-diff.json"
  status=0
  node tools/rustev-release/npm-api-diff.mjs "$old_dir/package" "$package_dir" > "$diff_report" || status=$?
  case $status in
    0|2) ;;
    *) echo "031 3.8: $pkg: declaration comparison failed; no verdict" >&2; exit 1 ;;
  esac
  cat "$diff_report" >> "$report"
  if node tools/rustev-release/npm-api-diff.mjs --version-rule "$diff_report"; then
    echo "031 3.8: $pkg: checked against $baseline: pass" | tee -a "$report"
  else
    cat "$diff_report" >&2
    echo "031 3.8: $pkg: checked against $baseline: fail" | tee -a "$report"
    exit 1
  fi
done
# The service API (`rustev.serve/1`) has no crate in this release (spec
# 010), so it has nothing to diff.
echo "031 3.8: rustev.serve/1: absent from this release; nothing to compare" | tee -a "$report"
echo "031 3.8: semver: $checked crate(s) checked against a baseline; the rest are no-baseline (not applicable)"

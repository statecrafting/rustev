#!/bin/sh
# 031 3.9: workflow entry points; run from the checkout root, never publish.
set -eu
export LC_ALL=C TZ=UTC
fail() { echo "031 3.9: $*" >&2; exit 1; }
release() { cargo run --quiet --locked -p rustev-release -- "$@"; }
platforms='darwin-arm64 darwin-x64 linux-arm64-gnu linux-arm64-musl linux-x64-gnu linux-x64-musl win32-x64-msvc'
epoch_check() { case "$1" in ''|*[!0-9]*) fail 'source epoch must be unsigned seconds' ;; esac; }
# The script runs under `set -eu`; the explicit `fail` keeps the refusal
# independent of how a caller composes the call.
version_check() {
  test "$1" = "$(release field version)" || release candidate-version "v$1" >/dev/null ||
    fail '053 3.2.2: version is neither release-set.json'"'"'s nor a candidate of it'
}
artifact_release() {
  case "${version:-}" in *-rc.*) release --candidate "v$version" "$@" ;; *) release "$@" ;; esac
}
triple() {
  case "$1" in
    darwin-arm64) echo aarch64-apple-darwin ;;
    darwin-x64) echo x86_64-apple-darwin ;;
    linux-arm64-gnu) echo aarch64-unknown-linux-gnu ;;
    linux-arm64-musl) echo aarch64-unknown-linux-musl ;;
    linux-x64-gnu) echo x86_64-unknown-linux-gnu ;;
    linux-x64-musl) echo x86_64-unknown-linux-musl ;;
    win32-x64-msvc) echo x86_64-pc-windows-msvc ;;
    *) fail "unknown platform $1" ;;
  esac
}
case "${1:-}" in
  archive-cli)
    # 031 3.9.3: raw inputs <bin-dir>/<platform>/rustev[.exe], all seven required.
    test "$#" = 5 || fail 'archive-cli <bin-dir> <out-dir> <version> <epoch>'
    bins=$2 out=$3 version=$4 epoch=$5
    epoch_check "$epoch"; version_check "$version"
    tar_version=$(tar --version) || fail 'GNU tar required; bsdtar is refused'
    case "$tar_version" in *'GNU tar'*) ;; *) fail 'GNU tar required; bsdtar is refused' ;; esac
    mkdir -p "$out"
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/rustev-archive.XXXXXX")
    trap 'rm -rf "$scratch"' 0
    trap 'exit 1' 1 2 3 15
    for p in $platforms; do
      exe=rustev; [ "$p" != win32-x64-msvc ] || exe=rustev.exe
      name="rustev-$version-$p"
      mkdir "$scratch/$name"
      cp "$bins/$p/$exe" "$scratch/$name/$exe"
      cp LICENSE "$scratch/$name/LICENSE"
      chmod 755 "$scratch/$name" "$scratch/$name/$exe"
      chmod 644 "$scratch/$name/LICENSE"
      # 031 3.9.3: separate commands preserve tar's failure status in POSIX sh.
      tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$epoch" --format=gnu -cf "$scratch/archive.tar" -C "$scratch" "$name"
      gzip -9n < "$scratch/archive.tar" > "$out/$name.tar.gz"
      (cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
    done
    ;;
  build-native)
    # 031 3.9.3: same fresh runner, images and commands in release and rebuild.
    test "$#" = 5 || fail 'build-native <platform> <cli|rebuild> <out-dir> <epoch>'
    p=$2 mode=$3 out=$4 SOURCE_DATE_EPOCH=$5
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"
    triple "$p" >/dev/null
    case "$mode" in cli|rebuild) ;; *) fail 'build mode must be cli or rebuild' ;; esac
    case "$p" in
      *-gnu) image='rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e' ;;
      *-musl) image='rust:1.98.1-alpine3.22@sha256:a1796ca6fa216d6727b5f61c69e4c665b120b1a4dcb969639e2f25f1ed309456' ;;
      *) image='' ;;
    esac
    if [ -n "$image" ]; then
      # The CLI as increment 6 built it (static on musl, at the source
      # date); the rebuild's addon exactly as bindings.yml builds it (no
      # source date, and `-crt-static` on musl), so each artifact's two
      # builds share one environment (031 3.9.3).
      docker run --rm -v "$PWD":/w -w /w -e SOURCE_DATE_EPOCH -e MODE="$mode" -e PLATFORM="$p" "$image" sh -euc '
        cargo build -p rustev-cli --locked --release
        target/release/rustev --help >/dev/null
        f=tools/rustev-conformance/fixtures/cli/lodging.check
        target/release/rustev plan check --definition "$f/lodging.definition.json" --rules "$f/lodging.rules.json" > target/release-check.json
        cmp target/release-check.json crates/rustev-cli/tests/golden/lodging.check.json
        if [ "$MODE" = rebuild ]; then
          case "$PLATFORM" in
            *-musl) env -u SOURCE_DATE_EPOCH RUSTFLAGS="-C target-feature=-crt-static" cargo build -p rustev-node --locked --release ;;
            *) env -u SOURCE_DATE_EPOCH cargo build -p rustev-node --locked --release ;;
          esac
        fi
      '
      sudo chown -R "$(id -u):$(id -g)" target
    else
      cargo build -p rustev-cli --locked --release
      bin=target/release/rustev; [ "$p" != win32-x64-msvc ] || bin=target/release/rustev.exe
      "$bin" --help >/dev/null
      f=tools/rustev-conformance/fixtures/cli/lodging.check
      "$bin" plan check --definition "$f/lodging.definition.json" --rules "$f/lodging.rules.json" > target/release-check.json
      cmp target/release-check.json crates/rustev-cli/tests/golden/lodging.check.json
      if [ "$mode" = rebuild ]; then env -u SOURCE_DATE_EPOCH cargo build -p rustev-node --locked --release; fi
    fi
    mkdir -p "$out/cli/$p" "$out/addons"
    exe=rustev; [ "$p" != win32-x64-msvc ] || exe=rustev.exe
    cp "target/release/$exe" "$out/cli/$p/$exe"
    if [ "$mode" = rebuild ]; then
      case "$p" in darwin-*) lib=librustev_node.dylib ;; win32-*) lib=rustev_node.dll ;; *) lib=librustev_node.so ;; esac
      cp "target/release/$lib" "$out/addons/rustev.$p.node"
    fi
    ;;
  pack)
    # 031 3.9.3: call separately on fresh release/rebuild checkouts and addon sets.
    test "$#" = 4 || test "$#" = 5 || fail 'pack <flat-addon-dir> <out-dir> <epoch> [<version>]'
    version=${5:-}
    test -z "$version" || version_check "$version"
    addons=$2 out=$3 SOURCE_DATE_EPOCH=$4
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"
    release manifests
    mkdir -p "$out"
    out=$(cd "$out" && pwd)
    test "$(wasm-bindgen --version)" = 'wasm-bindgen 0.2.129' || fail '027 3.10.2: wasm-bindgen 0.2.129 required'
    wasm_opt_version=$(wasm-opt --version)
    case "$wasm_opt_version" in 'wasm-opt version 116'|'wasm-opt version 116 (version_116)') ;; *) fail '027 3.10.2: wasm-opt 116 required (crate 0.116.1)' ;; esac
    cargo build -p rustev-wasm --locked --target wasm32-unknown-unknown --profile wasm
    target=$(release field target)
    for t in web bundler nodejs; do
      d=$t; [ "$t" != nodejs ] || d=node
      wasm-bindgen --target "$t" --out-dir "integrations/rustev-wasm/$d" "$target/wasm32-unknown-unknown/wasm/rustev_wasm.wasm"
      wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "integrations/rustev-wasm/$d/rustev_wasm_bg.wasm" "integrations/rustev-wasm/$d/rustev_wasm_bg.wasm"
      ty=module; [ "$t" != nodejs ] || ty=commonjs
      printf '{ "type": "%s" }\n' "$ty" > "integrations/rustev-wasm/$d/package.json"
    done
    # 031 3.9.2: stage the exports entry points (index.js, index.d.ts), as make bindings does.
    node tools/rustev-conformance/js/wrap-wasm.mjs integrations/rustev-wasm
    for p in $platforms; do cp "$addons/rustev.$p.node" "integrations/rustev-node/npm/$p/"; done
    dirs=$(jq -er '.npm[].directory' tools/rustev-release/release-set.json)
    for dir in $dirs; do
      node -e '
        const fs = require("fs"), f = process.argv[1] + "/package.json";
        const p = JSON.parse(fs.readFileSync(f, "utf8"));
        if (p.private !== true) throw Error("031 3.1: missing private guard: " + f);
        delete p.private;
        const version = process.argv[2];
        if (version.includes("-rc.")) {
          const members = new Set(JSON.parse(fs.readFileSync("tools/rustev-release/release-set.json", "utf8")).npm.map(p => p.name));
          p.version = version;
          for (const name of Object.keys(p.optionalDependencies || {})) {
            if (members.has(name)) p.optionalDependencies[name] = version;
          }
        }
        fs.writeFileSync(f, JSON.stringify(p, null, 2) + "\n");' "$dir" "$version"
      (cd "$dir" && npm pack --pack-destination "$out" --ignore-scripts)
    done
    (cd "$out" && sha256sum ./*.tgz > SHA256SUMS)
    artifact_release npm-audit "$out"
    ;;
  sbom)
    # 031 3.9.1: all 16 release archives reside directly in out-dir.
    test "$#" = 4 || fail 'sbom <out-dir> <version> <epoch>'
    out=$2 version=$3 SOURCE_DATE_EPOCH=$4
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"; version_check "$version"
    # `cargo cyclonedx --version` really prints the doubled name:
    # "cargo-cyclonedx-cyclonedx 0.5.9" (checked against 0.5.9).
    test "$(cargo cyclonedx --version)" = 'cargo-cyclonedx-cyclonedx 0.5.9' || fail 'cargo-cyclonedx 0.5.9 required: cargo install cargo-cyclonedx --locked --version 0.5.9'
    for p in $platforms; do
      target=$(triple "$p")
      for crate in rustev-cli rustev-node; do
        case "$crate" in
          rustev-cli) dir=crates/rustev-cli; artifact="rustev-$version-$p.tar.gz" ;;
          rustev-node) dir=integrations/rustev-node; artifact="rustev-rustev-$p-$version.tgz" ;;
        esac
        test -f "$out/$artifact" || fail "missing artifact $artifact"
        cargo cyclonedx --manifest-path "$dir/Cargo.toml" --format json --spec-version 1.5 --all --target "$target"
        cp "$dir/$crate.cdx.json" "$out/$artifact.cdx.json"
      done
    done
    artifact="rustev-rustev-wasm-$version.tgz"
    test -f "$out/$artifact" || fail "missing artifact $artifact"
    cargo cyclonedx --manifest-path integrations/rustev-wasm/Cargo.toml --format json --spec-version 1.5 --all --target wasm32-unknown-unknown
    cp integrations/rustev-wasm/rustev-wasm.cdx.json "$out/$artifact.cdx.json"
    artifact="rustev-rustev-$version.tgz"
    artifact_release main-sbom "$out/$artifact" "$SOURCE_DATE_EPOCH" > "$out/$artifact.cdx.json"
    artifact_release sbom-check "$out" "$out"
    ;;
  audit)
    # 031 3.9.2; 021 3.6: union of three runtime/build graphs, policy excludes dev.
    test "$#" = 2 || test "$#" = 3 || fail 'audit <npm-dir> [<version>]'
    version=${3:-}
    test -z "$version" || version_check "$version"
    artifact_release npm-audit "$2"
    test "$(cargo deny --version)" = 'cargo-deny 0.20.2' || fail 'cargo-deny 0.20.2 required: cargo install cargo-deny --locked --version 0.20.2'
    failed=0
    for manifest in crates/rustev-cli/Cargo.toml integrations/rustev-node/Cargo.toml integrations/rustev-wasm/Cargo.toml; do
      cargo deny --manifest-path "$manifest" --config tools/rustev-release/deny.toml --locked check || failed=1
    done
    test "$failed" = 0 || fail '021 3.6: dependency audit failed; policy changes require separate review'
    release exceptions "$(date -u +%Y-%m-%d)"
    ;;
  build-image)
    # 031 3.9.3, 042 3.16: one platform of the rustev-serve image, built
    # without cache on a fresh BuildKit builder pinned by digest, at the
    # commit's source date with every layer's timestamps clamped to it, as
    # an OCI archive; then loaded and checked by image/check.sh. Release and
    # rebuild run this same command on their own runners.
    test "$#" = 4 || fail 'build-image <linux/amd64|linux/arm64> <out-dir> <epoch>'
    platform=$2 out=$3 SOURCE_DATE_EPOCH=$4
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"
    case "$platform" in linux/amd64|linux/arm64) ;; *) fail "unsupported image platform $platform" ;; esac
    arch=${platform#linux/}
    buildkit='moby/buildkit:v0.34.0@sha256:b059f8d7226d0b326bc871af489a5dddf65e36af4d20d14251b072974d9ee87c'
    builder="rustev-image-$arch-$$"
    mkdir -p "$out"
    archive="$out/rustev-serve-linux-$arch.oci.tar"
    # The same build is also exported in Docker's format for the check: an
    # engine without the containerd image store cannot load an OCI archive.
    # Cleanup is registered first, so a failure at any later step leaves
    # neither the directory nor the builder behind.
    check_dir=
    trap 'docker buildx rm "$builder" >/dev/null 2>&1 || true; test -z "$check_dir" || rm -rf "$check_dir"' 0
    trap 'exit 1' 1 2 3 15
    check_dir=$(mktemp -d "${TMPDIR:-/tmp}/rustev-image-check.XXXXXX")
    docker buildx create --name "$builder" --driver docker-container --driver-opt "image=$buildkit" >/dev/null
    docker buildx build --builder "$builder" --no-cache --pull --platform "$platform" \
      --build-arg SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" --provenance=false --sbom=false \
      -f integrations/rustev-serve/image/Dockerfile \
      --output "type=oci,dest=$archive,rewrite-timestamp=true" \
      --output "type=docker,dest=$check_dir/image.tar" .
    loaded=$(docker load -i "$check_dir/image.tar")
    id=$(printf '%s\n' "$loaded" | sed -n 's/^Loaded image ID: //p')
    test -n "$id" || fail "docker load named no image ID: $loaded"
    sh integrations/rustev-serve/image/check.sh "$id" "$platform"
    (cd "$out" && sha256sum "${archive##*/}" > "${archive##*/}.sha256")
    ;;
  compare-image)
    # 031 3.9.3: the release and rebuild archives of each platform must be
    # identical; a difference names every differing archive entry and is
    # never normalized away.
    test "$#" = 3 || fail 'compare-image <release-dir> <rebuild-dir>'
    a=$2 b=$3 failed=0 scratch=
    trap 'test -z "$scratch" || rm -rf "$scratch"' 0
    trap 'exit 1' 1 2 3 15
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/rustev-image.XXXXXX")
    for arch in amd64 arm64; do
      name="rustev-serve-linux-$arch.oci.tar"
      test -f "$a/$name" || fail "missing $a/$name"
      test -f "$b/$name" || fail "missing $b/$name"
      if cmp -s "$a/$name" "$b/$name"; then
        echo "$name: identical $(sha256sum < "$a/$name" | cut -d' ' -f1)"
        continue
      fi
      failed=1
      echo "$name: the two builds differ" >&2
      for side in a b; do
        case "$side" in a) src=$a ;; b) src=$b ;; esac
        mkdir -p "$scratch/$arch/$side"
        tar -xf "$src/$name" -C "$scratch/$arch/$side"
        (cd "$scratch/$arch/$side" && find . -type f -exec sha256sum {} + | sort -k 2) > "$scratch/$arch/$side.sums"
      done
      diff "$scratch/$arch/a.sums" "$scratch/$arch/b.sums" >&2 || true
    done
    test "$failed" = 0 || fail 'two image builds produce different platform digests'
    ;;
  sbom-image)
    # 031 3.9.1: a CycloneDX 1.5 SBOM per image platform from the archive
    # (pinned syft: the base image's packages and the binary), and the
    # rustev-serve crate's own for the platform's target (cargo-cyclonedx).
    test "$#" = 3 || fail 'sbom-image <image-dir> <epoch>'
    dir=$2 SOURCE_DATE_EPOCH=$3
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"
    syft version | grep -qx 'Version: *1\.54\.1' || fail 'syft 1.54.1 required'
    test "$(cargo cyclonedx --version)" = 'cargo-cyclonedx-cyclonedx 0.5.9' || fail 'cargo-cyclonedx 0.5.9 required: cargo install cargo-cyclonedx --locked --version 0.5.9'
    for arch in amd64 arm64; do
      name="rustev-serve-linux-$arch.oci.tar"
      test -f "$dir/$name" || fail "missing $dir/$name"
      # Each SBOM is checked non-empty, and the crate's is removed before
      # generation, so a failed tool never leaves a stale or missing file.
      syft -q "oci-archive:$dir/$name" -o "cyclonedx-json@1.5=$dir/$name.cdx.json" || fail "syft failed for $name"
      test -s "$dir/$name.cdx.json" || fail "no image SBOM for $name"
      case "$arch" in amd64) target=x86_64-unknown-linux-gnu ;; arm64) target=aarch64-unknown-linux-gnu ;; esac
      rm -f integrations/rustev-serve/rustev-serve.cdx.json
      cargo cyclonedx --manifest-path integrations/rustev-serve/Cargo.toml --format json --spec-version 1.5 --all --target "$target" || fail "cargo cyclonedx failed for $target"
      test -s integrations/rustev-serve/rustev-serve.cdx.json || fail "no crate SBOM for $target"
      cp integrations/rustev-serve/rustev-serve.cdx.json "$dir/$name.crate.cdx.json" || fail "cannot copy the crate SBOM for $target"
    done
    ;;
  scan-image)
    # 031 3.9.2: the compiled crate through cargo deny, and the base layers
    # through a pinned grype over each platform's SBOM. While the release
    # set lists the image as absent, cargo deny's bans and the scan's
    # findings are reported and recorded without failing; once the image
    # is a member, they fail, and an image finding passes only under an
    # owner-approved, unexpired exception, which this command does not yet
    # read.
    test "$#" = 2 || fail 'scan-image <image-dir>'
    dir=$2
    grype version | grep -qx 'Version: *0\.120\.1' || fail 'grype 0.120.1 required'
    # grype reads its configuration from the checkout, the home directory
    # and XDG paths unless one is named, and GRYPE_FAIL_ON_SEVERITY
    # overrides any file. Naming this file and clearing that variable
    # leaves no fail-on setting, so grype exits 0 whatever it finds and a
    # non-zero status is a tool error, never a finding.
    grype_dir=
    trap 'test -z "$grype_dir" || rm -rf "$grype_dir"' 0
    trap 'exit 1' 1 2 3 15
    grype_dir=$(mktemp -d "${TMPDIR:-/tmp}/rustev-grype.XXXXXX")
    printf 'fail-on-severity: ""\n' > "$grype_dir/grype.yaml"
    grype_scan() { env -u GRYPE_FAIL_ON_SEVERITY grype -q -c "$grype_dir/grype.yaml" "$@"; }
    test "$(cargo deny --version)" = 'cargo-deny 0.20.2' || fail 'cargo-deny 0.20.2 required: cargo install cargo-deny --locked --version 0.20.2'
    member=yes
    python3 -c 'import json,sys; s=json.load(open("tools/rustev-release/release-set.json")); sys.exit(0 if any(a["kind"]=="image" for a in s.get("absent",[])) else 1)' && member=no
    deny() { cargo deny --manifest-path integrations/rustev-serve/Cargo.toml --config tools/rustev-release/deny.toml --locked check "$@"; }
    # Advisories, licenses and sources gate always. Bans (duplicate
    # versions, wildcard path dependencies) are the crate's own admission
    # to the family (031 3.1): reported while the image is absent, gating
    # once it is a member.
    deny advisories licenses sources ||
      fail '031 3.9.2: dependency audit of rustev-serve failed; policy changes require separate review'
    if ! deny bans; then
      test "$member" = no || fail '031 3.9.2: rustev-serve fails the bans policy'
      echo '031 3.9.2: rustev-serve fails the bans policy; recorded, not gating while the release set lists the image as absent'
    fi
    total=0
    for arch in amd64 arm64; do
      sbom="$dir/rustev-serve-linux-$arch.oci.tar.cdx.json"
      test -f "$sbom" || fail "missing $sbom"
      # Findings are counted from the JSON report below (grype_scan above).
      grype_scan "sbom:$sbom" -o json --file "$dir/rustev-serve-linux-$arch.vulnerabilities.json" ||
        fail "grype could not scan $sbom"
      grype_scan "sbom:$sbom" -o table || fail "grype could not scan $sbom"
      n=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["matches"]))' "$dir/rustev-serve-linux-$arch.vulnerabilities.json")
      echo "linux/$arch: $n finding(s)"
      total=$((total + n))
    done
    if [ "$total" != 0 ]; then
      test "$member" = no || fail "031 3.9.2: $total image finding(s) and no exception rule for image findings yet"
      echo "031 3.9.2: $total image finding(s) recorded; not gating while the release set lists the image as absent"
    fi
    ;;
  *) fail 'expected build-native, archive-cli, pack, sbom, audit, build-image, compare-image, sbom-image or scan-image' ;;
esac

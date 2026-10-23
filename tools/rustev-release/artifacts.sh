#!/bin/sh
# 031 3.9: workflow entry points; run from the checkout root, never publish.
set -eu
export LC_ALL=C TZ=UTC
fail() { echo "031 3.9: $*" >&2; exit 1; }
release() { cargo run --quiet --locked -p rustev-release -- "$@"; }
platforms='darwin-arm64 darwin-x64 linux-arm64-gnu linux-arm64-musl linux-x64-gnu linux-x64-musl win32-x64-msvc'
epoch_check() { case "$1" in ''|*[!0-9]*) fail 'source epoch must be unsigned seconds' ;; esac; }
version_check() { test "$1" = "$(release field version)" || fail 'version differs from release-set.json'; }
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
  dispatch)
    # 031 3.11, 3.12; 021 3.8: rehearsal is never publication authority.
    test "$#" = 4 || fail 'dispatch <rehearsal> <publish-npm> <tag>'
    rehearsal=$2 publish=$3 tag=$4
    case "$rehearsal:$publish" in true:true) fail '031 3.11: rehearsal cannot publish npm' ;; true:false|false:true|false:false) ;; *) fail 'dispatch booleans must be true or false' ;; esac
    if [ "$rehearsal" = true ]; then
      commit=$(git rev-parse "${GITHUB_SHA:?}^{commit}")
    else
      expected=$(jq -er .tag tools/rustev-release/release-set.json)
      test "$tag" = "$expected" || fail '031 3.12: tag differs from release set'
      test "${GITHUB_REF:?}" = "refs/tags/$tag" || fail '031 3.12: dispatch must use the release tag ref'
      test "$(git cat-file -t "refs/tags/$tag")" = tag || fail '021 3.8: annotated tag required'
      git -c gpg.format=ssh -c gpg.ssh.allowedSignersFile=tools/rustev-release/allowed_signers verify-tag "refs/tags/$tag"
      commit=$(git rev-parse "refs/tags/$tag^{commit}")
    fi
    test "$(git rev-parse HEAD)" = "$commit" || fail '031 3.12: checkout differs from dispatched commit'
    printf 'commit=%s\nversion=%s\nepoch=%s\n' "$commit" "$(release field version)" "$(git log -1 --format=%ct "$commit")"
    ;;
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
      target/release/rustev --help >/dev/null
      f=tools/rustev-conformance/fixtures/cli/lodging.check
      target/release/rustev plan check --definition "$f/lodging.definition.json" --rules "$f/lodging.rules.json" > target/release-check.json
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
    test "$#" = 4 || fail 'pack <flat-addon-dir> <out-dir> <epoch>'
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
    for p in $platforms; do cp "$addons/rustev.$p.node" "integrations/rustev-node/npm/$p/"; done
    dirs=$(jq -er '.npm[].directory' tools/rustev-release/release-set.json)
    for dir in $dirs; do
      node -e '
        const fs = require("fs"), f = process.argv[1] + "/package.json";
        const p = JSON.parse(fs.readFileSync(f, "utf8"));
        if (p.private !== true) throw Error("031 3.1: missing private guard: " + f);
        delete p.private;
        fs.writeFileSync(f, JSON.stringify(p, null, 2) + "\n");' "$dir"
      (cd "$dir" && npm pack --pack-destination "$out" --ignore-scripts)
    done
    (cd "$out" && sha256sum ./*.tgz > SHA256SUMS)
    release npm-audit "$out"
    ;;
  sbom)
    # 031 3.9.1: all 16 release archives reside directly in out-dir.
    test "$#" = 4 || fail 'sbom <out-dir> <version> <epoch>'
    out=$2 version=$3 SOURCE_DATE_EPOCH=$4
    export SOURCE_DATE_EPOCH
    epoch_check "$SOURCE_DATE_EPOCH"; version_check "$version"
    test "$(cargo cyclonedx --version)" = 'cargo-cyclonedx-cyclonedx 0.5.9' || fail 'cargo-cyclonedx 0.5.9 required: cargo install cargo-cyclonedx --locked --version 0.5.9'
    for p in $platforms; do
      target=$(triple "$p")
      for crate in rustev-cli rustev-node; do
        case "$crate" in
          rustev-cli) dir=crates/rustev-cli; artifact="rustev-$version-$p.tar.gz" ;;
          rustev-node) dir=integrations/rustev-node; artifact="statecrafting-rustev-$p-$version.tgz" ;;
        esac
        test -f "$out/$artifact" || fail "missing artifact $artifact"
        cargo cyclonedx --manifest-path "$dir/Cargo.toml" --format json --spec-version 1.5 --all --target "$target"
        cp "$dir/$crate.cdx.json" "$out/$artifact.cdx.json"
      done
    done
    artifact="statecrafting-rustev-wasm-$version.tgz"
    test -f "$out/$artifact" || fail "missing artifact $artifact"
    cargo cyclonedx --manifest-path integrations/rustev-wasm/Cargo.toml --format json --spec-version 1.5 --all --target wasm32-unknown-unknown
    cp integrations/rustev-wasm/rustev-wasm.cdx.json "$out/$artifact.cdx.json"
    artifact="statecrafting-rustev-$version.tgz"
    release main-sbom "$out/$artifact" "$SOURCE_DATE_EPOCH" > "$out/$artifact.cdx.json"
    release sbom-check "$out" "$out"
    ;;
  audit)
    # 031 3.9.2; 021 3.6: union of three runtime/build graphs, policy excludes dev.
    test "$#" = 2 || fail 'audit <npm-dir>'
    release npm-audit "$2"
    test "$(cargo deny --version)" = 'cargo-deny 0.20.2' || fail 'cargo-deny 0.20.2 required: cargo install cargo-deny --locked --version 0.20.2'
    failed=0
    for manifest in crates/rustev-cli/Cargo.toml integrations/rustev-node/Cargo.toml integrations/rustev-wasm/Cargo.toml; do
      cargo deny --manifest-path "$manifest" --config tools/rustev-release/deny.toml --locked check || failed=1
    done
    test "$failed" = 0 || fail '021 3.6: dependency audit failed; policy changes require separate review'
    release exceptions "$(date -u +%Y-%m-%d)"
    ;;
  *) fail 'expected dispatch, build-native, archive-cli, pack, sbom or audit' ;;
esac

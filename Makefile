# The check surface for this repository.
#
#   make tools    install the pinned spec-spine into .bin
#   make gate     read-only: judge the corpus (meaningful with no code at all)
#   make refresh  writing: recompute the committed shard trees
#   make code     build, test, clippy, fmt and the boundary check
#   make boundaries  the dependency rules of spec 001 section 3.4
#   make verify   every approved spec's declared acceptance (`spec-spine verify`)
#
# Owned by spec 001-boundaries-and-authority.
#
# The pin is authored once, in spec-spine.toml; nothing here repeats it.
SPEC_SPINE_VERSION := $(shell sed -n 's/^required_version = "=\(.*\)"/\1/p' spec-spine.toml)
SPEC_SPINE_LOCAL := .bin/spec-spine
SPEC_SPINE ?= $(if $(wildcard $(SPEC_SPINE_LOCAL)),$(SPEC_SPINE_LOCAL),spec-spine)

CRATE_MANIFESTS := $(wildcard crates/*/Cargo.toml backends/*/Cargo.toml tools/*/Cargo.toml integrations/*/Cargo.toml)
SKIP_NOTE := no crate exists yet, so cargo has nothing to judge
VERIFIED_SPECS := 001 002 003 004 005 006 007 008 009 010 012 013 014 015 017 019 020 026 027 033

.PHONY: tools gate refresh code build test clippy fmt boundaries verify release-candidate adoption msrv semver changelog conformance conformance-harvest bindings

tools:
	@test -n "$(SPEC_SPINE_VERSION)" || { echo "no required_version in spec-spine.toml"; exit 3; }
	sh scripts/statecraft/install-spec-spine.sh
	$(SPEC_SPINE_LOCAL) --version

## Read-only. A gate that writes repairs what it is meant to judge.
## `index coverage --fail-on-untraced` refuses any source file no spec claims.
gate:
	@echo "governing with: $(SPEC_SPINE) (pin =$(SPEC_SPINE_VERSION))"
	$(SPEC_SPINE) check --fail-on-warn
	$(SPEC_SPINE) lint --fail-on-warn
	$(SPEC_SPINE) index coverage --fail-on-untraced

refresh:
	$(SPEC_SPINE) compile
	$(SPEC_SPINE) index

code: build test clippy fmt boundaries

build:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo build --workspace --locked; fi

test:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo test --workspace --locked; fi

clippy:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo clippy --workspace --all-targets --locked -- -D warnings; fi

fmt:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo fmt --all --check; fi

boundaries:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo run -p rustev-boundaries --locked --quiet; fi

## Runs code the corpus declares; deliberately not part of `gate`.
verify:
	@for id in $(VERIFIED_SPECS); do $(SPEC_SPINE) verify $$id || exit 1; done

## Spec 021, 3.5 and 3.7: package evidence for the clean commit at HEAD.
## Two clean worktrees, byte comparison, archive policy and unpacked tests.
## Never uploads, tags, signs or reads a credential.
release-candidate:
	sh tools/rustev-release/candidate.sh

## Spec 031 3.5: each partial-adoption subset as a fresh consumer of the
## facade, with its graph checked for omitted crates.
adoption:
	sh tools/rustev-release/adoption.sh

## Spec 031 3.6.7: every release member built and tested at the MSRV.
msrv:
	sh tools/rustev-release/msrv.sh

## Spec 031 3.10: the release changelog, generated from the governed corpus
## at the clean commit at HEAD (PREVIOUS=<tag> compares with that release).
changelog:
	cargo build --quiet --locked -p rustev-release
	@# PREVIOUS reaches the shell as an environment variable (make exports
	@# command-line variables), never as text spliced into the command.
	t="$$(cargo run --quiet --locked -p rustev-release -- field target)" && "$$t/debug/rustev-release" changelog "$$t/changelog.md" $${PREVIOUS:+"$$PREVIOUS"}

## Spec 031 3.8: cargo-semver-checks against the previous release, or
## `no-baseline` (not applicable) when there is none. Needs curl, Node 20 or
## later and cargo-semver-checks 0.51.0 on PATH.
semver:
	sh tools/rustev-release/semver.sh

# Spec 027, 3.12: the shared conformance suite's Rust runner. Needs no Node
# toolchain.
conformance:
	cargo test -p rustev-conformance --locked

# Spec 027, 3.12.2: rebuild the CLI cases and their fixtures from the CLI
# tests that pin each golden (`RUSTEV_HARVEST`), then the manifest that
# aggregates every case, then check them.
conformance-harvest:
	rm -rf tools/rustev-conformance/cases/cli tools/rustev-conformance/fixtures/cli
	RUSTEV_HARVEST=1 cargo test -p rustev-cli --locked
	RUSTEV_BLESS=1 cargo test -p rustev-conformance --locked --test manifest
	cargo test -p rustev-conformance --locked

# Spec 027, 3.2, 3.10 and 3.12: build the WebAssembly package (web, bundler
# and nodejs targets, optimized at -Oz), check the generated declarations
# for drift and type-check them, enforce the import allowlist and the size
# and instantiation budgets, Node and headless Chromium (writing the
# measurement document), and run the JavaScript conformance runner on it in
# Node and in Chromium. Needs Node 20 or later and the exact
# tool pins below; it refuses any other version.
WASM_BINDGEN ?= wasm-bindgen
WASM_OPT ?= wasm-opt
WASM_BINDGEN_PIN := wasm-bindgen 0.2.129
WASM_OPT_PIN := wasm-opt version 116
# Cargo's target directory, wherever the configuration puts it.
# Parsed as JSON, so any path cargo reports is read exactly.
# Evaluated on first use, once: only `bindings` pays for cargo and node.
CARGO_TARGET = $(eval CARGO_TARGET := $(shell cargo metadata --format-version 1 --no-deps 2>/dev/null | node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>{try{process.stdout.write(JSON.parse(s).target_directory)}catch{}})'))$(CARGO_TARGET)
BINDINGS_OUT = $(CARGO_TARGET)/bindings
WASM_FEATURES := --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals

bindings:
	@test "$$($(WASM_BINDGEN) --version)" = "$(WASM_BINDGEN_PIN)" || { echo "027 3.10.2: need $(WASM_BINDGEN_PIN) (cargo install wasm-bindgen-cli --version 0.2.129 --locked)" >&2; exit 1; }
	@$(WASM_OPT) --version | grep -Eq "^$(WASM_OPT_PIN)( |$$)" || { echo "027 3.10.2: need $(WASM_OPT_PIN) (cargo install wasm-opt --version 0.116.1 --locked)" >&2; exit 1; }
	@node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 20 ? 0 : 1)' || { echo "027 3.12.5: need Node 20 or later" >&2; exit 1; }
	@test -n "$(CARGO_TARGET)" || { echo "bindings: no cargo target directory (it is read with cargo metadata and Node 20 or later, both needed on PATH)" >&2; exit 1; }
	cd tools/rustev-conformance/js && npm ci --no-audit --no-fund --ignore-scripts
	@# Every installed package's registry signature is verified, lodash 4.18.1 included.
	cd tools/rustev-conformance/js && npm audit signatures
	@# Headless Chromium at the build playwright-core 1.63.0 pins (3.10.5).
	cd tools/rustev-conformance/js && node_modules/.bin/playwright-core install chromium-headless-shell
	cargo build -p rustev-wasm --locked --target wasm32-unknown-unknown --profile wasm
	rm -rf "$(BINDINGS_OUT)/rustev-wasm"
	for t in web bundler nodejs; do \
	  d=$$t; [ $$t = nodejs ] && d=node; \
	  $(WASM_BINDGEN) --target $$t --out-dir "$(BINDINGS_OUT)/rustev-wasm/$$d" "$(CARGO_TARGET)/wasm32-unknown-unknown/wasm/rustev_wasm.wasm" || exit 1; \
	  $(WASM_OPT) -Oz $(WASM_FEATURES) -o "$(BINDINGS_OUT)/rustev-wasm/$$d/rustev_wasm_bg.wasm" "$(BINDINGS_OUT)/rustev-wasm/$$d/rustev_wasm_bg.wasm" || exit 1; \
	  ty=module; [ $$t = nodejs ] && ty=commonjs; \
	  printf '{ "type": "%s" }\n' $$ty > "$(BINDINGS_OUT)/rustev-wasm/$$d/package.json"; \
	done
	cp -R integrations/rustev-wasm/package.json integrations/rustev-wasm/types "$(BINDINGS_OUT)/rustev-wasm/"
	node tools/rustev-conformance/js/gen-types.mjs --check
	tools/rustev-conformance/js/node_modules/.bin/tsc --noEmit --strict integrations/rustev-wasm/types/rustev-documents.d.ts
	node tools/rustev-conformance/js/chromium.mjs time "$(BINDINGS_OUT)/rustev-wasm" "$(BINDINGS_OUT)/chromium-time.json" > "$(BINDINGS_OUT)/chromium-median"
	node tools/rustev-conformance/js/wasm-check.mjs "$(BINDINGS_OUT)/rustev-wasm" "$(BINDINGS_OUT)/wasm-measurement.json" --chromium-ms "$$(cat "$(BINDINGS_OUT)/chromium-median")" --chromium-record "$(BINDINGS_OUT)/chromium-time.json"
	node tools/rustev-conformance/js/run.mjs wasm "$(BINDINGS_OUT)/rustev-wasm"
	node tools/rustev-conformance/js/run.mjs chromium "$(BINDINGS_OUT)/rustev-wasm"
	@# 3.11.3: the differential floating-point corpus, natively, then as
	@# WebAssembly in Node and in Chromium, each against the committed digests.
	cargo run -p rustev-fp-corpus --locked --release -- check tools/rustev-conformance/fp-corpus/expected.json
	cargo build -p rustev-fp-corpus --locked --release --target wasm32-unknown-unknown
	node tools/rustev-conformance/js/fp-corpus.mjs node "$(CARGO_TARGET)/wasm32-unknown-unknown/release/rustev_fp_corpus.wasm" "$(BINDINGS_OUT)/fp-corpus-node.json"
	node tools/rustev-conformance/js/fp-corpus.mjs chromium "$(CARGO_TARGET)/wasm32-unknown-unknown/release/rustev_fp_corpus.wasm" "$(BINDINGS_OUT)/fp-corpus-chromium.json"
	cargo build -p rustev-node --locked --release
	node tools/rustev-conformance/js/copy-addon.mjs "$(CARGO_TARGET)" integrations/rustev-node
	tools/rustev-conformance/js/node_modules/.bin/tsc --noEmit --strict --lib es2022,dom integrations/rustev-node/index.d.ts
	node --test integrations/rustev-node/test/*.test.mjs
	node tools/rustev-conformance/js/run.mjs native integrations/rustev-node
	@# 3.13: the platform packages agree with the main package; this host's
	@# addon is staged into its own, both are packed and installed into a
	@# fresh project from the tarballs alone, and the suite runs on that install.
	node tools/rustev-conformance/js/platform-packages.mjs check
	node tools/rustev-conformance/js/platform-packages.mjs stage "$(CARGO_TARGET)"
	node tools/rustev-conformance/js/platform-packages.mjs fresh "$(BINDINGS_OUT)/native-fresh" > "$(BINDINGS_OUT)/native-fresh.path"
	node tools/rustev-conformance/js/run.mjs native "$$(cat "$(BINDINGS_OUT)/native-fresh.path")"

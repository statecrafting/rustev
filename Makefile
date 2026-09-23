# The check surface for this repository.
#
#   make tools    install the pinned spec-spine into .tooling/bin
#   make gate     read-only: judge the corpus (meaningful with no code at all)
#   make refresh  writing: recompute the committed shard trees
#   make code     build, test, clippy and fmt; inert until a crate exists
#
# The pin is authored once, in spec-spine.toml; nothing here repeats it.
SPEC_SPINE_VERSION := $(shell sed -n 's/^required_version = "=\(.*\)"/\1/p' spec-spine.toml)
SPEC_SPINE_LOCAL := .tooling/bin/spec-spine
SPEC_SPINE ?= $(if $(wildcard $(SPEC_SPINE_LOCAL)),$(SPEC_SPINE_LOCAL),spec-spine)

CRATE_MANIFESTS := $(wildcard crates/*/Cargo.toml)
SKIP_NOTE := no crate exists yet, so cargo has nothing to judge

.PHONY: tools gate refresh code build test clippy fmt

tools:
	@test -n "$(SPEC_SPINE_VERSION)" || { echo "no required_version in spec-spine.toml"; exit 3; }
	cargo install spec-spine-cli --version $(SPEC_SPINE_VERSION) --locked --root .tooling
	$(SPEC_SPINE_LOCAL) --version

## Read-only. A gate that writes repairs what it is meant to judge.
## `index coverage --fail-on-untraced` joins with the first source file: on a
## code-free tree it refuses an empty universe rather than passing vacuously.
gate:
	@echo "governing with: $(SPEC_SPINE) (pin =$(SPEC_SPINE_VERSION))"
	$(SPEC_SPINE) check --fail-on-warn
	$(SPEC_SPINE) lint --fail-on-warn

refresh:
	$(SPEC_SPINE) compile
	$(SPEC_SPINE) index

code: build test clippy fmt

build:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo build --workspace --locked; fi

test:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo test --workspace --locked; fi

clippy:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo clippy --workspace --all-targets --locked -- -D warnings; fi

fmt:
	@if [ -z "$(CRATE_MANIFESTS)" ]; then echo "$(SKIP_NOTE)"; else set -x; cargo fmt --all --check; fi

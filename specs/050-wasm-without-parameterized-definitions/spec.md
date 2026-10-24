---
id: "050-wasm-without-parameterized-definitions"
title: "The WebAssembly module without parameterized definitions (amends 027)"
status: approved
implementation: pending
created: "2026-10-24"
summary: >
  A narrow amendment of approved spec 027. Spec 034's parameterized
  definitions (rustev.definition/2 templates, profiles, instantiation and
  rustev.plan/3 instantiation records) put the optimized rustev_wasm_bg.wasm over spec 027's gzip
  budget, which spec 046 kept on purpose so that such a cost shows: 860,362
  bytes against 786,432 at spec 034 increment 3. Spec 034 leaves exposing
  its functions through WebAssembly to spec 027. This spec keeps the budget
  and keeps spec 034's code out of the WebAssembly module: rustev-core,
  rustev-eval and rustev-surface gain an opt-in cargo feature
  `parameterized` (spec 031 keeps every release crate's `default` empty)
  that every workspace consumer but rustev-wasm enables, and the
  WebAssembly binding refuses
  a definition that uses spec 034's members and a plan that carries an
  instantiation record with a typed refusal; version 2 definitions and
  version 3 plans of specs 032 and 033 are unchanged.
  The Node binding, the CLI and every other surface keep the feature.
  Ratified by the owner on 2026-10-24 (A-52); the feature made opt-in the
  same day, before merge, by the owner's decision (A-52).
amends:
  - "027-node-and-wasm-bindings"
extends:
  # The workspace dependency declarations (3.1).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  # The feature, its gates and the feature-off refusal (3.1, 3.2).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  # The WebAssembly binding without the feature, and its conformance
  # selection (3.1, 3.3).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # Every other consumer enables the feature, so its build is unchanged
  # (3.1); the facade enables it on its member dependencies; the release
  # set records the feature allowance and the archive list the new core
  # source file (3.5).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  - { spec: "005-reference-backends", unit: { kind: directory, path: "backends/rustev-backend-rules/" }, nature: additive }
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: additive }
  - { spec: "008-reference-package-lodging", unit: { kind: directory, path: "packages/rustev-pkg-lodging/" }, nature: additive }
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-aicortex/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-rahi/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  - { spec: "030-bedrock-backend", unit: { kind: directory, path: "integrations/rustev-bedrock/" }, nature: additive }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: directory, path: "integrations/rustev-otel/" }, nature: additive }
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "integrations/rustev/" }, nature: additive }
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "034-parameterized-definitions"
  - "046-wasm-size-budget"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "rustev-core, rustev-eval and rustev-surface compile spec 034's template, profile, instantiation and rustev.plan/3 code only under a cargo feature `parameterized`, not enabled by default (spec 031's empty `default`) and forwarded between them; every workspace consumer of them except rustev-wasm enables it, the facade `rustev` included, and rustev-wasm enables it on none of them."
    anchor: "3-behavior"
  - id: "R-2"
    kind: requirement
    text: "Without the feature, a command given a definition that uses any of spec 034's members (`parameters`, `variants`, `only_in`, `enabled` or a parameter reference, spec 034 3.1), a plan that carries an `instantiation` record, or a profile or variant argument refuses it with a typed refusal naming the member or argument and `parameterized`; it never ignores the member or argument. Definitions and plans without them, including version 2 definitions and version 3 plans of specs 032 and 033, are unchanged."
    anchor: "3-behavior"
  - id: "R-3"
    kind: requirement
    text: "With the feature, every surface's behavior and bytes are unchanged; the WebAssembly module stays within spec 046's budgets, and every conformance case other than spec 034's passes on every surface, WebAssembly included."
    anchor: "3-behavior"
---

# 050: The WebAssembly module without parameterized definitions (amends 027)

Approved: ratified by the owner on 2026-10-24 (decision A-52). Before
this spec merged, the owner approved making the feature opt-in, because
spec 031's release policy requires an empty `default` on every release
crate (recorded in A-52). The approved text of spec 027 is not edited; this spec records what
changes in 027 3.1 and 3.10.

## 1. Purpose

Spec 046 raised spec 027's uncompressed WebAssembly budget and kept the
gzip budget of 786,432 bytes on purpose, "so the next #76 specs that add
document types will meet it first, and that is the gate meant to make
their cost visible". Spec 034 met it:

| Change | gzip bytes of the optimized module |
|---|---:|
| main before spec 034 | 772,595 |
| spec 034 increment 1 (wire types), after removing a duplicated deserializer | 780,456 |
| spec 034 increment 3 (instantiation, rustev.plan/3 emission and loading) | 860,362 |

Increments 4 to 6 of spec 034 and spec 040's explanation add more. Spec
034 2 says the Node and WebAssembly bindings "expose the library
functions of 3.15; exposing them is those specs' work", and lists
exposing them through WebAssembly as out of scope. A browser host has no
use for them yet, so this spec keeps them out of the WebAssembly module
rather than raising the download size every browser host pays.

## 2. Territory

The manifests of `rustev-core`, `rustev-eval`, `rustev-surface` and
`rustev-wasm`, the root `Cargo.toml` workspace dependency declarations,
the manifest of every other workspace crate that must enable the feature
to keep its behavior, the facade `rustev` included (the feature and its
use), and spec 031's release set (`member_features`); the
`#[cfg(feature = "parameterized")]` gates in `rustev-core`, `rustev-eval`
and `rustev-surface`, the refusal in those crates' feature-off paths, and the
WebAssembly conformance selection of spec 027 3.12. The new `rustev-core` source file joins
`tools/rustev-release/archive-policy.json` (spec 031). No change to
`rustev-contract`: the document types stay, so every surface parses and
names a version 2 definition or version 3 plan before refusing it.

## 3. Behavior

1. **The feature.** `rustev-core`, `rustev-eval` and `rustev-surface`
   declare `parameterized`, with an empty `default` as spec 031 requires
   of every release crate, and compile spec 034's template
   validation, instantiation analysis, profile resolution, plan diff,
   profile check and `rustev.plan/3` emission and record validation only
   with it. Each forwards it to the crates below it (`rustev-surface` to
   `rustev-eval` and `rustev-core`, `rustev-eval` to `rustev-core`), so
   enabling it above enables it below. Every workspace crate that
   depends on any of the three enables `parameterized` on it, so every
   surface but WebAssembly builds with spec 034 as today; the facade
   `rustev` enables it on its `rustev-core` and optional `rustev-eval`
   dependencies (not a facade feature: its `default` stays empty, 031
   3.2.3), so facade users keep spec 034. `rustev-wasm` enables it on
   none of them. A direct dependent of `rustev-core`, `rustev-eval` or
   `rustev-surface` outside the workspace opts in with the feature.
2. **The refusal.** Without the feature, a command given a definition
   that uses any of spec 034's members (`parameters`, `variants`,
   `only_in`, `enabled` or a parameter reference; spec 034 3.1 item 1), a
   plan that carries an `instantiation` record (spec 034 3.10), or a
   profile or variant argument ends with the parse refusal (category 1)
   that 027 3.4 maps to `ParseError`, whose detail names the member or
   argument and says that this build has no `parameterized` feature.
   `rustev.definition/2` and `rustev.plan/3` documents of specs 032 and
   033 without those members keep their behavior, refusals and bytes.
3. **Conformance.** Spec 027 3.12's WebAssembly runner skips the cases of
   spec 034 and runs instead one refusal case per refused input above
   that the binding can receive; every other case, those of specs 032 and
   033 with version 2 definitions and version 3 plans included, runs
   unchanged. The native, Node and CLI runners run every case.
4. **Budgets.** Spec 046's budgets are unchanged and gated.
5. **Release set.** Spec 031's `member_features` records `parameterized`
   for `rustev-core`, `rustev-eval` and `rustev-surface` with this spec as
   the one that adds it (031 3.2.3: a member declares no feature "unless
   its own spec adds one"); the release policy's checks then hold
   unchanged.

## 4. Out of scope

Exposing spec 034's functions through WebAssembly (a later amendment of
027, with its own size measurement); any change to the Node binding, the
CLI, the service or `rustev-contract`; any budget change.

## 5. Observable negative cases

| Input to the WebAssembly binding | Result |
|---|---|
| `compile` or `check` of a definition declaring `parameters` or `variants` | Parse refusal naming the member and `parameterized` |
| `show` of a plan with an `instantiation` record | Parse refusal naming `instantiation` and `parameterized` |
| A version 2 definition or version 3 plan of specs 032 or 033 | Unchanged behavior and bytes |
| A profile or variant argument | Parse refusal naming the argument and `parameterized` |
| The build of any other workspace surface | Unchanged behavior and bytes |

## Acceptance

- `make bindings` passes with the WebAssembly module within 786,432 bytes
  gzip and 2,621,440 bytes uncompressed at the head that adds spec 034
  increment 3.
- The WebAssembly refusal cases of 3.3 pass; every other conformance case
  passes on every surface.
- `cargo test --workspace` passes, `cargo check -p rustev-core -p
  rustev-eval -p rustev-surface` (feature off) passes, and the
  `rustev-release` manifest checks pass with the allowance of 3.5.

## Verification

```sh
make bindings
cargo test --workspace --locked
cargo check -p rustev-core -p rustev-eval -p rustev-surface --locked
cargo test -p rustev-release --locked
```

## Implementation record

None yet.

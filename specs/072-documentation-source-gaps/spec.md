---
id: "072-documentation-source-gaps"
title: "Source gaps found by the selection gate feature page (amends 006, 007, 027 and 029)"
status: approved
implementation: in-progress
created: "2026-11-10"
summary: >
  The selection gate feature page and its runnable example (docs/features/)
  were written against rustev 0.7.0 and found four gaps in the sources a
  reader depends on. The support-routing package's committed definition
  cannot be compiled from the CLI, because its topic calibration is bound
  to a placeholder backend; an `--out` directory that does not exist is
  reported as a missing file inside it; `rustev.selection-gate/1`,
  `rustev.selection/1`, `rustev.baseline/1` and `rustev.repeat-config/1`
  have no published JSON Schema although hosts write and read them through
  the CLI and the Node binding; and nothing runs the page's example. This
  spec adds a CLI-ready synthetic definition to the package, makes the
  missing-directory refusal name the directory, emits schemas for those
  four documents from rustev-eval, and commits the page with its example
  under acceptance. No existing document, plan, golden or exit code
  changes meaning. Ratified by the owner on 2026-11-10 (A-92).
amends:
  - "006-cli-surface"
  - "007-support-routing-package"
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
establishes:
  # Written by increment 4 (3.4).
  - { kind: directory, path: "docs/features/", planned: true }
extends:
  # Increment 1: synthetic-example.definition.json and its test.
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: additive }
  # Increment 1: the new data file joins the package's reviewed archive list.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Increment 2: the output-directory precheck.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Increment 3: schema emission for spec 029's host documents.
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  # Increment 4: 072 joins VERIFIED_SPECS and an acceptance group.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
depends_on:
  - "006-cli-surface"
  - "007-support-routing-package"
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "`packages/rustev-pkg-support-routing/data/synthetic-example.definition.json` is the package's definition for its reference thresholds with `topic` bound to the committed synthetic example calibration; `rustev plan compile` accepts it with the committed example rules and calibration and no other input, and a package test fails when the committed bytes differ from the builder's output."
    anchor: "3-1-a-cli-ready-synthetic-definition"
  - id: "R-1"
    kind: requirement
    text: "A command given an output directory, or an output file whose parent directory, does not exist is refused with exit 3 `io_error` before reading any input, naming that directory and stating that it does not exist; existing directories and every other refusal keep their bytes."
    anchor: "3-2-a-missing-output-directory-is-named"
  - id: "I-2"
    kind: invariant
    text: "`rustev-eval` emits a JSON Schema (draft 2020-12) for `rustev.selection-gate/1`, `rustev.selection/1`, `rustev.baseline/1` and `rustev.repeat-config/1` from the types its parsers use, committed under `crates/rustev-eval/schema/` and checked byte for byte by a test; the schemas are descriptive and the bounded parsers remain the only authority on validity."
    anchor: "3-3-schemas-for-the-selection-documents"
  - id: "R-2"
    kind: requirement
    text: "This spec's acceptance builds the CLI and runs `docs/features/selection-gate/example/run.sh`, which exits 0 only when both gates' verdicts and rule results equal the committed expected files."
    anchor: "3-4-the-feature-page-and-its-example"
---

# 072: Source gaps found by the selection gate feature page (amends 006, 007, 027 and 029)

Approved: ratified by the owner on 2026-11-10 (A-92). Implementation in
progress.

## 1. Purpose

A feature page for the selection gate was written as a documentation
prototype, with an example that runs the whole workflow from committed
data and checks the result. Writing it exposed gaps in the sources, which
are fixed here rather than worked around in the page:

1. The support-routing package's definition document is bound to its
   reference topic calibration, which is bound to a placeholder backend
   artifact (`sha256:aaaa…`). No backend a CLI user can supply matches it,
   so `rustev plan check` refuses with `calibration_binding`. The example
   had to rewrite a calibration digest in the definition with `sed`,
   computed by hand.
2. `rustev eval --out base` with no `base/` directory fails with
   `io_error` naming `base/dataset.json` and "No such file or directory".
   Spec 006 3.3.3 requires output directories to exist; the refusal does
   not say so.
3. The four documents a host authors or parses to use selection
   (`rustev.repeat-config/1`, `rustev.baseline/1`, `rustev.selection-gate/1`,
   `rustev.selection/1`) have no JSON Schema, although spec 027 3.2.2 emits
   one for every public document type of the contract. They are defined in
   `rustev-eval`, which the contract cannot depend on. The page documents
   their fields by hand, including `verdict`'s two JSON shapes.
4. Nothing runs the page's example, so it can go stale.

## 2. Territory

- Establishes `docs/features/`: feature pages and their runnable examples.
- Extends spec 007's package (a data file, a constant and a test), spec
  006's CLI (the output precheck), spec 004's `rustev-eval` (schema
  emission and its committed directory), and spec 001's `Makefile` and
  `scripts/acceptance/`.
- Amends spec 027 3.2.2, which emits JSON Schemas from `rustev-contract`
  only: document types a host authors or parses that are defined outside
  the contract (here spec 029's, in `rustev-eval`) are emitted by their own
  crate under the same rules (3.3). It claims no unit of 027; generating
  TypeScript declarations from them stays 027's later work (3.3.4).
- Adding `schemars` to `rustev-eval`'s dependencies is a dependency-only
  change in its own pull request, as the policy-isolation guard requires.

## 3. Behavior

### 3.1 A CLI-ready synthetic definition

1. The package commits `data/synthetic-example.definition.json`: the
   definition the builder produces for `Params::with_topic` with
   `TopicCalibration::Calibrated` set to the identity of
   `data/synthetic-example.calibration.json`, and the reference thresholds.
   It is exposed as `EXAMPLE_DEFINITION`, beside `EXAMPLE_RULES`,
   `EXAMPLE_CALIBRATION` and `EXAMPLE_SNAPSHOT`.
2. A package test builds it and compares the committed bytes; the existing
   bless convention (`RUSTEV_BLESS=1`) rewrites it. It is SYNTHETIC like the
   other example files (spec 007 3.2.3) and supports no quality claim.
3. `support-routing.definition.json` and spec 002's golden are unchanged.

### 3.2 A missing output directory is named

1. Before reading any input, a command checks that each output directory
   it was given (`--out <dir>`, `--bundles` written by capture, `--store`
   when it is written) and the parent directory of each output file exists
   and is a directory.
2. If not, it stops with exit 3 `io_error`, `path` the directory as given,
   and `detail` `output directory does not exist` (or `is not a
   directory`). Nothing is read or written.
3. Spec 006 3.3.3's rule that output directories must already exist is
   unchanged; only the refusal's timing and wording change. Every other
   `io_error`, and every existing golden that does not involve a missing
   output directory, keeps its bytes.

### 3.3 Schemas for the selection documents

1. `rustev-eval` gains a schema emitter for `rustev.repeat-config/1`,
   `rustev.baseline/1`, `rustev.selection-gate/1` and `rustev.selection/1`,
   following spec 027 3.2.2: draft 2020-12, derived from the types the
   parsers use, closed objects where the parser denies unknown fields,
   canonical and deterministic.
2. The schemas are committed under `crates/rustev-eval/schema/`, one file
   per document version, and a crate test fails when emitted bytes differ;
   `RUSTEV_BLESS=1` rewrites them.
3. `verdict` is described as its three shapes (the string `qualifies`, and
   the objects `rejected` and `unknown` naming a clause), as the
   serializer writes them today. No document's bytes change.
4. Generating TypeScript declarations from these schemas for the Node and
   WebAssembly packages is a later increment of spec 027's generator, not
   part of this spec.

### 3.4 The feature page and its example

1. `docs/features/selection-gate.md` and `docs/features/selection-gate/`
   are committed: the page, `example/run.sh`, both gates and the two
   expected verdict files.
2. Once 3.1 lands, the example compiles `EXAMPLE_DEFINITION` directly; it no
   longer rewrites a digest.
3. The script compares verdicts and rule results, not bytes, because run
   records carry measured latency and digests differ between runs.
4. This spec's Verification builds the CLI and runs the script with
   `RUSTEV=target/debug/rustev`, so the merge queue fails when the page's
   example stops matching the product.

## 4. Out of scope

- A general feature-page format, generator or spec-spine feature layer;
  this page is the reference for designing one.
- A command that prints a document's identity.
- Moving spec 029's documents into `rustev-contract`.

## 5. Observable negative cases

- The committed synthetic definition differs from the builder's output:
  the package test fails.
- `rustev eval ... --out missing/`: exit 3, `path` `missing/`, detail
  `output directory does not exist`, and nothing read.
- A schema file differs from emission: the crate test fails.
- A change alters a verdict or rule result of the example: acceptance fails
  with a diff of the expected file.

## Acceptance

1. `rustev plan compile` accepts `synthetic-example.definition.json` with
   the committed example rules and calibration.
2. The missing-directory refusal of 3.2 for `eval`, `run --bundle-out` and
   `plan compile --out`.
3. The four schema files equal their emission.
4. The example script exits 0.

## Verification

```verify:cli
cargo test -p rustev-pkg-support-routing --locked --test example
cargo test -p rustev-cli --locked --test io
cargo test -p rustev-eval --locked --test schemas
cargo build -p rustev-cli --locked
env RUSTEV=target/debug/rustev sh docs/features/selection-gate/example/run.sh
```

## Implementation record

Not started. Planned increments:

1. The synthetic definition (3.1).
2. The output-directory precheck (3.2).
3. `schemars` for `rustev-eval` (dependency-only pull request), then the
   schemas (3.3).
4. The page, its example and acceptance (3.4).

**Increment 1 (2026-11-10).** `data/synthetic-example.definition.json` is
committed and exposed as `EXAMPLE_DEFINITION`. `tests/example.rs` builds it
from `Params::with_topic` with the example calibration's identity and
compares the committed bytes (`RUSTEV_BLESS=1` rewrites it with the other
example files). It equals `support-routing.definition.json` with only the
topic calibration reference changed, and `rustev plan check` accepts it
with `synthetic-example.rules.json` and `synthetic-example.calibration.json`
alone (plan `sha256:7e0b9ae5…`). The file joins the package's reviewed
archive list (`tools/rustev-release/archive-policy.json`).

**Increment 3, step 1 (2026-11-10, test only).** `rustev-eval`'s
`tests/dependencies.rs` pins its normal dependencies exactly. So that the
dependency-only change adding `schemars` passes on its own, the test accepts
either exact list, with or without `schemars`, until the schema change
tightens it to the list with `schemars`. No claim of spec 004 changes.
**Increment 3 (2026-11-10).** `rustev-eval` takes the workspace's
`schemars` as a normal dependency (its own pull request; spec 004's
dependency test `crates/rustev-eval/tests/dependencies.rs` now lists it)
and `jsonschema` as a dev-dependency. `rustev_eval::doc::document_schemas`
(`crates/rustev-eval/src/doc.rs`, now a public module, so the archive file
list is unchanged) emits the four schemas as spec 027 3.2.2 does: draft
2020-12, `schema` fixed to the version, `x-rustev-parse-limits`, canonical
bytes. `JsonSchema` is derived on `RepeatConfig` and its members
(`repeat_config.rs`), `Baseline`, `BaselineCohort` and `BaselineMetric`
(`baseline.rs`), the repeat report members a baseline embeds (`repeat.rs`),
`HeldOutRecord` (`holdout.rs`), `AdapterRules` and `Direction`
(`config.rs`), `Integrity` (`integrity.rs`), the gate, clause, selection
and verdict types (`selection.rs`) and the per-case results
(`selection/case_regressions.rs`, the generic list named `CaseListOf{T}`);
contract types (`Decimal`, the digests and identities, `Handle`, `Split`,
`DatasetProvenance`) reuse their own schemas. No manual `JsonSchema` impl
was needed: the three fields read by a custom deserializer carry a
`schemars(with)` override instead (`SelectionGate.integrity` and
`Integrity`'s two bounds are absent or a value, never `null`;
`ClauseResult.cases` is absent, `null` or the results). The committed
files are `crates/rustev-eval/schema/rustev.{baseline,repeat-config,selection,selection-gate}-1.schema.json`;
`crates/rustev-eval/tests/schemas.rs` checks them byte for byte
(`RUSTEV_BLESS=1` rewrites them), that every object is closed, the gate's
members, the verdict's three shapes, and that every committed document of
the four versions validates. No serialized byte changes.

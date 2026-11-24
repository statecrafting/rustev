---
id: "073-compile-extraction-documents-from-hosts"
title: "Extraction programs and prompt templates on the CLI's and the Node binding's compile (amends 006, 027 and 063)"
status: approved
implementation: complete
created: "2026-11-24"
summary: >
  An amendment of approved specs 006 (3.4), 027 (3.3, 3.12) and 063
  (3.2), for #598, ratified by the owner on 2026-11-24 (A-94) for release
  0.8.0. `rustev-core` compiles a
  definition with supplied `rustev.extract-rules/1` and `/2` programs and
  `rustev.prompt-template/1` documents through
  `compile_with_extraction_documents`, but no host surface reaches it: the
  Node binding's `compile` takes descriptors, calibrations, sources and an
  execution policy only, and the CLI's `--rules` takes a rules backend
  program (spec 005), not an extraction program. A host that keeps its
  definitions and extraction programs in its own repository can therefore
  run a `fill: rules_first` plan from Node but cannot build one. This spec
  adds two optional, repeatable inputs, extraction programs and prompt
  templates, to `rustev plan check` and `rustev plan compile` (the flags
  `--extract-rules` and `--template`), to the Node binding's
  `compile`/`compileSync` option bag (`extractRules`, `templates`) and to
  its `command('check' | 'compile')`, each routed through
  `compile_with_extraction_documents`. Compile refusals keep the core's
  category codes on every surface, and a malformed document is refused as
  each surface already refuses a malformed descriptor. A call that supplies
  neither input calls what it calls today, so every existing plan,
  `PlanId`, golden and output keeps its bytes. No contract type, JSON Schema
  or generated binding type changes.
amends:
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "063-node-binding-parity"
extends:
  # The dependency reader and the check/compile path the CLI and the Node
  # binding's `command` share (3.2, 3.3).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: amending }
  # `--extract-rules` and `--template` on `plan check` and `plan compile`.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The option-bag fields, the `command` input names and `index.d.ts`.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  # Tests only: the shared rules-first golden and its test (3.7); no change under src/.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: additive }
  # Conformance cases (3.7).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # Increment 1: the other `DepArgs` callers pass no documents; their
  # surfaces are unchanged (3.5).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  # Adds 073 to `make verify` and CI's `cli-backends` group at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "005-reference-backends"
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "037-generation-steps"
  - "038-structured-extraction"
  - "063-node-binding-parity"
  - "070-extract-rules-2"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "For the same definition, descriptors, calibrations, sources, execution policy, extraction programs and prompt templates, `rustev plan compile`, the Node binding's `compile` and `command('compile')`, and `rustev_core::compile_with_extraction_documents` produce byte-identical plans with the same `PlanId`."
    anchor: "3-3-mapping-to-the-core"
  - id: "I-2"
    kind: invariant
    text: "A call that supplies no extraction program and no prompt template calls the same core function as before this spec, so every existing plan, `PlanId`, CLI golden, conformance case and binding result keeps its bytes."
    anchor: "3-6-compatibility"
  - id: "I-3"
    kind: invariant
    text: "A compile refusal for a definition compiled with extraction programs or prompt templates carries the core's category and category name on every surface; no surface adds, renames, reorders or swallows a core refusal."
    anchor: "3-4-refusals"
  - id: "R-1"
    kind: requirement
    text: "No contract type, JSON Schema or generated binding type changes; `rustev-core` and `rustev-contract` take no new dependency; the WebAssembly package's surface is unchanged."
    anchor: "3-6-compatibility"
---

# 073: Extraction programs and prompt templates on the CLI's and the Node binding's compile (amends 006, 027 and 063)

Approved: ratified by the owner on 2026-11-24 (A-94) for release 0.8.0,
accepting every recommendation the draft wrote; implemented in two
increments (see the implementation record). The approved texts of 006,
027 and 063 are not edited; this spec records what it adds to them. Section numbers such as "063 3.2.1" refer to those specs.

## 1. Purpose

Spec 038 lets an extract step fall back on a `rustev.extract-rules/1`
program, spec 070 adds `rustev.extract-rules/2` and `fill: rules_first`,
and spec 037 binds generation steps to `rustev.prompt-template/1`
documents. The core compiles all of them through
`compile_with_extraction_documents(def, descriptors, calibrations, sources,
execution, templates, programs)`, which embeds each program and template in
the plan 3 step detail, so a compiled plan loads and runs without them
(`load_checked` recompiles from the embedded copies).

No host surface reaches that function. The Node binding's `compile`
(063 3.2.1) calls `compile_with_sources` or `compile_profiled_with`; the
CLI's `plan check` and `plan compile` (006 3.4) call `compile` or
`compile_with`. The CLI's `--rules` is unrelated: it takes a spec 005
rules backend program and derives a backend descriptor from it. So the only
way to build an extraction or generation plan is Rust code linked against
`rustev-core`.

A host that keeps its definitions and extraction programs in its own
repository wants to compile them in-process, as it compiles its other
plans, and assert the `PlanId` in its own tests. A plan built by a separate
Rust tool and committed as bytes does not rebuild from the host's sources,
so drift between the program a host edits and the plan it runs goes
unnoticed. This spec closes that gap on the CLI and the Node binding.

## 2. Territory

The `extends` edges, added at ratification:

- `crates/rustev-surface/` (027, amending): the dependency reader and the
  `check`/`compile` path the CLI and the Node binding's `command` share
  read the two new inputs and call `compile_with_extraction_documents`.
- `crates/rustev-cli/` (006, amending): the flags `--extract-rules` and
  `--template` on `plan check` and `plan compile`, their usage text, and
  goldens.
- `integrations/rustev-node/` (027, amending): the option-bag fields on
  `compile`/`compileSync`, the input names on `command('check' |
  'compile')`, the hand-written declarations in `index.d.ts`, and tests.
- `crates/rustev-core/` (002, additive): tests only, a committed
  `fill: rules_first` golden that every surface's test compiles and the
  test that pins it (3.7); no change to the crate's `src/`.
- `tools/rustev-conformance/` (027, additive): cases for 3.7.
- `Makefile` and `scripts/acceptance/` (001, additive): at completion, 073
  joins `make verify` and CI's `cli-backends` acceptance group beside 006.

This spec's compiled shards under `.statecraft/derived/` are committed with
it, as AGENTS.md requires; the AI review leaves that directory out
(`review.exclude`), so they do not appear in its diff.

Unchanged: `rustev-contract` and every document, schema and generated
binding type; `rustev-core`'s source and public API; the WebAssembly
package (3.5); `plan show`, `run`, `replay` and every other command;
`--rules` and spec 005's rules backend; `Engine.load`.

### 2.1 Binding principles

- **Proposals, never grants.** Compiling is pure and produces a plan; no
  new input authorizes anything.
- **Derivation and lineage.** Unchanged: a program's or template's identity
  enters the plan and its `PlanId` exactly as it does from Rust (038
  3.14.4, 070 3.10.2).
- **Unresolved, never a default.** A document a definition names and the
  caller did not supply is the core's refusal, never a default program or
  template; a surface never supplies one on the caller's behalf.
- **No silent conversion.** Documents are passed to the core as parsed,
  in the order supplied; a surface never rewrites, merges or deduplicates
  them.
- **Pure core.** No change to `rustev-core` or `rustev-contract` source,
  and no new dependency in either.

## 3. Behavior

### 3.1 The inputs

Two optional, repeatable inputs, each a list of documents:

| Input | Document | Parsed as | Byte bound |
|---|---|---|---|
| extraction programs | `rustev.extract-rules/1` or `/2` | `rustev_contract::extraction_documents::ExtractRules` | `DESCRIPTOR_V1` |
| prompt templates | `rustev.prompt-template/1` | `rustev_contract::generation::PromptTemplate` | `DEFINITION_V1` |

Each byte bound is the one the document type itself declares in
`rustev-contract`, not a bound this spec chooses: `ExtractRules` is
declared with `crate::limits::DESCRIPTOR_V1` and `PromptTemplate` with
`crate::limits::DEFINITION_V1` (their `versioned_document!` and
`document!` declarations), so a surface reads at most what the type's own
parser accepts and refuses nothing the parser would accept.

A list may hold documents the definition does not name; the core ignores
them, as it does from Rust. Two programs or templates that the core finds
in conflict are its refusal (3.4), never resolved by the surface.

### 3.2 The surfaces

1. **CLI (amends 006 3.4.1).** `plan check` and `plan compile` gain the
   repeatable flags `--extract-rules F` and `--template F`. The usage text
   lists them beside `--rules` with one line saying that `--rules` takes a
   rules backend program and `--extract-rules` an extraction program.
   **File access (amends 006 3.3.1 and 3.3.2).** Extraction programs are
   read under `DESCRIPTOR_V1` and templates under `DEFINITION_V1` (3.1),
   with 006 3.3.1's bounded reads. One command reads at most 4 096
   `--extract-rules` and 4 096 `--template` files, as for `--descriptor`;
   beyond that it stops with `io_error` before reading any file. The files
   it does read count toward 006 3.3.2's 256 MiB.
2. **Node, option bag (amends 027 3.3 and 063 3.2).** `compile` and
   `compileSync` gain two optional fields (027 3.3: "option bags may gain
   optional fields by later specs"):

   ```ts
   compile(args: {
     // ... every field of 063 3.2, unchanged ...
     extractRules?: DocumentInput[]; // rustev.extract-rules/1 or /2
     templates?: DocumentInput[];    // rustev.prompt-template/1
   }): Promise<PlanHandle>;
   ```

   They are refused with `plan` (a compiled plan embeds its programs and
   templates): `InvalidDocument` naming the field, before any parse, as 063
   3.2.2 refuses `plan` with an instantiation field.
3. **Node, pure commands (amends 063 3.2.4).** `command('check' |
   'compile')` and `commandSync` accept `extractRules` and `templates` as
   lists of input names, as they accept `descriptors`.
4. **Why fields, not a sibling method.** `compile` already chooses the core
   function from the fields it is given (063 3.2.1) and returns a prepared
   `PlanHandle`; a sibling method would duplicate `compile`, `compileSync`,
   their shutdown counting and `prepare` for two list arguments, and a host
   would have to switch methods to add one program. Optional fields keep
   every existing call valid and its result unchanged (I-2), widen no
   output and narrow no input (031 3.6.5).

### 3.3 Mapping to the core

1. When neither input is supplied, or both are supplied empty, each surface
   calls exactly the function it calls today (I-2).
2. Otherwise, for a concrete definition (one the test of
   `rustev_surface::profile::plan_command` does not route to instantiation),
   each surface calls `compile_with_extraction_documents(def, descriptors,
   calibrations, sources, execution, templates, programs)` with the
   descriptors (including those derived from `--rules`), calibrations,
   sources and execution policy it reads today, and the two lists in the
   order supplied. The CLI passes no sources, as today.
3. The result is the core's (I-1): the same plan bytes and `PlanId` as the
   Rust call with the same inputs. Supplying templates alone also compiles
   a spec 037 generation definition, because the same function binds
   generation steps; this spec claims nothing about generation beyond that
   mapping.
4. **Instantiation is out of reach in this version.** A call that also
   routes to instantiation and supplies either input is refused, naming
   the flag or field. When the instantiation is named by the arguments (a
   `profile`, `profileBases`, `variant`, `definitionVersion` or several
   definitions), it is a usage error, refused before any file is read: on
   the CLI with exit 2; on Node as `InvalidDocument` from `compile`,
   `compileSync` and `command('check' | 'compile')` alike, as every usage
   rule of 063 3.2.2 is on the command path (a usage error has no output
   bytes to return). When it is found in the definition (a parameterized
   `rustev.definition/2` document, one that declares `parameters` or
   `variants` under spec 034, the test of `plan_command`; not a
   `rustev.prompt-template/1`), it is refused after the definition is read
   and before any dependency is read: on the CLI as `invalid_input` (exit
   4, 006 3.2: a supplied input the plan does not use); on Node,
   `command('check' | 'compile')` returns those CLI bytes and exit code
   (3.4.2), and `compile` and `compileSync` throw `InvalidDocument`. The core's `compile_profiled_with` takes no programs
   or templates, and this spec does not add a core entry point (decision 2
   at approval).

### 3.4 Refusals

1. **A malformed document.** Each surface refuses a program or template
   that does not parse, or exceeds its bound, as it already refuses a
   malformed descriptor: the CLI with exit 5 `refused`, category 1
   (`parse`), subject `extract-rules:<path>` or `template:<path>` (006
   3.4.1's form); Node with `InvalidDocument` (`invalid_document`, 027
   3.4.1) naming the field and the index or input name. A `/1` document
   holding a `/2` construct is refused at parse (070 3.1.3) on every
   surface.
2. **A compile refusal.** Every refusal the core returns, including
   `invalid_extraction` (038, 070 3.5), `invalid_acceptance`,
   `uncorroborated_hint` (category 14, 070 3.8.3), the budget categories and
   a missing or conflicting program or template, reaches the host with the
   core's category: the CLI prints `refused` (exit 5) with `category`,
   `category_name`, `subject`, `detail` and `shortfalls` (006 3.4.2); Node
   throws `CompileRefusal` whose `code` is the category name (027 3.4.1)
   and `command('check' | 'compile')` returns the CLI's output bytes and
   exit code (063 3.2.4). No surface rewrites a refusal (I-3).
3. **Order of refusals.** Inputs are read and parsed in the order
   definition, descriptors, rules programs, calibrations, extraction
   programs, templates, execution policy; the first failure is reported.
   This is the order of error reporting only, and is independent of the
   core function's parameter order in 3.3.2. Compile refusals follow the
   core's phase order unchanged.

### 3.5 WebAssembly

The WebAssembly package gains nothing, as 063 3.12 holds for 063's
additions: its `compile` keeps its arguments. Conformance cases of this
spec declare the surfaces `cli` and `native` (027 3.12.2). Its module
size is therefore unchanged (027 3.10.4).

### 3.6 Compatibility

1. Every existing CLI invocation, Node call and conformance case keeps its
   bytes, `PlanId`, output and exit code (I-2). The plans this spec makes
   reachable are the ones the Rust API already produces.
2. No contract type changes, so no JSON Schema and no generated binding
   type (`integrations/*/types/`) changes; `gen-types --check` and
   `crates/rustev-contract/tests/schemas.rs` pass unchanged. The
   hand-written `integrations/rustev-node/index.d.ts` gains the two
   optional fields, which neither removes nor narrows an export (031
   3.6.5).
3. An earlier release refuses the new CLI flags as unknown (usage, exit
   2). An earlier Node binding's `compile` does not read the new fields;
   a definition that names a program or template is then refused by the
   core as not supplied, so no earlier release compiles a plan without a
   document its definition names.
4. **Release.** New CLI flags and new Node option fields are a public API
   change, which spec 021 3.2 (the rule 031 3.6.4 follows) assigns to the
   minor version before 1.0: a patch is reserved for a compatible
   correction. This spec therefore targets 0.8.0, not 0.7.1.

### 3.7 A shared rules-first golden

A committed synthetic fixture set under
`crates/rustev-core/tests/golden/` holds a `rustev.definition/2` with one
extract step declaring `fallback: { rules_program, fill: "rules_first",
required }`, a `rustev.extract-rules/2` program, the step's
`rustev.prompt-template/1`, a backend descriptor, the compiled
`rustev.plan/3` and its `PlanId`. The example is a fictional airline
booking confirmation: a confirmation code by `token_after`, a flight number
by `token_matching` and a departure date by `date{format: dmy_name}` with a
declared month table. A core test compiles it through
`compile_with_extraction_documents` and compares bytes; the CLI test, the
Node test and the conformance cases compile the same files and assert the
same bytes and `PlanId`.

## 4. Out of scope

Extraction programs and templates on instantiated or profiled compiles
(3.3.4); `plan diff` with these inputs; the WebAssembly package (3.5);
`--source` on the CLI; any change to how a plan is loaded or run; spec
005's rules backend and `--rules`; a quality claim for any program (070
3.12).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `plan compile` with neither new flag, on today's inputs | Byte-identical output, plan and exit code. |
| Node `compile` with neither new field | Same `PlanId` and `raw` as today. |
| `rules_first` definition compiled without `--extract-rules` | Exit 5, `invalid_extraction` (program not supplied), as the Rust path. |
| The same through Node `compile` | `CompileRefusal`, `code: "invalid_extraction"`. |
| An extract step compiled without its template | The core's refusal and category on both surfaces. |
| `--extract-rules` naming a file that is not JSON | Exit 5, category 1, subject `extract-rules:<path>`. |
| `extractRules: [<not JSON>]` on Node | `InvalidDocument`. |
| A `/1` document with a `token_after` value | Refused at parse on both surfaces (070 3.1.3). |
| Two programs with the same name and version and different bytes | The core's conflict refusal on both surfaces. |
| A program the definition does not name | Ignored; same `PlanId` as without it. |
| Programs supplied in a different order | Same plan bytes and `PlanId`. |
| `--extract-rules` with `--profile`, `--variant` or two `--definition` | Usage error, exit 2, before any parse. |
| `--template` with a version 2 definition declaring `variants` | `invalid_input`, exit 4, naming `--template`, before any dependency is read. |
| `compile({ plan, extractRules })` on Node | `InvalidDocument` naming `extractRules`, before any parse. |
| A `vocabulary_after` field read by a hard gate | Category 14 `uncorroborated_hint` on both surfaces. |
| The shared rules-first golden through CLI, Node and Rust | Byte-identical plan and equal `PlanId`. |
| `--rules` given an extraction program | Unchanged: `invalid_input` (exit 4), the rules backend refuses it. |
| 4 097 `--extract-rules` or `--template` files | `io_error` (exit 3) naming the flag, before any file is read. |

## Increments

Each a separately reviewed pull request of about 1,000 changed lines or
fewer (generated files excluded), in this order:

1. **Surface and CLI.** The shared golden of 3.7 with its core test; the
   dependency reader and the `check`/`compile` path in
   `crates/rustev-surface/`; the CLI flags and usage text; CLI goldens for
   the golden's compile, each refusal row of section 5 that the CLI
   reaches, and unchanged existing goldens; CLI conformance cases with
   surfaces `cli` only.
2. **Node.** The option-bag fields, the `command` input names and
   `index.d.ts`; Node tests that compile the shared golden through
   `compile`, `compileSync` and `command('compile')` and assert its
   `PlanId`, and section 5's Node rows; the increment 1 conformance cases
   gain `native`; 073 joins `make verify` and the `cli-backends` group;
   closure.

## Acceptance

- The shared rules-first golden compiles to the same bytes and `PlanId`
  through `compile_with_extraction_documents`, `rustev plan compile`, Node
  `compile`, `compileSync` and `command('compile')` (I-1).
- Every row of section 5 has a test on each surface it names.
- Conformance: a case for the golden's compile and one for each refusal
  kind (program not supplied, malformed document, conflict), surfaces
  `cli` and `native`, expected bytes generated by the CLI once.
- Compatibility: every existing CLI golden, conformance case, plan golden
  and Node test unchanged (I-2); `gen-types --check` and the schema test
  pass with no regenerated file (R-1); `make boundaries` passes.
- `make code`, `make bindings`, declared verification and `make gate`
  pass.

## Verification

Declared for `spec-spine verify 073-compile-extraction-documents-from-hosts`;
073 is in `make verify` and in CI's `cli-backends` acceptance group beside
006. The Node tests (`integrations/rustev-node/test/extraction-documents.test.mjs`)
and the native conformance runner run in `make bindings` (CI's `bindings`
job).

```verify:cli
# 3.7: the shared rules-first golden through the core; existing extraction
# goldens unchanged.
cargo test -p rustev-core --locked --test rules_first_golden --test extraction_compile
# 3.2 to 3.4 and section 5 through the CLI; every existing plan golden unchanged.
cargo test -p rustev-cli --locked --test extraction_documents --test plan
cargo test -p rustev-surface --locked
# The conformance cases on the Rust runner; every other case unchanged.
cargo test -p rustev-conformance --locked --test cli --test manifest
```

## Implementation record

- **Increment 1 (surface and CLI).** `rustev-surface`: `DepArgs` gains
  `extract_rules` and `templates`, and `Deps` the parsed `programs` and
  `templates`, read after calibrations in the order supplied, each under
  its type's bound (`DESCRIPTOR_V1`, `DEFINITION_V1`), a malformed one a
  category 1 refusal naming `extract-rules:<label>` or `template:<label>`
  (3.1, 3.4.1, 3.4.3). `plan::compile_definition_bytes` calls
  `compile_with_extraction_documents` when either list is non-empty and
  exactly today's `compile` or `compile_with` otherwise (3.3.1, 3.3.2).
  `profile::plan_command` refuses either input on the instantiation path
  with `invalid_input`, after the definition is read and before any
  dependency (3.3.4). `host::MAX_EXTRACT_RULES_FILES` and
  `MAX_TEMPLATE_FILES` are 4 096. Every other caller of `DepArgs` (the
  Node binding's commands, the WebAssembly package, the service's offline
  path, the repeat commands) passes no documents and is unchanged.
  `rustev-cli`: `--extract-rules` and `--template` on `plan check` and
  `plan compile`, their file-count caps, the usage refusal with
  `--profile`, `--profile-base`, `--variant`, `--definition-version` or a
  second `--definition` (3.3.4), and the help text (3.2.1).
  `rustev-core` tests: the shared golden under
  `tests/golden/rules-first/` (definition, descriptor, `/2` program,
  template, plan and `PlanId`) and `tests/rules_first_golden.rs`, which
  also checks that the inputs are canonical and that document order and an
  unnamed program change nothing (3.7). `rustev-cli` tests:
  `tests/extraction_documents.rs`, covering each section 5 row the CLI
  reaches, with goldens `extraction-documents.*.json`; the compiled plan is
  the core golden byte for byte. Five conformance cases
  (`extraction-documents.*`, surfaces `cli`) are harvested from those
  goldens. `DiffArgs` keeps its `Copy` bundle under an allowed
  `large_enum_variant`. No contract type, schema, generated binding type,
  existing golden or existing conformance case changed.
- **Increment 2 (Node) and closure.** `integrations/rustev-node`:
  `compile` and `compileSync` take `extractRules` and `templates`
  (3.2.2), routed by `profile::compile`: a concrete definition with either
  calls `compile_with_extraction_documents`, without them exactly
  `compile_with_sources` as before (3.3.1); a malformed document is
  `InvalidDocument` naming `extractRules[i]` or `templates[i]` (3.4.1).
  `command('check' | 'compile')` passes them as input names to the shared
  `plan_command` (3.2.3). `plan` with either field, and either field with
  `profile`, `profileBases`, `variant`, `definitionVersion` or several
  definitions, is `InvalidDocument` before any parse, in `index.js` and in
  the native usage checks; a template found in the definition is
  `InvalidDocument` after it is read (3.3.4). `index.d.ts` gains the two
  optional fields. Tests: `test/extraction-documents.test.mjs` compiles the
  shared golden through `compile`, `compileSync` and `command('compile')`
  to the Rust `PlanId` and plan bytes, compares `command('check')` with the
  CLI golden, and covers section 5's Node rows. The five
  `extraction-documents.*` conformance cases gain `native`
  (`js/run.mjs` maps `--extract-rules` and `--template`); the WebAssembly
  package is unchanged. 073 joins `make verify` and the `cli-backends`
  group.

## Decisions at approval

Ratified by the owner on 2026-11-24 (A-94), with every question the draft
left open answered as recommended:

1. **Release.** 0.8.0, under spec 021 3.2: new CLI flags and Node option
   fields are a public API change, which takes the minor version before
   1.0 (3.6.4).
2. **Instantiated compiles.** Extraction programs and templates with a
   profile, a variant or a parameterized definition are refused in this version
   (3.3.4); no core entry point is added.
3. **Names.** `--extract-rules` / `extractRules` and `--template` /
   `templates` (3.2).
4. **WebAssembly.** Unchanged (3.5), as 063 3.12 held.
5. **One spec per issue.** This spec covers #598; #597 is spec 074.

Added before merge, under the owner's approval of 2026-11-24 (A-94): the
CLI file-count caps of 3.2.1, which 006 3.3.2 requires for every flag that
names files and which the draft omitted. They follow `--descriptor`'s cap.
Also added before merge, under the same approval: the refusal of 3.3.4 for
an instantiation found in the definition, which can only be detected after
the definition is read, is `invalid_input` (exit 4) on the CLI rather than
a usage error.

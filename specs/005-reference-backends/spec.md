---
id: "005-reference-backends"
title: "Deterministic rules backend"
status: approved
implementation: complete
created: "2026-09-23"
summary: >
  Increment 2: `rustev-backend-rules`, a deterministic, in-process backend
  that implements the backend seam of spec 003 by evaluating a bounded,
  versioned rules program (`rustev.rules/1`): typed field access into the
  canonical projection, ordered rules with explicit precedence and no-match
  behavior, constant, linear and lookup contributions to authored logits in
  exact fixed-point arithmetic, and a descriptor derived from the program so
  the program's digest is the artifact identity plans bind. Metered logical
  cost units, bounded work, local cancellation observed at declared points.
  Narrowed by owner decision R-13; the semantic backend is spec 011.
establishes:
  - { kind: directory, path: "backends/rustev-backend-rules/" }
extends:
  # Adds the backends/ member glob.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  # Records the backend crate in the lockfile.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds backend manifests to the code targets and 005 to `make verify`.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
---

# 005: Deterministic rules backend

Rationale: design section 8.2. Owner decisions R-12 (implemented before
spec 004, as a single exception to ordinal order), R-13 (narrowed to the
rules backend; the semantic backend of R-01 moves to spec 011), R-17 (no
semantic inference) and approval A-04, all in
`docs/decisions/00-founding-decisions.md`.

## 1. Purpose

Provide a production backend that answers semantic requests from an
authored, identified rules program, discloses exactly what it returns and
what it costs, and lets both reference plans run end to end through
`rustev-runtime` without a model. Its outputs are authored heuristics over
the projected inputs. They are not calibrated probabilities, not evidence of
semantic understanding, and not independent evidence about the claims the
inputs describe.

## 2. Territory

- `backends/rustev-backend-rules/`: the program document, its validation,
  the evaluator, the `DecisionBackend` implementation, the plan check of
  3.11, its tests, synthetic reference programs, and its negative control.
- Additive edits to spec 001's `Cargo.toml` (member glob `backends/*`),
  `Cargo.lock` and `Makefile`.
- No change to any unit spec 002 or 003 owns. The seam of spec 003 can
  express the identity binding this backend needs (3.10), so no contract
  amendment is made.
- Spec 003's scripted backends stay as the runtime's test fixtures; this
  backend does not replace them and never manufactures their failures
  (overload, transient faults, hangs, panics).

## 3. Behavior

### 3.1 Scope

1. The backend answers `classify`, `proposition` and `rubric` requests with
   `logits`, and nothing else. It does not answer `rank`, and it never
   returns `label`, `distribution` or `scores`. Its descriptor advertises
   exactly the operations its program has tasks for, each with output
   `logits` (3.9).
2. It is in-process, deterministic and synchronous: one request is evaluated
   within one poll of the attempt future. It performs no I/O, reads no clock,
   environment or randomness, spawns nothing and holds no mutable state
   between calls.

### 3.2 The rules program, `rustev.rules/1`

A JSON document, parsed with the contract's bounded parser under
`DESCRIPTOR_V1` (spec 002, 3.2.5), with unknown fields and duplicate keys
refused and every field required. Decimals are strings in the spec 002
3.4.1 syntax. Enums are `snake_case`.

```text
program      = { schema: "rustev.rules/1", backend_id, limits, cost, tasks: [task] }
limits       = { max_projection_bytes }
cost         = { units_per_call }
task         = { operation: classify | proposition | rubric,
                 task, question, options: [string], interpretation,
                 fields: [field], base: {option: decimal},
                 rules: [rule], on_no_match: base | fail }
field        = { name, ref, type: text | bool | integer | decimal }
rule         = { name, when: condition, then: [contribution], stop: bool }
condition    = "always"
             | { contains_any: { field, terms: [string] } }
             | { equals: { field, value: string } }
             | { compare: { field, op: lt | le | ge | gt, value: decimal } }
             | { all: [condition] } | { any: [condition] } | { not: condition }
contribution = { add: {option: decimal} }
             | { linear: { field, coefficients: {option: decimal} } }
             | { lookup: { field, entries: [{ key, add: {option: decimal} }],
                           on_missing: skip | fail } }
```

### 3.3 Validation and limits

`RulesBackend::new` validates a parsed program and refuses it with a typed
error naming the task, rule or field when any of these fails. Every limit is
a constant of schema version 1.

| Item | Rule |
|---|---|
| `backend_id` | 1 to 64 bytes of `[a-z0-9._-]` |
| `max_projection_bytes` | 1 to 65 536 |
| tasks | 1 to 32; `task` names unique; `task` 1 to 256 bytes; `question` 1 to 4 096 bytes |
| `operation` | `classify`, `proposition` or `rubric`; `rank` is refused |
| options | 2 to 64, unique, each 1 to 256 bytes; a proposition's are exactly `["false", "true"]` |
| `interpretation` | 1 to 1 024 bytes, not only whitespace |
| fields | at most 16 per task; names unique, 1 to 64 bytes; `ref` as 3.4 |
| rules | at most 128 per task; names unique within a task, 1 to 64 bytes |
| conditions | at most 32 nodes per rule; `all`, `any` and `not` nest at most 4 deep; `all` and `any` have 1 to 16 members |
| `contains_any` | 1 to 32 terms, each 1 to 64 bytes; a `text` field |
| `equals` | a `text`, `bool` or `integer` field; the value is the field's canonical text (3.4) and, for `bool` and `integer`, must be one |
| `compare` | an `integer` or `decimal` field |
| terms per task | the sum of `contains_any` terms over every rule is at most 1 024 |
| contributions | at most 8 per rule; every option key names a declared option; maps are non-empty |
| `linear` | an `integer` or `decimal` field |
| `lookup` | a `text`, `bool` or `integer` field; 1 to 256 entries; keys 1 to 256 bytes and unique within the table (a duplicate key is refused, never resolved by position); at most 1 024 entries per task |
| references | every field a condition or contribution names is declared in the task |

### 3.4 Field access and validation

1. A field's `ref` is `input:<name>` or `bind:<name>`, optionally followed by
   `/<field>` to read one member of a record. It addresses the projection's
   `values` member, whose keys are the step's `project` references (spec 002,
   3.10.1). `step:` references are not accepted.
2. At each request, before any rule runs, every declared field of the chosen
   task is read and checked. A key that is absent, a `null` value or an
   absent record member is a missing field. A value of the wrong JSON type is
   an incompatible field: `text` needs a string, `bool` a boolean, `integer`
   a JSON integer in `i64`, and `decimal` a string that parses as a spec 002
   decimal. Either fails the request (3.7.6).
3. A field's canonical text, for `equals` and `lookup`: a text field's
   string; `true` or `false`; an integer's shortest decimal digits with a
   leading `-` when negative. Text comparison is byte equality.
4. `contains_any` compares after mapping ASCII `A` to `Z` to lower case in
   both the field and the terms; other bytes compare exactly. It holds when
   any term occurs as a substring.

### 3.5 Choosing the task

The projection is parsed with the contract's bounded parser under limits
derived from the program: `max_projection_bytes`, depth 16, strings no
larger than the projection, 4 096 members per collection, 50 000 values, no
fractional numbers. The request's task is the one whose `operation`, `task`,
`question` and `options` (in order) all equal the projection's. No match, a
projection over the limit or malformed, fails the request.

### 3.6 Numbers

1. Logits are computed per option as spec 002 decimals: exact, checked
   addition; `linear` multiplies the coefficient by the field value with
   `Decimal::checked_mul`, rounding half-even at 10^-9 (spec 002, 3.4.2) and
   converting an integer field exactly first. There is no other rounding in
   the program's arithmetic, and no saturation or wrapping: an overflow fails
   the request naming the task, rule and option.
2. Order is declared order: `base`, then each applied rule in rule order and
   each contribution in its declared order.
3. The output converts each option's decimal to the nearest `f64` (spec 002,
   3.4.5). Distinct decimals closer than `f64` resolution at their magnitude
   may convert to the same logit; the conversion is the only rounding of the
   returned value.
4. Ties are not broken by the backend: equal logits are returned equal. Any
   tie-breaking is the core's (spec 002, 3.11.3: declared option order).

Boundary examples, kept as tests: `linear` with coefficient `0.5` over
`0.000000005` adds `0.000000002` (the exact `0.0000000025` is a tie, half-even
goes to 2); over `0.000000007` it adds `0.000000004`; a sum beyond the
`i128` unit range fails rather than saturating.

### 3.7 Evaluation

1. Logits start at `base` (an option not listed starts at 0).
2. Rules are considered in declared order. A rule whose condition holds is
   applied: each contribution is added to the logits. Overlapping rules are
   all applied, in order, until an applied rule has `stop: true`; no later
   rule is considered. Precedence is therefore declared by order and `stop`.
3. `add` adds its constants. `linear` adds `coefficient * value` for each
   listed option. `lookup` finds the entry whose key equals the field's
   canonical text and adds its constants; with no such entry, `skip` adds
   nothing and `fail` fails the request.
4. No match: when no rule was applied, `on_no_match: base` returns the base
   logits and `fail` fails the request. A task with no rules always returns
   its base under `base`.
5. The output is `logits` with exactly the task's options as keys.
6. A failed request is an adapter failure of class `permanent` (it recurs for
   the same program and projection), with a detail naming the cause and
   location. The runtime supplies it as `backend_unavailable` (spec 003,
   3.6.4); it is never retried under `retry.on`, since `permanent` is not
   retryable.

### 3.8 Interpretation

Every task carries an authored `interpretation` stating what its logits
mean. Logits are authored preferences over the options, not
log-probabilities: the core's softmax turns them into a `Distribution` that
is still an authored heuristic, and applying a calibration artifact to them
establishes only which transformation was applied (spec 002, 3.6.2). The
interpretation is part of the program and therefore of its identity; the
descriptor schema has no field for it, so it reaches a reader through the
program (`RulesBackend::program`), not through the descriptor, the output or
the run record.

### 3.9 Disclosure: the derived descriptor

The descriptor is derived from the program alone:

- `backend_id` is the program's;
- `artifact` is the program's identity (3.10);
- `operations` has one entry per operation that has at least one task, in
  the order `classify`, `proposition`, `rubric`, each with output `logits`
  and `max_options` the largest option count among that operation's tasks;
- `input_limit` is `max_projection_bytes` with `on_excess: refuse`;
- `determinism` is `bitwise`: the same program and projection produce the
  same output bytes on every platform (integer arithmetic and one correctly
  rounded conversion). This is repeatability of the backend's own output,
  not of the core's normalization, which uses `exp` (spec 002, 3.4.6).

### 3.10 Identity

1. The artifact identity is the program's digest under the contract's own
   identity function (spec 002, 3.3.2): `sha256` over `rustev.rules/1`, a
   zero byte, and the program's canonical bytes, in `ArtifactId`'s textual
   form. The backend is the artifact's producer and supplies it (spec 002,
   3.3.3); no other hashing convention is introduced.
2. Every behavior-affecting input is in the program: rules, conditions,
   terms, coefficients, lookup entries, base logits, options, output mapping,
   interpretation, limits and cost. `RulesBackend::new` takes the program and
   nothing else. So changing any of them changes the `ArtifactId`, therefore
   the derived `DescriptorId`, therefore the `PlanId` of any plan compiled
   against it, and invalidates any calibration bound to the old artifact
   (spec 002 category 10).
3. The evaluation semantics are fixed by the schema version. A change to how
   a `rustev.rules/1` program is evaluated is a new schema version, which
   changes every artifact identity; it is never made silently under version
   1. The crate version is not part of the identity.
4. `rustev-runtime` refuses to prepare a plan whose bound `DescriptorId`
   differs from the registered backend's (spec 003, 3.6.5), so a backend
   loaded with a changed program cannot serve a plan compiled against the
   old one.

### 3.11 Setup responsibilities

Loading is explicit and outside inference:

1. The host obtains the program bytes (its own I/O) and parses them with
   `RulesProgram::parse`, which applies `DESCRIPTOR_V1`.
2. `RulesBackend::new` validates the program (3.3) and derives the
   descriptor and artifact identity.
3. The host compiles plans against `descriptor()`.
4. `check_plan(plan)` confirms that every semantic step bound to this
   backend, as primary or runtime fallback target, has a task with the same
   operation, task, question and options, and that every field `ref` is in
   the step's `project` list and has a compatible declared type (`text` for
   text and enum inputs, `bool`, `integer` for integers and timestamps,
   `decimal`), resolving `bind:` references through the step's `for_each`
   list items. It returns every mismatch. The runtime does not call it;
   skipping it moves each mismatch to a `permanent` failure per request.
5. The host registers the backend with the runtime, whose `prepare` checks
   descriptor identity (3.10.4).

### 3.12 Cost

1. The program's `units_per_call` is a count of **metered logical units**
   the deployment chooses (for example a quota unit). It is not money: this
   backend calls no provider and incurs no external monetary cost.
2. `cost_model` is `bounded`, and `cost_bound` is
   `bounded{max_units: units_per_call}` for every projection. Every call
   whose future is polled reports `observed{units: units_per_call}`, whatever
   its result (output, failure or cancellation), so the runtime's
   reservation always equals the charge and there is no bound violation.
3. `units_per_call: 0` means no metered charge. It does not mean the call
   used no computation: every call parses, validates and evaluates.
4. The work of one call is bounded by the limits of 3.3 and 3.5, not by
   cost: at most one bounded parse of `max_projection_bytes`, 16 field
   reads, 128 rules of at most 32 condition nodes, 1 024 substring searches
   over one field each, and 8 contributions per applied rule. Neither this
   bound nor a cost cap is a CPU-time guarantee.

### 3.13 Cancellation

1. The backend observes the attempt's `CancelSignal` at three kinds of
   point: before parsing the projection, after field validation, and before
   each rule. Between two points it does at most one rule's work.
2. When it observes the signal raised, it stops, returns the failure
   `cancelled` with the point in its detail, acknowledges `stopped` (it has
   no remote work, so stopping its own evaluation is all there is to stop),
   and reports the call's metered charge.
3. When it completes without observing the signal it returns its result
   with `not_requested`, even if the signal was raised after the last point:
   the computation had completed, and it says so.
4. Evaluation is synchronous within one poll, so the runtime's own deadline
   and attempt timeout cannot interrupt it; they apply between polls. A
   signal raised elsewhere (the caller's, propagated to the attempt's child
   signal) is observed at the next point. Nothing here describes remote
   cancellation.

### 3.14 Provenance

A value this backend returns is a semantic step value, and spec 001 3.3.1
and spec 002 3.12.2 class it `model-derived` ("produced by a semantic
backend"); an exact step or judgment reading it is `mixed-derived`. That is
the honest class under the current contract: the value is not computed by
the plan's exact-operator registry, its rules are not in the plan's
definition, and it is bound only by identity. Repeatability does not make it
exact-derived. The class name says "model" although no model is involved;
the plan's binding (backend id, artifact, descriptor) identifies the
producer. The core keeps the step's lineage (its projected inputs). A rules
result is evidence that this program produced it from those inputs, never
independent evidence about the real-world claim (a keyword in a ticket is
not evidence that it concerns billing).

### 3.15 Dependencies and partial adoption

`rustev-backend-rules` depends on `rustev-contract`, `rustev-core`, `serde`
and `serde_json`. It does not depend on `rustev-runtime`, Tokio, an HTTP
stack or any ecosystem crate; its integration tests may use
`rustev-runtime` and Tokio as dev-dependencies. It is usable without the
runtime: `RulesBackend::evaluate(projection, &CancelSignal)` answers one
projection synchronously.

### 3.16 Reference programs

Two synthetic programs, one per reference plan, live under the crate's
`tests/fixtures/`. Their ids and interpretations say `SYNTHETIC`. They
establish that the software runs both plans; they say nothing about
recommendation quality, calibration or semantic accuracy (R-04).

## 4. Out of scope

The semantic backend (spec 011), `rank` and `scores`, label or
distribution outputs, scripting or embedded interpreters, regular
expressions, text normalization beyond ASCII case, reading `step:` values,
remote execution, evaluation and replay (spec 004), the CLI (spec 006).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A program with a duplicate lookup key, an unknown option key, 129 rules, 1 025 terms in one task, or a `rank` task | Refused by `RulesBackend::new`, naming the item. |
| A projection whose task, question or options differ from every task | `permanent` failure; the runtime supplies `backend_unavailable`. |
| A declared field absent, `null`, or of the wrong type in the projection | `permanent` failure naming the field. |
| No rule applies and `on_no_match: fail` | `permanent` failure; with `base`, the base logits. |
| Two overlapping rules, the first with `stop: true` | Only the first is applied. |
| A `linear` product or a sum past the `i128` unit range | `permanent` failure naming task, rule and option; no saturation. |
| One changed coefficient, lookup value, term, option mapping or cost | A different `ArtifactId`, `DescriptorId` and `PlanId`. |
| A backend loaded with a changed program, registered for a plan compiled against the old one | `prepare` refuses with `DescriptorMismatch`. |
| The attempt's signal raised before the call, or between two rules | `cancelled`, acknowledged `stopped`, charge reported; no output. |
| A hard budget smaller than `units_per_call` | `budget_exhausted{cost}` without dispatch. |
| A rules output altered to carry an undeclared option | Refused by the core's validation as `invalid_backend_output`. |

## Acceptance

- Every validation rule of 3.3 has a failing program and a passing
  neighbour, including each limit at and one past its bound.
- Overlap, `stop` precedence, duplicate lookup keys, `skip` and `fail` on a
  missing key, and both no-match behaviors are tested.
- Missing, `null`, absent-member and wrongly typed fields fail with
  `permanent`; a mismatched task, question or options fails with `permanent`.
- `logits` for `classify`, `proposition` and `rubric`, the only advertised
  output kind, are tested, and the descriptor advertises nothing else.
- The rounding examples of 3.6, overflow in `linear` and in a sum, and equal
  logits returned equal are tested.
- Changing any behavior-affecting program item changes `ArtifactId`,
  `DescriptorId` and `PlanId`; an unchanged program reproduces them.
- The runtime refuses a descriptor mismatch; `check_plan` reports each kind
  of mismatch and accepts both reference plans.
- Repeated evaluation of the same program and projection is byte-identical.
- Cancellation before the call and between rules stops with `stopped`;
  a completed evaluation reports `not_requested`.
- Through `rustev-runtime`, a nonzero `units_per_call` is reserved, charged
  and summarized exactly, under a hard budget within its guaranteed cap; a
  zero cost is charged as zero; a budget too small refuses dispatch.
- Both reference plans run through the runtime on the synthetic programs
  under hard budgets to the expected judgments, and their semantic steps are
  `model-derived` with their projected inputs in lineage.
- The backend answers a projection with no runtime, and its normal
  dependency tree holds no runtime, Tokio, HTTP or ecosystem crate.
- An altered rules output is refused by the core as
  `invalid_backend_output`.
- Seeded defects in identity, accounting, cancellation and validation are
  each detected (a bounded negative control).

## Verification

```verify:cli
# 3.2 to 3.16: validation, evaluation, identity, cost, cancellation, the
# plan check, and both reference plans through the runtime.
cargo test -p rustev-backend-rules --locked
# 3.15: dependency rules and partial adoption.
cargo run -p rustev-boundaries --locked --quiet
sh -c 't=$(cargo tree -p rustev-backend-rules -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -E "^(rustev-|tokio$)" | grep -vxE "rustev-(contract|core|backend-rules)" | grep -q .; then exit 1; fi'
# 3.1.2: no I/O, clock, environment, process or thread use in the backend.
sh -c 'if grep -rnE "std::(fs|net|env|process|thread|time)|tokio" backends/rustev-backend-rules/src; then exit 1; fi'
cargo clippy -p rustev-backend-rules --all-targets --locked -- -D warnings
cargo fmt -p rustev-backend-rules --check
# Negative control: seeded defects must each be detected.
sh backends/rustev-backend-rules/mutation/seeds.sh
```

## Implementation record

- `backends/rustev-backend-rules/`: `RulesProgram` (the document,
  `program.rs`), validation into a checked form, the evaluator (`eval.rs`),
  `check_plan` (`check.rs`) and the `DecisionBackend` implementation
  (`lib.rs`). Production dependencies: `rustev-contract`, `rustev-core`,
  `serde`, `serde_json`. `rustev-runtime` and Tokio are dev-dependencies of
  the integration tests only. No contract, core or runtime unit changed.
- Synthetic reference programs `tests/fixtures/support-routing.rules.json`
  (3 units per call) and `tests/fixtures/lodging.rules.json` (2 units per
  call). Through the runtime, support routing is judged
  `route_ticket{queue: billing-priority, priority: high}` under a hard cap of
  exactly 9 units (3 requests), and lodging ranks three candidates using
  10 requests and 20 units under a hard cap of 32. These establish software
  behavior only (R-04).
- Tests: `tests/program.rs` (every validation rule and limit, at and one
  past), `tests/evaluate.rs` (semantics, fields, numbers, ties,
  repeatability, standalone `infer` with no executor),
  `tests/identity.rs` (identity, calibration refusal, `check_plan`
  including fallback targets), `tests/runtime.rs` (both reference plans,
  exact nonzero and zero cost, a budget one unit short, descriptor refusal,
  cancellation observed by the backend and recorded `stopped` with its
  charge, an altered output refused by the core, provenance), and
  `src/tests.rs` (cancellation points through a deterministic observation
  hook).
- Independent review of the rules semantics, identities, numbers and
  cancellation and cost disclosures found no high-severity defect. It
  found that 9 of 11 mutations it seeded survived the tests then present:
  identity was not tested for field `ref`, field `type` or `on_missing`;
  `le`, `gt`, `not` and `any` were never evaluated at a boundary;
  `check_plan` fallback targets and a `null` record were untested. Each
  gap now has a regression test, and each of those mutations is a seed of
  the negative control. Also fixed: the projection parse had a 40-byte
  number cap that 3.5 does not state (now bounded by the byte limit only, as
  3.5 says), and `check_plan` could list a step twice.
- Negative control: `mutation/seeds.py` seeds 21 defects (identity ignoring
  the program, a field reference or `on_missing` left out of the identity,
  charge or bound misreported, cancellation never observed, a stop claimed
  for a failure, distributions advertised, `stop` or no-match `fail`
  ignored, mismatched options accepted, an unchecked integer field,
  saturating overflow, truncating `linear`, duplicate lookup keys accepted,
  unbounded condition depth, `check_plan` ignoring options or fallback
  targets, and `le`, `not` and `any` miscomputed). Each must compile and be
  detected; a timeout counts as inconclusive, never as detected.

### Clarifications of the approved text

Found in review and recorded here rather than edited into the approved
sections above (R-16). None changes behavior.

1. 3.6.1 and the overflow row of section 5: `linear` uses
   `Decimal::checked_mul`, which refuses when the exact `i128` intermediate
   overflows (spec 002, 3.4.2). A product therefore fails once its
   magnitude exceeds about 1.7 * 10^20, well inside the unit range of about
   1.7 * 10^29 that sums may reach. It fails; it never saturates.
2. 3.2 and 3.3: the program is parsed under `DESCRIPTOR_V1`, so a program
   within every per-item limit of 3.3 can still be refused by that set's
   256 KiB total or its 4 KiB escaped-string cap (for example 32 tasks of
   128 rules, or a 4 096-byte question containing a quote).
3. 3.13.1: between `before parsing` and `after field validation` the backend
   does the bounded parse, task choice and field reads; between later points
   it does at most one rule.
4. 3.4.1: a field `ref` of `input:<name>/<member>` reads a member of the
   projected `input:<name>` record. A plan that projects the record-field
   reference `input:<name>/<member>` itself does not match it, and
   `check_plan` reports `not_projected`.
5. 3.3, `lookup`: keys on `bool` and `integer` fields are not checked for
   canonical form; a key such as `TRUE` or `07` is accepted and never
   matches. `equals` does check its value. Refusing such keys would change
   approved behavior, so it is left to a reviewed amendment.
6. Acceptance, provenance: evidence records carry each semantic step's
   derivation class (`model-derived`); lineage is carried by the judgment,
   which lists the inputs the rules read (for support routing,
   `ticket.message`) and the rules steps.

### Limitations

- Outputs are authored heuristics. Nothing here measures quality,
  calibration or semantic accuracy, and the synthetic calibration used for
  `topic` in tests is unfitted.
- `check_plan` is the host's call; the runtime does not make it.
- Evaluation is synchronous within one poll: the runtime's deadline and
  attempt timeout cannot interrupt it; its work is bounded by the program
  and projection limits, which are not a CPU-time guarantee.
- The interpretation reaches readers through the program, not the
  descriptor or run record.

## Decision history

- 2026-09-23: drafted with the increment 2 contracts, then aligned with the
  delivered runtime seam.
- 2026-09-23: the owner narrowed this spec to the rules backend (R-13),
  moved the semantic backend to spec 011, ordered it before spec 004 as a
  single exception (R-12), and approved it within that scope (A-04). The
  concrete rules above were written by the agent within the owner's brief;
  its engineering choices are listed below as its own.

## Engineering choices

Made by the agent within A-04; open to the owner's review.

| Id | Choice | Reason |
|---|---|---|
| E-17 | The artifact identity is the program's tagged digest; the descriptor is derived from the program. | Uses spec 002's identity function; binds every behavior-affecting input without a contract change. |
| E-18 | Logits only, for classify, proposition and rubric. | The reference plans need nothing else; advertising less is honest. |
| E-19 | One evaluation model: base plus ordered, additive rules with `stop`. | Covers first-match (every rule stops) and additive linear heads with one rule of precedence. |
| E-20 | A failed request is `permanent`. | Program and projection determine it; a retry repeats it. |
| E-21 | A flat charge per polled call, including cancelled calls. | Reservation equals charge, so accounting is exact; charging a stopped call errs toward liability, not refund. |
| E-22 | Synchronous evaluation with cancellation checks, no yielding. | Bounded work and no executor dependency; the limitation (3.13.4) is stated rather than hidden. |
| E-23 | `check_plan` is a backend function the host calls, not a runtime hook. | No contract or runtime change; the runtime's descriptor check already refuses the identity mismatch. |

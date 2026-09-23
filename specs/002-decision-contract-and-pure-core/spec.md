---
id: "002-decision-contract-and-pure-core"
title: "Decision contract and pure core"
status: approved
implementation: complete
created: "2026-09-23"
summary: >
  Increment 1: the versioned wire contract (`rustev-contract`) and the pure
  core (`rustev-core`). Bounded parsing, canonical form and identities, value
  kinds without silent conversion, fixed-point decimals and millisecond time,
  input validation with provenance, freshness and conflict, a closed versioned
  exact-operator registry, decision definitions entered through a typed builder
  or canonical JSON, the plan compiler with capability matching, declared
  fallbacks and typed refusals, calibration binding, deterministic selection
  policy with explicit unresolved handling, and derivation classes with
  lineage. No I/O, no clock, no async executor, no model.
establishes:
  - { kind: directory, path: "crates/rustev-contract/" }
  - { kind: directory, path: "crates/rustev-core/" }
extends:
  # Adds the crates/ member glob and workspace dependencies.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  # Records the two crates' resolved dependencies (serde, serde_json, sha2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 002 to the specs `make verify` runs.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "000-bootstrap"
  - "001-boundaries-and-authority"
---

# 002: Decision contract and pure core

Rationale: `docs/design/001-decision-engine-architecture.md` sections 6, 7,
8 (trait shapes only), 13, 14 and reference plans 16.1 and 16.2.

## 1. Purpose

Make a decision definition compile into an identified plan, or be refused with
a typed reason, and make a plan evaluate deterministically over a supplied
context snapshot, a supplied evaluation time and supplied semantic backend
outputs, before any runtime or model exists.

## 2. Territory

- `crates/rustev-contract/`: serde types for every document in 3.2, bounded
  parsing, canonical form, identities, decimals and time. Depends on `serde`,
  `serde_json` and `sha2` only. Any outside project reads Rustev documents
  with this crate alone.
- `crates/rustev-core/`: validation, value kinds, the operator registry,
  compiler, calibration application, staged evaluation, selection policy, the
  typed builder and the seam traits. Depends on `rustev-contract`, `serde`,
  `serde_json` and `sha2` only. Neither crate has dev-dependencies.
- Tests, golden documents and synthetic fixtures live under each crate's
  `tests/` directory and are owned with it. Every synthetic fixture says so in
  its name or content (R-04); none is evidence of semantic quality.

## 3. Behavior

### 3.1 Purity

1. `rustev-core` performs no I/O, reads no clock, environment or randomness,
   spawns nothing, and is `#![forbid(unsafe_code)]`; so is `rustev-contract`.
   The evaluation time is an input value.
2. Seam traits (context source, decision backend, evaluator, evidence sink)
   are declared in `rustev-core::seams` as signatures over `std::future`
   types. Nothing in increment 1 calls them.

### 3.2 Documents and bounded parsing

1. Every document carries a `schema` string; increment 1 defines
   `rustev.definition/1`, `rustev.plan/1`, `rustev.backend/1`,
   `rustev.calibration/1`, `rustev.snapshot/1`, `rustev.backend-output/1`,
   `rustev.judgment/1`, `rustev.evidence/1` and `rustev.eval-report/1`. A
   different `schema` value is refused.
2. Parsing supplied bytes happens in two stages. The scan validates the whole
   input as RFC 8259 JSON against a `ParseLimits` value before any typed value
   is constructed: total bytes (checked first, before reading content),
   nesting depth, raw string bytes, members per array or object, total value
   count, raw number bytes, and whether fractional numbers are allowed. The
   scan allocates only its container stack (bounded by depth) and per-object
   key sets (bounded by the input length). Typed deserialization runs only on
   accepted input, so its allocations are bounded by a function of the
   accepted limits (a small multiple of the accepted size), not by anything
   the input can choose beyond them. Every scalar, array and object counts as
   one value; object keys do not.
3. Duplicate object keys are refused by the scan, compared after decoding
   escapes. Unknown fields are refused by every document type.
4. Reading bytes from a transport, and bounding that buffer, belongs to the
   transport (a later runtime or integration), not to this parser. No claim is
   made that checks after deserialization prevent allocations.
5. Named limit sets, each a constant:

| Set | bytes | depth | string | collection | values | number | fractions |
|---|---|---|---|---|---|---|---|
| `DEFINITION_V1` | 1 MiB | 32 | 64 KiB | 4 096 | 100 000 | 40 | no |
| `SNAPSHOT_V1` | 4 MiB | 32 | 256 KiB | 10 000 | 500 000 | 40 | no |
| `DESCRIPTOR_V1` | 256 KiB | 16 | 4 KiB | 1 024 | 20 000 | 40 | no |
| `BACKEND_OUTPUT_V1` | 1 MiB | 8 | 4 KiB | 4 096 | 50 000 | 40 | yes |
| `PLAN_V1` | 2 MiB | 32 | 64 KiB | 4 096 | 200 000 | 40 | no |
| `RECORD_V1` | 4 MiB | 32 | 256 KiB | 10 000 | 500 000 | 40 | yes |

   Definitions use `DEFINITION_V1`; snapshots `SNAPSHOT_V1`; backend
   descriptors and calibration artifacts `DESCRIPTOR_V1`; backend outputs
   `BACKEND_OUTPUT_V1`; plans `PLAN_V1`; judgments, evidence records and
   evaluation reports `RECORD_V1`.

### 3.3 Canonical form and identities

1. Canonical bytes of a document: its serde value with object keys sorted by
   byte order, no insignificant whitespace, `serde_json` string escaping, and
   no fractional or exponent numbers (identity-bearing documents carry
   decimals as strings). Array order is significant and preserved. Every
   field of every identity-bearing type is always written: there are no
   omitted or `null` fields; absence is an explicit variant. Enums are
   externally tagged with `snake_case` names unless the contract type states
   otherwise. The serde definitions in `rustev-contract` at schema version 1
   are the normative encoding; a change that alters canonical bytes is a new
   schema version, and the golden documents pin it.
2. An identity is `sha256:` followed by 64 lowercase hex digits of
   SHA-256 over `tag || 0x00 || canonical bytes`, where `tag` is the schema
   string of the identified document. Identified documents: definition
   (`DefinitionId`), plan (`PlanId`), backend descriptor (`DescriptorId`),
   calibration artifact (`CalibrationId`), snapshot (`SnapshotId`).
3. `ArtifactId`, `DatasetId` and `EvaluatorConfigId` are supplied by their
   producers in the same textual form and are validated, not computed.
4. The plan document embeds the canonical definition, the definition id,
   every bound backend's id, artifact and `DescriptorId`, every calibration's
   id and binding, every operator id and version, the exact-registry version
   `rustev.exact/1`, every field of the definition's limits, the topological
   step order, each step's static derivation class (3.12), notices as
   `{kind, step, reason}`, and the compiler identity
   `rustev-core/<crate version>`.
   `PlanId` is the identity of that document.

### 3.4 Numbers and time

1. `Decimal` is fixed-point: an `i128` count of 10^-9 units, written as a
   JSON string matching `-?(0|[1-9][0-9]*)(\.[0-9]{1,9})?`. More than nine
   fractional digits is refused, never rounded. Canonical text has no
   trailing fractional zeros and never `-0`.
2. Addition and subtraction are exact and checked. Multiplication and
   division compute the exact `i128` intermediate (overflow of the
   intermediate is refused) and round half-even to 10^-9; division by zero is
   refused. Rescaling (`scale` 0 to 9) rounds half-even. Overflow and division
   by zero make the step `InvalidInput` naming the step's input fields; there
   is no saturation and no wrapping.
3. `Integer` is `i64` with checked arithmetic; integer division is not
   offered. Integer and decimal never mix without the explicit `to_decimal`
   conversion.
4. `Timestamp` is an integer count of milliseconds since the Unix epoch, UTC,
   in `0..=253402300799999`. `Duration` is a non-negative `u64` of
   milliseconds. `timestamp - timestamp` is a duration and is refused when
   negative; `timestamp +/- duration` is checked against the range.
5. A mass, score or logit is an `f64` and must be finite. A decimal compared
   with an `f64` is first converted to the nearest `f64`.
6. Exact computation, for byte determinism (principle XIII), means: decimal,
   integer, timestamp and text operations above; conversion of a decimal to
   the nearest `f64`; and IEEE 754 `f64` addition, subtraction,
   multiplication and division (each correctly rounded) performed in a fixed,
   declared order with no fused operations. It excludes `exp` and `ln`, which
   logit normalization and temperature calibration use; those are
   backend-output transformations with numerical, not byte, repeatability.

### 3.5 Value kinds

1. `ModelScore`, `Distribution`, `CalibratedProbability`, `SelectedLabel`,
   `OrdinalLevel` and `RankPosition` are distinct types in `rustev-core` with
   no `From` or `Into` between any two of them.
2. A `Distribution` has exactly the declared options as keys, each mass
   finite and non-negative, summing (in declared option order) to 1 within
   the plan's `distribution_tolerance`. It is stored as supplied, never
   renormalized. Logits are normalized by softmax with max subtraction, in
   declared option order.
3. A `ModelScore` carries its scope (plan, step, instance). Comparing two
   scores of different scopes is an error.
4. `OrdinalLevel` holds a distribution over the declared levels, or a single
   level. Its expectation (level index weighted by mass) is an index summary,
   not an interval quantity.
5. `RankPosition` holds the ordered candidates with the scores and components
   that produced the order. It is not a probability of being best.
6. `Unresolved` is closed: `missing_evidence{fields}`,
   `stale_evidence{fields}`, `invalid_input{fields, detail}`,
   `abstained{rule}`, `backend_unavailable{detail}`,
   `invalid_backend_output{detail}`, `budget_exhausted{resource}`,
   `deadline_exceeded`, `conflict{facts}`, `unsupported{capability}`. Field and fact lists are
   sorted and deduplicated.

### 3.6 Calibration

1. A calibration artifact (`rustev.calibration/1`) declares a binding
   (backend artifact, task, question as the step id, dataset, method), its
   options, and its parameters. The only method in increment 1 is
   `temperature/1` with a decimal temperature in `(0, 1000]`, applied to
   logits, or to a distribution through natural logarithms of its masses (a
   zero mass stays zero): `softmax(z / T)`.
2. `CalibratedProbability` is constructible only by applying a calibration
   artifact to an output whose step, task, backend artifact and options match
   the binding. The constructor establishes which transformation was applied
   and its binding. It does not establish that the result is calibrated for
   current data or under distribution shift; that is measured by evaluation
   over named data (spec `004` onward). Fitting is not in this spec.

### 3.7 Inputs, provenance and freshness

1. A definition declares each input: name, type, accepted provenance classes
   and freshness (`max_age_ms` or `not_required`). Provenance classes are
   closed: `authenticated-app-field`, `system-of-record`, `user-supplied`,
   `third-party`, `attributed-claim`, `model-derived`.
2. Types: `bool`, `integer`, `decimal`, `text{max_bytes}`, `timestamp`,
   `enum{options}`, `list{item, max_items}`, `record{fields}` (every record
   field is required). Values are decoded strictly: no coercion, no default.
3. A snapshot is a list of entries `{field, value, provenance, as_of_ms,
   source}`. For each declared input, at evaluation time `now`:
   - entries with an accepted provenance class whose values differ
     (canonical comparison) make the field `conflict{facts: [field]}`;
     entries with a non-accepted class are ignored;
   - an accepted entry with `as_of_ms > now` makes the field
     `invalid_input` (future timestamp; no skew allowance in increment 1);
   - no entry with an accepted provenance class makes it
     `missing_evidence{field}`;
   - a value failing its type makes it `invalid_input`;
   - otherwise the newest accepted entry's age `now - as_of_ms` greater than
     `max_age_ms` makes it `stale_evidence{field}`; equal is fresh.
   Checks apply in that order. An entry for an undeclared field is refused.
4. A failed input does not stop evaluation. Every step reading it is
   unresolved; the selection policy decides.

### 3.8 Exact operators

A closed registry, `rustev.exact/1`. Each operator is referenced by name and
version; an unknown name or version is refused at compile time. Expressions
and predicates inside operator arguments are part of the same registry
version: references to inputs (`input:<name>`, record fields as
`input:<name>/<field>`), steps (`step:<id>`), list items
(`item:<field>.<field>`) and `now`; typed literals; `add`, `sub`, `mul`,
`div` (decimals, half-even at 10^-9), `round` (with `scale`), `fx`, `len`,
`to_decimal`;
comparisons (`eq`, `ne` on every scalar; ordering on integer, decimal,
timestamp and duration); `all`, `any`, `not`, `member`, `present`.

| Operator | Result | Empty input | Failure |
|---|---|---|---|
| `count_where@1` | integer count of items satisfying a predicate | 0 | a predicate fault (overflow) is `invalid_input` |
| `window_filter@1` | items whose timestamp field is within `[now - within_ms, now]` and satisfy a predicate | empty list | an item timestamp after `now` is `invalid_input` |
| `extremum@1` | `min` or `max` of a field, as `maybe` | `none` | none |
| `timestamp_diff@1` | `floor((later - earlier) / unit_ms)` as `maybe` integer | `none` in, `none` out | negative difference or overflow is `invalid_input` |
| `compare@1` | bool | n/a | overflow in an operand is `invalid_input`; an `fx` operand fails as `fx_convert@1` |
| `arith@1` | decimal or integer expression | n/a | overflow or division by zero is `invalid_input` |
| `fx_convert@1` | `amount / rate(from) * rate(to)`, rescaled half-even to `scale` | n/a | missing rate is `missing_evidence`; two different rates for one currency is `conflict` |
| `member_of@1` | bool | n/a | a fault in the value expression is `invalid_input` |
| `field_presence@1` | declared inputs that are absent or have no accepted provenance, in declared order | empty list | a present input that is stale, invalid or conflicting makes the step unresolved with that reason |
| `filter_with_reasons@1` | eligible ids, excluded ids with every failed rule in rule order, and a count per rule | empty result | an item whose rule cannot be evaluated is excluded with `unevaluable:<rule>`; a duplicate or missing id is `invalid_input` |
| `top_k@1` | the first `k` eligible ids ordered by a key (ties by id ascending), with the count truncated | empty | as `filter_with_reasons@1` |
| `weighted_rank@1` | `RankPosition` over candidates from declared components and decimal weights | empty ranking | a candidate whose component is unresolved is excluded and listed with the reason |

`weighted_rank@1` components: the weighted mean of a proposition's `true`
mass over a second index with weights from a table keyed by an item field
(zero items gives 0; an item whose class has no declared weight excludes the
candidate); a rubric's expectation divided by its highest level index; and a
shortlist position `1 - i / (n - 1)` (1 when `n = 1`). The score is
`sum(weight * component)` in declared component order; ties are broken by
candidate id ascending.

### 3.9 Definitions and compilation

1. A definition (`rustev.definition/1`) declares name, version
   (`MAJOR.MINOR.PATCH`), package, inputs, steps, policy and limits. Step ids
   and input names are unique. Exact steps name an operator, a version and
   arguments. Semantic steps name an operation (`classify`, `proposition`,
   `rubric`, `rank`, R-05), a task, options or levels, the required value
   kind, a projection of inputs and steps, an optional `for_each` over one or
   two id lists, backend requirements (minimum determinism, optional artifact
   pin), an optional calibration id, and a fallback (`none`, `unsupported`, or
   an alternative required kind with a reason).
2. The typed builder in `rustev-core` produces the same definition document
   the JSON parser produces; both go through one validation (R-02). Golden
   documents are emitted by the builder and compared byte for byte.
3. Required kinds and the backend outputs that satisfy them:

| Required | Satisfied by | Operations |
|---|---|---|
| `label` | `label` | classify, proposition, rubric |
| `distribution` | `distribution`, `logits` | classify, proposition |
| `calibrated_probability` | `distribution`, `logits`, plus a matching calibration | classify, proposition |
| `ordinal_distribution` | `distribution`, `logits` | rubric |
| `scores` | `scores`, `logits` | rank |

4. Compilation phases, in order: (a) parse and bound; (b) structure: ids,
   references, literals, versions, limits; (c) resolve operators and
   calibrations; (d) graph: order steps topologically (ties by declaration
   order); (e) types: every reference and edge; (f) bind backends (an
   explicit binding makes that backend the only candidate; otherwise the
   satisfying descriptors in `backend_id` byte order, first wins), fallbacks
   and calibration artifacts; (g) budget; (h) policy; (i) emit. Compilation
   stops at the first refusal: the earliest phase wins, and within a phase the
   first step in declaration order, then the first policy item in declaration
   order.
5. Refusals are typed, name the step or policy item and the requirement, and
   belong to exactly one phase:

| Category | Phase | Refused when |
|---|---|---|
| 1 `parse` | a | a bound, syntax, duplicate key, unknown field, unknown schema, or a JSON value of the wrong shape |
| 11 `invalid_definition` | b | an unknown reference, duplicate id, a well-formed but invalid literal, version or limit, or a policy whose last rule is not `always` |
| 2 `unknown_operator` | c | an operator name or version not in `rustev.exact/1` |
| 10 `calibration_binding` | c, f | a `calibrated_probability` step names no calibration, or a named one is not supplied, or its task, question, options or method differ from the step (c); or its artifact differs from the bound backend's (f) |
| 4 `cycle` | d | the step graph has a cycle; the error lists its steps |
| 3 `kind_mismatch` | e, h | a reference, edge or policy condition reads a type or value kind its consumer does not accept, including a label where mass is required |
| 5 `no_capable_backend` | f | no candidate satisfies a semantic step and the fallback is `none`; the error lists each candidate's shortfall (operation, output kind, option count, input limit, determinism, artifact pin) |
| 9 `invalid_fallback` | f | a fallback kind its consumers cannot accept, or that no candidate satisfies either |
| 7 `budget` | g | worst-case semantic requests exceed `max_semantic_requests` without `truncate_visible`, or a projection's worst-case bytes exceed `max_projection_bytes` |
| 6 `uncalibrated_threshold` | h | a probability threshold reads a `distribution` without a declared `uncalibrated_threshold` reason (after the kind check) |
| 8 `unhandled_unresolved` | h | the policy reads a value whose possible unresolved reasons are not all handled, applies `not` to a value handled `as_unmet`, or its last rule reads such a value |

6. A declared fallback that is taken at compile time is recorded in the plan
   binding and as a notice. `unsupported` makes the step always
   `unsupported{capability}`, which the policy must handle.

### 3.10 Staged evaluation

1. `Evaluation::start(plan, snapshot, now)` validates the context and computes
   every exact step whose inputs are available. `pending()` lists semantic
   requests (step, instance key, backend, canonical projection) in step order
   and then instance order. `supply` accepts one backend output, or one
   runtime failure (`backend_unavailable`, `invalid_backend_output`,
   `budget_exhausted`, `deadline_exceeded`), per request; any other reason,
   and a request not pending, is refused. `finish(decision_id)`
   refuses while requests are pending and otherwise returns the judgment and
   its evidence record.
2. Supplied outputs are validated against the plan: output kind exactly as
   bound (a distribution supplied to a logits binding is invalid), the bound
   artifact when a document names one, keys exactly the declared options or
   candidates, finite values, distributions within tolerance. A failure is
   `invalid_backend_output` for that instance. A fan-out step's instance
   failures stay in its instances; the step itself is unresolved only when a
   dependency is. An instance whose bound item cannot be found is
   `invalid_input`.
3. Requests beyond `max_semantic_requests` under `truncate_visible` are not
   issued; those instances are `budget_exhausted{semantic_requests}` and the
   judgment carries a truncation notice.
4. An exact step reading an unresolved value is unresolved with the first
   unresolved dependency's reason in argument order; `missing_evidence` and
   `stale_evidence` merge the fields of every dependency with the same reason.

### 3.11 Selection policy

1. A policy has `on_unresolved` handlers, ordered `rules`, and
   `adjustments`. A handler names an input or step, the reasons it covers
   (`any` or a list), and an action: `propagate` (the judgment is that
   unresolved reason), `escalate{reason}`, or `as_unmet` (atomic conditions
   reading it are false; a rule whose outcome reads it does not match).
2. Every input or step the policy reads must have handlers covering every
   reason it can be unresolved with, computed statically from its operator,
   operation, fallback and dependencies. `not` over a value handled `as_unmet`
   is refused. The last rule's condition is `always` and it reads nothing
   handled `as_unmet`.
3. Conditions: `always`, `top_label`, `mass_at_least`, `top_mass_below`,
   `expectation_at_least`, exact predicates over inputs and steps, `empty`,
   `nonempty`, `all`, `any`, `not`. `mass_at_least` and `top_mass_below` are
   probability thresholds (3.9.5 category 6). `top_label` breaks ties by
   declared option order.
4. Outcomes: `propose{action, params}` (never a grant; parameters are exact
   values or literals, never semantic values), `escalate{reason}`,
   `missing_evidence_from{step}` (a list of field names from the step). The
   policy reads semantic steps without fan-out only.
   Adjustments raise a named proposal parameter one position on a declared
   ordered scale, saturating at its top.
5. Evaluation is a pure function of the plan, the validated context, the step
   values and `now`: handlers in order, then the first matching rule, then
   adjustments in order.

### 3.12 Derivation and lineage

1. Every step value and the judgment carry a derivation class (spec `001`
   3.3) and lineage: the input fields and steps read, directly or
   transitively, sorted.
2. A semantic step value is `model-derived`. An input's class is
   `model-derived` when its accepted entry's provenance is `model-derived`,
   otherwise `exact-derived`. A value computed exactly (an exact step, or the
   policy producing the judgment) from a set of read values is
   `exact-derived` when every read value is `exact-derived` or nothing is
   read, and `mixed-derived` otherwise.
3. The plan records each step's static class, computed from the declared
   accepted provenance classes as the worst case (any accepted
   `model-derived` class counts as model-derived). The runtime class is
   computed from the snapshot actually supplied and is never less derived
   than it reads.

### 3.13 Reference plans

Definitions of reference plans 16.1 (support routing) and 16.2 (lodging) are
built with the typed builder in tests, compiled against synthetic backend
descriptors and a synthetic calibration artifact with no inference, and
committed as golden canonical definition and plan documents with golden
`PlanId`s. Each is evaluated end to end over synthetic snapshots and supplied
semantic outputs.

## 4. Out of scope

Executing semantic steps, scheduling, deadlines as they elapse, caching,
retries, evidence sinks, metrics, calibration fitting, the CLI, and any
backend. Specs `003` to `006`.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A step requiring `distribution`, bound only to a `label` backend, fallback `none` | Refused, category 5, naming the backend and its output kind. |
| A routing threshold over `distribution` with no calibration and no `uncalibrated_threshold` | Refused, category 6. |
| A supplied distribution with NaN, a negative mass, a missing or an undeclared key | `invalid_backend_output`. |
| `payments.events` older than its maximum age at `now` | `stale_evidence{payments.events}`; no rule reads it as fresh. |
| Lodging request without dates | Judgment `missing_evidence{trip.dates}`. |
| Input over the byte bound, or nested past the depth bound | Refused by the scan before any typed value is built. |
| The same definition compiled twice with the same bindings | Byte-identical plan and identical `PlanId`. |
| A binding changes only the calibration artifact | A different `PlanId`. |
| An unrelated backend descriptor is added and not selected | The same `PlanId`. |

## Acceptance

- Each refusal category 1 to 11 has a test that triggers it, with a passing
  neighbour that differs only in the refused property.
- Cycles, incompatible edges, unsupported capabilities, and each fallback
  form are tested.
- Duplicate keys, unknown fields and each parse limit are tested at and one
  past the limit.
- Non-finite values, invalid distributions, decimal overflow, division by
  zero and half-even rounding are tested.
- Missing, stale, conflicting, future-dated and invalid evidence are tested.
- An unhandled unresolved reason is refused; each handler action is tested.
- Canonical bytes and `PlanId`s of both reference plans match committed
  goldens; the builder and the parsed golden produce identical bytes.
- `PlanId` changes when the calibration artifact, a bound artifact, an
  operator version or the definition changes, and not when an unbound
  descriptor is added.
- Ties in `top_label`, `top_k` and `weighted_rank` resolve as specified.
- Both reference plans evaluate end to end on supplied semantic values.
- `rustev-contract` builds alone (spec `001` 3.7, partial adoption).
- `make code` (build, test, clippy, fmt, boundaries) passes; the value-kind
  types have compile-fail doctests for conversion.

## Verification

```verify:cli
# 3.2 to 3.4: bounded parsing, canonical form, identities, decimals, time.
cargo test -p rustev-contract --locked
# 3.5 to 3.13: value kinds, validation, operators, compiler refusals,
# calibration, evaluation, policy, reference plans and goldens.
cargo test -p rustev-core --locked
# 3.1: dependency rules for both crates.
cargo run -p rustev-boundaries --locked --quiet
# 001 3.7: the contract crate builds with no other workspace crate.
cargo build -p rustev-contract --locked
cargo clippy -p rustev-contract -p rustev-core --all-targets --locked -- -D warnings
cargo fmt -p rustev-contract -p rustev-core --check
```

## Engineering choices

Made by the implementing agent after the owner's approval (A-02), to settle
details the approved draft left open. They are not owner decisions and are
open to the owner's review; none reopens R-01 to R-06.

| Id | Choice | Reason |
|---|---|---|
| E-01 | Fixed-point `i128` decimals at 10^-9, half-even, refusing excess precision. | Currency and FX need exact, reviewable arithmetic; a dependency adds nothing the spec does not already fix. |
| E-02 | Millisecond integer timestamps, no skew allowance, future `as_of` is invalid. | Unambiguous and checkable; skew is a policy a later spec can add explicitly. |
| E-03 | Own canonical form over `serde_json` with sorted keys and no fractional numbers, instead of an external canonicalization crate. | Identity-bearing documents contain no floats, so the form is fully specified by 3.3. Not claimed to be RFC 8785. |
| E-04 | SHA-256 with a schema tag and a zero byte. | Domain separation between document types. |
| E-05 | Ties by declared option order (labels) and byte order of ids (candidates, backends). | Deterministic and visible in the document. |
| E-06 | Operator and registry versions are integers and `rustev.exact/1`. | A change in operator meaning is a new version, never an edit. |
| E-07 | A snapshot entry list, not a map, so conflicts between sources are representable. | Conflict is an explicit outcome (3.7). |

## Implementation record

Increment 1 landed with this spec marked `complete`. While implementing, the
agent settled these points within the approved scope; they are recorded here
rather than made silently, and the text above now states them:

- Record fields of an input are addressed as `input:<name>/<field>` (needed
  by the lodging plan's date, budget and currency rules). Division rounds
  half-even at 10^-9 and `round` rescales; the draft attached `scale` to
  `div`.
- `count_where@1`, `member_of@1` and `compare@1` can fail on arithmetic
  faults inside their expressions; the draft table said "none" for two of
  them.
- `supply` accepts only runtime failure reasons, so a caller cannot inject
  `missing_evidence` or `conflict` as a backend result.
- An output kind must equal the bound kind exactly; there is no implicit
  distribution-for-logits acceptance at supply time.
- Policy parameters are exact values, and the policy reads only non-fan-out
  semantic steps. Both are compile-time `kind_mismatch` refusals.
- `Compiled::load` accepts a plan document only if recompiling its embedded
  definition against the supplied descriptors and calibrations reproduces it
  byte for byte.

Evidence: `spec-spine verify 002` runs the block above; the goldens for both
reference plans are under `crates/rustev-core/tests/golden/`.

## Decision history

- 2026-09-23: approved by the owner with bounded corrections (A-02 in
  `docs/decisions/00-founding-decisions.md`): enforceable parse bounds with
  transport buffering owned separately; calibration as identity and binding
  with one explicit method; determinism scoped; explicit acceptance and
  verification. The concrete rules in section 3 and the engineering choices
  were then written by the implementing agent within those corrections; they
  are reviewable in the change that introduced them and are not the owner's
  decisions.

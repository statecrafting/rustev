---
id: "066-segment-ancestry-predicate"
title: "Definition predicate: segment ancestry on a declared separator (amends 002 and 032)"
status: approved
implementation: pending
created: "2026-11-06"
summary: >
  An amendment of approved specs 002 (3.8, 3.9) and 032 (3.1, 3.12), for
  release 0.5.1 (#509), ratified by the owner on 2026-11-06 (A-85). A partner rule needs "this location
  slug sits under that city slug", for example `us:ny:new-york:soho` under
  `us:ny:new-york`; today the host projects the slug's parent into the
  snapshot and the rule compares with `member_ref`. This spec adds one
  predicate to the registry `rustev.exact/2`, `under{value, ancestor,
  separator}`, true when the ancestor's segments are a proper prefix of the
  value's segments on a separator the definition declares, so
  `us:ny:new-york-city` is not under `us:ny:new-york`. Comparison is by
  bytes with no normalization; an operand with an empty segment is
  `invalid_input`, and an unresolved operand makes the reading step
  unresolved, never false. Definition 1 refuses it; plans that do not use
  it keep their bytes and `PlanId`.
amends:
  - "002-decision-contract-and-pure-core"
  - "032-evidence-quality"
extends:
  # The predicate in the definition and plan types and the regenerated schemas.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Builder, compilation, evaluation, grade propagation and the site rule.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # The generated TypeScript declarations gain one predicate member.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/types/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/types/" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "034-parameterized-definitions"
  - "036-mid-plan-acquisition"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "`under` is true only when both operands are valid segmented text on the declared separator and the ancestor's segments are a proper prefix of the value's segments, compared byte for byte; it is never true for a text prefix that ends inside a segment, and no case folding, Unicode normalization or trimming is applied."
    anchor: "3-2-semantics"
  - id: "I-2"
    kind: invariant
    text: "An operand that is unresolved makes the step or condition reading `under` unresolved with that operand's reason (spec 002 3.10.4), and an operand that is empty or has an empty segment is `invalid_input`; neither is ever evaluated as false."
    anchor: "3-3-unresolved-and-invalid-operands"
  - id: "R-1"
    kind: requirement
    text: "Every committed definition, plan, judgment, evidence and run golden keeps its bytes and `PlanId`, and a `rustev.definition/1` using `under` is refused with category 11 `requires_definition_2`."
    anchor: "3-5-compatibility"
---

# 066: Definition predicate: segment ancestry on a declared separator (amends 002 and 032)

Approved: ratified by the owner on 2026-11-06 (A-85) for release 0.5.1;
the implementation is pending. The approved texts of 002 and 032 are not
edited; this spec records what it adds to them.

## 1. Purpose

A partner rule needs "this location slug sits under that city slug", for
example `us:ny:new-york:soho` under `us:ny:new-york`. Today the host
projects the slug's parent into the snapshot and the rule compares with
`member_ref` (032 3.7.2). The rule's meaning then lives partly in host
code: which parent is projected, and how it was cut. A predicate the
definition states would let the rule live entirely in the definition,
with its identity in the `PlanId`.

A plain text prefix is the wrong primitive for this: `us:ny:new-york-city`
starts with `us:ny:new-york` and is not under it. The predicate here is
segment-aware: the definition declares the separator, and ancestry is a
prefix of whole segments. It is the inverse of spec 036's `compose_key@1`,
which joins normalized parts with one of the same separators.

## 2. Territory

`crates/rustev-contract/` (002): the predicate in the definition and plan
types, and the regenerated JSON Schemas of `rustev.definition/1` and `/2`
and `rustev.plan/2` and `/3` (one enum shared by both versions; version 1
still refuses it in the core). `crates/rustev-core/` (002): the typed
builder, compilation (types, refusals), evaluation, grade propagation and
the parameter-site rule. Generated binding types
(`integrations/rustev-node/types`, `integrations/rustev-wasm/types`)
regenerate with one added predicate member; neither binding's code
changes.

Ratification added the `extends` edges to `crates/rustev-contract/` and
`crates/rustev-core/` (002, amending) and to the generated type
directories `integrations/rustev-node/types/` and
`integrations/rustev-wasm/types/` (027, additive); the edges to
`Makefile` and `scripts/acceptance/` (001, additive) are added by the
change that completes this spec.

Unchanged: `rustev.definition/1`, `rustev.plan/2`, `rustev.exact/1`;
every existing operator and predicate; snapshot, evidence, run and
judgment documents; the runtime, eval, CLI, service and backends
(including spec 005's rules programs, whose language is separate); every
refusal category and its phase order.

### 2.1 Binding principles

- **Proposals, never grants.** A predicate result is an exact boolean
  inside a definition; it authorizes nothing.
- **Derivation and lineage.** The result is `exact-derived` from its
  operands, and its value metadata is their combination (032 3.2.3), as
  for every exact predicate.
- **Unresolved, never a default.** A missing, stale, conflicting or
  invalid operand is unresolved; a malformed slug is `invalid_input`;
  neither is false (I-2).
- **No silent kind conversion.** Operands are `text`; nothing is
  converted to text, and the definition, not the runtime, owns case and
  Unicode handling.
- **Pure core.** Byte comparison only: no I/O, no clock, no locale table.

## 3. Behavior

### 3.1 The predicate

1. `rustev.exact/2` (032 3.1) gains the predicate:

   ```text
   predicate = ... | { under: { value: expr, ancestor: expr,
                                separator: ":" | "/" | "|" | "." } }
   ```

   For example:

   ```json
   { "under": { "value": { "ref": "input:location" },
                "ancestor": { "ref": "input:city" },
                "separator": ":" } }
   ```

2. `value` and `ancestor` are exact expressions of type `text` (a
   reference to a `text` input, step result, record field or item field,
   or a `text` literal). Any other type, including `enum` and
   `nullable{text}`, is refused at compilation: category 3
   `kind_mismatch`, phase e, naming the predicate's position.
3. `separator` is a literal from the closed set of `compose_key@1`'s
   separators (036 3.7.1): `:`, `/`, `|` or `.`, each one ASCII byte. Any
   other value is refused: category 1 `parse` (the document's shape).
   The separator is part of the plan and so of the `PlanId`.
4. `under` is accepted wherever a `rustev.exact/2` predicate is: in exact
   operator arguments that take a predicate, in policy conditions (002
   3.11.3), and in the checks of a corroboration step (032 3.7).

### 3.2 Semantics

1. **Segments.** A text is split into segments at every occurrence of the
   separator byte. A text is **valid segmented text** when it is non-empty
   and every segment is non-empty: it has no leading separator, no
   trailing separator and no two adjacent separators.
2. **Ancestry.** With both operands valid, `under` is true exactly when
   the ancestor has fewer segments than the value and each of the
   ancestor's segments equals the value's segment at the same position,
   byte for byte. Equivalently: the value's bytes begin with the
   ancestor's bytes followed immediately by the separator byte.
3. **Not under itself.** Equal operands give false (strict
   ancestry). "At or under" is written `any: [under, cmp eq]`.
4. **No normalization.** Bytes are compared as given: no case folding, no
   Unicode normalization, no trimming of whitespace, no percent or escape
   decoding. `US:NY` is not an ancestor of `us:ny:soho`. A definition that
   needs a normalized comparison normalizes upstream, for example with
   `compose_key@1`'s `ascii_token` (036 3.7.2).
5. Examples, separator `:`:

   | value | ancestor | result |
   |---|---|---|
   | `us:ny:new-york:soho` | `us:ny:new-york` | true |
   | `us:ny:new-york:soho` | `us` | true |
   | `us:ny:new-york-city` | `us:ny:new-york` | false |
   | `us:ny:new-york` | `us:ny:new-york` | false |
   | `us:ny` | `us:ny:new-york` | false |
   | `us:ny:new-york:soho` | `us:ny:new-york:` | `invalid_input` |
   | `us::soho` | `us` | `invalid_input` |
   | `` (empty) | `us` | `invalid_input` |

### 3.3 Unresolved and invalid operands

1. An operand that reads an unresolved value (missing, stale, conflicting
   or invalid input; an unresolved or skipped step, 036 3.8) makes the
   reading exact step unresolved with the first unresolved operand's
   reason in argument order (`value`, then `ancestor`), as spec 002
   3.10.4 states for every exact step. In a policy condition the value's
   handlers apply (002 3.11.1); in a corroboration check the check is
   `no_evidence` with the reason (032 3.7.3). The result is never false
   because an operand is unresolved (I-2).
2. A resolved operand that is not valid segmented text (3.2.1) makes the
   reading step `invalid_input`, naming the step's input fields and the
   operand (`under value` or `under ancestor`) in the detail. It is never
   false, never true, and nothing is trimmed or repaired.
3. **Static reasons.** For handler coverage (002 3.11.2), a step or
   condition reading `under` can be unresolved with every reason its
   operands can, plus `invalid_input`, whatever their types. A policy that
   reads such a value without a handler covering `invalid_input` is
   refused, category 8 `unhandled_unresolved`.
4. A constructed operand (036 3.7.4) makes the result constructed, as for
   every exact computation; `is_constructed` reads it unchanged.

### 3.4 Grades, parameters and builder

1. Grade propagation is 032 3.2.3 unchanged: the result's metadata is the
   combination of its operands' (literals contribute nothing), so a `hint`
   operand makes the result at most `hint`.
2. The positions of `under` are not parameter sites (034 3.3.3): `value`
   and `ancestor` are expressions whose references are never sites, and
   `separator` is a fixed literal.
3. The typed builder in `rustev-core` gains `under` and emits the same
   document the JSON parser accepts (002 3.9.2).

### 3.5 Compatibility

1. In a `rustev.definition/1`, `under` is refused at the start of phase
   b, category 11 `invalid_definition`, requirement
   `requires_definition_2`, as 032 3.1.3 refuses every registry 2
   construct. A `rustev.definition/2` using it compiles to `rustev.plan/3`,
   which records registry `rustev.exact/2`, as 036 3.10.3 added
   `compose_key@1` and `is_constructed` to the same registry version.
2. A plan that does not use `under` has the same bytes and `PlanId`; every
   committed definition, plan, judgment, evidence and run golden, including
   both reference plans and the package goldens, is byte-identical (R-1).
   A definition that changes from a host-projected parent with `member_ref`
   to `under` is a different plan with a different `PlanId`, as any
   definition change is.
3. The predicate is an added member of the definition and plan types, so
   the JSON Schemas of `rustev.definition/1` and `/2` and `rustev.plan/2`
   and `/3` regenerate with it (the enum is shared; version 1 refuses it
   in the core, as for `member_ref`), and the generated TypeScript
   declarations of both bindings gain one union member. Adding a member
   neither removes nor narrows an export (031 3.6.5), so the change is
   compatible for npm. The binding code and the conformance runner are
   unchanged; the WebAssembly module's added code is one byte comparison
   and stays within its uncompressed budget, or that budget's amendment of
   027 is its own reviewed change.
4. An earlier release refuses a definition or plan that uses `under`
   (unknown field, category 1), never reads it as another predicate.

## 4. Out of scope

A plain text prefix predicate; case-insensitive or
normalized comparison; multi-byte or definition-chosen separators outside
the closed set; extracting a parent, a depth or a segment as a value;
ancestry over lists or maps; spec 005's rules language; a new registry or
document version.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `under` in a `rustev.definition/1` | Category 11, `requires_definition_2`, phase b. |
| `value` of type `integer`, `enum` or `nullable{text}` | Category 3 `kind_mismatch`, phase e. |
| `separator: "-"` or `"::"` | Category 1 `parse`. |
| A policy reading a step that uses `under` with no handler covering `invalid_input` | Category 8 `unhandled_unresolved`. |
| `value` input missing from the snapshot | The reading step `missing_evidence{location}`; not false. |
| `ancestor` input stale | The reading step `stale_evidence`; not false. |
| `value` with a trailing separator, or two adjacent separators | `invalid_input`, naming `under value`. |
| `ancestor` empty | `invalid_input`, naming `under ancestor`. |
| `us:ny:new-york-city` under `us:ny:new-york` | false. |
| Equal operands | false. |
| `US:NY:SOHO` under `us:ny` | false (bytes differ). |
| A `hint` operand | Result at most `hint`. |
| Every existing definition and plan golden recompiled | Byte-identical, same `PlanId`. |

## Acceptance

- Contract: the predicate parses and round-trips in canonical form;
  unknown fields, duplicate keys and separators outside the set are
  refused; regenerated schemas and binding types match their sources
  (`crates/rustev-contract/tests/schemas.rs`, `gen-types --check`).
- Core: the builder emits the parser's bytes; each compilation refusal of
  section 5 with its category and phase; the examples of 3.2.5 and every
  row of section 5 evaluated, for each separator; unresolved operands in
  an exact step, a policy condition under `propagate` and `as_unmet`, and
  a corroboration check; grade propagation; handler coverage.
- A synthetic package-style golden: a definition 2 using `under` for the
  issue's location rule, with its plan, `PlanId` and judgments for under,
  not under, sibling-prefix, missing and malformed slugs.
- Compatibility: every committed definition, plan, judgment, evidence and
  run golden byte-identical with its `PlanId`.
- Mutation seeds, each detected: a byte prefix accepted without the
  separator check; an empty segment evaluated as false; equal operands
  true; an unresolved operand evaluated as false; ASCII case folded.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

## Decisions at approval

Decided in this text, with the reason beside each: a segment-aware
predicate rather than a byte prefix (section 1, the issue's
`new-york-city` case); the closed separator set of `compose_key@1`
(3.1.3), so a key built by 036 can be tested by this predicate; byte
comparison with no normalization (3.2.4, the issue); empty segments and
empty operands are `invalid_input` (3.2.1, 3.3.2); registry
`rustev.exact/2` with no new document version (3.5.1, 036's precedent).

Ratified by the owner on 2026-11-06 (A-85), with every question the
draft left open answered as recommended:

1. **A slug is not under itself.** Strict ancestry; "at or under" is
   `any: [under, cmp eq]` (3.2.3).
2. **No plain text prefix predicate** (section 4).
3. **The separator** is a literal from the closed set `:`, `/`, `|`,
   `.` (3.1.3).
4. **Operand types:** `text` only (3.1.2).

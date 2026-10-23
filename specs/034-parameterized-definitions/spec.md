---
id: "034-parameterized-definitions"
title: "Parameterized definitions, variants and tenant profiles (amends 002 and 006)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002 and 006 (R-16), for #76 child B4.
  A definition may declare, as optional members this spec adds to the
  #76 track version `rustev.definition/2` (introduced by spec 032), typed, bounded
  parameters with lock levels (`fixed`, `bounded`, `free`), reference them
  from a closed set of literal positions, declare named variants and scope
  inputs, steps, handlers, rules and adjustments to them. A versioned
  profile document (`rustev.profile/1`) binds parameter values, may extend
  one other profile, and names its definition by a version range.
  Instantiation, a pure phase before compilation, resolves the range
  against the supplied definitions, resolves the overlay chain, checks
  every lock and bound, substitutes values and prunes the variant, yielding
  a definition without parameterization that the compiler compiles into a
  `rustev.plan/3` (the #76 track version, introduced by spec 032, to which
  this spec adds the optional member `instantiation`) whose `PlanId` binds
  the template, the variant, the profile chain and every resolved value. Adds `rustev plan diff`
  (`rustev.plan-diff/1`) and `rustev profile check`. A
  `rustev.definition/1` compiled without a profile still yields a
  byte-identical `rustev.plan/2` and `PlanId`. Approved (A-31);
  implementation is in progress (see the implementation record).
amends:
  - "002-decision-contract-and-pure-core"
  - "006-cli-surface"
extends:
  # Optional members of `rustev.definition/2` and `rustev.plan/3` (the #76
  # track versions of spec 032), `rustev.profile/1`, `rustev.plan-diff/1`, the version range grammar and the plan schema
  # list accepted in replay bundles (3.11).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Instantiation, its refusals, plan loading for version 3, the plan
  # diff and the profile check report, all pure (3.8 to 3.13).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Bundle assembly accepts `rustev.plan/3` beside `rustev.plan/2` (3.11).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  # Profile flags on `plan check|compile`, `plan show` for version 3,
  # `plan diff` and `profile check` (3.14).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The new contract source files join the reviewed archive file list.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # The generated document declarations gain `rustev.profile/1`,
  # `rustev.plan-diff/1` and the plan's `instantiation` member.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  # Adds 034 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "033-provenance-rules-and-actionable-outcomes"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A `rustev.definition/1` compiled without a profile and without a variant yields a `rustev.plan/2` byte-identical to today's, with the same `PlanId`, and every existing CLI output for it keeps its bytes."
    anchor: "3-16-compatibility"
  - id: "I-2"
    kind: invariant
    text: "An instantiated `rustev.plan/3` records the exact template document and its identity, the variant, the identity of every profile in the resolved chain and every resolved parameter value with its source, so its `PlanId` never depends on a version range, a pin or which other definitions were supplied."
    anchor: "3-10-plan-version-3-and-identity"
  - id: "I-3"
    kind: invariant
    text: "A profile that sets a `fixed` parameter, sets a `bounded` parameter outside its bounds, sets an undeclared parameter or sets an ill-typed value is refused with the parameter named; no plan is produced."
    anchor: "3-7-overlay-resolution"
  - id: "I-4"
    kind: invariant
    text: "A profile carries values for declared parameters only: it cannot add or remove an input, step, operator, expression, handler, rule, adjustment or action name, and no parameter value is interpreted as code, an expression or a reference."
    anchor: "3-5-profiles"
  - id: "I-5"
    kind: invariant
    text: "Rustev never selects a profile or variant from a principal, tenant handle, scope or request content; the host names both for every compilation."
    anchor: "3-15-library-surface"
  - id: "R-1"
    kind: requirement
    text: "A profile's definition range resolves to the highest supplied definition version that satisfies it, or to the host's pin when the pin satisfies it; an unsatisfiable range or a pin outside the range is refused, listing the supplied versions."
    anchor: "3-6-definition-version-binding"
  - id: "R-2"
    kind: requirement
    text: "`rustev.plan-diff/1` is a deterministic function of the two plan documents, and `rustev plan diff` output is golden-tested."
    anchor: "3-12-plan-diff"
---

# 034: Parameterized definitions, variants and tenant profiles (amends 002 and 006)

Approved by the owner (A-31, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is in progress (see the implementation record). This spec implements #76 child B4 (#62). The approved texts of
specs 002 and 006 are not edited; this spec records what changes, as 029
does for 004 and 006. Approval authorizes no release or publication, no
live provider call or spend, no real-user data, and no change to the
bytes, meaning or identity of any existing document or plan.

## 1. Purpose

A host often serves several tenants or request modes with one pipeline
shape that differs in a few business rules: thresholds, tier order, which
filters relax and in what order, which constraints are protected, ranking
weights, text style parameters, screening posture, whether a clarifying
turn is allowed. Today each difference is a copied definition or code, and
nothing states which differences are intended. This spec lets one
definition serve many tenants and modes without code:

1. typed, bounded **parameters** with lock levels;
2. named **variants** that share steps and differ in a few;
3. versioned **profiles** that bind parameter values, overlay one another
   and bind to a definition version range;
4. **instantiation**: `compile(definition, profile, variant)` yields a plan
   whose `PlanId` binds all three; and
5. tooling that shows what a profile changes and what it cannot change.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): this spec's optional
  members of `rustev.definition/2` (3.1) and `rustev.plan/3` (3.10), both
  #76 track versions introduced by spec 032; the new documents
  `rustev.profile/1` (3.5) and `rustev.plan-diff/1` (3.12); the version
  and range grammar (3.6); and `rustev.plan/3` in the replay bundle's
  accepted plan schemas (3.11), unless spec 032 has already added it.
- `crates/rustev-core/` (owner spec 002): instantiation (3.8) and its
  refusals (3.9), loading version 3 plans (3.11), the plan diff (3.12) and
  the profile check report (3.13), as pure functions. The compiler phases
  of spec 002 3.9.4 are unchanged and run on the instantiated definition.
- `crates/rustev-eval/` (owner spec 004): bundle assembly and replay accept
  a version 3 plan wherever they accept a version 2 plan (3.11). No metric,
  report or gate changes.
- `crates/rustev-cli/` (owner spec 006): the flags and commands of 3.14.
- `Makefile` (owner spec 001): 034 joins `make verify` at completion.

No change to `rustev-runtime`, any backend, package or integration, and no
new dependency. Test data lives under `crates/rustev-core/tests/golden/` and
`crates/rustev-cli/tests/`. The Node and WebAssembly bindings (#76 A1, spec
027) and the service (#76 A2) expose the library functions of 3.15;
exposing them is those specs' work.

### 2.1 Binding principles of #76

- **Proposals, never grants.** Action names in `propose` outcomes are not
  parameterizable (3.3.3), so a profile cannot change which action a rule
  proposes; it can change only declared literal values. No document here is
  accepted by an executor, and no `Permitted` type is defined.
- **Derivation classes and lineage.** A parameter value is an exact literal
  fixed at compile time, like any literal of spec 002; values computed from
  it keep spec 002 3.12's derivation rules. Its origin is recorded in the
  plan (the parameter, its source profile and every substitution site,
  3.10), so evidence that names the `PlanId` names it.
- **Unresolved, never a default; no silent kind conversion.** A parameter
  default is a declared plan constant, never a stand-in for evidence: no
  parameter can be placed where an input, a snapshot value or a step value
  is read (3.3.3), and missing, stale, conflicting or invalid evidence is
  unresolved exactly as today. A parameter value is checked against its
  declared type and the type of every site; nothing is coerced. Parameters
  are exact literals, so R-14 holds: a policy control never becomes a
  semantic value.
- **Pure core; no async runtime in contract and core.** Instantiation, the
  diff and the profile check are pure functions over supplied bytes: no
  I/O, no clock, no `unsafe`.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and still passes.
- **No dynamic plugin loading** (D-06). A profile is data: values for
  parameters the definition declares, never code, expressions, steps or
  operators (3.5.4).
- **No identity, authority or knowledge store.** Selecting which profile and
  variant serve a request is the host's decision; it owns tenant identity.
  Rustev never maps a principal or tenant to a profile (3.15.3), and keeps
  no profile store: profiles are documents the host supplies.
- **Plan identities.** A `rustev.definition/1` compiled without a profile
  or variant keeps its byte-identical `rustev.plan/2` and `PlanId` (3.16).

## 3. Behavior

### 3.1 Definition version 2

1. This spec adds to `rustev.definition/2` (introduced by spec 032, the
   #76 track version; spec 032 fixes its bounds, `DefinitionId` and
   canonical form) these optional members, each omitted when unused: the
   top-level `parameters` (3.2) and `variants` (3.4), the item members
   `only_in` and `enabled` (3.4), and parameter references in the positions
   of 3.3. A definition that uses any of them is a version 2 document; one
   that uses none of them is unaffected by this spec. When this spec's
   text says "template of version 2" it means a version 2 document, with
   or without other #76 constructs.
2. `rustev.definition/1` is unchanged and stays current. A version 1
   document is accepted wherever a template is (3.8) and behaves as a
   version 2 document with no parameters, no variants and no item scopes.
3. Bounds: at most 256 parameters, 16 variants, and 4 096 parameter
   reference sites in one definition. A parameter or variant name matches
   `[a-z][a-z0-9_]{0,63}` and is unique among its kind.

### 3.2 Parameter declarations

1. A declaration is `{ name, type, lock, bounds, default }`:

   ```text
   type   = "bool" | "integer" | "decimal" | "timestamp" | "duration_ms"
          | { "text": { max_bytes } } | { "enum": { options } }
          | { "list": { item: type, max_items } }
          | { "set": { item: enum-type, max_items } }
          | { "record": { fields: [ { name, type } ] } }
   lock   = "fixed" | "bounded" | "free"
   bounds = "none" | { "range": [min, max] } | { "one_of": [value] }
          | { "list": { item: bounds, min_items, max_items, unique_items } }
          | { "set": { min_items, max_items, must_include: [value] } }
          | { "record": { fields: [ { name, bounds } ] } }
   default = "none" | { "value": value }
   ```

   `text.max_bytes` is 1 to 65 536; `max_items` is 1 to 4 096; enum options
   are 1 to 256 unique strings; record fields are 1 to 64 with unique names.
2. **Values.** A value is JSON of its type: a JSON boolean; an integer
   (`i64`) for `integer`, `timestamp` (spec 002 3.4.4 range) and
   `duration_ms` (non-negative); a Decimal string (spec 002 3.4.1) for
   `decimal`; a string for `text` (at most `max_bytes` UTF-8 bytes) and
   `enum` (one of `options`); an array for `list` (order significant) and
   `set` (items unique, written in declared option order in canonical
   form); an object with exactly the declared fields for `record`.
   Nothing is coerced: `"20"` is not an integer and `20` is not a decimal.
3. **Bounds forms by type.** `range` for `integer`, `timestamp` and
   `duration_ms` (integers) and `decimal` (Decimal strings), inclusive,
   `min <= max`; `one_of` for `enum` and `text`, a non-empty list of unique
   well-typed values; `list` for `list` (`item` bounds may be `"none"`;
   `min_items <= max_items <= type max_items`; `unique_items` a boolean,
   default `false`, which when `true` refuses a value with two equal items);
   `set` for `set`
   (`must_include` items must be options and at most `max_items`); `record`
   for `record`, one entry per declared field in declared order, each
   `"none"` or a bounds form for that field's type. `bool` has no bounds
   form. A bounds form for another type is refused.
4. **Lock levels.**
   - `fixed`: no profile may set it. `default` is required and `bounds` is
     `"none"`.
   - `bounded`: a profile may set it to a value within `bounds`. `bounds` is
     required, except for a type whose every leaf is `enum` (an enum, or a
     list or set of an enum), where `"none"` means the declared options are
     the bound. `bool` cannot be `bounded`.
   - `free`: a profile may set any value of the type. `bounds` is `"none"`.
5. A `default`, when present, must be well-typed and within `bounds`. A
   `bounded` or `free` parameter without a default must receive a value
   from the profile chain (3.7.4).
6. A violation of 3.2.1 to 3.2.5 is the refusal `invalid-parameterization`
   naming the parameter (3.9).

### 3.3 Parameter references

1. A **reference** is the JSON object `{ "param": "<name>" }` with exactly
   that one member, naming a declared parameter. It may appear only at the
   sites of 3.3.2; anywhere else it is a parse refusal (category 1) of the
   definition, as any unknown shape is today.
2. **Sites.** Each site states the parameter types it admits and how the
   value is written into the instantiated definition (3.8.3):

   | Site | Admitted parameter types | Written as |
   |---|---|---|
   | An `Expr` in exact arguments or policy predicates (in place of `ref` or `lit`) | `bool`, `integer`, `decimal`, `text`, `timestamp`, `duration_ms`, `enum` | `{ "lit": <literal> }` of the matching literal kind |
   | A proposal parameter's `value` (in place of `ref` or `lit`) | as above | `{ "lit": <literal> }` |
   | `member.set` (the whole list) | `set` of a scalar above, or `list` of one that either declares bounds with `unique_items: true` or has lock level `fixed` (3.2.4) with a default holding no two equal items; any other `list` is `invalid-parameterization` (3.3.4) | the list of literals, in value order |
   | `mass_at_least.threshold`, `top_mass_below.threshold`, `expectation_at_least.value` | `decimal` | the Decimal string |
   | A semantic step's `backend.min_determinism` | `enum` whose options are a subset of `unspecified`, `tolerance`, `bitwise` | the determinism name |
   | A semantic step's `backend.artifact` | `text` | `{ "pinned": <value> }` |
   | `limits.max_semantic_requests`, `limits.max_projection_bytes`, `limits.deadline_ms` | `integer`, `duration_ms` (for `deadline_ms` only) | the integer |
   | `limits.distribution_tolerance` | `decimal` | the Decimal string |
   | An adjustment's `scale` (the whole list) | `list` of `text` or `enum` | the list of strings |
   | An item's `enabled` (3.4.3) | `bool` | nothing: the member is stripped on emit (3.8.4); the value only decides whether the item is kept or pruned |
   | Any JSON value inside an exact step's `args` | any type | the value's JSON (3.2.2) |

3. Not parameterizable: names and ids of inputs, steps, rules, handlers and
   adjustments; references to inputs, steps and items (`ref` strings);
   operator names and versions; semantic operations, tasks, questions,
   options, candidates, projections and `for_each`; required kinds,
   calibration ids and fallbacks; `backend.binding`; handler targets,
   reasons and actions; rule outcomes' kinds and action names; input
   declarations; the definition's name, version and package. Later specs
   that add a literal position (for example tier lists of #76 B1 or
   generation template ids of #76 B5) state whether it is a site and which
   types it admits; without that statement it is not a site. Spec 032
   and the other #76 specs that add constructs to `rustev.definition/2`
   declare no sites today, so their positions are not sites.
4. **Static compatibility.** Every value the declaration admits (its type
   and bounds) must be valid at every site that references it: for example
   a limit site needs an integer whose bounds or fixed default exclude
   negative values, so a `free` integer cannot stand at a limit site. This
   is checked once per reference, before any profile is read; a failure is
   `invalid-parameterization` naming the parameter and the site's JSON
   Pointer (RFC 6901) in the template.
5. Inside exact `args`, an object whose only member is `param` with a string
   value is always a reference. No operator of `rustev.exact/1` takes an
   argument object of that shape, and a later operator version must not
   define one.

### 3.4 Variants and optional items

1. `variants` is a list of 0 to 16 unique names in declared order. An
   input, step, handler, rule or adjustment may carry `only_in`: a
   non-empty list of declared variant names, meaning the item exists only
   in those variants. An item without `only_in` exists in every variant. A
   name not declared is `invalid-parameterization`.
2. A definition that declares variants is instantiated for exactly one of
   them; a definition that declares none is instantiated with variant
   `none`.
3. A step, handler, rule or adjustment may carry `enabled`: `true`,
   `false`, or a reference to a `bool` parameter. An item whose `enabled`
   resolves to `false` is removed as if out of variant. This is how a
   profile turns an optional step on or off.
4. **Pruning.** For the selected variant and resolved values: items out of
   variant or disabled are removed; then every handler whose target was
   removed is removed. Any remaining reference to a removed input or step
   (in an expression, predicate, projection, `for_each`, item source,
   condition or outcome) is the refusal `dangling-reference`: `subject` is the
   referencing item (3.9), and `detail` is
   `removed <item> in variant <name>` (the variant `none` when the
   definition declares no variants). Pruning never
   rewrites a reference or a condition.
5. Each variant is instantiated and compiled independently; the compiler's
   rules (unique ids, the last rule `always`, handler coverage) apply to
   each instantiated definition.

### 3.5 Profiles

1. `rustev.profile/1`, parsed under `DESCRIPTOR_V1`, canonical (no binary64:
   decimals are strings), unknown fields and duplicate keys refused,
   identified by `ProfileId`, the identity of spec 002 3.3.2 with tag
   `rustev.profile/1`:

   ```text
   profile = { schema: "rustev.profile/1",
               profile: name, version: "MAJOR.MINOR.PATCH",
               definition: { name: string, version: range },
               extends: "none" | "<name>@<MAJOR.MINOR.PATCH>",
               values: { <parameter name>: value } }
   ```

2. A profile name matches `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`; `version`
   follows spec 002's definition version rule; `values` has at most 256
   members. Profile names are opaque labels chosen by the host: Rustev does
   not compare them with any scope, tenant or principal handle.
3. `values` holds values only, typed per 3.2.2. A value is never evaluated:
   a string is text even if it looks like a reference or an expression, and
   an object such as `{ "param": "y" }` is a record value checked against
   the declared record type (and otherwise `parameter-type`), never a
   parameter reference.
4. A profile cannot add, remove or alter an input, step, operator,
   expression, handler, rule, adjustment, variant or action name; its only
   effect is the values it binds, within the locks and bounds the
   definition declares.

### 3.6 Definition version binding

1. **Version.** `MAJOR.MINOR.PATCH` with no leading zeros, ordered by
   major, then minor, then patch, numerically. No pre-release or build
   suffix.
2. **Range.** One to four comparators separated by single spaces; a
   comparator is `>=`, `>`, `<`, `<=` or `=` followed by a version, or a
   bare version meaning `=`. A version satisfies the range when it
   satisfies every comparator. `>=2.0.0 <3.0.0` and `1.4.0` are ranges;
   `^2`, `~2.1`, `*`, `||` and wildcards are refused (`invalid-profile`).
3. **Resolution.** The host supplies one to 64 definition documents
   (version 1 or 2). The candidates are those whose `name` equals the
   profile's `definition.name`. Two candidates with the same version and
   different `DefinitionId`s are `definition-ambiguous`. Without a pin, the
   highest satisfying version is selected. With a host pin (an exact
   version), that version is selected when it satisfies the range and is
   supplied. A pin outside the range is `definition-pin-outside-range`
   (whether or not that version is supplied); a pin inside the range whose
   version is not supplied is `definition-unsatisfied`. No satisfying candidate is
   `definition-unsatisfied`. Every refusal lists the supplied candidates'
   versions in ascending order.
4. Every profile of the chain (3.7) must admit the selected version by its
   own range and name; otherwise `definition-unsatisfied` naming that
   profile.
5. The plan records the selected template and its identity (3.10); the
   range and the pin select, and enter no identity, so the same selection
   reached with or without a pin is the same plan.
6. Without a profile, the host supplies exactly one definition and no range
   applies.

### 3.7 Overlay resolution

1. The host names one profile (the leaf) and supplies 0 to 64 further
   profiles available to `extends`. The chain is built from the leaf by
   following `extends` to the profile with exactly that name and version.
   A target not supplied is `profile-missing`; two supplied profiles with
   one name and version and different `ProfileId`s are
   `profile-duplicate`; a profile reached twice is `profile-cycle`, listing
   the chain; a chain longer than 8 is `profile-chain-too-deep`.
2. Resolution is root first: each parameter starts at its default (or
   unbound); each profile from the root to the leaf replaces the whole
   value of each parameter it sets. There is no deep merge of lists, sets
   or records: a profile that sets a record sets every field.
3. Every profile of the chain is checked, root first and within a profile
   by parameter declaration order: a member naming an undeclared parameter
   is `parameter-unknown`; setting a `fixed` parameter is
   `parameter-locked`, even when the value equals the default; an ill-typed
   value is `parameter-type`; a value outside `bounds` is
   `parameter-out-of-bounds`, naming the parameter, the profile, the value
   and the bound violated.
4. After resolution, a parameter with neither a default nor a value is
   `parameter-unbound`, naming it.
5. The result records, per parameter in declaration order, its value and
   its source: `default` or the `name@version` of the last profile that
   set it.

### 3.8 Instantiation

1. Instantiation is a pure function of: the supplied definitions, the leaf
   profile (or none), the supplied base profiles, the variant (or none) and
   the pin (or none). It runs before spec 002's phase (a), in this order,
   stopping at the first refusal: (1) parse and select the template
   (3.6); (2) check the template's declarations and references (3.2, 3.3.4,
   3.4.1); (3) parse and resolve the profile chain (3.7); (4) check the
   variant: a definition with variants and no variant named is
   `variant-required`, a name not declared (or any name when none is
   declared) is `variant-unknown`; (5) substitute (3.8.3); (6) prune
   (3.4.4); (7) emit. After spec 002's compiler accepts the emitted
   definition, writing the version 3 plan is stage (8), plan emission
   (3.10.4).
2. Within a stage, items are visited in document order, so the first
   refusal is deterministic. Each variant is its own instantiation:
   with no variant named, `profile check` runs stages 5 and 6 once per
   variant (3.13.3) and `plan check` instantiates and compiles every variant
   (3.14.1); both continue past a refused variant.
3. **Substitution** replaces each reference with its parameter's resolved
   value written as 3.3.2 states. Each parameter's **sites** are recorded
   as JSON Pointers (RFC 6901) into the template (`instantiation.template`,
   3.10.1), in document order: every reference that takes effect, which is
   each reference whose value is written into the instantiated definition
   and each `enabled` reference (whether its item was kept or removed). A
   reference inside an item removed by pruning, other than that item's own
   `enabled`, has no effect and is not recorded. Every recorded pointer
   therefore resolves in the embedded template. A site need not resolve in
   the instantiated definition: the `enabled` site of a removed item, and
   every `enabled` site (the member is stripped on emit), point at
   template content only. No check may require a site to correspond to an
   item of the instantiated definition.
4. **Emit.** The instantiated definition has the template's name, version
   and package, the surviving items in template order, and none of this
   spec's members (`parameters`, `variants`, `only_in`, `enabled`, or a
   reference). It is a `rustev.definition/2` document carrying whatever
   other #76 constructs the template uses (spec 032 3.1); it is written as
   version 2 even when it carries none, because it is only ever embedded
   in a `rustev.plan/3`, whose `definition` member is a version 2
   definition (spec 032 3.1.1). The rule that refuses a version 2
   definition using no #76 construct applies to supplied templates, never
   to an instantiated definition. Its canonical bytes must fit
   `DEFINITION_V1`; otherwise `instantiation-bound`. It then goes through
   spec 002's compiler, as the other #76 specs extend it, with no change
   made by this spec; a compile refusal is reported as today, plus the parameters
   whose sites lie inside the refused subject (3.14.2).
5. Instantiation reads no clock and no environment, and its output is a
   function of its inputs' bytes only.

### 3.9 Instantiation refusals

A refusal is `{ stage: "instantiate", code, subject, detail }`, a type
separate from spec 002's compile `Refusal`, so no compile category number
changes or is added. `subject` is `definition:<name>@<version>`,
`profile:<name>@<version>`, `parameter:<name>`, `variant:<name>` or the
item (`step:<id>`, `rule:<id>`, `input:<name>`, `handler:<index>`,
`adjustment:<id>`). The closed set of codes:

| Code | Stage | Refused when |
|---|---|---|
| `invalid-definition-document` | 1 | a supplied definition fails parsing (its category 1 detail is kept) |
| `definition-ambiguous` | 1 | two candidates share a version with different ids |
| `definition-unsatisfied` | 1, 3 | no candidate satisfies the range, or a chain profile's range or name does not admit the selected version |
| `definition-pin-outside-range` | 1 | the pin does not satisfy the range |
| `invalid-parameterization` | 2 | a declaration, bound, default, reference, site or `only_in` breaks 3.2 to 3.4 |
| `invalid-profile` | 3 | a profile fails parsing, its name, version or range grammar |
| `profile-missing`, `profile-duplicate`, `profile-cycle`, `profile-chain-too-deep` | 3 | 3.7.1 |
| `parameter-unknown`, `parameter-locked`, `parameter-type`, `parameter-out-of-bounds` | 3 | 3.7.3 |
| `parameter-unbound` | 3 | 3.7.4 |
| `variant-required`, `variant-unknown` | 4 | 3.8.1 |
| `dangling-reference` | 6 | 3.4.4 |
| `instantiation-bound` | 7, 8 | the instantiated definition exceeds `DEFINITION_V1` (stage 7, 3.8.4), or the compiled version 3 plan would exceed `PLAN_V1` (stage 8, 3.10.4); in both cases `subject` is `definition:<name>@<version>` of the selected template and `detail` names the bound and the byte count |

### 3.10 Plan version 3 and identity

1. A compilation that goes through instantiation with a template of
   version 2, or with any profile or variant, emits `rustev.plan/3`, the
   #76 track version introduced by spec 032 (which fixes its other members,
   bounds and registry). This spec adds to it one optional member,
   `instantiation`, omitted when the plan was not instantiated; a version 3
   plan of another #76 spec that uses none of this spec's constructs has
   no `instantiation` member and is unaffected. In an instantiated plan,
   `definition` and `definition_id` are the instantiated definition (3.8.4)
   and its identity.

   ```text
   instantiation = {
     template_schema: "rustev.definition/1" | "rustev.definition/2",
     template: <the template's canonical document>,
     template_id: DefinitionId,
     variant: "none" | { "named": name },
     profile: "none" | { "chain": [ { profile, version, id: ProfileId } ] },
     parameters: [ { name, value, source: "default" | { "profile": "<name>@<version>" },
                     sites: [ JSON Pointer ] } ] }
   ```

   `chain` is root first; `parameters` lists every declared parameter in
   declaration order, with its resolved value in canonical form (3.2.2)
   and its sites as pointers into `template` (3.8.3).
2. **Identity.** `PlanId` is the identity of the version 3 document (spec
   032), so for an instantiated plan it
   binds the template and its exact version and digest, the variant, every
   profile's identity in the chain, every resolved value and its source,
   every site, the bindings, the execution policy and the compiler, as
   `PlanId = digest(definition, variant, resolved profile, bindings, ...)`
   in #62 requires. The range, the pin, and the other definitions or
   profiles supplied are not recorded and do not affect it.
3. Consequences: two profiles that resolve to the same values but differ
   in identity yield different `PlanId`s (the source is evidence); the same
   template, profiles, variant and bindings yield the same `PlanId` on
   every compilation.
4. Plan documents of version 3 are parsed under `PLAN_V1`; a version 3 plan
   whose canonical bytes would exceed it is not emitted
   (`instantiation-bound`, stage 8, 3.9).
5. **Version 2 kept.** A `rustev.definition/1` compiled with no profile and
   no variant goes through the existing path and emits `rustev.plan/2`,
   byte for byte as today (3.16).

### 3.11 Loading, replay and evaluation

1. `Compiled::load` and `load_checked` accept both versions. For version 3
   they first re-instantiate: the embedded template must parse and match
   `template_id`; its declarations must pass stage 2; every recorded value
   must be well-typed and within bounds, and a `fixed` parameter must hold
   its default with source `default`; substituting the recorded values for
   the recorded variant must reproduce the embedded instantiated definition
   byte for byte and the recorded sites exactly, each site resolving in
   the embedded template (3.8.3). Then spec 002's recompile and
   equality check applies to the instantiated definition. A failure is
   `plan_mismatch` with the reason. Loading needs no profile document: the
   chain identities are a record, and the values are authoritative.
2. The runtime (spec 003) and evaluation (spec 004) take plans through
   `Compiled` and are otherwise unchanged. Replay bundles and their
   assembly accept `rustev.plan/3` beside `rustev.plan/2` as the plan item
   (spec 032 may already add this; this spec requires it for instantiated
   plans either way).
3. **Evaluation keying.** Spec 029's repeat reports, baselines and gates
   bind the plan id; a version 3 `PlanId` binds definition, variant and
   profile, so every baseline is keyed by that triple without a change to
   any spec 029 document. A profile change is a candidate plan like any
   other (spec 029 3.11), and offline candidate replay (spec 004 3.4)
   applies when the candidate's projections and descriptors allow reuse of
   raw outputs; otherwise its cases are incomparable, as today.

### 3.12 Plan diff

1. `rustev.plan-diff/1` (canonical, no binary64, parsed under `RECORD_V1`,
   the class spec 002 assigns to records and reports; `DESCRIPTOR_V1`, at
   256 KiB, cannot hold a diff of two plans of up to 2 MiB each)
   compares two plan documents of either version. It is a pure function of
   the two documents: they are parsed and identified, not recompiled.
2. Contents, each list in the stated order:
   - `left` and `right`: `plan_id`, plan schema, `definition_id`,
     `template_id` (or `none`), variant and profile chain (or `none`);
     `identical` (equal `PlanId`s);
   - `parameters`: every parameter declared on either side whose value,
     source or site list differs, by name in byte order, with both sides'
     value and source (`absent` when undeclared);
   - `inputs`, `steps`, `rules`, `adjustments`: `added`, `removed` and
     `changed` by name or id in byte order; handlers keyed by target and
     position among that target's handlers; a changed item lists the JSON
     Pointers, relative to the item, of differing leaves (at most 64, with
     `truncated`);
   - `order` (topological order equal or both orders), `rule_order` (rule
     ids in order, when it differs);
   - `bindings`: per semantic step present on both sides, differing
     `backend_id`, artifact, descriptor, output kind, calibration or
     capability fallback; `execution`: differing execution policy ids and
     runtime fallbacks;
   - `limits`: differing fields; `notices`: added and removed;
     `compiler`, `registry`: both values when they differ.
3. Identical plans give `identical: true` and every list empty.

### 3.13 Profile check

1. A pure report over the same inputs as instantiation (the profile and
   variant optional): the selected definition (name, version,
   `DefinitionId`), every supplied candidate version with whether it
   satisfies the range, the chain with identities, and per parameter in
   declaration order: name, type, lock, bounds, default, resolved value,
   source, `changeable` (`bounded` or `free`), and `changed` (the resolved
   value differs from the default, or there is no default).
2. It also lists `locked` (every `fixed` parameter) and `changes` (every
   parameter the chain sets), so the report states what the profile may
   change, what it does change and what it cannot change.
3. The report is produced only when stages 1 to 3 of 3.8.1 succeed and,
   when a variant is named, stage 4 succeeds; otherwise the instantiation
   refusal is returned. When variants are declared and none is named,
   stage 4 is not run (no `variant-required`) and the report adds
   `variants`: per variant, in declared order, `{name, pruned}`, where
   `pruned` lists the inputs, steps, handlers, rules and adjustments
   instantiation would prune for it, computed by running stages 5
   (substitute) and 6 (prune) of 3.8.1 for that variant. Each variant is
   analysed even when an earlier one is refused. Nothing is compiled, so the report has
   no `plan_id`. When pruning a variant is refused (for example
   `dangling-reference` after a profile switches an item off), that
   variant's entry is `{name, refused: {stage, code, subject, detail}}`
   instead, the command prints `refused` (exit 5) with the full report, and
   `first_refused` names the first such variant in declared order.

### 3.14 CLI surface (amends spec 006)

Grammar, bounded file access, no-clobber writes and stdout rules of spec
006 3.1 and 3.3 apply unchanged. Profiles are read under `DESCRIPTOR_V1`. A
command with a subcommand path is matched before its parent.

1. **`rustev plan check | compile`** gain `--profile <file>`,
   `--profile-base <file>` (repeatable, at most 64), `--variant <name>` and
   `--definition-version <X.Y.Z>`; with `--profile`, `--definition` becomes
   repeatable (at most 64). Usage errors (exit 2, before any file is read):
   `--profile-base`, `--definition-version` or a second `--definition`
   without `--profile`. Without any new flag and with a version 1
   definition, both commands behave and print exactly as today. Otherwise:
   - `check` with a definition declaring variants and no `--variant`
     instantiates and compiles every variant in declared order and prints
     `ok` only when all succeed, with `variants`: per variant, in declared
     order, `{name, result: "ok", plan_id, pruned}` where `pruned` lists the
     inputs, steps, handlers, rules and adjustments pruned from it.
     Otherwise it prints `refused` (exit 5) with the same list, where a
     refused variant's entry is `{name, result: "refused", stage, code,
     subject, detail}` (no `plan_id` and no `pruned` member), and
     `first_refused` names the first refusing variant in declared order.
   - `check` and `compile` otherwise print `ok` or `compiled` with
     `plan_id`, `plan_schema`, the selected definition version and id, the
     variant and the chain; `compile` writes the plan (version 2 or 3, per
     3.10) to `--out`. `compile` with variants declared and no `--variant`
     is `refused` with `variant-required`; nothing is written.
2. **Refusals.** An instantiation refusal prints `refused` (exit 5) with
   `stage: "instantiate"`, `code`, `subject` and `detail`. A compile
   refusal of an instantiated definition prints today's members plus
   `stage: "compile"` and `parameters`, the parameters with a site inside
   the refused subject. A compile refusal of a version 1 definition without
   new flags keeps today's bytes (no `stage` member).
3. **`rustev plan show`** on a version 3 plan prints today's members plus
   `plan_schema` and `instantiation` (template id and version, variant,
   chain and every parameter with value, source and site count). On a
   version 2 plan its bytes are unchanged.
4. **`rustev plan diff`**, two modes, mixing them a usage error:
   `--left-plan <file> --right-plan <file>`; or `--definition` (repeatable)
   with any of `--left-profile`, `--right-profile`, `--left-variant`,
   `--right-variant` (at least one), `--profile-base`,
   `--definition-version` and the dependency flags of `plan compile`, which
   instantiates and compiles both sides (an absent side profile means
   defaults) and diffs the results. Plan-mode flags are `--left-plan` and
   `--right-plan`; definition-mode flags are `--definition` and every other
   flag listed here except `--out`, so `--profile-base` or
   `--definition-version` with `--left-plan` is check (a) below. Both modes
   require `--out <file>`,
   write `rustev.plan-diff/1` there and print `diffed` with both plan ids,
   `identical` and the count of each list. A side that cannot be read is
   `io_error`, that cannot be parsed `invalid_input` (plan mode), or that
   is refused `refused` (exit 5) naming the side; nothing is written.
   Usage errors (exit 2, before any file is read), each an independent
   check, in this order: (a) flags of both modes, or of neither (no
   `--left-plan`, `--right-plan` or `--definition`); (b) in plan mode, only
   one of `--left-plan` and `--right-plan`; (c) in definition mode, none of
   `--left-profile`, `--right-profile`, `--left-variant` and
   `--right-variant`; (d) in either mode, no `--out`.
5. **`rustev profile check`**: `--definition` (repeatable), optional
   `--profile`, `--profile-base`, `--variant` and `--definition-version`.
   Prints `ok` with the report of 3.13, or `refused` (exit 5) with the
   instantiation refusal (or, with no variant named, the report of 3.13.3). It compiles nothing and writes nothing.
6. **Exit codes.** No code is added and none changes meaning. `diffed` is
   a status of exit code 0 (spec 029 already uses it for `eval diff`).
   Instantiation refusals are compile-stage refusals of a plan's inputs and
   take exit code 5 with spec 006 3.2's other refusals.

### 3.15 Library surface

1. `rustev-core` exposes: `instantiate(definitions, profile, bases,
   variant, pin)` returning the instantiated definition with its
   instantiation record or an instantiation refusal; `compile_profiled` and
   `compile_profiled_with` (instantiate then `compile` / `compile_with`,
   returning either refusal type); `plan_diff(left, right)`; and
   `profile_check(...)`. `compile`, `compile_with` and `compile_bytes` keep
   their signatures and behavior.
2. The builder (R-02) gains parameter, variant, `only_in`, `enabled` and
   reference constructors; the builder and JSON parser produce the same
   `rustev.definition/2` document through one validation, and goldens are
   emitted by the builder.
3. No function takes a principal, tenant, scope handle or request to pick a
   profile or variant; the host names both. The Node and WebAssembly
   bindings and the service expose these functions without adding such a
   mapping.

### 3.16 Compatibility

1. `rustev.definition/1`, `rustev.plan/2` and every other existing document
   keep their schemas, bytes and identities. A version 1 definition
   compiled with no profile and no variant emits a byte-identical
   `rustev.plan/2` with the same `PlanId`, and every existing CLI command
   on today's inputs prints the same bytes and exit codes.
2. Under spec 031 3.6, `rustev.profile/1` and `rustev.plan-diff/1` are
   new documents, and `rustev.definition/2` and `rustev.plan/3` are the #76
   track versions spec 032 introduces beside the current ones; this spec
   adds optional members to them: no support window starts and no
   existing version is superseded. Under spec 031 3.7, a release that ships this spec
   states that no existing `PlanId` changed, proven by the goldens.
3. **Shared #76 versions.** Under the #76 coordination rule, the track
   shares one new version per document instead of each spec claiming its
   own: `rustev.definition/2` and `rustev.plan/3` are introduced by spec
   032, the lowest-numbered of these specs, and this spec only adds the
   optional members of 3.1 and 3.10, omitted when unused (spec 032
   3.1.2). A `rustev.definition/1` that uses no #76 construct, and
   is compiled with no profile and no variant, keeps `rustev.plan/2` in
   every case.
4. Spec 002 3.9.4's phases, 3.9.5's categories 1 to 11 and spec 003's
   category 12 are unchanged; instantiation refusals have their own type.

## 4. Out of scope

Choosing a profile or variant for a request (the host's decision);
storing or distributing profiles; profile-level variants or per-variant
values; deep merging of structured values; definition version ranges
with pre-release, caret, tilde, wildcard or alternative syntax; selecting
among definitions of different names; parameterizing names, ids,
references, operators, operations, projections, fallbacks or action names
(3.3.3); computing a parameter from evidence or a model (R-14); exposing
these functions through Node, WebAssembly or the service (#76 A1, A2);
changes to spec 029's documents; any release, publication, live provider
call or spend.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A profile setting a `fixed` parameter, even to its default | `parameter-locked`, naming the parameter and profile; exit 5. |
| A profile setting `pool_target` 101 against `range [5, 100]` | `parameter-out-of-bounds`, naming parameter, profile, value and bound. |
| A profile value `"20"` for an integer parameter | `parameter-type`. |
| A profile setting an undeclared parameter | `parameter-unknown`. |
| A `bounded` parameter with no default and no profile value | `parameter-unbound`. |
| A set value omitting a `must_include` member | `parameter-out-of-bounds`. |
| `bounded` on a decimal with `bounds: "none"`, or on a `bool` | `invalid-parameterization`. |
| A `free` integer referenced at `limits.max_semantic_requests` | `invalid-parameterization` with the site's pointer. |
| A reference `{ "param": ... }` in a step id or an action name | Category 1 parse refusal of the definition. |
| `extends` naming a profile not supplied | `profile-missing`. |
| Profiles A extends B, B extends A | `profile-cycle`, listing the chain. |
| A chain of nine profiles | `profile-chain-too-deep`. |
| Range `>=2.0.0 <3.0.0` with 1.9.0, 2.1.0 and 2.4.1 supplied | 2.4.1 selected and recorded. |
| The same with pin 2.1.0 | 2.1.0 selected; `PlanId` equals compiling with only 2.1.0 supplied. |
| Pin 3.0.0 | `definition-pin-outside-range`, listing 1.9.0, 2.1.0, 2.4.1. |
| Range `>=5.0.0` | `definition-unsatisfied`, listing the supplied versions. |
| Range `^2.0.0` | `invalid-profile`. |
| Two supplied 2.4.1 definitions with different ids | `definition-ambiguous`. |
| A base profile whose range excludes the selected version | `definition-unsatisfied` naming the base. |
| `plan compile` on a definition with variants, no `--variant` | `refused`, `variant-required`; nothing written. |
| `plan check` on a definition with variants `a` and `b`, no `--variant`, where `b` is refused | `refused` (exit 5); `a`'s entry has `plan_id` and `pruned`, `b`'s entry has `stage`, `code`, `subject` and `detail` and no `plan_id`; `first_refused` is `b`. |
| `profile check` on a definition with variants, no variant named | The report covers every variant; no `variant-required`. |
| `profile check`, no variant named, where the profile switches off an item that variant `b` still references | `refused` (exit 5); `b`'s entry carries `dangling-reference`; `first_refused` is `b`. |
| A `list` parameter without `unique_items: true` referenced at `member.set` | `invalid-parameterization` naming the site. |
| A pin `2.5.0` with range `>=2.0.0 <3.0.0` and only `2.4.0` supplied | `definition-unsatisfied`. |
| A pin `3.0.0` with range `>=2.0.0 <3.0.0`, `3.0.0` supplied | `definition-pin-outside-range`. |
| `plan diff --definition d.json --out o.json` with no side profile or variant flag | Usage error, exit 2; nothing read. |
| `plan diff --left-plan a.json --right-plan b.json` with no `--out` | Usage error, exit 2; nothing read. |
| A plan whose `enabled` parameter removed a handler, loaded with `load_checked` | Loads; the handler's `enabled` site resolves in the embedded template only. |
| `--variant batch` on a definition declaring none | `variant-unknown`. |
| A rule reading a step scoped `only_in: ["interactive"]`, compiled for `batch` | `dangling-reference` naming rule, step and variant. |
| An optional step disabled by a profile, with a handler targeting it | Handler pruned; plan compiles. |
| A threshold parameter that makes a compile check fail | Compile refusal with `stage: "compile"` and that parameter in `parameters`. |
| `--profile-base` or a second `--definition` without `--profile` | Usage error, exit 2, nothing read. |
| `--left-plan` with `--left-profile` | Usage error, exit 2. |
| A version 3 plan whose recorded `fixed` value differs from its default | `load_checked` refuses with `plan_mismatch`. |
| A version 3 plan whose instantiated definition was edited | `plan_mismatch`. |
| Two profiles with equal values and different identities | Different `PlanId`s. |
| A profile value that is the text `"step:x"` or `{ "param": "y" }` for a text parameter | Taken as the literal text, or `parameter-type`; never a reference. |
| A version 1 definition with no new flags | Byte-identical stdout, plan bytes and `PlanId`. |

## Acceptance

- One synthetic definition, two profiles and two variants compile to four
  distinct `PlanId`s, stable across repeated compilation and between the
  builder and the parsed JSON; the template, profiles, plans and `PlanId`s
  are committed goldens under `crates/rustev-core/tests/golden/`.
- A profile setting a `fixed` parameter, and a `bounded` one out of range,
  are refused with the parameter named; each code of 3.9 has a test that
  triggers it and a passing neighbour that differs only in the refused
  property.
- `rustev plan diff` output (stdout and `rustev.plan-diff/1`) is
  golden-tested in both modes, including identical plans, a changed
  parameter, a variant-pruned step, a changed binding and truncation of
  changed leaves.
- `rustev profile check` output is golden-tested for a profile changing two
  parameters, a definition with variants, and a refusal.
- Partial adoption: the committed goldens of both reference plans, every
  package plan golden, and every existing CLI golden are byte-identical,
  with unchanged `PlanId`s.
- A range compiles against the highest satisfying supplied definition and
  records it; a pin selects within the range and yields the same `PlanId`
  as compiling with only that version supplied; an unsatisfiable range is
  refused.
- A version 3 plan loads, replays through a bundle, and is refused when its
  template, instantiated definition, values or sites are altered.
- Every row of section 5 has a test.
- Seeds in the core and CLI mutation harnesses (a lock not checked on a
  base profile, an inclusive bound made exclusive, highest version replaced
  by first supplied, the pin entered into identity, a dangling reference
  pruned silently, a handler kept for a pruned target, a value source
  dropped from the plan) are each detected.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it.

**Increment 1 (contract).** `rustev-contract` carries the documents and
shapes of 3.2, 3.5, 3.6, 3.10 and 3.12. `parameter` holds the declaration
shapes core reads out of a template (`ParameterDecl`, `ParamType`, `Lock`,
`Bounds`, `ParamDefault`, the reference `ParamRef` and `Enabled`) and the
bounds of 3.1.3 and 3.2.1; `Definition` itself is unchanged, because the
instantiated definition carries none of this spec's members (3.8.4) and a
template is handled by core as canonical JSON. `version` is the version and
range grammar of 3.6.1 and 3.6.2. `profile` is `rustev.profile/1` under
`DESCRIPTOR_V1`, identified by `ProfileId`, refusing a bad name, version,
range, `extends` form or more than 256 values. `Plan` gains the optional
`instantiation` member (3.10.1), omitted when absent, so every existing
plan keeps its bytes and `PlanId`. `plan_diff` is `rustev.plan-diff/1`
under `RECORD_V1` with fractional numbers refused (3.12.1). A replay
bundle's plan item may be `rustev.plan/3` (3.11.2). The schemas and the
generated Node and WebAssembly declarations are regenerated
(`crates/rustev-contract/tests/parameterized_wire.rs`).

**Increment 2 (instantiation analysis).** `rustev-core` gains
`instantiate`: stages 1 to 4 of 3.8 over a template held as canonical JSON.
`declarations` checks the parameter declarations of 3.1 and 3.2, `sites`
finds every reference site and refuses one outside the permitted kinds,
`template` selects the base definition by the version and range rules of
3.6 (pins, ambiguity, a range that excludes the selected version, `extends`
chains with cycles and the depth limit), `overlay` resolves the profile
chain, `check_variant` the variant, and `values` checks each value against its type,
lock and bounds. Refusals are `InstantiationRefusal` with the string codes
of 3.9, never compile categories. `tests/instantiate_analysis.rs`
covers every code reachable by stages 1 to 4 with a passing neighbour, the
section 5 rows those stages decide, and a version 1 definition with no
profile and no variant analysing to no parameters and variant `none`. The
codes of stages 5 to 8 (`dangling-reference`, `instantiation-bound`) are the
next increment. No I/O, clock or `unsafe` is added to core.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #62 leaves the points below open, and each is
decided by the implementer within this spec, as written:

1. **Instantiation before compilation.** Parameters and variants are
   resolved into a definition without parameterization (a version 2
   document carrying only the other #76 constructs it uses), so the
   compiler, its refusal categories and identities need nothing from this
   spec, and every check applies to each variant's instantiated
   definition.
2. **Separate refusal type.** Instantiation refusals carry string codes,
   not new compile category numbers, so they cannot collide with categories
   other #76 specs may add, and existing refusal output keeps its bytes.
3. **Shared versions, used only when needed.** Plans that use no #76
   construct stay `rustev.plan/2`; parameterized or profiled plans are
   `rustev.plan/3`, the shared #76 version introduced by spec 032, to which
   this spec adds optional members only.
   This is how #62's "unchanged `PlanId`s for unparameterized definitions"
   is met by construction, not by a compatibility mode.
4. **What enters identity.** The template, variant, chain identities,
   values, sources and sites; not the range, pin or other supplied
   documents, so `PlanId` never floats with what the host happened to
   supply.
5. **Overlay semantics.** Whole-value replacement, root first, every
   profile in the chain checked against locks and bounds; a `fixed`
   parameter cannot be set even to its default, so locks are visible in
   profile review rather than tolerated.
6. **Bounded enums.** For enum-leaf types, `bounded` without explicit
   bounds means the declared options, as #62 says ("within declared bounds
   or enum"); a `one_of` or set bound narrows further.
7. **Sites are a closed list.** #62 says "any place in a definition that
   takes a literal"; this spec lists those places (3.3.2), excludes names,
   references, operations and action names (3.3.3) to keep R-14 and
   proposals-never-grants, and requires later specs (B1 tier lists, B5
   template ids) to declare their own sites.
8. **Optional steps.** `enabled` on items, bound to a `bool` parameter;
   pruning never rewrites references, and only handlers of removed items
   are pruned with them.
9. **Range grammar.** Comparator sets only, which covers #62's
   `>=2.0.0 <3.0.0` and exact pins; definitions have no pre-release
   versions.
10. **Selecting among definitions** by `name` only, as #62's profile
    example does; a name collision across packages is
    `definition-ambiguous` only when versions also collide, and a host
    that supplies two packages' definitions of one name is choosing them.
11. **Evaluation keying** through `PlanId` (3.11.3); no change to spec 029's
    documents.
12. **Loading without profiles.** A plan is self-contained: values are
    authoritative and re-validated against the embedded template; profile
    documents are not needed to load, show or replay it.
13. **Plan and definition versions.** Not this spec's own: it adds
    optional members to the #76 track versions introduced by spec 032
    (3.16.3). The instantiated definition is a version 2 document, and
    sites point into the embedded template, so every recorded pointer
    resolves, including `enabled` sites whose members are not emitted.
14. **Binding equivalents.** The library functions of 3.15; #76 A1 and A2
    expose them.

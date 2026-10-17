---
id: "032-evidence-quality"
title: "Evidence quality: grades, presence predicates, typed joins, corroboration and conflict policy (amends 002 and 003)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved specs 002 and 003 (R-16), for #76 child B2.
  Adds an ordered evidence grade (`authoritative`, `cached`, `estimated`,
  `hint`) and value metadata (grade, provenance set, sources, oldest
  as-of) to every value of a plan that opts in, declared per field by a
  new context source descriptor (`rustev.source/1`) and lowered, never
  raised, by the definition; presence predicates so key existence is
  never presence; typed keyed joins (`join_keyed@1`) and per-field merges
  (`merge_fields@1`) with no implicit fallback; a corroboration step, the
  only way a grade rises; and a per-fact conflict policy that makes silent
  last-writer-wins impossible. Introduces the shared #76 track versions
  `rustev.definition/2`, `rustev.plan/3`, `rustev.snapshot/2`,
  `rustev.evidence/2`, `rustev.run/3` and the registry `rustev.exact/2`,
  to which later #76 specs add optional members; every version 1
  definition keeps its bytes, its plan and its `PlanId`. Approved (A-29);
  implementation is pending.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
extends:
  # Grade, value metadata, presence, map and nullable types, the source
  # descriptor, definition 2, plan 3, snapshot 2, evidence 2, the new
  # reason and refusal categories (spec 002, 3.2 to 3.12).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Grade propagation, presence evaluation, the new operators and steps,
  # the grade compilation phase and the conflict policy.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Evidence 2 in the run record, and capture refusal for plan 3
  # decisions (spec 003, 3.9).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Synthetic evidence-quality fixtures and goldens beside, not inside,
  # the package's reference definition.
  - { spec: "008-reference-package-lodging", unit: { kind: directory, path: "packages/rustev-pkg-lodging/" }, nature: additive }
  # Adds 032 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "008-reference-package-lodging"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every rustev.definition/1 document compiles to the same rustev.plan/2 bytes and PlanId as before this spec, evaluates to the same judgment and rustev.evidence/1 bytes, and no existing refusal changes category or order."
    anchor: "3-12-compatibility"
  - id: "I-2"
    kind: invariant
    text: "A value's grade is never higher than the minimum of its source ceiling, its definition cap, its snapshot claim and the grades of the graded values it was computed from, except through a corroboration step whose supporting checks read values of at least the upgraded grade."
    anchor: "3-2-evidence-grades-and-value-metadata"
  - id: "I-3"
    kind: invariant
    text: "A map key, list item or record that fails its presence predicate is absent, and reading an absent value is missing_evidence; key existence alone is never presence."
    anchor: "3-4-presence-predicates"
  - id: "I-4"
    kind: invariant
    text: "merge_fields@1 never takes a base field the field's rule does not name; a field its rule cannot fill is absent, never a fallback value."
    anchor: "3-6-typed-merge-merge-fields-1"
  - id: "R-1"
    kind: requirement
    text: "The compiler refuses a hard gate, or a grade requirement, that reads a value whose static grade ceiling is below the required grade, with category 13 grade_requirement, or category 14 uncorroborated_hint when the ceiling is hint and no corroboration step raises it."
    anchor: "3-9-grade-compilation-phase"
  - id: "R-2"
    kind: requirement
    text: "Every disagreement between accepted snapshot entries and every corroboration contradiction is resolved only by a declared rule and recorded in rustev.evidence/2 with each value's digest, source and grade."
    anchor: "3-8-conflict-policy"
---

# 032: Evidence quality: grades, presence predicates, typed joins, corroboration and conflict policy (amends 002 and 003)

Approved by the owner (A-29, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child B2 (#60). The approved texts of
specs 002 and 003 are not edited; this spec records what changes, as 003
does for 002. Spec 033 (#76 B3, provenance-conditional rules and actionable
outcomes) builds on the grade and value metadata defined in 3.2.

## 1. Purpose

Provenance says who supplied a value; it does not say how far the value can
be trusted for a given use. Three failures recur where catalogue data meets
live third-party services: a map entry exists but carries no evidence and
is treated as present, so a cached figure stands in for a missing live one;
a third-party label declared unreliable gates a filter as if it were true;
and when two sources disagree, whichever is read last wins with no record.
Spec 002 makes missing, stale and conflicting evidence `Unresolved` for
whole inputs. This spec carries the same rule into the shape of evidence
inside records, maps, joins and merges, and adds a grade that says what a
value may be used for.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): `Grade`, `ValueMeta`,
  `Presence`, the `map` and `nullable` types, the source descriptor
  `rustev.source/1` (identified, `SourceId`), and the documents
  `rustev.definition/2`, `rustev.plan/3`, `rustev.snapshot/2`,
  `rustev.evidence/2` and `rustev.run/3`; the reason kind
  `insufficient_grade`; refusal categories 13 to 15.
- `crates/rustev-core/` (owner spec 002): the registry `rustev.exact/2`
  (`join_keyed@1`, `merge_fields@1`, the `member_ref` predicate, graded
  references), the `corroborate` step, presence evaluation, grade
  propagation, the conflict policy, the grade compilation phase and the
  typed builder for all of them.
- `crates/rustev-runtime/` (owner spec 003): delivering `rustev.run/3`,
  and the preparation and capture refusals of 3.11.
- `packages/rustev-pkg-lodging/` (owner spec 008): the synthetic fixtures
  and goldens of the Acceptance section, under
  `tests/evidence_quality/`. The package's reference definition, its plan
  and its `PlanId` are unchanged.
- `Makefile` (owner spec 001): 032 joins `make verify` at completion.

No new dependency in any crate. Semantic backends, the remote protocols,
the CLI, evaluation and replay are unchanged (section 4).

### 2.1 Binding principles of #76

- **Proposals, never grants.** A grade states evidence quality, not
  permission. `authoritative` means "attested live by the system that owns
  the fact", never "authorized"; no grade, corroboration outcome or
  resolution is accepted by an executor, and no `Permitted` type is added.
- **Derivation classes and lineage.** Grade is orthogonal to derivation
  class: no grade change, corroboration or merge changes a value's
  derivation class or lineage, so a corroborated `model-derived` value stays
  `model-derived`. Lineage gains the per-field and per-item metadata of 3.2
  and the resolution records of 3.7 and 3.8.
- **Unresolved, never a default; no silent kind conversion.** Absent
  values are `missing_evidence`, unmet grade requirements are
  `insufficient_grade` or `stale_evidence`, contradictions are `conflict`
  or a declared resolution; nothing substitutes a value. Grade is metadata,
  not a value kind: no kind converts, and no operator's result changes
  meaning with grade.
- **Pure core; no async runtime in contract and core.** All evaluation is
  in `rustev-core`, which reads `now` as an input and performs no I/O.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and passes.
- **No dynamic plugin loading.** Source descriptors are documents bound at
  compile time; nothing is loaded as code.
- **No identity, authority or knowledge store.** A source descriptor
  states what the host says a source can attest; Rustev stores nothing and
  verifies no source.
- **Plan identities.** Version 1 definitions keep byte-identical plans and
  `PlanId`s (3.12); plans using the new constructs are `rustev.plan/3`.

## 3. Behavior

### 3.1 Documents and versions

1. **#76 track versions.** This spec introduces the document versions the
   #76 decision-model specs share, each a superset of its predecessor:
   - `rustev.definition/2` (`DEFINITION_V1` bounds, `DefinitionId`):
     version 1 plus the optional input members `evidence` (3.3.3) and
     `on_disagreement` (3.8), the types of 3.4, the step kind `corroborate`
     (3.7), and the operators, predicates and references of
     `rustev.exact/2`.
   - `rustev.plan/3` (`PLAN_V1`, `PlanId`): version 2 with a version 2
     embedded definition, plus the optional members `sources` (3.3.4) and
     `grades` (the static grade table, 3.9.3).
   - `rustev.snapshot/2` (`SNAPSHOT_V1`, `SnapshotId`): version 1 entries
     plus the optional entry member `grade` (3.3.5).
   - `rustev.evidence/2` (`RECORD_V1`): 3.10.
   - `rustev.run/3` (`RECORD_V1`, not identified): 3.10.3.
   - `rustev.source/1` (`DESCRIPTOR_V1`, `SourceId`): 3.3.1.
   - Registry `rustev.exact/2`: every operator, expression and predicate of
     `rustev.exact/1` with unchanged meaning, plus 3.5, 3.6, 3.7.2 and
     3.9.1.
2. **Later specs add to these versions.** A later #76 spec adds optional
   members and constructs to the same versions (stated as "adds to
   `<version>` (introduced by spec 032)") and never defines a further
   version of one of these documents for a separate feature. For these
   versions only, spec 002 3.3.1 is amended: an optional member is omitted
   from canonical bytes when unused, never written as `null` or empty, so a
   document using only this spec's constructs keeps its bytes and identity
   when a later spec adds members. Members this spec marks optional follow
   the same rule; every other member is always written.
3. **Constructs and selection.** The constructs of this spec are the
   optional members and step kind above, the types of 3.4, the operators
   `join_keyed@1` and `merge_fields@1`, the predicate `member_ref` and
   graded references (3.9.1). A version 1 definition compiles exactly as
   before to `rustev.plan/2`. In a version 1 definition, an operator not in
   `rustev.exact/1` is refused as today, category 2 `unknown_operator`
   (phase c); every other construct of this spec is refused, category 11
   `invalid_definition` (phase b), requirement `requires_definition_2`. A
   version 2 definition compiles only to `rustev.plan/3`; one that uses no
   #76 construct at all is refused, category 11, requirement
   `use_definition_1`, so each content has one canonical document and one
   identity. A plan records the lowest registry version containing every
   operator, predicate and reference form it uses.
4. `Evaluation::start` accepts a `rustev.snapshot/1` or `/2` for a plan 3,
   and only `rustev.snapshot/1` for a plan 2. A snapshot 2 supplied to a
   plan 2 is refused by `Evaluation::start` with the existing core error
   `StartError::Schema` (spec 002 3.10.1), detail `snapshot_version`, so a
   grade claim is never silently dropped. It is not a compile category. In
   the runtime it surfaces, as every `StartError` does today, as a
   rejection before admission (`Rejection::InvalidRequest`, spec 003
   3.4.1): no run record, nothing dispatched.
5. The value digest of a value is `sha256:` followed by 64 lowercase hex
   digits of SHA-256 over `rustev.value/1 || 0x00 || canonical bytes` of
   the decoded value (spec 002, 3.3.1). Evidence uses it wherever this
   spec says "digest".

### 3.2 Evidence grades and value metadata

1. **Grade** is closed and ordered, strongest first: `authoritative` (live,
   attested by the system that owns the fact), `cached` (was authoritative
   at its as-of time and has not been revalidated), `estimated` (computed
   or crawled approximation that never was authoritative: an indicative
   rate, a model estimate, an interpolation), `hint` (a label or
   classification from a source declared unreliable for that field, or a
   constructed key not yet corroborated). `min` and `at least` use this
   order. `cached` and `estimated` stay distinct because a cached value can
   be refreshed to authoritative (#76 F1) and an estimated one cannot.
2. **Value metadata** (`ValueMeta`) is carried by every input value and
   every step value of a plan 3 evaluation:
   - `grade`: a grade, or `ungraded` when no graded value contributed;
   - `provenance`: the sorted set of provenance classes contributing;
   - `sources`: the sorted set of source names contributing;
   - `as_of_ms`: the oldest as-of among contributing graded values, or
     `none`;
   - `basis`: `declared` (an input), `computed` (an exact step or the
     policy), `corroborated{step}`, `merged{step}` or `resolved{input}`.
   A record value additionally carries one `ValueMeta` per field, and a
   list or map value one per item or entry. The metadata of a composite is
   the **combination** of its parts: minimum grade over graded parts
   (`ungraded` if none), union of provenance and sources, oldest as-of.
3. **Propagation.** An exact step's value metadata is the combination of
   every value it reads (literals and `now` contribute nothing); a step
   reading a `hint` is at most `hint`. `ungraded` reads do not participate,
   so mixing with ungraded values never raises a grade. A semantic step
   whose projection reads at least one graded value has the grade
   `min(estimated, g)`, where `g` is the combined grade of its graded
   reads; a backend output is never better than `estimated`. A semantic
   step whose projection reads only ungraded values (or none) is
   `ungraded`, exactly as an exact step would be: `estimated` is never
   assigned to it, so an ungraded projection cannot acquire a grade that
   satisfies a requirement. `ungraded` is not a point of the grade order;
   it is never an operand of `min` or `max`. Only a corroboration step
   (3.7) raises a grade.
4. Spec 033 (#76 B3) reads `grade`, `provenance` and `sources` of
   `ValueMeta` for provenance-conditional rules. Their meaning is fixed
   here; a later spec may add members to `ValueMeta` only through a new
   document version.

### 3.3 Sources, ceilings and claims

1. **Source descriptor** `rustev.source/1`, identified as `SourceId`:
   `name` (1 to 64 bytes of `[a-z0-9_-]`), `version` (`MAJOR.MINOR.PATCH`),
   and the optional member `facts`, a list of `{name, provenance, grades}`
   where `name` is a definition input name, `provenance` one
   provenance class, and `grades` a non-empty list of `{path, max_grade,
   reliability}`. `reliability` is `reliable` or `known_unreliable{note}`
   (note 1 to 256 bytes). A path is `""` (the whole value) or segments
   joined by `/`: a record field name, or `*` for every list item or map
   value. Each fact declares `""` exactly once and no path twice. A field
   marked `known_unreliable` has a ceiling of at most `hint`, whatever its
   `max_grade`. Later #76 specs add optional members (spec 036, #76 B1,
   adds acquisition operations whose item fields declare a grade in this
   vocabulary) under 3.1.2.
2. **Ceiling** of a path: the minimum of `max_grade` (lowered to `hint`
   when unreliable) over every declaration on that path and its ancestors.
   A deeper declaration can only lower.
3. **Definition declaration.** A version 2 input may declare the optional
   member `evidence`, `graded{sources, caps}`, with `sources` a non-empty,
   sorted, duplicate-free list of source names, and `caps` a list of `{path,
   max_grade}` that lowers the ceiling of a path and its descendants. A
   definition can lower a grade, never raise it. An input without
   `evidence` is ungraded.
4. **Binding.** `compile` takes source descriptors beside backend
   descriptors and calibrations. Phase (c) binds every source a graded
   input names; plan 3 records per bound source its `name`, `version` and
   `SourceId` in `sources`, in name order, and the ceilings it uses in
   `grades`. `Compiled::load` recompiles with the supplied descriptors, as
   for backends. Refused, category 15 `source_binding`, naming the input: a
   named source not supplied, two supplied descriptors with one name, a
   descriptor
   with no fact for the input, a fact whose `provenance` the input does not
   accept, or a declared or capped path that does not exist in the input's
   type. An unbound supplied descriptor changes nothing, as for backends.
5. **Claims.** A snapshot 2 entry may carry the optional member `grade`:
   the grade the supplier
   observed (for example `cached` when it served from its own cache). For a
   graded input the effective grade of each path is the minimum of its
   ceiling, its definition cap and the entry's claim; an entry without a
   claim (every snapshot 1 entry) claims nothing, so the ceiling and cap
   decide. A claim above the
   ceiling is not an error; evidence records claim, ceiling and effective
   grade. For an ungraded input a snapshot 2 claim is recorded and the
   value is `ungraded`.
6. **Entry admission** for a graded input, before spec 002's checks of
   3.7.3: an entry whose `source` (compared with descriptor `name`) is not one
   of the input's sources is
   ignored and recorded `unbound_source`; an entry whose provenance differs
   from its source's fact provenance makes the input `invalid_input`,
   detail `source-provenance-mismatch`. Spec 002's ordering then applies,
   with disagreement handled by 3.8.

### 3.4 Presence predicates

1. Version 2 types add `nullable{type}`, allowed only as a record field
   type, which accepts JSON `null` or a missing key (stored as `null`), and
   `map{key_max_bytes, value, max_entries}`, a JSON object with text keys
   of at most `key_max_bytes` bytes and at most `max_entries` entries
   (at most 10 000).
2. A record type may declare `present_when`, a predicate of one of four
   forms: `{any_non_null: [fields]}`, true when at least one named field is
   non-null; `{all_non_null: [fields]}`, true when every named field is
   non-null; `{all: [predicates]}`, true when every sub-predicate is true;
   `{any: [predicates]}`, true when at least one is. Each list is non-empty;
   `all` and `any` take predicates of any of the four forms, nested at most
   4 deep with at most 32 leaf predicates in total (category 11 otherwise).
   Every field named anywhere in the predicate must be a `nullable` field
   of that record (category 3 otherwise). The canonical predicate is the
   declared tree with each field list sorted and deduplicated.
   A record without `present_when` is present when it decodes.
3. A record value that fails its predicate has presence
   `absent{reason: predicate_failed, predicate}`, where `predicate` is the
   canonical predicate; it is never a present record with empty fields.
   A map entry whose value fails is absent, and so is a list item. Key
   existence alone is never presence.
4. Reading an absent value or a `null` field is `missing_evidence` naming
   the path (`<input>/<field>` or `step:<id>/<field>`). An input whose top
   value is absent is `missing_evidence{fields: [input]}`, and evidence
   records the presence status. In `rustev.exact/2`, `filter_with_reasons@1`
   and `top_k@1` exclude, instead of reporting `unevaluable:<rule>`, an item
   that is itself absent (it fails its own presence predicate, or is a join
   entry `matched_absent`) with the reason `predicate_failed`, and an item
   whose rule reads an absent value or a `null` field with the reason
   `missing:<path>`; and `field_presence@1` lists an input whose value is
   absent. Every other operator reading an absent item is
   `missing_evidence` for that path. A version 1 plan cannot contain a
   predicate, so none of this is observable there.
5. **Exclusion reasons.** In `rustev.exact/2` an exclusion reason is
   exactly one of: a rule name; `unevaluable:<rule>` (spec 002);
   `unmatched` (3.5.4); `predicate_failed`; or `missing:` followed by a
   path. A reason is parsed by these rules in order: equal to `unmatched`
   or `predicate_failed`; starts with `unevaluable:` or `missing:` (the
   remainder is the rule or path); otherwise a rule name. So the forms
   never collide, a version 2 `filter_with_reasons@1` rule named
   `unmatched` or `predicate_failed`, or containing `:`, is refused,
   category 11, requirement `reserved_reason`. A version 1 plan produces
   only rule names and `unevaluable:<rule>`, unchanged.

### 3.5 Typed keyed join: `join_keyed@1`

1. Arguments: `left` (a reference to a list of records, or a list of ids),
   `key` (`id` for a list of ids, or an item field of type `text` or
   `enum`), and `right` (a reference to a `map`). The left list's
   `max_items` bounds the result.
2. Result, in left order, one entry per left item: `{id, match}` where
   `match` is `matched{value}` (an entry exists and is present),
   `unmatched` (no entry), or `matched_absent{reason, predicate}` (an
   entry exists and fails its presence predicate). Each `matched` value
   keeps its own `ValueMeta`; the entry's id is the left item's id.
3. A left key that is `null` or absent makes that entry `unmatched` with
   the key path recorded. Duplicate left ids are `invalid_input`, as in
   `filter_with_reasons@1`.
4. `filter_with_reasons@1` accepts a join result as its items: before its
   rules it excludes `unmatched` items with reason `unmatched` and
   `matched_absent` items with `predicate_failed` (3.4.5), counted per
   reason, and its rules read a matched value as `item:match/<field>`.
   This is the meaning of `filter_with_reasons@1` in `rustev.exact/2` for
   an input kind version 1 cannot produce.

### 3.6 Typed merge: `merge_fields@1`

1. Arguments: `base` and `overlay`, either two record references, or a
   list of records and a join result over that same list (merged per item
   by id), and `rules`: one entry `{field, rule}` per output field, where
   `rule` is `overlay_only`, `base_only`, or
   `overlay_else_base_if_grade_at_least{grade}`. A field with no rule, a
   rule naming a field twice, or a rule naming a field missing from a side it
   may read is refused, category 11 (`overlay_only` reads the overlay,
   `base_only` the base, and `overlay_else_base_if_grade_at_least` both, so
   its field must exist on both sides); differing field types between the two
   sides of a rule are refused, category 3. There is no default rule.
2. Per field: `overlay_only` takes the overlay field if present, otherwise
   the field is absent with reason `overlay_missing`; `base_only` takes the
   base field if present, otherwise absent `base_missing`;
   `overlay_else_base_if_grade_at_least{g}` takes the overlay field if
   present; otherwise the base field if it is present with a grade at least
   `g`; otherwise, when the base field is present with a lower grade, the
   field is absent `overlay_missing_base_below_grade`; and when the base
   field is not present either, absent `overlay_and_base_missing`. An
   `unmatched` or `matched_absent` overlay item, and a base record or item
   that fails its presence predicate, has no present fields, so a wholly
   absent base yields `base_missing` or `overlay_and_base_missing`, never a
   grade reason.
3. Each merged field keeps its own `ValueMeta` with basis `merged{step}`
   and the side it came from; the merged record's metadata is the
   combination of its fields. A consumer can see that `price` is
   `authoritative` from the overlay and `description` `cached` from the
   base. The output record may declare `present_when` over its fields
   (3.4.2), evaluated after merging.

### 3.7 Corroboration

1. A version 2 step `corroborate{claim, upgrade_to, by, mode,
   on_contradiction}`: `claim` references an input path or step value;
   `upgrade_to` is a grade above `hint`; `by` is a non-empty list of
   checks `{predicate, on_false}` with `on_false` in `contradicts`,
   `no_evidence`; `mode` is `all` or `any`; `on_contradiction` is
   `conflict`, `treat_as_hint` or `abstain`.
2. `rustev.exact/2` adds the predicate `member_ref{value, set}`, true when
   the value is a member of the list or the keys of the map `set`
   references (version 1 `member` takes literals only).
3. Each check is evaluated: true is `supports`; false is its `on_false`; a
   check that reads an unresolved or absent value, or a value whose grade
   is below `upgrade_to`, is `no_evidence` with that reason recorded.
4. Outcome: `contradicted` if any check contradicts; otherwise
   `corroborated` if every check supports (`all`) or at least one does
   (`any`); otherwise `uncorroborated`.
5. Value: `corroborated` yields the claim with grade
   `max(claim grade, upgrade_to)` and basis `corroborated{step}`;
   `uncorroborated` yields the claim unchanged (it keeps its pre-step grade and
   basis, so a `hint` claim stays `hint` and may be used only where a hint
   is allowed); `contradicted` yields, per
   `on_contradiction`, `conflict{facts}` naming the claim path and every
   contradicting check's reads, the claim at grade `hint`, or
   `abstained{rule: <step id>}`. Provenance, sources and derivation class
   never change.
6. Evidence records per corroboration step the claim's digest and grade
   before and after, the outcome, and per check its result, reason and the
   digests and grades of the values it read. An unresolved claim makes the
   step unresolved with the claim's reason.

### 3.8 Conflict policy

1. A graded input may declare the optional member `on_disagreement`
   (omitted means `require_agreement`), applied when accepted,
   admitted entries for one input have differing values (canonical
   comparison): `require_agreement` (the input is `conflict{facts:
   [input]}`, spec 002's behavior), `prefer{sources}` (the entry from the
   first listed source that supplied one is used), `treat_as_hint{sources}`
   (as `prefer`, with the used value's grade lowered to `hint`), or
   `abstain` (the input is `abstained{rule: "disagreement:<input>"}`). The
   sources listed by `prefer` or `treat_as_hint` must be a permutation of
   the input's `evidence.sources`: a source listed twice, a source the
   input does not name, or a named source left out is refused, category 11
   `invalid_definition` (phase b), requirement `disagreement_sources`,
   naming the input and the source. `on_disagreement` on an ungraded input
   is refused, category 11, requirement `disagreement_on_ungraded`.
   Ungraded inputs keep spec 002's `conflict`.
2. Two differing entries from one source are `conflict` under every rule:
   a preference cannot choose within a source.
3. There is no recency or order rule: entry order in the snapshot never
   chooses a value, so silent last-writer-wins cannot be expressed.
4. Every disagreement, under every rule, is recorded in evidence 2: each
   entry's value digest, source, provenance, as-of and effective grade, the
   rule, and which entry was used or that none was. Freshness (spec 002,
   3.7.3) then applies to the used entry.

### 3.9 Grade compilation phase

1. **Requirements.** In `rustev.exact/2` a reference may be written
   `{ref, require: {min, max_age_ms}}` with `max_age_ms` a duration or
   `not_required`. At evaluation the read is unresolved
   `insufficient_grade{fields}` when the value's grade is below `min` (an
   `ungraded` value never meets a requirement), and `stale_evidence{fields}`
   when its grade is `cached` or `estimated` and `now - as_of_ms` exceeds
   `max_age_ms` (equal is fresh). Input freshness (spec 002) still applies
   to every value.
2. **Hard gates** are the predicates of `count_where@1`,
   `window_filter@1` and `filter_with_reasons@1`, the `compare@1` and
   `member_of@1` steps, and every policy condition other than `always`. A
   graded read in a hard gate without an explicit `require` has the
   implicit requirement `{min: estimated, max_age_ms: not_required}`; an
   explicit `min` of `hint` in a hard gate is refused, category 11. #76 B1
   applies the same rule to protected constraints.
3. **Static grades.** A new phase (e2), after types (e) and before backend binding (f),
   computes for every input path and step value its static ceiling (the
   highest grade it can have) from ceilings, caps, propagation and
   corroboration, and records it in plan 3 `grades`. A requirement or hard
   gate whose read's ceiling is below its `min` is refused: category 14
   `uncorroborated_hint` when the ceiling is `hint` and no corroboration
   step lies between the read and the input, otherwise category 13
   `grade_requirement`. Both name the step or policy item, the read, the
   ceiling and the required grade. A `corroborate` step whose `upgrade_to`
   exceeds the ceiling of any value a check reads is refused, category 13.
4. **Reasons.** `insufficient_grade{fields}` joins the closed `Unresolved`
   set (spec 002, 3.5.6). Phase (h) treats it like every reason: a policy
   reading a value that can be unresolved with it must handle it
   (category 8). Version 1 plans can produce none of the new reasons.
5. **Refusal ordering.** The phases run (a), (b), (c), (d), (e), (e2), (f),
   (g), (h), (i); the earliest phase wins, as in spec 002 3.9.4.

| Category | Phase | Refused when |
|---|---|---|
| 13 `grade_requirement` | e2 | a requirement or hard gate reads a value whose static ceiling is below the required grade (not `hint`), or a corroboration upgrades beyond what its checks can read |
| 14 `uncorroborated_hint` | e2 | a requirement or hard gate reads a value whose ceiling is `hint`, with no corroboration step raising it |
| 15 `source_binding` | c | 3.3.4 |

### 3.10 Evidence (amends spec 003, 3.9)

1. A plan 3 evaluation produces `rustev.evidence/2`: version 1 plus these
   optional members (3.1.2), per input, its `ValueMeta`, presence status, and for graded inputs each
   entry considered (source, provenance, claim, ceiling, effective grade,
   value digest, as-of, and disposition `used`, `outvoted`, `ignored` with
   reason, or `rejected` with reason) and the 3.8 resolution; per step, its
   `ValueMeta`; for `join_keyed@1` each entry's id and match status; for
   `merge_fields@1` each field's side, rule and grade; and the corroboration
   records of 3.7.6. Values appear only as digests.
2. A plan 2 evaluation produces `rustev.evidence/1`, unchanged.
3. The run record of a plan 3 decision is `rustev.run/3`: `rustev.run/1`
   with its core member holding `rustev.evidence/2`; later #76 specs add
   optional members under 3.1.2. A plan 2 decision still produces
   `rustev.run/1` (or spec 020's `rustev.run/2`), byte-identical.
4. Bounds: evidence 2 stays within `RECORD_V1`. Per-item and per-field
   records are bounded by the plan's declared `max_items`, `max_entries`
   and record field counts, which phase (g) checks against `RECORD_V1`
   (category 7, detail `evidence-bound`).

### 3.11 Runtime (amends spec 003)

1. The runtime runs plan 3 like plan 2: grades and presence are computed in
   the core, and semantic projections carry values only, never metadata,
   so no backend or adapter changes.
2. **Preparation.** The runtime refuses to prepare a plan 3 unless its
   evidence sink declares that it accepts `rustev.run/3` (a new
   `EvidenceSink` capability whose default is `rustev.run/1` only). This is
   a `PrepareError` (spec 003 3.6.5), named `sink_version_unsupported`, not
   a compile category: no decision is admitted and nothing is dispatched.
3. **Capture.** Spec 004's replay bundles admit plan 2 and snapshot 1
   items only. Until an amendment of spec 004 admits plan 3 and snapshot 2
   items in `rustev.replay/2` (spec 036, #76 B1, amends 004), a plan 3
   decision submitted through `decide_with_capture` is refused as an
   invalid capture configuration (spec 004 section 5, item 4), a `CaptureConfigError`
   named `capture_unsupported`, before admission: not a compile category,
   no run record, nothing dispatched, and no bundle spec 004 would reject
   is ever written. `decide` without capture runs normally.

### 3.12 Compatibility

1. `rustev.definition/1`, `rustev.plan/2`, `rustev.snapshot/1`,
   `rustev.evidence/1`, `rustev.run/1`, `rustev.exact/1`, every refusal
   category 1 to 12 and its phase and order, and every operator's meaning
   on version 1 inputs are unchanged.
2. Every committed definition, plan, judgment, evidence and run golden,
   including both reference plans, keeps its bytes and `PlanId`; a test
   recompiles each through the amended compiler and compares bytes.
3. Adding `insufficient_grade` and categories 13 to 15 extends closed
   enums; their canonical names are new and no existing name moves. The
   numbers are the next free ones in merge order of the #76 specs; the
   names never change.
4. `PlanId` of a plan 3 changes with any bound source descriptor, cap,
   presence predicate, disagreement rule, requirement, join, merge or
   corroboration declaration, and not when an unbound source descriptor is
   added.

## 4. Out of scope

Acquisition steps, `compose_key@1`, paging and widening (#76 B1, which
gives a constructed key grade `hint` under 3.2 and may supply corroboration
sets through acquisitions; here they come from the snapshot);
provenance-conditional rules and actionable outcomes (#76 B3, spec 033);
refreshing cached values (#76 F1); the metric of how often a `hint`
disagreed with a corroborated value (#76 E1, which reads the corroboration
records of 3.10); replay of plan 3 decisions (3.11.3); CLI flags for source
descriptors and snapshot 2 (an amendment of spec 006); carrying grades
through the remote protocols or bindings; verifying a source's claims;
quality claims from synthetic fixtures (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A quote map entry for an item with `total_price` and `unit_price` both null | `matched_absent`; the item is excluded `predicate_failed`; no cached price reaches the output. |
| A `present_when` of `{all: [{any_non_null: [a, b]}, {all_non_null: [c]}]}` over a record with `a` null, `b` set, `c` null | Absent `predicate_failed`; the recorded predicate is the canonical tree. |
| A `present_when` nested 5 deep, or with 33 leaf predicates, or an empty list | Category 11. |
| `overlay_else_base_if_grade_at_least` naming a field the base record lacks | Category 11. |
| A `corroborate` step whose `estimated` claim is `uncorroborated` | The claim stays `estimated` with its basis unchanged. |
| No map entry for an item | `unmatched`; excluded `unmatched`. |
| Merge with `overlay_only` price and an unmatched overlay | `price` absent `overlay_missing`; a rule reading it excludes the item `missing:<path>`; the base price is never used. |
| `overlay_else_base_if_grade_at_least{authoritative}` with a cached base and no overlay | Absent `overlay_missing_base_below_grade`. |
| The same rule with no overlay and a base record that fails its presence predicate | Absent `overlay_and_base_missing`, not a grade reason. |
| A version 2 filter rule named `predicate_failed`, or `x:y` | Category 11, `reserved_reason`. |
| `prefer` listing a source twice, a source the input does not name, or omitting one | Category 11, `disagreement_sources`. |
| `on_disagreement` on an ungraded input | Category 11, `disagreement_on_ungraded`. |
| A merge with a field and no rule | Category 11. |
| A filter on a label whose source marks it `known_unreliable`, no corroboration | Category 14. |
| The same filter reading a `corroborate` step whose check passes | Grade `authoritative`; the filter applies the label. |
| The check fails with `on_false: contradicts`, `on_contradiction: conflict` | `conflict{facts}` naming both facts; evidence holds both digests and grades. |
| The check reads an unresolved set | `no_evidence`; `uncorroborated`; the gate read is `insufficient_grade`. |
| A `corroborate` upgrading to `authoritative` with a check reading a `cached`-ceiling value | Category 13. |
| `require: {min: cached}` on a value whose ceiling is `estimated` | Category 13. |
| `require: {min: cached, max_age_ms: 600000}` on a cached value 600 001 ms old | `stale_evidence`; at 600 000 ms it is accepted. |
| A snapshot 2 claim `hint` on a field whose ceiling is `authoritative`, read by a gate | `insufficient_grade`; evidence records claim, ceiling, effective. |
| An exact step adding an `authoritative` and a `hint` value | Result grade `hint`. |
| A semantic step projecting only ungraded inputs | Value `ungraded`; a `require: {min: estimated}` read of it is `insufficient_grade`. |
| A semantic step projecting an `authoritative` and an ungraded input | Grade `estimated`. |
| A definition cap raising a ceiling | No effect: the ceiling stands (caps only lower). |
| An entry from a source the input does not name | Ignored, recorded `unbound_source`; with no other entry, `missing_evidence`. |
| Two accepted sources disagree under `prefer` | The first listed source's value is used; both digests recorded. |
| One source supplies two differing entries under `prefer` | `conflict`. |
| A graded input naming a source not supplied | Category 15. |
| A version 1 definition using `join_keyed@1` | Category 2 `unknown_operator`, phase c (3.1.3). |
| A version 1 definition with a `nullable` field, an `evidence` member or a `corroborate` step | Category 11, `requires_definition_2`, phase b. |
| A version 2 definition using no #76 construct | Category 11, `use_definition_1`. |
| A plan 3 document using only this spec's constructs, re-serialized | No member of a later spec appears; bytes and `PlanId` unchanged. |
| A snapshot 2 supplied to a plan 2 | `StartError::Schema`, detail `snapshot_version`; through the runtime, `Rejection::InvalidRequest` before admission. |
| A plan 3 prepared with a sink that accepts only `rustev.run/1` | `PrepareError` `sink_version_unsupported`; nothing admitted. |
| A plan 3 decision through `decide_with_capture` | `CaptureConfigError` `capture_unsupported` before admission; no run record, nothing dispatched. |
| A policy reading a value that can be `insufficient_grade` without a handler | Category 8. |
| Every existing golden recompiled | Byte-identical, same `PlanId`. |

## Acceptance

- Join: a quote map entry with no price field is `matched_absent`; the item
  is excluded with a recorded reason, and a negative test proves no cached
  price reaches any output, judgment or proposal parameter.
- Merge: a merged record with an `overlay_only` live price and a
  `base_only` cached description reports per-field grades, sides and
  as-of in the step value and in evidence 2.
- Corroboration: a type label from a `known_unreliable` source cannot gate
  a filter without a corroboration step (category 14); with corroboration
  by sub-area membership (`member_ref` over the parent's sub-area keys,
  supplied in the snapshot) it gates correctly; a contradicted claim yields
  `conflict`, and each `on_contradiction` choice is tested.
- Propagation through every exact operator of `rustev.exact/1` and the new
  ones: a `hint` operand makes the result at most `hint`; `ungraded`
  operands do not raise a grade; a semantic step is at most `estimated`.
- Freshness by grade: a `cached` value accepted under `max_age_ms` becomes
  `stale_evidence` one millisecond past its age; an `estimated` value is
  refused at compile time where `cached` or better is required (category
  13), and at runtime a `cached` claim on an `authoritative` ceiling is
  `insufficient_grade` where `authoritative` is required.
- Conflict policy: each of the four rules, the within-source conflict and
  the recorded resolution, with digests, sources and grades.
- Presence predicates on inputs, list items, map entries and merged
  records, including `field_presence@1` and `top_k@1` over absent items.
- Every refusal of categories 11, 13, 14 and 15 introduced here has a
  golden refusal fixture with a passing neighbour differing only in the
  refused property.
- Golden fixtures under `packages/rustev-pkg-lodging/tests/evidence_quality/`
  (synthetic, named as such) exercise all of the above: a version 2
  lodging-style definition, source descriptors for a catalogue (`cached`),
  a live quote service (`authoritative`) and a geocoder (label
  `known_unreliable`), snapshots, the plan 3 golden with its `PlanId`, and
  judgment and evidence 2 goldens.
- Runtime: a plan 3 decision through scripted backends delivers a
  `rustev.run/3` record embedding evidence 2; a sink accepting only
  `rustev.run/1` is refused `sink_version_unsupported` at preparation; a
  capture request is refused `capture_unsupported` before admission.
- Optional members: canonical bytes of plan 3, definition 2, snapshot 2,
  evidence 2 and run 3 documents omit unused optional members, tested by
  golden bytes.
- Compatibility: every existing golden is byte-identical (3.12.2).
- Seeds in the core mutation harness (grade combination taking the
  maximum, a merge falling back to base under `overlay_only`, key
  existence counted as presence, a cap raising a ceiling, a corroboration
  ignoring check grades, `prefer` choosing the last entry, a requirement
  ignoring `max_age_ms`) are each detected.
- Every row of section 5 has a test. `make code`, declared verification,
  and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #60 leaves the points below open, and each is
decided here within the binding principles:

1. **Where grades are declared.** In a new, identified source descriptor
   (`rustev.source/1`, `SourceId`) bound at compile time, recorded in plan
   3 by name, version and `SourceId` with the ceilings used in `grades`, so
   `PlanId` changes with any ceiling. Spec 036 (#76 B1) adds acquisition
   operations to this descriptor as optional members rather than
   introducing a second one.
2. **Per-entry claims.** Snapshot 2 lets a supplier lower a grade for one
   entry (a cache hit), never raise it; snapshot 1 claims nothing.
3. **Versioning.** This spec introduces the shared #76 track versions
   (`rustev.definition/2`, `rustev.plan/3`, `rustev.snapshot/2`,
   `rustev.evidence/2`, `rustev.run/3`, `rustev.exact/2`) and
   `rustev.source/1`. Later #76 specs add optional members to them, omitted
   when unused, instead of defining further versions, so version 1
   definitions keep their bytes and `PlanId`s (#76 identity-stability
   risk) and a plan using only this spec's constructs keeps its bytes as
   later specs land. A version 2 definition with no #76 construct is
   refused (`use_definition_1`). Refusal numbers 13 to 15 are the next
   free in merge order; the names (`grade_requirement`,
   `uncorroborated_hint`, `source_binding`) never change.
4. **`Absent` is a presence status, not an `Unresolved` reason.** Reading
   an absent value is `missing_evidence`, so existing handlers cover it;
   only `insufficient_grade` is new, because "present but not good enough"
   is neither missing nor stale.
5. **Hard gates never accept `hint`.** The implicit gate minimum is
   `estimated`; a corroboration step is the only way a hint reaches a gate,
   and an uncorroborated value at runtime is `insufficient_grade`, never a
   pass.
6. **Corroboration cannot launder.** A check counts only if what it reads
   has at least the target grade; contradiction wins over support; grade
   changes never change derivation class or provenance.
7. **Conflict rules.** The issue's four (`prefer`, `require_agreement`,
   `treat_as_hint`, `abstain`) with `prefer` and `treat_as_hint` ordered
   by an explicit source list; no recency rule exists. `require_agreement`
   is spec 002's existing behavior.
8. **E1 measure.** This spec records corroboration outcomes per source and
   path; computing the disagreement rate is #76 E1's metric.
9. **Replay and CLI.** Replay of plan 3 decisions waits for spec 036's
   (#76 B1) amendment of spec 004 and `rustev.replay/2`; meanwhile capture
   is refused as an invalid capture configuration, a runtime refusal with
   no compile category. CLI exposure waits for an amendment of spec 006.
   The library surface is complete without them.
10. **`member_of` extended** as the predicate `member_ref` over a
    referenced list or map keys, leaving `member_of@1` and `member` with
    literal sets unchanged.

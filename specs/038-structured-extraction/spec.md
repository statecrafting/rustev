---
id: "038-structured-extraction"
title: "Structured extraction step and the model-derived input pattern (amends 002, 003 and 036)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002, 003 and 036 (R-16), for #76 child B6.
  Two supported ways to turn free text into typed decision inputs, both
  keeping extracted values model-derived. Pattern A: the host extracts and
  submits snapshot entries with provenance `model-derived` and an optional
  producer identity (artifact, template digest, production time), recorded
  in evidence and lineage. Pattern B: an `extract` step, a model step of
  spec 037's bounded envelope, asks a backend for a typed record against a
  declared schema and vocabularies under an identified prompt template;
  every field is validated and is present or absent with a reason, never
  coerced, clamped or truncated; each field carries its origin
  (`extracted`, `fallback`, `absent`) and derivation; a deterministic
  in-core fallback extractor (`rustev.extract-rules/1`) yields
  exact-derived fields; per-field distributions exist only from backend
  log-probabilities and are thresholded only under a bound calibration or
  a declared `uncalibrated_threshold`. In both patterns an exact operator,
  policy predicate, proposal parameter, acquisition request or other model
  step reads a model value only through a declared acceptance rule (an
  `accept_model_derived` step, or a spec 033 check covering
  `model-derived`); selection-policy controls never read one. Extraction
  can drive spec 033's `needs_input` outcome and propose which declared
  questions it asks. Adds to the shared #76 versions; every version 1
  definition keeps its bytes, its plan and its `PlanId`. Approved (A-35);
  implementation is in progress.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "036-mid-plan-acquisition"
extends:
  # The extract and acceptance step kinds, the extraction schema, the
  # typed record output kind, snapshot producers, the fallback program and
  # extraction request documents, evidence and judgment members, and the
  # new refusal categories (spec 002, 3.2 to 3.12).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Field validation, presence and origin, the fallback evaluator, the
  # acceptance step, the model-value gate and the typed builder.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Extract requests under the execution policy, step deadline, budgets
  # and evidence delivery (spec 003, 3.2 to 3.9).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # The synthetic Pattern A and Pattern B example beside, not inside, the
  # package's reference definition.
  - { spec: "008-reference-package-lodging", unit: { kind: directory, path: "packages/rustev-pkg-lodging/" }, nature: additive }
  # Adds 038 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Exhaustive matches over the new step bodies, the new snapshot entry
  # member in struct literals, and regenerated document types of the
  # bindings; no behavior.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-aicortex/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: directory, path: "integrations/rustev-otel/" }, nature: additive }
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: additive }
  # The reviewed archive lists gain the new contract source files.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "008-reference-package-lodging"
  - "032-evidence-quality"
  - "033-provenance-rules-and-actionable-outcomes"
  - "034-parameterized-definitions"
  - "035-screening-seam"
  - "036-mid-plan-acquisition"
  - "037-generation-steps"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every rustev.definition/1 document compiles to the same rustev.plan/2 bytes and PlanId as before this spec, with the same judgment, evidence and refusals; snapshot 1 documents keep their bytes and SnapshotId."
    anchor: "3-14-compatibility-and-identity"
  - id: "I-2"
    kind: invariant
    text: "An extracted field is either present with a value that satisfies its declared type, bounds and vocabulary exactly as received, or absent with a reason; no value is coerced, clamped, truncated, defaulted or repaired."
    anchor: "3-4-field-validation-and-presence"
  - id: "I-3"
    kind: invariant
    text: "A field filled by the model is model-derived; a field filled by the fallback extractor is exact-derived only when every source it read is exact-derived; acceptance never lowers a value's derivation class."
    anchor: "3-5-origin-derivation-and-lineage"
  - id: "I-4"
    kind: invariant
    text: "A per-field distribution exists only from log-probabilities the backend returned for every declared option, normalized by the core; a self-reported confidence is recorded as provider-reported and never becomes mass."
    anchor: "3-6-per-field-distributions-and-calibration"
  - id: "I-5"
    kind: invariant
    text: "In a version 2 definition, no exact operator, policy predicate, proposal parameter, acquisition request or other model step reads a model value except through a declared acceptance rule, and no selection-policy control reads one at all; the compiler refuses every other read with unaccepted_model_value."
    anchor: "3-9-the-model-value-gate"
  - id: "R-1"
    kind: requirement
    text: "The fallback extractor is a pure, bounded, identified program evaluated in rustev-core with identical results on every target, and its identity is part of the PlanId."
    anchor: "3-7-the-fallback-extractor"
  - id: "R-2"
    kind: requirement
    text: "An accept_model_derived step declares its validation and its consequence on failure, applies them to every model-derived value it reads, passes caller and system values through unchanged, and emits a field_dropped notice with its computed action for every dropped value."
    anchor: "3-8-acceptance-steps"
  - id: "R-3"
    kind: requirement
    text: "Snapshot producers are accepted only on model-derived entries and are recorded per input and in the lineage of every value that read the input."
    anchor: "3-2-pattern-a-host-side-extraction"
  - id: "R-4"
    kind: requirement
    text: "Questions of a needs_input outcome are always declared templates; an extraction may only select among a declared set, through an accept step, and never supplies question text."
    anchor: "3-13-needs-input-from-extraction"
---

# 038: Structured extraction step and the model-derived input pattern (amends 002, 003 and 036)

Approved by the owner (A-35, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child B6 (#64). The approved texts of
specs 002, 003 and 036 are not edited; this spec records what changes, as 003
does for 002. It relies on owner decisions OD-2 (two bounded model step
kinds; extracted values never feed exact operators, selection-policy
controls or acquisition requests except through declared acceptance rules;
R-14 clarified, not relaxed) and OD-4 (`generate` and `extract` join R-05's
primitive names), recorded as R-41 and R-42 by spec 037. Approval
authorizes no live provider call or spend, no quality claim (R-04, R-37),
no real-user data and no release.

## 1. Purpose

Many pipelines start by reading a free-text request and producing search
filters: a date window, a party size, feature ids, an area name. Those
values then drive exact logic. If they enter as if the caller had typed
them, every downstream rule treats a model's guess as a caller's
statement, and a rule written for caller-supplied dates refuses a request
over dates the caller never sent. Spec 002 already admits `model-derived`
inputs and records their class, but lets any exact operator read them like
any other value, and Rustev cannot itself ask a backend for a typed record.

This spec makes both routes accountable:

1. **Pattern A, host-side extraction.** The host runs its own extractor and
   submits the result as snapshot entries with provenance `model-derived`
   and a declared producer identity (3.2).
2. **Pattern B, the `extract` step.** Rustev asks a backend for a typed
   record against a declared schema and vocabularies, validates each
   field, records per-field origin and derivation, and offers a
   deterministic fallback extractor (3.3 to 3.7).
3. **Acceptance.** In both patterns an exact step reads an extracted value
   only through a declared acceptance rule that states its validation and
   what happens on failure (3.8, 3.9), with spec 033's actionable outcomes
   telling the caller or host what to do (3.12, 3.13).

**Prerequisites.** This spec builds on spec 037: `extract` is a model step of 037's bounded envelope (037 3.1),
and its template, projection, vocabularies, repair, screen, limits, fault
and evidence rules are 037's (3.3, 3.7, 3.11, 3.12). It may not be
implemented before 036 and 037 are implemented.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the definition 2 step kinds
  `extract` and `accept_model_derived`, the extraction schema types, the
  output kind `typed_record` and the `RawOutput` variant `typed_record`,
  the `rustev.request/1` operation variant `extract`, the snapshot 2 entry
  member `producer`, the documents `rustev.extract-rules/1` and
  `rustev.extract-request/1`, the plan 3 step details of both kinds, the
  evidence 2 and judgment 2 members of 3.12, and refusal categories
  `invalid_extraction`, `invalid_acceptance` and `unaccepted_model_value`,
  and the snapshot refusal reason `invalid_producer` (3.2.2).
- `crates/rustev-core/` (owner spec 002): field validation and presence,
  distribution normalization, the fallback evaluator, the acceptance step,
  the model-value gate, question selection, compilation of both kinds and
  the typed builder.
- `crates/rustev-runtime/` (owner spec 003): dispatching extract requests
  under the execution policy, the step deadline and cost sub-ledger,
  cancellation and evidence delivery (3.11).
- `packages/rustev-pkg-lodging/` (owner spec 008): the synthetic example of
  3.15 under `tests/extraction/`. The package's reference definition, its
  plan and its `PlanId` are unchanged.
- `Makefile` (owner spec 001): 038 joins `make verify` at completion.

No new dependency in any crate. `make boundaries` is unchanged. The CLI,
spec 004's evaluation, the remote protocols of spec 009 and every
integration are unchanged by this spec (section 4).

**Shared #76 versions.** Following the track's coordination rule, this spec
introduces no version of a shared document. It adds optional members and
constructs, omitted when unused, to `rustev.definition/2`, `rustev.plan/3`,
`rustev.snapshot/2`, `rustev.evidence/2` and the registry `rustev.exact/2`
(introduced by spec 032), to `rustev.judgment/2` (introduced by spec 033),
and relies on spec 032's presence rules (032 3.4). Its two new documents,
`rustev.extract-rules/1` and `rustev.extract-request/1`, are its own.
Refusal categories are named here; their numbers are the next free numbers
in merge order.

**Relation to sibling #76 specs.** Spec 037 (#76 B5)
records OD-2 and OD-4 and defines the bounded model step envelope (037 3.1)
and prompt templates (037 3.3), which `extract` uses. Spec 033 (B3) defines
checks, actionable outcomes and `needs_input`; 035 (B7) screening; 036 (B1)
acquisition requests; 041 (E1) extraction metrics. Section 3.16 lists each
interface relied on, so every edge can be checked when that spec lands.

### 2.1 Binding principles of #76

- **Proposals, never grants.** An extracted record, an acceptance outcome
  and a producer identity are evidence, never authorization. No value of
  this spec is accepted by an executor and no `Permitted` type is added.
  An accepted value may appear in a proposal's parameters, still a
  proposal, still carrying its derivation and origin (3.12.3).
- **Derivation classes and lineage.** Model-filled fields are
  `model-derived`; fallback fields are `exact-derived` only over exact
  sources; acceptance passes a value's class through unchanged and never
  lowers it (3.5, 3.8.5). Lineage names the extract step, template,
  backend artifact, fallback program and producer of every value.
- **Unresolved, never a default; no silent kind conversion.** An invalid,
  missing or out-of-vocabulary field is absent with a reason; reading an
  absent value is `missing_evidence` (032 3.4.4). Nothing is coerced,
  clamped or truncated. A label never becomes mass and a self-reported
  confidence never becomes a distribution (3.6).
- **Pure core; no async runtime in contract and core.** Validation,
  normalization, template rendering, the fallback evaluator and acceptance
  are pure functions in `rustev-core`, which reads `now` as an input and
  performs no I/O.
- **New dependency classes only under `integrations/`.** None is added.
- **No dynamic plugin loading.** The fallback program and prompt templates
  are documents bound at compile time and interpreted by closed
  evaluators; backends are composed explicitly as today.
- **No identity, authority or knowledge store.** Producer identities are
  host declarations that Rustev records and does not verify; vocabularies
  are literals, profile parameters or inputs the host supplies.

## 3. Behavior

### 3.1 Terms

1. A **model value** is, in a version 2 definition: (a) every field of an
   `extract` step; (b) every input whose declared accepted provenance
   includes `model-derived`.
2. An **exact consumer** is: an argument of an exact operator, an exact
   predicate in a policy condition, a proposal parameter, a field, guard or
   stop count of an acquisition request (036), and a protected constraint
   (a check of a spec 033 constraint group). A **selection-policy control**
   is a threshold, weight, adjustment scale, limit, template parameter or
   profile parameter (R-14).
3. A **presence-only read** is `present()`, `field_presence@1` and the
   presence predicates of 032 3.4; it reads whether a value exists, not the
   value.
4. A **declared acceptance rule** is an `accept_model_derived` step (3.8),
   or, for an input only, a spec 033 check whose subject is that input and
   whose `on_fail` covers `model-derived` with a branch or a `default`
   (3.9.2).

### 3.2 Pattern A: host-side extraction

1. `rustev.snapshot/2` entries gain the optional member `producer`:
   `{artifact_id, template_digest, produced_at_ms}`, where `artifact_id` is
   an `ArtifactId` (spec 002, 3.3.3), `template_digest` is `none` or a
   `sha256:` identity, and `produced_at_ms` a `Timestamp`. Absent members
   are omitted, so an entry without a producer has its version 1 bytes.
2. A `producer` on an entry whose provenance is not `model-derived` refuses
   the snapshot before evaluation with `invalid_producer`, naming the field.
   It is a snapshot refusal, like spec 002 3.7.3's refusal of an entry for
   an undeclared field, not a compile-time category.
   A `produced_at_ms` later than `now` makes the field `invalid_input`
   (future timestamp), in the position of spec 002 3.7.3's future check.
3. The producer of the entry spec 002 3.7.3 selects for a field is recorded
   in evidence 2 for that input, and joins the lineage of every value that
   reads it (3.5.4). A producer is a host declaration: Rustev neither
   verifies it nor uses it to choose between entries, and two entries with
   equal values and different producers are not a conflict.
4. Pattern A needs no producer: an input that accepts `model-derived` is a
   model value whether or not its entries carry one. In a version 2
   definition every exact consumer of it is gated (3.9).

### 3.3 The extract step

`extract` is a model step of spec 037's envelope (037 3.1): declared and
finite, budgeted at compile time, identified, model-derived, isolated
except through acceptance rules, and accounted like a semantic step. This
section states what `extract` adds and which envelope members it uses.

1. `rustev.definition/2` gains the step body `extract`; in a
   `rustev.definition/1` it is refused, category 11, detail
   `requires-definition-2`. Illustrative form (contract types normative):

```json
{ "id": "request_filters", "body": { "extract": {
    "for_each": "none",
    "template": { "id": "filters", "digest": "sha256:<64 hex>", "params": {} },
    "projection": ["input:request.text"],
    "vocabularies": { "feature_vocabulary": { "input": "input:feature_vocabulary" } },
    "output": { "fields": [
        { "name": "window", "type": { "record": { "fields": [
            { "name": "start", "type": { "date": {} } },
            { "name": "end", "type": { "date": {} } } ] } } },
        { "name": "party_size", "type": { "integer": { "bounds": [1, 30] } } },
        { "name": "features", "type": { "list": {
            "item": { "enum": { "vocabulary": "feature_vocabulary" } }, "max_items": 10 } } },
        { "name": "area_name", "type": { "text": { "max_bytes": 64 } } },
        { "name": "insufficient", "type": { "bool": {} } } ] },
    "distributions": "when_available",
    "repair": { "max_attempts": 0, "only_missing_fields": false },
    "screen": { "input": "untrusted_fields", "output": "none" },
    "limits": { "deadline_share": "0.3", "max_cost_units": "none" },
    "backend": { "min_determinism": "unspecified", "artifact": "any", "binding": "auto" },
    "fallback": { "rules_program": "filters-fallback@1", "fill": "absent_fields" }
} } }
```

2. Envelope members, with 037's meaning and bounds unless stated:
   - `for_each`: `none` only in this version (any other value is refused,
     category `invalid_extraction`); one request per decision.
   - `template`: a `rustev.prompt-template/1` id, digest and parameter
     bindings (037 3.3), supplied to the compiler, embedded in the plan and
     rendered by the core. Parameters bind only to literals, exact-derived
     references or profile parameters; never to a model value.
   - `projection`: 1 to 8 references, each `input:<name>` or
     `input:<name>/<field>`, plus accept step values (3.9.3; reading an extract field
     directly is refused, 3.9.1). They are the
     only values the backend sees, in the request's projection channel,
     never in the instructions.
   - `vocabularies`: named option lists, each a literal (1 to 256 options
     of 1 to 64 bytes), a profile parameter, or, added by this spec,
     `{input: <ref>}` naming an input of type `list{item: text}` or
     `enum{options}` whose accepted provenance excludes `model-derived`. An
     `enum{options}` input declaring more than 4096 options is refused at
     compile time (category `invalid_extraction`). An input vocabulary is
     read at evaluation; an unresolved one, or a `list{item: text}` value
     with more than 4096 items, a duplicate item, or an item outside 1 to
     64 bytes, makes every field using it absent with
     `vocabulary_unavailable` (nothing is truncated or deduplicated).
   - `repair`: `max_attempts` must be 0 in this version.
   - `screen`: `input` is always `untrusted_fields` and `output` is
     always `none` (3.11.4).
   - `limits`: `deadline_share` and `max_cost_units` as 037 defines; an
     extract step has no per-item deadline and no concurrency member.
   - `backend`: spec 002's backend requirements.
3. Members `extract` adds:
   - `output`: the extraction schema (3.4).
   - `distributions`: `none` or `when_available` (3.6).
   - `fallback`: `none`, or `{rules_program, fill}`, where `rules_program`
     is `<name>@<version>` of a `rustev.extract-rules/1` document supplied
     to the compiler beside calibration artifacts and templates, and `fill`
     is `request_failure` (the program runs only when the request produced
     no output) or `absent_fields` (it also fills every field the model
     left absent).
4. **Budget.** The request counts once against `max_semantic_requests` and
   the execution policy's worst-case attempts, and its worst-case request
   bytes against `max_projection_bytes` and the backend's input limit
   (phase g, category 7). At most 8 extract steps per definition.
5. **Request.** For each extract step the core lists one request in
   `pending()` in step order, like a semantic request (extraction is part
   of the decision, not of 037's generation stage). The request document
   `rustev.extract-request/1` holds the step, the template id and
   identity, the rendered instructions, the canonical projection, the
   output schema with its schema digest, and every vocabulary's options in
   declared order. Its `RequestId` (spec 004, 3.4) covers all of these
   through the operation variant `extract` that this spec adds to
   `rustev.request/1`; existing request documents keep their bytes.
6. **Plan 3.** A new `PlanStepDetail` variant `extract` records the bound
   backend id, artifact, `DescriptorId` and output kind, the embedded
   template document with its identity, the literal and profile
   vocabularies resolved and input vocabularies by reference, the schema
   digest, the worst-case request bytes, and, when declared, the fallback
   program's identity and canonical document. The static derivation class
   is `model-derived`.

### 3.4 Field validation and presence

1. **Schema.** `output.fields` holds 1 to 32 fields, each `{name, type}`
   with `name` matching `[a-z][a-z0-9_]{0,63}`, unique. Field types:

| Type | Value | Bounds and checks |
|---|---|---|
| `bool` | JSON boolean | |
| `integer{bounds: [lo, hi]}` | JSON integer in `i64` | `lo <= v <= hi`, `lo <= hi` |
| `decimal{bounds: [lo, hi], scale}` | decimal string (spec 002, 3.4.1) | at most `scale` (0 to 9) fractional digits, within bounds |
| `text{max_bytes}` | JSON string | 1 to 4096 bytes declared; at most `max_bytes` UTF-8 bytes; no `Cc` character |
| `date` | string `YYYY-MM-DD`, a valid proleptic Gregorian date | becomes a `timestamp` at 00:00 UTC of that date |
| `enum{vocabulary}` | JSON string | a member of the named vocabulary |
| `list{item, max_items}` | JSON array | `item` is `enum{vocabulary}` or `text{max_bytes}`; `max_items` 1 to 64 |
| `record{fields}` | JSON object | 1 to 16 fields, each `bool`, `integer`, `decimal`, `text`, `date` or `enum`; no `list` or `record` subfield |

2. **Record-level check** (spec 003's pre-supply check, as 037 3.7.3): the
   output is an `invalid_output` attempt when it is not a `typed_record`,
   when it has a member that is not a declared field, or when its
   log-probabilities break 3.10.3. Record-level failures follow the step's
   retry and fallback policy.
3. **Presence.** On an accepted record every top-level field is present
   with a value, or absent with exactly one reason. The reason codes
   shared with 037 3.7.4 keep their names: `missing` (not returned or
   `null`), `wrong_type`, `too_long`, `control_character`,
   `not_in_vocabulary`, `duplicate_item`, `too_many_items`. Extraction adds
   `out_of_bounds`, `invalid_date`, `vocabulary_unavailable`,
   `source_unavailable`, `screen_blocked`, `screen_unavailable`,
   `request_failed{reason}` (one of spec 003 3.6.4's supplied reasons, or
   `cancelled`), and, after a fallback attempt, `fallback_no_match` or
   `fallback_invalid`. A list whose every returned item was rejected
   takes the per-item reason of its first rejected item as its field
   reason (3.4.5).
4. **No coercion.** A value outside its bounds or vocabulary, a text longer
   than `max_bytes`, a list longer than `max_items`, a number in a string,
   a string for a boolean, a date in another format: each is absent with
   its reason. A decimal with more fractional digits than `scale` is
   absent `out_of_bounds` (scale is a declared bound); a string that is
   not a decimal string is absent `wrong_type`. Nothing is clamped, rounded, truncated, case-folded,
   trimmed, matched approximately or defaulted.
5. **Lists.** Checks apply in this order; the first that makes the field
   absent decides its reason:
   1. an array of more than `max_items` items is absent `too_many_items`,
      before any item is checked;
   2. each item that is not in its vocabulary, of the wrong type, too
      long or holding a control character is rejected individually,
      recorded with its index, reason and value digest;
   3. two remaining items that are equal make the field absent
      `duplicate_item`;
   4. a non-empty array with no remaining item is absent with the reason
      of its rejected item with the lowest index in the returned array (for example `not_in_vocabulary` for enum
      items, `too_long` for text items);
   5. otherwise the field is present with the remaining items in returned
      order. An empty array returned by the backend is a present, empty
      list.
6. **Records.** A record field is present only when every subfield is
   present and valid; otherwise it is absent with the first failing
   subfield's reason in declared order, the subfield named in the detail.
7. **Reading.** A present field is read as `step:<id>/<field>` with the core
   type of the table (a `record` subfield as `step:<id>/<field>/<sub>`).
   An absent field read by value is `missing_evidence` naming the path (032
   3.4.4); presence-only reads see absence. Every read other than by an
   accept step, a presence-only read or a mass condition (3.6.4) is gated
   (3.9).

### 3.5 Origin, derivation and lineage

1. Every top-level field carries one origin: `extracted` (validated value
   from the backend), `fallback` (from the fallback extractor), or
   `absent{reason}`. This is the *field origin* of an extract
   step. The *accepted-value origin* of 3.8.3 and 3.12.3 is a different
   enumeration for values that leave acceptance: `extracted` and
   `fallback` there mean the same as here, `supplied` marks a caller or
   system input and `host_extracted` a model-derived snapshot entry
   (Pattern A); an absent field never becomes an accepted value.
2. Derivation: an `extracted` field is `model-derived`. A `fallback` field
   is `exact-derived` when every projected source it read is
   `exact-derived`, and `mixed-derived` otherwise (spec 002, 3.12.2). An
   absent field has no value and no class.
3. A field the model returned valid is never replaced by the fallback.
   Under `fill: absent_fields` the fallback fills only absent fields; under
   `request_failure` only when the request produced no output. A fallback
   value is validated by 3.4 like a model value; a failing one leaves the
   field absent with `fallback_invalid`.
4. Lineage of a field value: the projected inputs, the step, and, by
   evidence 2 reference, the template identity, the backend artifact
   (extracted) or the fallback program identity (fallback). Evidence 2
   adds the member `producers` to the lineage of every value whose lineage
   includes an input whose selected entry carried a producer (3.2.3),
   sorted by input name.
5. The extract step itself resolves even when every field is absent: field
   absence is not an unresolved step, and an extraction failure never
   becomes the decision's outcome except through an acceptance rule or a
   policy handler that a definition declares.

### 3.6 Per-field distributions and calibration

1. **Eligible fields.** `bool` (options `false`, `true`) and `enum`
   fields, and each item of a `list` whose item is `enum`. A list item's
   distribution is recorded in evidence and lineage only: no condition or
   accept step addresses a single list item in this version, so it cannot
   carry a threshold (item 4). Thresholds over list items are left to a
   later amendment.
2. **Source.** With `distributions: when_available` and a binding whose
   output kind declares `option_logprobs: true`, the backend may return,
   per eligible field (and list item), log-probabilities over the declared
   options. The core normalizes them by softmax with max subtraction in
   declared option order (spec 002, 3.5.2) into a `Distribution`. When any
   declared option lacks a finite log-probability, no distribution is
   attached and the field records `distribution: absent{incomplete_logprobs}`;
   nothing is filled in or invented. A field with a value but no
   log-probabilities records `absent{not_returned}`. The field's value is
   the returned value regardless of which option has the most mass.
3. **Self-reported confidence.** A confidence, score or probability the
   backend reports in any other way is kept in evidence as
   `provider_reported` (at most 256 bytes per field; a longer string is a
   record-level fault, 3.10.3) and is never a mass,
   never a distribution and never read by any condition.
4. **Thresholds.** A mass threshold on an eligible field is
   `mass_at_least` or `top_mass_below` in a policy condition, or `min_mass`
   in an accept step (3.8.2). Both follow spec 002's existing rule: the
   compiler refuses one with category 6 `uncalibrated_threshold` (spec
   002, 3.9.5) unless a calibration is bound to the field (checked by
   category 10 `calibration_binding` rules) or the threshold declares an
   `uncalibrated_threshold` reason, which is carried into every judgment
   that applies it as spec 002 records it. A threshold on a
   field of a step with `distributions: none`, of a list field, or bound to
   an output kind without `option_logprobs`, is category 3
   `kind_mismatch`.
5. **Calibration binding.** A `rustev.calibration/1` artifact binds to an
   extracted field with `task` = `<template id>@<template digest>`,
   `question` = `<step id>/<field>`, the bound backend artifact and the
   field's options. Its document shape is unchanged. A calibrated threshold
   on a field whose vocabulary is an input is refused at compile time,
   category 10, because its options are not known until evaluation.
6. A distribution that a threshold needs and that is absent at evaluation
   is `missing_evidence` for the policy (handled under category 8) and a
   failure with cause `distribution_absent` for an accept step's `min_mass`
   (3.8.4).

### 3.7 The fallback extractor

1. `rustev.extract-rules/1`, parsed under `DESCRIPTOR_V1`, unknown fields
   and duplicate keys refused, identified by its tagged digest:

```text
program   = { schema: "rustev.extract-rules/1", name, version, sources: [source],
              fields: [field_rules] }
source    = { name, ref }                         # ref: input:<name> or input:<name>/<field>
field_rules = { field, rules: [rule] }
rule      = { name, when: condition, then: value }
condition = "always"
          | { contains_any: { source, terms: [string] } }
          | { contains_all: { source, terms: [string] } }
          | { all: [condition] } | { any: [condition] } | { not: condition }
value     = { literal: json }
          | { integer_after: { source, terms: [string], max_digits } }
          | { date_iso: { source, occurrence } }
          | { record: { <subfield>: value } }
```

2. **Binding.** At compile time the program named by `rules_program` must
   be supplied, every `source` must be an input reference in the step's
   `projection`, and each `field` must name a declared field whose type its
   values can produce; otherwise category `invalid_extraction`. Its
   identity and canonical document enter the plan 3 step entry and the
   `PlanId`.
3. **Evaluation.** Pure and in `rustev-core`. Text matching folds ASCII
   `A` to `Z` to lower case in both text and terms and compares other bytes
   exactly (spec 005, 3.4.4). For a scalar or record field the first rule,
   in declared order, whose condition holds and whose value yields a value
   wins. For a list field every such rule contributes its literal item, in
   rule order. Every rule of a list field must have a `literal` value of
   the item type (`integer_after`, `date_iso` and `record` are refused
   there), and the compiler refuses two rules of one list field with the
   same literal item (both category `invalid_extraction`). No item can
   therefore repeat and nothing is deduplicated at evaluation; a list
   longer than `max_items` is absent with `fallback_invalid` (3.5.3).
   `integer_after` finds the first occurrence of any term and parses the
   run of 1 to `max_digits` ASCII digits that follows after optional ASCII
   spaces; `max_digits` is 1 to 18 (compile-time, `invalid_extraction`),
   so the value always fits `i64`, and a longer run makes the rule not
   apply rather than being cut short. `date_iso` takes the
   `occurrence`-th substring matching `YYYY-MM-DD`, `occurrence` being 1
   or 2 (compile-time, `invalid_extraction` otherwise). A value that
   cannot be produced makes the rule not apply. No rule applies: the field
   stays absent with `fallback_no_match`.
4. **Bounds.** At most 32 field entries, 128 rules per field, 64 terms per
   condition, 64 bytes per term, condition depth 8; a source longer than
   `max_projection_bytes` is not read and the fields stay absent with
   `source_unavailable`.
5. **A source the fallback cannot read.** An unresolved source, or one that
   screening withholds from exact steps (035), leaves its fields absent with
   `source_unavailable`. A source withheld only from model steps is
   readable: the program is exact and executes no instruction from text.
6. The evaluator gives byte-identical results on every target, including
   `wasm32` (no floating point, no locale, no regular expression engine).

### 3.8 Acceptance steps

1. `rustev.definition/2` gains the step body `accept_model_derived`:

```json
{ "id": "window_ok", "body": { "accept_model_derived": {
    "field": "step:request_filters/window",
    "validate": { "predicate": { "compare": { "op": "gt",
        "left": { "ref": "step:request_filters/window/start" },
        "right": { "ref": "now" } } } },
    "min_mass": "none",
    "on_fail": { "drop_field": { "notice": "extracted_window_dropped" } }
} } }
```

2. Members, all required:
   - `field`: one model value, `step:<extract id>/<field>`, or
     `input:<name>` (or `input:<name>/<field>`) of an input that accepts
     `model-derived`.
   - `validate`: `none` (schema validation only) or `{predicate}`, an exact
     predicate of `rustev.exact/2` of type `bool` that may read `field` and
     its subpaths, `now`, literals, exact-derived values and other accept
     steps, and no other model value.
   - `min_mass`: `none` or `{option, at_least, basis}` with `basis`
     `{calibration: <id>}` or `{uncalibrated_threshold: <reason>}`, for
     eligible fields only (3.6.4).
   - `on_fail`: `{drop_field: {notice}}` or `unresolved`. `notice` is a
     code matching `[a-z][a-z0-9_]{0,63}`.
3. **Evaluation.** For the value read:
   - the source is unresolved (an unresolved input or extract dependency):
     the step is unresolved with that reason (spec 002, 3.10.4);
   - an input whose selected entry is not `model-derived`: the value
     passes through unchanged with origin `supplied`; acceptance does not
     judge caller or system values, which spec 033's checks govern;
   - otherwise (an extracted or fallback field, or a model-derived input
     entry) the value fails when it is absent, when `validate` is false or
     faults, or when `min_mass` is not met or its distribution is absent;
     otherwise it is accepted with its origin (`extracted`, `fallback`, or
     `host_extracted` with the producer when present).
4. **On failure.** `drop_field`: the step's value is absent with
   `acceptance_failed{cause}`, where cause is the source's absence reason,
   `validation_failed`, `validation_fault`, `min_mass_not_met` (a
   distribution below `min_mass`) or `distribution_absent` (`min_mass` set
   and the distribution absent). The
   judgment carries a spec 033 `field_dropped` notice (033 3.5.4) with
   subject the step id, reason the declared notice code, and an action
   whose `caused_by` is the source with `observed{model-derived}` (an
   extracted field is named `<step>/<field>`), so its computed party is
   `host` (033 3.5.2). Dependents see absence (032 3.4.4): a filter can
   continue without the value, and a by-value read is `missing_evidence`.
   `unresolved`: the step is `invalid_input{fields: [field], detail:
   cause}`, or `missing_evidence{fields: [field]}` when the source was
   absent, and the policy must handle it (category 8), including with 033's
   `propagate_with` and `needs_input` handler actions. The named field is
   the input's name for a model-derived input and `<step>/<field>` for an
   extract field, as spec 036 names a step in `missing_evidence{fields:
   [<step>]}`; its 033 3.5.6 cause is that source with
   `observed{model-derived}`, as in the `drop_field` notice, so a
   propagated outcome is addressed to `host`.
5. **Derivation.** An accepted or passed-through value keeps exactly the
   runtime derivation class of the value read; acceptance is a check, not a
   computation, so it neither lowers nor raises the class. Its lineage is
   the source's plus the step and every value `validate` read.
6. **Compile checks** (category `invalid_acceptance`, phase e): `field` is
   not a model value; `validate` reads another unaccepted model value or is
   not boolean; `min_mass` names a field that is not eligible or an option
   it lacks. An accept step is a node of the step graph, with edges from
   its `field` source and from every value `validate` reads, so a cycle
   through accept steps is refused by spec 002's category 4 `cycle`
   (phase d), before these checks.

### 3.9 The model-value gate

1. In a version 2 definition the compiler refuses, in phase (e) with
   category `unaccepted_model_value`, any exact consumer (3.1.2) that
   reads a model value directly, and any model step projection (`semantic`,
   `generate`, `extract`) or template parameter that reads an extract field
   directly (037 3.1.5), naming the reader and the model value.
   Presence-only reads, accept steps and the mass conditions of 3.6.4 are
   not readers in this sense. A model step projection may read an input
   that accepts `model-derived`, as today.
2. **Accepted by a check.** An input that is the subject of a spec 033
   check covering `model-derived` (by branch or `default`) is accepted for
   every exact consumer that reads it: the check runs before any step (033
   3.4.1), so a failing model-derived value has already met its declared
   consequence. This is how Pattern A's future-window example of 033 3.2.4
   is accepted without an accept step.
3. An accepted value may feed any exact consumer, including a proposal
   parameter (OD-2, R-41), and another model step's projection. It never
   feeds a selection-policy control: such a read is refused, category
   `unaccepted_model_value`, detail `policy_control`, accepted or not (R-14
   is retained).
4. Acquisition requests: spec 036 3.9.2 requires request references of
   static class `exact-derived` and leaves model-derived values to a later
   spec's acceptance rules. This spec admits into a request field, guard or
   stop count exactly two kinds of non-exact reference: an accept step's
   value, and an input accepted by a check under 3.9.2. Every other
   non-exact reference stays refused as 036 states. This amends 036 3.9.2
   by name.
5. A version 1 definition has no model values in this sense and is not
   gated: it compiles exactly as before (3.14).

### 3.10 Backend capability and output

1. `Operation` gains `extract` (OD-4, R-42). `OutputKind` gains
   `typed_record{structure, max_output_bytes, max_options,
   option_logprobs}`, following 037 3.17's `text_record`: `structure` is
   `json_schema`, `tool_call` or `grammar`; `max_options` (1 to 32) is the
   most fields per record; `option_logprobs` is a boolean. A descriptor without `extract` has its existing bytes
   and `DescriptorId`.
2. A descriptor satisfies an extract step (phase f) when it supports
   `extract`, its `max_options` is at least the field count,
   `max_output_bytes` is at least the schema's worst-case record bytes,
   its input limit admits the worst-case request, its determinism and
   artifact meet `backend`, and, when a threshold reads one of its
   distributions, `option_logprobs` is `true`. Shortfalls are listed as
   category 5 `no_capable_backend`.
3. `RawOutput` (`rustev.backend-output/1`) gains `typed_record{fields,
   logprobs, provider_reported}`: `fields` a JSON object, `logprobs` a list
   of `{field, item, values}` with `item` `none` or an index and `values`
   option to finite `f64`, `provider_reported` field name to string. Logprobs
   on a binding without `option_logprobs`, keys outside the field's declared
   options, a non-finite value, and a `provider_reported` entry that names
   an undeclared field or holds more than 256 UTF-8 bytes are record-level
   faults (3.4.2); nothing is truncated.
4. Per-field value faults are not record-level: they make fields absent
   (3.4.3) and are never retried.
5. Remote adapters (spec 009) and the Bedrock backend (spec 030, which
   reserves `extract` in its binding, 030 3.12) gain `extract` by their own
   changes on top of this contract (section 4).

### 3.11 Runtime (amends spec 003)

1. An extract request is dispatched like a semantic request: admission,
   concurrency, cost reservation before dispatch, retries, runtime
   fallback, cancellation and the attempt trace of spec 003 3.3 to 3.8
   apply. Spec 003 3.2.3 no longer refuses an execution policy naming an
   extract step as non-semantic; a fallback target must satisfy 3.10.2.
2. **Step deadline.** The request's deadline is `min(D, T_x +
   floor(s * L))`, where `s` is the step's `limits.deadline_share` (3.3.2),
   `D` is the decision deadline (spec 003 3.3.2), `L` is the definition's
   own `limits.deadline_ms` (spec 002 3.9.1, the duration behind `D`) and
   `T_x` is when the core lists it, computed with spec 002's decimal arithmetic, as 037
   3.12.2 computes a stage deadline. No attempt is dispatched at or after
   it; an unresolved request at it is supplied as `deadline_exceeded`.
3. **Cost.** Under `max_cost_units: units{n}`, a step sub-ledger of n units
   is reserved first, as 037 3.12.4; a refused reservation is
   `budget_exhausted`.
4. **Screening.** As 037 3.11 does for generation, and as 035 3.17.3 asks:
   spec 035's `unscreened_text` check applies to every extract step whether
   or not the definition declares screening; a request whose projection
   would hold a withheld value is not dispatched and every field is absent
   with `screen_blocked`; under `fail_closed` with the screener unavailable
   every field is absent with `screen_unavailable`. The fallback then runs
   under 3.7.5. Extraction never proceeds over a withheld marker. Until
   spec 035 is implemented, an extract step whose projection can carry
   untrusted text (035's definition) is refused, category
   `invalid_extraction`, so extraction fails closed.
5. A request that ends as a failure (spec 003 3.6.4) makes every field
   absent with `request_failed{reason}`; the fallback extractor then runs
   when declared. The decision continues.
6. **Capture and replay.** Supplied `typed_record` outputs are captured as
   any supplied value, subject to the host's retention policy (R-19).
   Bundles of plan 3 decisions are `rustev.replay/2` (spec 036); capture
   before 036 is implemented follows 032 3.11.2. Replay supplies the
   captured record and never dispatches; it reproduces every field, origin
   and distribution exactly.

### 3.12 Evidence and judgment

1. **Evidence 2, extract steps.** The optional member `extraction`, omitted
   when the plan has no extract step: per step, the template identity,
   schema digest, bound backend artifact, fallback program identity or
   `none`, the request outcome, and per field: name, origin, derivation,
   absence reason, rejected list items (index, reason, value digest), the
   value digest (032 3.1.4), the value itself for `bool`, `integer`,
   `decimal`, `date` and `enum` fields and lists of `enum` items, whatever
   the vocabulary's source (literal, profile parameter or input), and never
   for `text` fields or lists of `text` items (digest-only, per field and
   per item, R-19), the distribution (masses in option order, or `absent{reason}`),
   the calibration applied or `none`, and `provider_reported`.
2. **Evidence 2, accept steps and inputs.** The optional member
   `acceptance`: per accept step, the value read, its origin, the outcome
   (`accepted`, `passed_through`, `dropped{cause}`, `unresolved`) and the
   validation result. Per input: the producer of the selected entry when
   present. Lineage `producers` as 3.5.4 states.
3. **Judgment.** A plan with an extract or accept step produces
   `rustev.judgment/2` (introduced by spec 033), to which this spec adds:
   proposal parameters whose value comes from an accept step, or from an
   input accepted by a check, are written as `attributed{value, derivation,
   origin}`, with `origin` `supplied`, `extracted`, `fallback` or
   `host_extracted{producer}`; a parameter from any other value keeps its
   encoding. Notices: `field_dropped` (3.8.4) and, for a field filled by
   the fallback, a `fallback` notice (existing kind, causes
   `capability{step}`) naming `<step>/<field>`.
4. These members are what spec 041 (#76 E1) reads for per-field accuracy
   and absence rates broken down by origin.

### 3.13 Needs input from extraction

1. **Insufficient requests.** A bool field such as `insufficient` is an
   ordinary field. A policy rule whose condition reads its accept step can
   produce spec 033's `needs_input{fields, questions}` outcome (033 3.7),
   whose fields must be inputs accepting a caller-controlled class and
   whose action is addressed to `caller` by construction.
2. **Model-proposed question selection** (the extension 033 3.7.3 leaves to
   this spec). A `needs_input` outcome's `questions` may be
   `{proposed_by, allowed}` instead of a list: `allowed` is a non-empty list
   of declared question template ids, and `proposed_by` names an accept
   step whose source is an extract `list` field of `enum` items whose
   vocabulary is a literal list equal, as a set, to `allowed` (category
   `invalid_acceptance` otherwise). At evaluation the questions are the
   accepted proposed ids in the order of `allowed`; when the accept step's
   value is absent or empty they are all of `allowed`. Question text is
   always the declared template's (033 3.7.3): an extraction chooses among
   declared questions and never supplies text.
3. The `continuation` digest covers the questions actually selected.

### 3.14 Compatibility and identity

1. `rustev.definition/1`, `rustev.plan/2`, `rustev.snapshot/1`,
   `rustev.evidence/1`, `rustev.judgment/1`, `rustev.exact/1`, the backend
   descriptor, output and request documents without the new variants,
   every refusal category 1 to 12 with its phase and order, and every
   operator's meaning are unchanged.
2. Every committed definition, plan, judgment, evidence, run and CLI
   golden, including both reference plans, keeps its bytes and `PlanId`; a
   test recompiles each through the amended compiler and compares bytes.
3. A version 2 definition that uses no construct of this spec compiles as
   spec 032 and its successors define; the new members are omitted, so its
   bytes and `PlanId` do not depend on this spec, except that the gate of
   3.9 refuses its direct exact reads of an input accepting
   `model-derived` that no check accepts (Decision 5).
4. A plan 3 `PlanId` changes with any extract step's template document or
   bindings, schema, projection, vocabularies, `distributions`, limits,
   backend binding or fallback program, and with any accept step's member.
   It does not change with producer identities, input vocabulary values or
   calibration artifacts not bound.
5. New enum variants (`extract`, `typed_record`, step kinds, the request
   operation, judgment `attributed`, categories) extend closed enums; no
   existing canonical name moves.

### 3.15 Lodging reference example

Under `packages/rustev-pkg-lodging/tests/extraction/`, synthetic fixtures
only (R-04), a version 2 example definition that is not the package's
reference definition:

1. **Pattern A.** Input `trip.window` (a record with `start` and `end`)
   accepts `user-supplied` and `model-derived`; `caller_controlled` lists
   `user-supplied`. The spec 033 check `future_window` of 033 3.2.4
   (refuse with remediation `send_future_window` for `user-supplied`,
   `drop_field` with notice `inferred_window_dropped` for `model-derived`)
   accepts the input (3.9.2). The date filter reads `trip.window` with a
   presence predicate so it applies only when the window is present. One
   snapshot supplies a past window as `model-derived` with a producer: the
   window is dropped with a `field_dropped` notice addressed to `host`, and
   the decision proposes, undated. Another supplies the same past window as
   `user-supplied`: the judgment is `refused`, addressed to `caller`, with
   remediation `send_future_window` and `caused_by` the window with
   `observed{user-supplied}`. A third, a future window from either source,
   is used as supplied.
2. **Pattern B.** An extract step over `trip.text` with the field set of
   3.3.1, a feature vocabulary input, and a fallback program; the accept
   step `window_ok` of 3.8.1 feeds the same date filter. Fixtures cover a
   valid record, an out-of-vocabulary feature, an out-of-bounds party
   size, an over-long area name, an extracted past window (dropped,
   decision continues undated), a backend failure filled by the fallback
   (origin `fallback`, `exact-derived`), and a record with
   log-probabilities for `insufficient`.
3. **Needs input.** The accepted `insufficient` field set to `true` yields
   `needs_input` for `trip.window` with a declared question, and a second
   fixture lets an extract field propose questions among a declared set
   (3.13.2).

### 3.16 Alignment with sibling #76 specs

1. **037 (B5).** `extract` uses the envelope of 037 3.1 and the members
   `for_each`, `template` (`rustev.prompt-template/1`), `projection`,
   `vocabularies`, `repair`, `screen`, `limits` and `backend` with 037's
   names; field reason codes shared with 037 3.7.4 keep 037's names; the
   output kind follows 037 3.17's form; isolation follows 037 3.1.5 (an
   extract field reaches another model step only through acceptance).
   Where 037 changes a shared name before it merges, the implementing
   change of this spec follows 037's final name without changing behavior.
2. **033 (B3).** Checks covering `model-derived` are acceptance rules
   (3.9.2); `field_dropped` notices and actions (3.8.4); `refused`,
   `needs_input` and `propagate_with` outcomes (3.13, 3.15); judgment 2
   (3.12.3); the question selection 033 3.7.3 leaves to this spec (3.13.2).
3. **032 (B2)** provides the shared versions and presence. **035 (B7)**
   provides screening (3.11.4). **036 (B1)** is amended by 3.9.4 and owns
   `rustev.replay/2`. **041 (E1)** reads 3.12. **034 (B4)** profile
   parameters are selection-policy controls (3.1.2) and may bind
   vocabularies and template parameters.

## 4. Out of scope

Implementation; fan-out (`for_each`) over extract steps; repair attempts of
an extraction; streaming; extraction during 037's generation stage; the
remote protocol mapping of `extract` (spec 009) and Bedrock's `extract`
mapping (spec 030 3.12), each its own change on this contract; producer
pinning or verification; quality or calibration claims for any extractor
(R-04, R-37); mass thresholds over single list items (3.6.1); multi-turn continuations of `needs_input` (future work after
#76).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Model returns `party_size: 45` with bounds `[1, 30]` | Absent `out_of_bounds`; not clamped. |
| Model returns `party_size: "4"` | Absent `wrong_type`; not parsed. |
| Model returns features `["pool", "spaceship"]`, vocabulary lacks `spaceship` | Present `["pool"]`; item 1 rejected `not_in_vocabulary` with its digest. |
| Model returns only `["spaceship"]` | Absent `not_in_vocabulary`. |
| Model returns 11 features with `max_items: 10` | Absent `too_many_items`; not truncated. |
| Model returns features `[]` | Present, empty list; no item rejected. |
| `area_name` of 65 bytes with `max_bytes: 64` | Absent `too_long`; not truncated. |
| `window.start` as `"10/03/2026"` | `window` absent `invalid_date`, subfield `start` named. |
| Model returns an undeclared member `price` | Record-level `invalid_output`; retry and fallback policy applies. |
| Model returns a self-reported `confidence` per field | `provider_reported`; no distribution. |
| `provider_reported` string of 300 bytes for a field | Record-level `invalid_output`; retried or falls back; not truncated. |
| Model returns `price: "12.345"` with `scale: 2` | Absent `out_of_bounds`; not rounded. |
| Logprobs for 3 of 4 options of an enum field | Value kept; distribution `absent{incomplete_logprobs}`. |
| `mass_at_least` on an extracted field with no calibration and no `uncalibrated_threshold` | Refused, category 6. |
| Threshold on a field bound to an output kind without `option_logprobs` | Refused, category 3. |
| Exact `compare` reads `step:request_filters/party_size` directly | Refused `unaccepted_model_value`. |
| A `classify` projection reads an extract field directly | Refused `unaccepted_model_value`. |
| Version 2 `arith@1` reads an input accepting `model-derived`, no check covers it | Refused `unaccepted_model_value`; version 1 unchanged. |
| The same read where a 033 check covers `model-derived` | Compiles. |
| An accepted value used as a weight, threshold or template parameter | Refused `unaccepted_model_value`, detail `policy_control`. |
| Accept step on a `user-supplied`-only input | Refused `invalid_acceptance`. |
| `validate` reads another unaccepted extracted field | Refused `invalid_acceptance`. |
| Extracted past window under `drop_field` | Absent; `field_dropped` notice addressed to `host`; decision continues. |
| Caller-supplied past window under the 033 check | `refused`, addressed to `caller`, remediation `send_future_window`. |
| Backend unavailable, fallback declared | Fields `fallback` (`exact-derived`) or absent `fallback_no_match`; decision continues. |
| Fallback program not supplied, or reads a source not projected | Refused `invalid_extraction`. |
| Two fallback rules of one list field with the same literal item | Refused `invalid_extraction`. |
| A fallback rule of a list field with a non-literal value | Refused `invalid_extraction`. |
| Every item of a `list` of `text` items is over `max_bytes` | Absent `too_long`. |
| Input vocabulary accepting `model-derived` | Refused `invalid_extraction`. |
| Input vocabulary of type `list{item: text}` holding 5000 items at evaluation | Every field using it absent `vocabulary_unavailable`; not truncated. |
| `repair.max_attempts` 1 | Refused `invalid_extraction`. |
| `for_each` other than `none` on an extract step | Refused `invalid_extraction`. |
| Extract over `user-supplied` text before spec 035 is implemented | Refused `invalid_extraction`. |
| `questions.allowed` differs from the proposing field's vocabulary | Refused `invalid_acceptance`. |
| Snapshot 2 entry `user-supplied` with a producer | Snapshot refused `invalid_producer`. |
| Snapshot 2 supplied to a plan 2 | Refused `snapshot_version` (032 3.1.3). |
| Screener blocks `request.text` | Request not dispatched; fields absent `screen_blocked`; fallback may fill. |
| Template document changed | Different digest, `DefinitionId`, `PlanId` and `RequestId`. |

## Acceptance

- Same definition, two snapshot variants (3.15.1): the model-derived past
  window is dropped and the decision continues undated, with a
  `field_dropped` notice addressed to `host`; the caller-supplied past
  window is `refused`, addressed to `caller`, with remediation
  `send_future_window`. The same drop holds for an extracted window
  through an accept step (3.15.2).
- Out-of-vocabulary feature ids are rejected with reasons per item, a list
  of only invalid ids is absent, and no case of the section 5 table is
  coerced.
- The fallback extractor path yields `exact-derived` fields with origin
  `fallback` over exact sources and `mixed-derived` over a model-admitting
  source; results are identical in native and `wasm32` builds.
- Per-field distributions appear only from complete log-probabilities;
  thresholds without a bound calibration or declared
  `uncalibrated_threshold` are refused at compile time; self-reported
  confidences never become mass.
- The model-value gate: golden refusals for each reader kind; accept steps
  and 033 checks covering `model-derived` pass; selection-policy controls
  are refused even when accepted.
- `needs_input` from an accepted `insufficient` field, and question
  selection limited to a declared set with declared text.
- Pattern A producers recorded per input and in lineage; `invalid_producer`
  and future `produced_at_ms` cases.
- Per-field evidence (3.12) is sufficient for spec 041's
  `origin/extraction` metrics, demonstrated by a fixture once 041 is
  implemented.
- Every existing golden and reference `PlanId` is byte-identical (3.14).
- `make code`, declared verification and `make gate` pass.

The fixtures that need specs 032, 033, 035, 036 or 037 implemented run
when those implementations are present; until then they are reported as
externally blocked, never as passed.

## Verification

Pending with implementation. The implementing change adds `038` to `make
verify` with repository-local deterministic tests over scripted backends
and synthetic fixtures for every acceptance item. No live provider call is
part of declared verification.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
contract vocabulary: the `extract` and `accept_model_derived` declarations,
the extraction schema and the snapshot producer; (2) the remaining contract
members: `rustev.extract-rules/1`, `rustev.extract-request/1`, the
`typed_record` output, the plan 3 step details, the evidence 2 and judgment
2 members and the refusal categories; (3) field validation, presence and
distributions in the core; (4) the fallback extractor; (5) compilation of
both kinds, acceptance and the model-value gate; (6) the runtime (3.11);
(7) needs input, the lodging example, declared verification and closure.

**Increment 1 (2026-10-26): the contract vocabulary.**
`crates/rustev-contract/src/extraction.rs` adds the `extract` step body of
3.3 with the extraction schema of 3.4.1, reusing spec 037's envelope types
for the template, vocabularies, repair, screen, limits and backend, and the
`accept_model_derived` step body of 3.8.1 and 3.8.2, each with its
document-local checks (`for_each` `none` only, no repair, no output
screening, declared vocabularies, field names, bounds, list and record
item kinds, notice codes). Both kinds are version 2 constructs: in a
`rustev.definition/1` they are refused, category 11 `requires-definition-2`;
in a `rustev.definition/2` compilation refuses them with
`invalid_definition` detail `extraction-not-compiled` until the compile
increment. `rustev.snapshot/2` entries gain the optional `producer` of
3.2.1; a producer on an entry that is not `model-derived` refuses the
snapshot with `invalid_producer` (3.2.2), and a producer in a
`rustev.snapshot/1` is refused as a snapshot version error. The predicate
of 3.8.1's illustrative example is spelled with the contract's `cmp`
operator. Documents without these members keep their bytes and identities
(`crates/rustev-contract/tests/extraction_wire.rs`,
`crates/rustev-core/tests/extraction_contract.rs`).

**Increment 2, part a (2026-10-26): the fallback program, the request
document and the typed record.** `crates/rustev-contract/src/extraction_documents.rs`
adds `rustev.extract-rules/1` (3.7.1) with its document-local bounds
(3.7.3, 3.7.4) and tagged digest, and `rustev.extract-request/1` (3.3.5),
whose `RequestId` covers its members through the new `extract` operation
variant of `rustev.request/1`. `Operation` gains `extract`, `OutputKind`
gains `typed_record` and `RawOutput` gains `typed_record` with its
document-local record-level checks (3.10.1, 3.10.3); the schema digest is
the SHA-256 of the schema's canonical bytes. Checks that need the
definition (program sources, field types, declared options) are left to
compilation and validation. Existing request, descriptor and output
documents keep their bytes and identities
(`crates/rustev-contract/tests/extraction_documents.rs`).

**Increment 2, part b (2026-10-26): plan details, evidence and judgment
members and the refusal categories.** `crates/rustev-contract/src/extraction_results.rs`
adds the `rustev.plan/3` step details of both kinds (3.14), the
`rustev.evidence/2` members `extraction` and `acceptance` (3.12.1, 3.12.2)
with lineage `producers` (3.5.4), and the `attributed` parameter encoding
of `rustev.judgment/2` with its four origins (3.12.3); the existing
`field_dropped` notice and the `fallback` notice's `capability` cause
carry 3.8.4 and 3.12.3 unchanged. Evidence lists exactly the members
3.12.1 names: a field entry's value is built only through a constructor
that takes the declared field type and keeps the value for `bool`,
`integer`, `decimal`, `date`, `enum` and lists of `enum`, and the digest
alone for `text` fields and items (R-19). The categories
`invalid_extraction`, `invalid_acceptance` and `unaccepted_model_value`
take 25 to 27 in that order (spec 039 3.12); nothing produces them until
the compile increment. Documents without these members keep their bytes
and identities (`crates/rustev-contract/tests/extraction_results.rs`).

**Increment 3 (2026-10-26): field validation, presence and distributions
in the core.** `crates/rustev-core/src/extraction_validate.rs` implements
the record-level check of 3.4.2, presence with exactly one reason per
absent field (3.4.3), no coercion (3.4.4), the list order of 3.4.5 with
per-item rejections and value digests, records (3.4.6), the derivation
classes of 3.5.2, and distributions by the existing softmax in declared
option order with `incomplete_logprobs` and `not_returned` (3.6.2); a
value is never changed by its distribution. Reason codes shared with spec
037 reuse its validation. A valid `YYYY-MM-DD` date whose 00:00 UTC lies
outside spec 002 3.4.4's `Timestamp` range (before 1970-01-01) has no core
value, so the field is absent `out_of_bounds`; a malformed or impossible
date is `invalid_date`. Nothing calls these functions until the compile and
evaluation increments (`crates/rustev-core/tests/extraction_validate.rs`).

**Increment 4 (2026-10-26): the fallback extractor.**
`crates/rustev-core/src/extraction_fallback.rs` evaluates a
`rustev.extract-rules/1` program over resolved sources (3.7.3 to 3.7.6):
ASCII-folded matching, first applying rule for scalar and record fields,
rule-order accumulation for list fields, `integer_after` and `date_iso`,
absence with `fallback_no_match`, `fallback_invalid` (a produced value
failing 3.4, including a list over `max_items`) and `source_unavailable`
(unresolved, withheld from exact steps, or over `max_projection_bytes`).
Conditions short-circuit in declared order; a field's derivation (3.5.2)
and its recorded sources are the sources actually read while evaluating
it, including reads by earlier rules that did not apply, since their
outcome selected the winning rule. `integer_after` takes the earliest
occurrence of any term, ties going to declared term order. The fill modes
of 3.5.3 merge a fallback result into a model result without replacing a
valid model field. The binding checks of 3.7.2 and the list and digit
refusals of 3.7.3 are a pure `bind` function that the compile increment
maps to `invalid_extraction`; nothing calls the evaluator until then
(`crates/rustev-core/tests/extraction_fallback.rs`).

**Increment 5, part a (2026-10-26): compile-time checks of the extract
step.** `crates/rustev-core/src/extraction_compile.rs` applies the
definition-level checks of 3.3 and 3.7.2 in spec 002's phase order:
structure and the at-most-8 count, projection references, vocabularies
(literal, profile parameter and `{input}`, with the 4096-option bound on
an `enum{options}` input; a `list{item: text}` input's bounds are read at
evaluation, 3.3.2), enum fields naming declared vocabularies, template
resolution and profile parameter sites through 037's helpers, request,
attempt and projection budgets, untrusted projection rules, and the
supplied fallback program through `extraction_fallback::bind` with its
canonical document and identity, each refused `invalid_extraction` unless
3.3 names another category. A definition passing every check still
refuses with `extraction-not-compiled` until part b adds the plan
details, the backend capability phase and the model-value gate
(`crates/rustev-core/tests/extraction_compile.rs`).

**Increment 5, part b (2026-10-26): extract steps compile into plan 3.**
An extract step binds a backend in phase f (3.10; category 5
`no_capable_backend` otherwise), choosing among descriptors in 037's
deterministic order, and fills its plan 3 step detail; the template,
schema, fallback document, backend artifact and limits enter the `PlanId`
(3.14.4), pinned by `crates/rustev-core/tests/golden/extraction.plan.json`
and its `.id`. The model-value gate of 3.9 walks every exact consumer,
model step projection and template parameter in phase e; until accept
steps compile (part c), only an input accepted by a spec 033 check
(3.9.2) passes, so a direct exact read of any other model value refuses
27 `unaccepted_model_value`. Per 3.14.3 this also applies to version 2
definitions without extraction constructs: two existing tests that fed a
model-derived input to an acquisition request or a generate template
parameter now see 27 in phase e before 036's and 037's own later checks.
Evaluating a plan with an extract step is refused until the runtime
increment. Remaining for part c: accept steps, the accepted-value
exemption, extract field-path projections and calibrated thresholds.

**Increment 5, part c (2026-10-26): accept steps compile.** An
`accept_model_derived` step compiles with the checks of 3.8.6 (category
26 `invalid_acceptance`, phase e; a cycle through accept steps is spec
002's category 4 in phase d), its `validate` predicate typed as 3.8.2
allows, and `min_mass` bound to a calibration (its vocabulary, task,
question, backend and parameters checked as 3.6 requires) or declared
`uncalibrated_threshold`. Every accept step member enters the `PlanId`,
pinned by `crates/rustev-core/tests/golden/acceptance.plan.json` and its
`.id`; an accepted value keeps its source derivation. The gate now exempts
accepted values for every exact consumer, model step projection and
acquisition request field, guard or stop count (3.9.3, 3.9.4), and still
refuses a selection-policy control (`policy_control`). Extract field-path
references (`step:<extract id>/<field>` and subpaths) resolve in
projections and accept steps. Left for a later part: calibrated policy
thresholds over extract fields outside accept steps (3.6.4), whose plan
detail needs a calibration-binding slot; evaluation of both kinds is the
runtime increment.

**Increment 6, part a1 (2026-10-26): core evaluation of extract steps.**
`crates/rustev-core/src/extraction_eval.rs` lists one
`rustev.extract-request/1` per decision and step when its projection
resolves, accepts a supplied `typed_record` output, validates it (3.4),
records field origins, derivations, distributions and lineage with
producers (3.5, 3.6), runs the fallback under its `fill` mode or on a
request failure (`request_failed{reason}`, 3.11.5), and takes the
screening outcomes of 3.11.4 (`screen_blocked`, `screen_unavailable`) from
the runtime. The evidence 2 `extraction` member is filled through the
increment 2b constructors (R-19); evidence and judgment bytes are pinned
by `crates/rustev-core/tests/golden/extraction.evidence.json` and
`extraction.judgment.json`. A plan with an accept step is refused at
evaluation until part a2 (`crates/rustev-core/tests/extraction_compile.rs`).

## Decisions at approval

Each is decided within this spec under the work order, inside the binding
principles; none relaxes one.

1. **Two forms of acceptance.** An `accept_model_derived` step, a node in
   the plan graph identified in `PlanId`, accepts extract fields and
   inputs; a spec 033 check covering `model-derived` accepts its subject
   input, because it already declares the validation and its consequence
   before any step runs. No other path exists.
2. **Schema failure is absence, not refusal.** A field that fails 3.4 is
   absent with a reason, and the decision continues; refusing the whole
   decision is the definition's choice through `on_fail: unresolved` and
   policy handlers, or through 033 checks.
3. **Lists reject per item.** The issue says out-of-vocabulary ids are
   absent with reasons; items are rejected individually and the field keeps
   its valid items, but a list of only invalid items, an over-long list or a
   duplicate makes the field absent, because truncating or deduplicating
   would be coercion.
4. **Fallback in the core.** The fallback extractor is a closed, bounded
   document evaluated in `rustev-core`, not a backend, because the issue
   requires its fields to be `exact-derived` and every backend output is
   `model-derived` (spec 002, 3.12.2).
5. **The gate covers Pattern A inputs in version 2 only.** The issue
   requires acceptance "in both" patterns, so a version 2 input accepting
   `model-derived` is gated unless an accept step or a covering check
   accepts it. Version 1 definitions keep today's behavior, so no existing
   compile result changes. This also applies to version 2 definitions that
   use only other #76 constructs; none is released or implemented.
6. **Proposal parameters but never controls.** OD-2 lets accepted values
   feed what extracted values otherwise cannot; this spec admits them into
   exact operators, predicates, proposal parameters, acquisition requests
   and other model steps, and keeps every selection-policy control closed
   to them, which is stricter than OD-2 requires and keeps R-14 intact.
7. **Envelope names from 037.** The issue's `from` is 037's `projection`,
   its `enum_ref` is a named entry of 037's `vocabularies` (extended with
   input vocabularies), and its `screen` member is 037's, always
   `untrusted_fields`, so every model step is declared the same way.
8. **Unknown members are record-level faults,** as 037 3.7.3, so an output
   that does not match the declared schema is retried or falls back rather
   than partly read.
9. **Dates.** `date` is a calendar date that becomes a `timestamp` at 00:00
   UTC, a declared mapping of a declared type, so exact time operators can
   read it.
10. **Calibration binding by convention.** Field calibration uses the
    existing `rustev.calibration/1` with `task` and `question` naming the
    template and field, so no calibration document version is needed.
11. **No fan-out, no repair** in this version (`repair.max_attempts` 0).
12. **Question selection.** Extraction may choose questions only within a
    declared set equal to its field's vocabulary, with declared text, which
    is the bound 033 3.7.3 sets.
13. **Producer is optional and unverified.** The issue makes it optional;
    Rustev owns no identity, so it records and never verifies it.

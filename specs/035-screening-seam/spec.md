---
id: "035-screening-seam"
title: "Untrusted-text screening seam (amends 002, 003 and 009)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002, 003 and 009, for #76 child B7 (#65).
  A definition may declare a `screening` block that names untrusted text
  fields (caller text, third-party text, list item text), the screener
  backend that judges each one, a consequence on block (`refuse`,
  `drop_field`, `drop_text`, `drop_item`) and a consequence when the
  screener is unavailable (`fail_closed`, `skip_generation`, `fail_open`).
  The block compiles down to ordinary `proposition` steps, one text value
  per request, plus consequence rules evaluated before the selection
  policy; a screener is an ordinary backend that declares a `screen`
  profile (categories, maximum text bytes, text unit size) and uses the
  general per-capability `isolation: per_item` member this spec
  introduces. In a
  definition that declares screening, the compiler refuses a model step
  that reads unscreened untrusted text. A blocked value under `refuse`
  yields spec 033's `refused` outcome with its computed addressing. Adds a
  `screen_unavailable` unresolved reason, an explicit withheld marker in
  projections, per-value
  screening evidence with digests only, text-unit budgets, verdict caching
  through spec 017's cache, the reserved `rustev.screening` metric of spec
  028, a per-item isolation rule for spec 009 remote adapters, and the
  mapping constraints for spec 030's reserved Bedrock screening values.
  New members go into the shared #76 track versions (`rustev.definition/2`,
  `rustev.plan/3`, `rustev.evidence/2`, `rustev.judgment/2`,
  `rustev.run/3`); definitions, descriptors and plans that use no #76
  construct keep byte-identical identities. Approved (A-32);
  implementation is in progress.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "009-remote-adapter-protocol"
extends:
  # Definition `screening` member, descriptor `screen` profile, `screen`
  # output and required kind, `screen_unavailable` reason, screening
  # notice, plan, evidence and remote vocabulary.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Lowering, compile checks, consequences, withheld marker, evidence.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Per-item isolation, withdrawal on refusal, units, cache and run record.
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Client and server per-item isolation for remote screeners.
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: amending }
  # Exhaustive matches over the new outcome and output variants only.
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  # Absent-member initializers where a struct literal names every field,
  # and the regenerated document types of the bindings; no behavior.
  - { spec: "005-reference-backends", unit: { kind: directory, path: "backends/rustev-backend-rules/" }, nature: additive }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  - { spec: "030-bedrock-backend", unit: { kind: directory, path: "integrations/rustev-bedrock/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  # A new contract source file joins rustev-contract's reviewed archive list.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Adds 035 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "009-remote-adapter-protocol"
  - "017-general-runtime-optimization"
  - "020-remote-protocol-refinements"
  - "028-opentelemetry-evidence-sink"
  - "032-evidence-quality"
  - "033-provenance-rules-and-actionable-outcomes"
  - "034-parameterized-definitions"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Under every consequence except a declared `fail_open`, no model step request carries screened text whose verdict is not `pass`: blocked or unavailable text is withheld by the marker of 3.8 or the request is not dispatched."
    anchor: "3-8-withheld-values-in-projections"
  - id: "I-2"
    kind: invariant
    text: "A request to a screener whose capability declares `isolation: per_item` carries exactly one text value per call under every batching, coalescing and dispatch setting of specs 009, 017 and 020."
    anchor: "3-10-isolation-and-batching"
  - id: "I-3"
    kind: invariant
    text: "Definitions that use no #76 construct, descriptors without a `screen` profile, and plans compiled from them keep byte-identical canonical documents and identities, and their judgments, evidence and run records keep byte-identical bytes."
    anchor: "3-18-compatibility-and-identity"
  - id: "I-4"
    kind: invariant
    text: "No screening evidence, notice, run record member or metric carries screened text; text is identified only by its tagged digest."
    anchor: "3-13-evidence"
  - id: "R-1"
    kind: requirement
    text: "In a definition that declares screening, the compiler refuses with `unscreened_text` any model step whose projection can carry untrusted text not covered by a screening entry."
    anchor: "3-5-compilation"
  - id: "R-2"
    kind: requirement
    text: "The compiler refuses `fail_open` on any field that accepts `user-supplied` or `authenticated-app-field`, whether or not the definition declares `caller_controlled`, or any other class the definition declares caller-controlled; and a screening refusal is addressed to the caller only when spec 033's computed party is `caller`."
    anchor: "3-5-compilation"
  - id: "R-3"
    kind: requirement
    text: "The first blocked value under a `refuse` entry makes the judgment spec 033's `refused` outcome at once, and no request not yet dispatched for that decision is dispatched afterwards."
    anchor: "3-7-screening-refusals"
---

# 035: Untrusted-text screening seam (amends 002, 003 and 009)

Approved by the owner (A-32, under the 2026-10-16 delivery work order) as
its own reviewable change before any code (R-16); implementation is
pending. This spec implements #76 child B7 (#65). The approved texts of
specs 002, 003 and 009 are not edited; this spec records what changes, as
013 does for 003. It fills members that spec 028 (the `rustev.screening`
counter) and spec 030 (the `screen` operation and `guardrail` member)
reserved for this child. It builds on #76 specs that merge before it in
number order: spec 032 introduces the shared track versions, spec 033
the `refused` outcome, `caller_controlled` and computed addressing. It
introduces the general `isolation` descriptor member (3.2.2), which spec
044 (#76 F1, merging later) applies everywhere. Approval authorizes no
live provider call or spend, no real-user data, no release, and no
claim that any screener detects any attack or makes any text safe.

## 1. Purpose

Pipelines that pass caller text and third-party text (catalogue
descriptions, reviews, messages) to model steps need prompt-injection and
unsafe-content screening before the text reaches a model. Today a host does
this outside Rustev, so a plan cannot state what protected its model steps,
evidence cannot show it, and later model steps (#76 B5 generation, B6
extraction) cannot be held to it. The needs are:

1. blocked caller text refuses the request with an outcome addressed to the
   caller, or drops that field and continues, as the definition declares;
2. a third-party item whose text is blocked is dropped or keeps its place
   with its text withheld, and the decision does not fail;
3. screeners bill per text unit and rate-limit, so verdicts are cached and
   budgeted;
4. batching items into one screening call can be unsafe: one item in a
   batch has been observed to change another item's verdict, so some
   screeners must be called one item per request;
5. an unavailable screener fails closed by default.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the definition `screening`
  member (3.4), the descriptor `screen` profile (3.2), the `screen` output
  and `screen_verdict` required kind (3.3), the screening refusal's check
  name (3.7), the `screen_unavailable` reason (3.9), the `screening` notice
  kind, the plan's `screening` member (3.5.8), the evidence and run record
  members (3.13), and the remote vocabulary (3.15).
- `crates/rustev-core/` (owner spec 002): the lowering and compile checks
  (3.5), verdicts and consequences (3.6 to 3.9), and screening evidence.
- `crates/rustev-runtime/` (owner spec 003): per-item isolation (3.10),
  withdrawal after a refusal (3.7.3), text units (3.12), the verdict cache
  through spec 017 (3.11) and run record members (3.13).
- `integrations/rustev-remote-http/` (owner spec 009): client and server
  isolation (3.15).
- `crates/rustev-eval/` and `crates/rustev-cli/` (owners 004, 006):
  additive only, where an exhaustive match over outcomes, reasons or
  outputs needs the new variants. No evaluation document, metric, command,
  flag or exit code changes.
- `Makefile` (owner spec 001): 035 joins `make verify` at completion.

**Documents and versions.** Under the #76 coordination rule, this spec
claims no document version of its own. It adds optional members, omitted
when unused, to the shared track versions: `screening` to
`rustev.definition/2` and `rustev.plan/3` (both introduced by spec 032),
`screening` to `rustev.evidence/2` (introduced by spec 032), screening
refusals and the `screening` notice kind to `rustev.judgment/2`
(introduced by spec 033), and the request member `screen` adds to
`rustev.run/3` (introduced by spec 032). A `screening` member
in a `rustev.definition/1` is refused, category 11 `invalid_definition`,
detail `requires-definition-2`, as spec 032 3.1.2 states for its own
constructs. Three further members are added within version 1 documents,
omitted when unused, following spec 017's precedent for the execution
policy's `optimization` member: the per-capability `isolation` member
(3.2.2) and the descriptor operation's `screen`, both in
`rustev.backend/1`, and the `screen` variant of `RawOutput` in
`rustev.backend-output/1`. An empty value of any
new member is refused, so each meaning has exactly one canonical form.

**Amendment of spec 002.** 3.5.6 (`Unresolved`), 3.9.3 (required kinds),
3.9.5 (refusal categories), 3.10 (staged evaluation) and 3.12 (lineage)
gain what sections 3.3 to 3.9 state. Nothing else in 002 changes.

**Amendment of spec 003.** 3.4 (dispatch), 3.9 (run record) and 3.10
(deferred optimization, now spec 017) gain 3.7.3, 3.10, 3.11, 3.12 and
3.13 below. Retries, fallback, deadlines, cost ledgers and sink policies
are unchanged and apply to screening requests as to any request.

**Amendment of spec 009.** Part A gains the per-item isolation rule of 3.15
and Part B carries the `screen` output in `rustev.remote-infer/1`; protocol
version, describe and cancel documents, error taxonomy and HTTP paths are
unchanged.

**What does not change.** No new crate, dependency or dependency class:
`make boundaries` is unchanged. No new step kind or execution path:
screening steps are `proposition` steps dispatched through the same
runtime path as every other semantic request. No vendor name enters a crate
outside `integrations/` (spec 009, I-7).

### 2.1 Binding principles of #76

- **Proposals, never grants.** A pass verdict is not authorization and is
  not a claim that text is safe; it only lets the text reach the model
  steps the plan declares. A screening refusal is an outcome of the
  judgment, not an authority decision, and no `Permitted` type is defined.
- **Derivation classes and lineage.** Screening verdicts are
  `model-derived` step values. Every value whose content depends on a
  verdict (a model step reading a screened field, an exact step reading a
  list after `drop_item`, a refused judgment) lists the screening steps in
  its lineage and is at least `mixed-derived` (3.6.5).
- **Unresolved, never a default; no silent kind conversion.** A screener
  that does not answer yields `unavailable`, never `pass`. Under
  `fail_closed` the reading model steps resolve as `screen_unavailable`;
  nothing substitutes a verdict, a text or a value. Withholding is an
  explicit, declared consequence carried by an explicit marker, and the
  `screen` output is never read as a label, distribution or probability.
- **Pure core; no async runtime in contract and core.** Lowering,
  consequences, markers and digests are pure functions in core; caching,
  isolation and units are runtime work. Core reads no clock and performs no
  I/O.
- **New dependency classes only under `integrations/`.** None is added.
- **No dynamic plugin loading.** A screener is a backend the host
  constructs and passes in, including one implemented in a host language
  through the Node binding (R-38).
- **No identity, authority or knowledge store.** The verdict cache is
  spec 017's bounded cache under the host's scope and retention; Rustev
  keeps no list of blocked texts or senders.
- **Plan identities.** Plans that use no #76 construct keep
  byte-identical documents and `PlanId`s (3.18).

## 3. Behavior

### 3.1 Terms

1. **Text value.** A value of type `text` (spec 002, 3.7.2): a `text`
   input, a `text` field of a record input, or a `text` field of each
   record of a list input. Enum values are declared options and are not
   text values.
2. **Untrusted.** A text value is untrusted when its input accepts any
   provenance class other than `system-of-record`. Provenance is per input
   entry, so every text field of a record or list input shares its input's
   classes.
3. **Caller-controlled classes.** The definition's `caller_controlled`
   declaration of spec 033 decides addressing (3.7). For the `fail_open`
   refusal of 3.5.3 only, a field is **caller-supplied** when its input
   accepts `user-supplied`, `authenticated-app-field` or any declared
   caller-controlled class: the fixed pair keeps `fail_open` refused for
   caller text even in a definition that declares no `caller_controlled`
   set.
4. **Model step.** A semantic step (`classify`, `proposition`, `rubric`,
   `rank`), and the `generate` and `extract` steps of #76 B5 and B6 when
   they land. Its projection (`project`, the `instance` member, and every
   bound item) is the set of values it reads; spec 009 3.8.3 already makes
   the projection list the disclosure list.
5. **Screened value.** One text value at one position: a field, or a field
   of one list item identified by the item's id. Each screened value is one
   screening request.

### 3.2 The screen profile (descriptor)

1. A backend that screens declares, on its `proposition` entry of
   `rustev.backend/1`, the member `screen`:

   | Member | Meaning |
   |---|---|
   | `policy_version` | non-empty string naming the screener's policy (for example a guardrail version); part of `DescriptorId` |
   | `categories` | non-empty, sorted, unique list of category names the screener reports, at most 64 |
   | `max_text_bytes` | the longest text, in UTF-8 bytes, one request may carry |
   | `text_unit_bytes` | bytes per billed text unit, positive |

   The entry's `output` is `screen` (3.3). A descriptor whose `proposition`
   entry has `output: screen` without a `screen` member, or a `screen`
   member with another output, is refused at parse (category 1). The member
   is absent for every other backend (2, documents and versions).
2. **Isolation.** Whether a screener may be batched is not a screening
   member. This spec introduces the general per-capability descriptor
   member `isolation`, valid on every capability: `per_item`, or the
   default `shared`, which is omitted so existing descriptors keep their
   bytes. A screener's `per_item` is one instance of that member, on its
   `proposition` entry, with the meaning 3.10 states. Spec 044 (#76 F1),
   which merges later, applies the same member to every other capability
   and batching path.
3. A screener's `input_limit.on_excess` must be `refuse`: truncated text is
   unscreened text. The compiler checks this (3.5.4).
4. A screener is capability-matched, identified, budgeted, retried,
   evaluated and swappable like any backend. A change of policy version,
   categories, limits or `isolation` is a different `DescriptorId` and so a
   different `PlanId` (spec 009, 3.2.5 drift rules apply).

### 3.3 The screen output and required kind

1. `RawOutput` (`rustev.backend-output/1`) gains the variant
   `screen{blocked, categories, units}`: `blocked` a boolean; `categories`
   a sorted, unique list, each declared in the profile, empty allowed;
   `units` is `reported{n}` with `n` a non-negative integer, or
   `not_reported`. Spec 002 3.10.2 validation applies: an undeclared or
   duplicated category is `invalid_backend_output`.
2. `OutputKind` gains `screen`; `RequiredKind` gains `screen_verdict`,
   satisfied only by `screen` and only for `proposition`. Only lowered
   screening steps (3.5.1) may require it; an authored step that does is
   refused (category 11). A `screen` output is never converted to a label,
   distribution or probability; its only reader is the consequence logic of
   3.6.

### 3.4 Screening declarations

1. `rustev.definition/2` gains the optional member `screening`:

```json
"screening": {
  "max_text_units": 600,
  "entries": [
    { "id": "caller_text", "fields": ["input:request.text"],
      "item_id": "none", "screener": "input-screen",
      "on_block": "refuse", "address": "computed",
      "on_unavailable": "fail_closed" },
    { "id": "behaviour", "fields": ["input:request.behaviour_text"],
      "item_id": "none", "screener": "input-screen",
      "on_block": "drop_field", "address": "computed",
      "on_unavailable": "fail_closed" },
    { "id": "catalogue", "fields": ["input:candidates[]/description"],
      "item_id": { "field": "id" }, "screener": "catalogue-screen",
      "on_block": "drop_text", "address": "computed",
      "on_unavailable": "skip_generation" }
  ]
}
```

2. Members of an entry, all required:
   - `id`: unique among entries, a valid step name (spec 002);
   - `fields`: 1 to 32 references, each `input:<name>` (a text input),
     `input:<name>/<field>` (a text field of a record input) or
     `input:<name>[]/<field>` (a text field of each record of a list
     input); all list references of one entry name the same list;
   - `item_id`: `none` for an entry without list references, otherwise
     `{field}` naming the list records' `text` or `enum` id field;
   - `screener`: the `backend_id` of the screener, an explicit binding
     (spec 002, 3.9.4) so the protecting backend is never chosen by order;
   - `on_block`: `refuse`, `drop_field` (non-list fields only), `drop_text`
     (list fields only: keep the item, withhold the text from model steps)
     or `drop_item` (list fields only: remove the item);
   - `address`: `computed`, or for `refuse` only an override `{to,
     reason}` of spec 033 3.5.3 with `to` one of `host`, `upstream`,
     `none` (never `caller`), which moves the refusal toward a party less
     able to act; the computed party is always recorded;
   - `on_unavailable`: `fail_closed`, `skip_generation` or `fail_open`. The
     typed builder defaults it to `fail_closed`; the canonical document
     always states it.
3. `max_text_units`: a positive integer bounding the text units one
   decision may send to screeners (3.12). At most 32 entries; `entries`
   must be non-empty.
4. The issue's sketch writes `screening` as a bare array; this spec wraps
   it in an object so the unit budget and later members have a place
   without a schema version (Decision 3).

### 3.5 Compilation

1. **Lowering.** For each entry and each field, in declaration order, the
   compiler emits one `proposition` step with id
   `screen:<entry id>:<field index>` (a colon cannot occur in an authored
   id, so no collision), required kind `screen_verdict`, task
   `rustev.screen/1`, question `screen`, options `["false", "true"]`,
   bound to the entry's screener, no calibration and fallback `none`. A
   list field's step fans out over the list's items keyed by `item_id`
   with spec 002's `for_each`, over a generated exact id-list step
   `screen:<entry id>:<field index>:items`; the exact registry form of that
   step is pinned by the plan goldens. Its projection is exactly
   `{"candidates": [], "instance": {}, "operation": "proposition",
   "options": ["false", "true"], "question": "screen", "task":
   "rustev.screen/1", "values": {"text": <the text>}}`: no item id, field
   name or other value, so equal texts produce byte-equal projections
   (3.11). This is spec 002's existing projection envelope, which every
   semantic request carries with all seven members (`candidates` is empty
   for every operation but `rank`); only the content of `values` and
   `instance` is fixed here. Lowered steps are not referenceable by
   authored steps or the policy. Consequences are lowered to the rules of 3.6, evaluated before
   the selection policy's handlers.
2. **Dependencies.** Every model step whose projection can carry a screened
   value depends on that value's screening step. Every step (model or
   exact) reading a list input that has a `drop_item` entry depends on all
   of that list's screening steps. These edges join the topological order
   of phase (d).
3. **`invalid_screening`** (new category, phases b and e), naming the
   entry, when: an entry id is duplicated or invalid; a field reference is
   unknown, not text, or listed in two entries or twice in one; list
   references in one entry name different lists; `item_id` is missing for
   list references, present without them, or names a field that is not
   `text` or `enum`; a consequence does not fit the field shape (3.4.2),
   including `drop_text` on an id field (only `refuse` or `drop_item` fit
   an id field, 3.5.5);
   an override `address` on a non-refusing entry; `fail_open` on a
   caller-supplied field (3.1.3); `entries` is empty or longer than 32;
   `max_text_units` is 0. (An authored step requiring `screen_verdict` is
   category 11, 3.3.2, not this category.)
4. **Screener capability** is category 5 `no_capable_backend` (phase f)
   for the lowered step, with shortfalls: no `screen` profile; profile
   `max_text_bytes` below a field's declared `max_bytes`; `input_limit`
   below the worst-case projection or `on_excess` other than `refuse`; the
   named backend not supplied.
5. **`unscreened_text`** (new category, phase e). In a definition that
   declares `screening`, a model step is refused, naming the step and the
   field, when its projection can carry an untrusted text value no entry
   covers. A projection can carry: a projected `input:` text value; every
   text field of a projected record or list input; every text field of a
   bound item (`bind:`), including its id field; the `instance` ids of a
   fan-out over a list input's id field; and the text a projected exact
   step copies from its lineage (ids for `filter_with_reasons`, `top_k`
   and `weighted_rank`, the selected field for `extremum`), by a per
   operator table the implementation pins in tests. An id field is covered
   by an entry over that field (only `refuse` or `drop_item` fit an id
   field; `drop_text` on an id field is `invalid_screening`). Definitions
   without `screening` are not checked, so no existing definition changes
   behavior (Decision 2); B5 and B6 apply the check to their own steps
   unconditionally (3.17).
6. **Category numbers.** The two new categories are 18
   `invalid_screening` and 19 `unscreened_text`, the next free after spec
   033's 16 and 17; each has a golden refusal fixture.
7. **Not this spec's categories.** An override `address` naming `caller`
   is refused by spec 033 as category 17 `caller_cannot_act`, not as
   `invalid_screening`.
8. **Plan.** `rustev.plan/3` gains the optional member `screening`: the
   canonical screening block, and per lowered step its entry, field
   reference, screener binding (`backend_id`, artifact, `DescriptorId`) and
   consequence. The lowered steps appear in the step order and bindings
   like any step. Budgets (phase g) count lowered requests against
   `max_semantic_requests`.

### 3.6 Verdicts and consequences

1. **Verdict per screened value**, from what is supplied for its request:
   - `screen{blocked: false}`: `pass`;
   - `screen{blocked: true, categories}`: `blocked{categories}`;
   - any unresolved reason for the request (`backend_unavailable`,
     `invalid_backend_output`, `budget_exhausted`, `deadline_exceeded`,
     truncation under `truncate_visible`): `unavailable{reason}`;
   - the empty string: `empty`, with no request issued. An `empty` value
     has no consequence: model and exact steps read it unchanged, as for
     `pass`, since an empty text carries no instructions.
   An input that is itself missing, stale, invalid or conflicting is not
   screened; its own unresolved reason propagates as spec 002 states.
2. **Consequences of `blocked`.** A blocked value under a `refuse` entry
   decides the judgment at once (3.7), whatever other screening requests
   are outstanding; no other consequence is then applied. Otherwise each
   consequence below applies to a consumer once every screening step that
   consumer depends on (3.5.2) has resolved:
   - `drop_item`: the item is removed from the list for every consumer,
     exact or model; removed ids are listed in a `screening` notice;
   - `drop_text`, `drop_field`: the value is withheld from model steps by
     the marker of 3.8 with cause `screen_blocked`; exact steps and the
     policy read it unchanged (they do not prompt a model).
   Several entries over one item combine: any `drop_item` removes it.
3. **Consequences of `unavailable`** are those of 3.9.
4. Every consequence that changes what a step reads emits one judgment
   notice of the new kind `screening` naming the entry, the field and the
   consequence; item ids are listed, text never.
5. **Lineage and derivation** (amends 002 3.12). A step whose projection or
   input depends on a screening consequence lists the screening steps in
   its lineage, so its static and runtime class is `mixed-derived` when it
   is exact and stays `model-derived` when it is semantic. A refused
   judgment reads model-derived verdicts and is `mixed-derived`.

### 3.7 Screening refusals

1. A blocked value under a `refuse` entry is a terminal consequence in the
   sense of spec 033 3.4.6: the judgment is spec 033's `refused` outcome
   (`rustev.judgment/2`), whose `refusals` member is a list. Each blocked
   value contributes exactly one refusal `{check, subject, action}`:
   - `check` is the reserved name `screen:<entry id>` (a colon cannot
     occur in an authored check name);
   - `subject` is one field reference: the entry's reference for a
     non-list field, or `input:<name>[<item id>]/<field>` for one list
     item's field;
   - `action` is computed by spec 033 3.5.2 from the input cause `{field,
     provenance: observed{class}}` of the blocked value's accepted entry,
     so its `addressed_to` is `caller` only when that class is declared
     caller-controlled, `upstream` for `third-party` or `system-of-record`
     text, and `host` otherwise. This is spec 033's party mapping, applied
     unchanged: a `system-of-record` value is produced by a system outside
     the decision, so the party able to correct it is upstream of the host.
     A host that treats such text as its own declares an override
     `address` with `to: host` on the entry, which spec 033 3.5.3 admits. Remediation is the code `screen_blocked` with
     no parameters, and `transient` is false.
2. **Decided at the first block.** The first `refuse`-entry verdict that is
   `blocked`, in the order the core receives supplied outputs, decides the
   outcome; it does not wait for outstanding screening requests. If
   several refusing verdicts are already known when the core decides (for
   example from one supply batch or from a cache), `refusals` holds one
   refusal per blocked value (3.7.1) in lowered-step and item order, and
   the outcome's `addressed_to` is spec 033's combination of their
   parties. Requests withdrawn or cancelled under 3.7.3 are never
   consulted afterwards.
   **Known limitation, stated deliberately.** The outcome `refused` is a
   function of the inputs whenever any refusing verdict is `blocked`, but
   the `refusals` list, and so the combined `addressed_to`, is a function
   of the verdicts supplied before the decision, which depends on arrival
   order in a live run. Two live runs on identical inputs can therefore
   list different subsets of the blocked values. The core stays pure: capture
   records the supplied outputs in actual supply order (spec 004), and
   replay supplies exactly those in that order, so replay reproduces the
   judgment byte for byte. A host that needs every blocked value listed
   uses `drop_field` or `drop_text` entries with a policy rule instead of
   `refuse`.
3. **Withdrawal.** Once the outcome is decided, `pending()` lists no
   further request of the decision, a request not yet dispatched is never
   dispatched and is recorded `withdrawn`, and the runtime cancels
   in-flight attempts under spec 003 3.7, keeping every charge and
   liability. A late output of a cancelled attempt is recorded but never
   supplied. `finish` is allowed as soon as no attempt is awaited, and
   returns the refusal decided under 3.7.2. The refusal is evaluated before
   the policy's handlers, so no handler or rule can turn it into a
   proposal.
4. **With spec 033 checks.** Spec 033 evaluates its checks at `start`,
   before any semantic request exists (033 3.4.6). A terminal consequence
   of a check is therefore decided before any screening request is issued
   and wins; no screening request is then issued. A screening refusal can
   only be decided after that, and only when no check was terminal.

### 3.8 Withheld values in projections

1. A withheld text value is written into a model step's projection, at its
   position (a projected input, a record field, a bound item field), as the
   JSON object `{"withheld": "<cause>"}` with cause `screen_blocked`. The
   text never appears. The step proceeds and its value is model-derived as
   usual; the backend sees an explicit absence, never a default text.
2. A backend that cannot interpret the marker fails or answers as its own
   contract states; Rustev does not repair its answer.
3. Generation steps (#76 B5) never proceed over a withheld value: they take
   their backstop with reason `screen_blocked` (3.17).

### 3.9 Unavailability

1. `Unresolved` (spec 002, 3.5.6) gains `screen_unavailable{fields}`: the field
   references, deduplicated and sorted by the byte order of their UTF-8
   strings (as spec 002 orders ids). Under spec 033 its action's cause is
   `backend{step, origin}` for the screening step, so it is never
   addressed to the caller.
2. `fail_closed` (default): every model step instance whose projection
   would carry the unscreened value is not dispatched and resolves as
   `screen_unavailable{fields}`; the policy must handle it where it reads
   such a step (category 8 computes this statically). Generation steps take
   their backstop with reason `screen_unavailable`.
3. `skip_generation`: as `fail_closed`, and in addition every generation
   step of the decision takes its backstop with reason
   `screen_unavailable`. Until B5 lands no plan has a generation step, and
   `skip_generation` behaves exactly as `fail_closed`.
4. `fail_open`: model steps read the value as supplied; the evidence
   verdict stays `unavailable` with consequence `fail_open`, and a
   `screening` notice names the field. It is refused for caller-supplied
   fields (3.5.3).
5. A `refuse` entry whose value is unavailable does not refuse: the caller
   did not cause the outage. Its `on_unavailable` applies.

### 3.10 Isolation and batching

1. A screening request bound to a screener whose capability declares
   `isolation: per_item` (3.2.2) is dispatched alone: the
   runtime never places it in a spec 017 general batch (whatever `batch_capability` answers), a
   spec 020 batch dispatch, or a spec 009 shared-state exchange, under any
   execution or optimization policy. This holds for one screener and
   per-item fan-outs of any size.
2. Screening requests to a `shared` (default) capability follow spec 017
   3.3 like any request: batched only when the plan enables batching and the backend declares
   compatible semantics, each member keeping its own outcome, charge and
   evidence.
3. Spec 017's concurrent duplicate suppression (increment 2) may join
   byte-equal screening requests within one authorized scope; a joined call
   still carries one text, so it is not a batch.
4. The runtime's per-backend attempt concurrency limit (spec 003, 3.4.2) is
   the rate bound for a screener.

### 3.11 Verdict cache

1. The verdict cache is spec 017's cache: the memory cache now, and the
   persistent cache when increment 2 lands, enabled, sized and invalidated
   by the plan's identified optimization policy. No second cache exists.
2. The key is spec 017's complete request identity. Because a screening
   projection holds only the text (3.5.1), it covers exactly: the text's
   content, the screener's artifact and `DescriptorId` (which carry the
   policy version, categories and `isolation`), and the authorized scope.
   The scope is always part of the key, because spec 017 forbids any entry
   to cross it (Decision 6).
3. Only validated `screen` outputs are admitted; `unavailable` is never
   cached. A hit is recorded (3.13) with its source attempt, and charges no
   text units.

### 3.12 Text units and budgets

1. `units(text) = ceil(utf8_bytes / text_unit_bytes)` for a non-empty text.
   Bytes bound characters from above, so a character-billed screener is
   never under-counted.
2. **Compile time** (phase g, category 7 `budget`): the worst case is the
   sum over lowered steps of `units(max_bytes)` times the list's
   `max_items` for list fields (spec 002 3.7.2 makes `max_items` a required
   member of every `list` type, and `text` a required `max_bytes`, so the
   bound always exists); it must not exceed `max_text_units`.
3. **Run time.** Each dispatched request records `computed` units, and the
   backend's `reported` units when its output reports them; the charged
   units are the reported value when present, else the computed value, with
   that basis named. A reported value above the computed bound is recorded
   as a unit bound violation, as spec 003 3.5.4 records cost bound
   violations. Cost in deployment units still flows through spec 003's
   ledgers unchanged.

### 3.13 Evidence

1. `rustev.evidence/2` gains `screening`, present only when the plan has
   screening: one entry per screened value, in lowered-step and item
   order, with entry id, field reference, item id or none, the text digest
   (tagged digest with tag `rustev.screen-text/1` over the UTF-8 bytes),
   verdict (`pass`, `blocked`, `unavailable{reason}`, `empty`), categories,
   the consequence applied (`none`, `refuse`, `drop_field`, `drop_text`,
   `drop_item`, `fail_closed`, `skip_generation`, `fail_open`), the
   screener's `backend_id`, artifact and `DescriptorId`, and the lowered
   step id.
2. Each `rustev.run/3` request record of a screening request gains
   `screen{cache, units}`: `cache` is `hit{source_attempt}`, `miss` or
   `disabled`; `units` is `{charged, basis}` with basis `reported`,
   `computed` or `cache_hit`. Withdrawn requests are recorded as such.
3. No member carries text. Under spec 028 the digest is `content-digest`,
   entry and field names are `plan-metadata`, item ids are `id`, and
   verdicts, categories and consequences are `enum`.

### 3.14 Metrics

Spec 028 3.6.1 reserved the `rustev.screening` counter for this child. It
counts screened values, with attributes `rustev.screening.verdict`
(`pass`, `blocked`, `unavailable`, `empty`), `rustev.screening.entry` and
`rustev.plan.id`. A histogram `rustev.screening.units` records charged
units per decision. Block rate and unavailable rate are the counter's
verdicts over its total; units per decision is the histogram. Attributes
come only from `enum` and `plan-metadata` classes, as spec 028 requires.
Emitting them is part of the OpenTelemetry sink's projection and lands with
spec 028's implementation or after it.

### 3.15 Remote adapters (amends 009)

1. A remote adapter's descriptor carries the `screen` profile through
   `rustev.remote-describe/1` unchanged; a profile that changes after
   setup is `identity_mismatch` (009, 3.2.5).
2. **Client.** An adapter never sends a `rustev.remote-infer/1` request
   with more than one item when any item is for a screener whose
   capability declares `isolation: per_item`,
   whatever its shared-state terms or coalescing window.
3. **Server.** A request with more than one item for a backend whose
   screening capability declares `isolation: per_item` fails every item
   with `capability` before the backend is called.
4. The `screen` output travels as a `RawOutput` in the infer response like
   any other output.

### 3.16 Alignment with spec 030 (Bedrock)

Spec 030 3.12 reserved the `operations` value `screen` and the `guardrail`
member `{identifier, version, trace}` for this child. Admitting them is a
change to `rustev-bedrock` made under 030 3.12 and this section, not part
of this spec's core increment, and it must:

1. map `screen` to this spec's profile through the ApplyGuardrail action
   (Fact 1) with `source: INPUT`, exactly one text content block per call,
   and `outputScope: INTERVENTIONS`;
2. declare `isolation: per_item` (3.2.2) on the `screen` capability
   always (need 4 of section 1);
3. set `policy_version` from the bound guardrail identifier and version,
   and declare as categories the fixed set of assessment policies in the
   mapping version (`automated_reasoning`, `content_filter`,
   `contextual_grounding`, `denied_topic`, `sensitive_information`,
   `word_filter`); filter types go to the exchange record;
4. map `action: GUARDRAIL_INTERVENED` to `blocked` and `NONE` to pass,
   report as units the largest per-policy text unit count in `usage`, keep
   every usage member in the exchange record, and map errors through 030
   3.10;
5. never attach the guardrail to Converse or Responses inference calls
   (030 Decision 17 stands), so a binding with `screen` serves only
   `screen`;
6. use only existing binding members; a pricing or other new member is
   `rustev.bedrock-binding/2` under 030 3.12.2, and until one exists the
   charge is `unknown`;
7. stay inside A-26's live-call scope and R-29's cap.

### 3.17 Relation to B3, B5 and B6

1. **B3** (#61, spec 033) owns the `refused` outcome, `caller_controlled`
   and computed addressing; screening refusals are instances of them
   (3.7), so a caller is never told to fix text it did not control.
2. **B5** (#63, generation) depends on this seam and must: apply the
   `unscreened_text` check to every generation step whether or not the
   definition declares screening; take its backstop with reason
   `screen_blocked` when a read value is withheld and `screen_unavailable`
   under 3.9.2 and 3.9.3; and never place withheld or unscreened text in a
   prompt except under a declared `fail_open`.
3. **B6** (#64, extraction) applies the same check to extraction steps.

### 3.18 Compatibility and identity

1. A definition that uses no #76 construct stays `rustev.definition/1`
   with byte-identical canonical bytes and `DefinitionId`; a descriptor
   without `screen` or `isolation` has byte-identical bytes and
   `DescriptorId`; a plan compiled from them stays `rustev.plan/2` with
   byte-identical bytes and `PlanId`; judgments, evidence and run records
   for them keep their existing versions and bytes. A
   `rustev.definition/2` that uses other #76 constructs but no `screening`
   gains no screening member, so this spec changes none of its bytes. The
   implementing change keeps every existing golden byte-identical and adds
   a test asserting it for both reference plans.
2. Existing definitions are not subject to `unscreened_text` (3.5.5), so
   no existing compile result changes.
3. Adding `screening` to a definition, or a `screen` profile to a
   descriptor, changes the identity of that document and of every plan
   compiled from it, as spec 002 3.3 requires.
4. The new variants (`screen_unavailable`, `screen`, `screen_verdict`,
   the `screening` notice, `screen:` refusal checks) do not occur in any
   document of a plan without screening.

## 4. Out of scope

Implementation; any particular screener, detection quality or category
taxonomy; screening of generated output (output guardrails, B5's concern);
image or binary content; redaction or rewriting of text (a screener never
returns replacement text into the decision); cross-scope verdict sharing;
a reference screener backend (tests use scripted backends); admitting the
Bedrock values beyond 3.16's constraints; CLI or evaluation changes beyond
exhaustive matches; quality claims (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Screening declared; a `classify` step projects a `third-party` text input no entry covers | Refused `unscreened_text`, naming step and field. |
| Same definition without `screening` | Compiles as today, same `PlanId` as before this spec. |
| A fan-out over a `third-party` list keyed by an unscreened `text` id field | Refused `unscreened_text`, naming the id field. |
| `fail_open` on a `user-supplied` field | Refused `invalid_screening`. |
| An override `address` with `to: caller` | Refused, spec 033's `caller_cannot_act`. |
| Third-party text blocked under `refuse` | `refused` addressed to `upstream`, never `caller`. |
| `screening` in a `rustev.definition/1` | Refused category 11, `requires-definition-2`. |
| Two refusing entries; the second's verdict blocks first while the first is in flight | `refused` decided at once on the second; the first is cancelled and never consulted. |
| `drop_text` on a non-list field, or `drop_field` on a list field | Refused `invalid_screening`. |
| One field in two entries | Refused `invalid_screening`. |
| Screener without a `screen` profile, or `max_text_bytes` below the field's `max_bytes`, or `on_excess: truncate_start` | Refused category 5, naming the lowered step. |
| Worst-case text units above `max_text_units` | Refused category 7. |
| An authored step requiring `screen_verdict` | Refused category 11. |
| `user-supplied` caller text, declared caller-controlled, blocked under `refuse` | Judgment `refused` with check `screen:<entry>`, `addressed_to: caller`, remediation `screen_blocked`; no model step reading it dispatched; undispatched requests `withdrawn`. |
| Third-party item text blocked under `drop_text` | The model step's projection holds `{"withheld": "screen_blocked"}` at that field; other items unchanged; a `screening` notice; decision not failed. |
| Item blocked under `drop_item` | Item absent from every consumer; its id in the notice; exact steps reading the list are `mixed-derived`. |
| Screener outage under `fail_closed` | Reading model instances `screen_unavailable{fields}`; no unscreened text in any projection. |
| Caller text unavailable under a `refuse` entry | No refusal; `on_unavailable` applies. |
| Screener returns an undeclared category | `invalid_backend_output`, verdict `unavailable`. |
| Ten items to an `isolation: per_item` screener with spec 017 batching enabled and `batch_capability` returning `Some` | Ten single-text calls. |
| A remote server receives two items for a `per_item` screener | Both fail `capability`; backend not called. |
| Same text screened twice in one scope with the memory cache enabled | Second is a hit, 0 units, source attempt recorded. |
| Same text, different scope | Miss. |
| Empty text | Verdict `empty`, no request, 0 units. |
| Two `refuse` entries both blocked, live, the second verdict arriving after the decision | `refused` with one refusal; the capture holds the one supplied verdict; replay reproduces the same judgment bytes. |
| Reported units above the computed bound | Unit bound violation recorded. |
| Any screening record, notice or metric | Contains no screened text, only its digest. |
| Bedrock binding with `operations: [screen]` before the 3.16 admission lands | Refused `reserved_by_spec` (spec 030). |

## Acceptance

- Blocked caller text refuses with a caller-addressed `refused` outcome;
  blocked third-party text is withheld by the marker, and once B5 lands its
  generation step backstops with reason `screen_blocked` (B5 acceptance).
- A `per_item` screener receives one text per call under every batching
  setting (spec 017 batching on and off, spec 009 shared-state terms, and
  spec 020 batch dispatch once implemented), shown by a scripted backend
  counting texts per call.
- A screener outage under `fail_closed` puts no unscreened text in any
  model projection, verified by asserting every dispatched projection
  against the screened texts.
- The compiler refuses a model step reading an unscreened untrusted field,
  with golden refusal fixtures for `invalid_screening` and
  `unscreened_text`.
- Lowering is golden-tested: plan document, step order, bindings and the
  screening projection bytes for a three-entry definition.
- Evidence and run record goldens for pass, blocked (each consequence),
  unavailable (each `on_unavailable`), empty, cache hit and withdrawal;
  a canary text never appears in any of them.
- Remote client and server isolation tests over loopback.
- Every row of section 5 has a test.
- Compatibility: every existing contract, core, runtime, eval and CLI
  golden, and both reference plans' `PlanId`s, are byte-identical.
- Mutation seeds (unavailable treated as pass, marker omitted so text
  leaks, a `per_item` request batched, scope dropped from the cache key,
  `caller` allowed on third-party text, refusal evaluated after handlers,
  units counted from characters) are each detected.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
contract vocabulary, in three parts (descriptor, output and required
kind; the definition and plan `screening` members; the judgment,
evidence and run record members); (2) core lowering and the compile
checks of 3.5; (3) core verdicts, consequences, refusals, the withheld
marker and evidence (3.6 to 3.9, 3.13); (4) runtime isolation,
withdrawal, text units and the verdict cache (3.7.3, 3.10 to 3.12);
(5) remote isolation (3.15); (6) metrics, mutation seeds, declared
verification and closure.

**Increment 1, part 1 (2026-10-24): descriptor, output and required
kind.** `OperationSupport` gains the optional `isolation` (`per_item`;
`shared` is the omitted default) and `screen` profile members (3.2.1,
3.2.2), validated at parse: `output: screen` requires exactly a `screen`
profile, only on `proposition`, with a non-empty `policy_version`, 1 to
64 sorted unique non-empty categories and positive byte bounds; an
explicit `null` is refused, so each meaning has one form. `OutputKind`
gains `screen`, `RawOutput` gains `screen{blocked, categories, units}`
with `units` `reported{n}` or `not_reported` (3.3.1), and
`RequiredKind` gains `screen_verdict`, satisfied by no authored output;
an authored step requiring it is refused at compile, category 11 (3.3.2).
New variants are last in their enums. Descriptors, definitions and plans
that use none of them keep their bytes and identities
(`crates/rustev-contract/tests/screening_wire.rs`, 3.18.1). Elsewhere
only absent-member initializers and regenerated schemas and binding
types change.

**Increment 1, part 2 (2026-10-24): the definition and plan `screening`
members.** `rustev.definition/2` gains the optional `screening` block of
3.4 (`crates/rustev-contract/src/screening.rs`), checked at parse only for
what is local to the document: `max_text_units` positive, 1 to 32 entries
and 1 to 32 field references each, reference syntax, non-empty names, and
an override `address` with a non-empty reason. An override naming
`caller` parses, because compilation refuses it as spec 033's
`caller_cannot_act` (3.5.7). A `screening` member in a
`rustev.definition/1` is refused at compile, category 11, detail
`requires-definition-2`, as spec 033's constructs are. `rustev.plan/3`
gains the optional `screening` member of 3.5.8 (the canonical block and
each lowered step's entry, field, screener binding and consequence),
refused in an earlier plan version; compilation does not fill it yet.
Both members are omitted when absent and refused when explicitly `null`,
and documents without them keep their bytes and identities. Until the
core increment admits screening, a `rustev.definition/2` that uses
`screening` and no construct of spec 032 or 033 is refused by the existing
version check. `screening.rs` joins rustev-contract's reviewed archive
list in `tools/rustev-release/archive-policy.json`.

## Facts relied on

Read on 2026-10-17 from the AWS documentation named; re-confirmed at the
3.16 admission.

| # | Fact | Source |
|---|---|---|
| 1 | ApplyGuardrail is `POST /guardrail/{guardrailIdentifier}/version/{guardrailVersion}/apply` on `bedrock-runtime`, body `content` (array of content blocks), `source` (`INPUT` or `OUTPUT`), optional `outputScope` (`INTERVENTIONS` or `FULL`). Response: `action` (`NONE` or `GUARDRAIL_INTERVENED`), `actionReason`, `assessments` (per policy: content, topic, word, sensitive information, contextual grounding, automated reasoning), `outputs`, `usage` with per-policy unit counts (`contentPolicyUnits`, `topicPolicyUnits`, `wordPolicyUnits`, `sensitiveInformationPolicyUnits`, `contextualGroundingPolicyUnits`, `automatedReasoningPolicyUnits` and others), `guardrailCoverage`. Errors: 403, 404, 400 quota and validation, 429, 500, 503. | https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_ApplyGuardrail.html |

**Owner decision A-49 (2026-10-23): dependencies.** Spec 030 is no longer
a declared dependency of this spec, so its core increment can be
implemented and completed while 030 is in progress. The Bedrock admission
of 3.16 was already "not part of this spec's core increment"; it is made
only after spec 030 is complete, under 030 3.12 and 3.16, whose
constraints bind it unchanged. Section 5's last row (a Bedrock binding
with `operations: [screen]` refused `reserved_by_spec` before the
admission lands) is acceptance of that admission change, not of the core
increment. No other behavior, row or acceptance item of this spec moves.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #65 leaves the points below open, and each is
decided within the binding principles, as written:

1. **Untrusted and caller-controlled.** Untrusted is any accepted class
   other than `system-of-record`. Addressing uses spec 033's declared
   `caller_controlled` set; the `fail_open` refusal also covers
   `user-supplied` and `authenticated-app-field` whatever is declared, so
   an undeclared set cannot open caller text to unscreened prompts.
2. **Scope of the compile check.** Only definitions that declare
   `screening` (and, by B5 and B6, every generation and extraction step).
   Applying it to every existing definition would refuse both reference
   plans and change no protection a host asked for.
3. **Declaration shape.** An object with `max_text_units` and `entries`,
   explicit entry ids, an explicit screener backend id, `item_id` for list
   fields, and `address` (`computed` or a spec 033 override away from the
   caller) in place of the issue's authored `addressed_to`, because spec
   033 forbids authoring addressing toward the caller; the issue's array
   sketch is kept as `entries`.
4. **One text per request.** A screening request carries one text and no
   item identity, which makes per-item isolation structural and equal
   texts cacheable.
5. **Consequence semantics.** `drop_text` and `drop_field` withhold from
   model steps only, with an explicit marker; `drop_item` removes the item
   for every consumer; exact steps and the policy are not prompts and read
   withheld text unchanged. Unavailability never refuses the caller.
6. **Tenant in the cache key.** Always, as the authorized scope, because
   spec 017 forbids any cache entry to cross a scope; the issue's
   "optionally tenant" is met by the host's choice of scope handle.
7. **Categories.** A new `screen` output carries them, rather than
   overloading `Label`; it is read only by consequences, never as a value
   kind.
8. **Omitted-when-unused members** follow spec 017's precedent so every
   existing identity holds.
9. **Bedrock.** Constraints only (3.16); the admission is its own change
   under 030 3.12, with ApplyGuardrail, `per_item`, and no guardrail on
   inference calls.
10. **Refusal category numbers** are 18 and 19, the next free after spec
    033.
11. **Shared versions and outcomes.** Under the #76 coordination rule the
    spec adds to the shared track versions instead of claiming its own,
    reuses spec 033's `refused` outcome and addressing instead of a
    separate refusal outcome, and expresses per-item isolation as a general
    per-capability `isolation: per_item` member (introduced here, applied
    to every capability by spec 044) instead of a screening-only flag.
12. **Refusal timing.** A screening refusal is decided at the first
    blocked refusing verdict, not after every screening request resolves,
    so outstanding work is withdrawn as early as possible; other
    consequences need every verdict and wait for them.

---
id: "036-mid-plan-acquisition"
title: "Mid-plan context acquisition and tiered widening (amends 002, 003 and 004)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002, 003 and 004 (R-16), for #76 child B1
  under owner decision R-40 (OD-3 of #76). A plan may declare bounded
  acquisition steps that ask a host-implemented acquisition source for
  more context between steps, with requests built only from exact values
  and literals, and a tiered widening construct that tries a finite,
  declared list of increasingly broad requests (optionally across
  sources) until a stop condition holds, never relaxing protected
  constraints. Paging is bounded by pages, items and a stop condition.
  Adds acquisition operations to spec 032's source descriptor
  (`rustev.source/1`), acquisition request and page documents, the
  `compose_key@1` operator, a typed `skipped`
  outcome, compile-time call, item, byte, deadline and cost budgets, and
  the in-process `AcquisitionSource` seam a later remote protocol (#76 A2)
  carries. Every page is recorded and captured by request identity;
  offline replay never dispatches and an uncaptured request is
  `missing_evidence`. Adds to the #76 track versions introduced by spec
  032 (source 1, definition 2, plan 3, evidence 2, run 3, exact 2),
  introduces the track version `rustev.replay/2`, and admits every plan 3
  decision to capture and replay, lifting spec 032's interim
  `capture_unsupported` refusal. Plans that use no #76 construct keep
  byte-identical documents and `PlanId`s. Approved (A-33); implementation
  is in progress.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
extends:
  # Acquisition members of the #76 track documents, request and page
  # documents, run 3 and replay 2 types (3.2 to 3.10, 3.14, 3.15).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Compiler phases, `compose_key@1`, skipped handling, staged evaluation
  # of acquisitions and the `AcquisitionSource` seam (3.7 to 3.12).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Dispatch, deadline shares, retries, cost, cancellation, caching
  # eligibility and run record 3 (3.13, 3.14).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Capture of pages, replay bundle 2, offline replay of acquisitions (3.15).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # The synthetic widening and corroboration reference definitions and
  # their goldens (Acceptance).
  - { spec: "008-reference-package-lodging", unit: { kind: directory, path: "packages/rustev-pkg-lodging/" }, nature: additive }
  # Regenerated document types of the bindings; no behavior.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  # Exhaustive matches over the new step bodies, predicate and reason only.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: directory, path: "integrations/rustev-otel/" }, nature: additive }
  # Default acquisition budget members in shared fixtures and the
  # reference package's definition builder; no behavior. The lodging
  # package's builder is covered by the spec 008 edge above.
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  - { spec: "007-support-routing-package", unit: { kind: directory, path: "packages/rustev-pkg-support-routing/" }, nature: additive }
  # The CLI refusal golden for 20 joins the shared conformance suite as a
  # harvested case.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # New contract source files join rustev-contract's reviewed archive list.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Adds 036 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "008-reference-package-lodging"
  - "017-general-runtime-optimization"
  - "032-evidence-quality"
  - "033-provenance-rules-and-actionable-outcomes"
  - "034-parameterized-definitions"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A definition that uses no construct of this spec is a `rustev.definition/1` document and compiles to a `rustev.plan/2` document byte-identical to today's, with the same `PlanId`; its run, evidence and replay documents keep their existing schemas and bytes."
    anchor: "3-16-compatibility"
  - id: "I-2"
    kind: invariant
    text: "Every acquisition request, guard, stop count and dedupe input is computed only from values whose static derivation class is exact-derived, or from literals; anything else is refused at compile time."
    anchor: "3-9-compilation-refusals-and-budget"
  - id: "I-3"
    kind: invariant
    text: "Offline replay and candidate comparison never call an acquisition source; an acquisition request with no captured page is supplied as `missing_evidence` and the case is never reported as reproduced."
    anchor: "3-15-capture-and-offline-replay"
  - id: "I-4"
    kind: invariant
    text: "A skipped, empty, failed or truncated acquisition is never presented as an empty or default value: each has its own typed outcome, visible to the policy and recorded in evidence."
    anchor: "3-8-skipped-values-and-policy-handling"
  - id: "R-1"
    kind: requirement
    text: "A widening never sends a request in which a field of a protected constraint group differs from the base request; a definition that could do so is refused with invalid_acquisition."
    anchor: "3-5-the-widen-construct"
  - id: "R-2"
    kind: requirement
    text: "The worst-case number of acquisition calls, retained items, response bytes, deadline share and, under a hard cost policy, cost units of a plan is computed at compile time and a plan exceeding a declared ceiling is refused with category 7."
    anchor: "3-9-compilation-refusals-and-budget"
  - id: "R-3"
    kind: requirement
    text: "Given the same plan, snapshot, evaluation time and supplied pages and outputs, the core produces byte-identical judgments and evidence regardless of the order in which independent acquisitions complete."
    anchor: "3-11-staged-evaluation-in-the-core"
---

# 036: Mid-plan context acquisition and tiered widening (amends 002, 003 and 004)

Approved by the owner (A-33, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child B1 (#59) under owner decision
R-40 (OD-3 of #76), which turns design 5.1 step 1 ("obtain an authorized
context snapshot") into "an initial snapshot plus declared, recorded
acquisitions" and amends spec 003. The approved texts of specs 002, 003 and
004 are not edited; this spec records what changes. Approval authorizes no
live source or provider call, no spend, no real-user data, and no change to
any existing document schema, golden or `PlanId`.

## 1. Purpose

Real decisions depend on lookups whose inputs come from earlier steps: a
corroborating lookup that needs a resolved parent id, live availability for
exactly the candidates a catalogue returned, a specific request tried before
a broad one, and a search widened group by group until it yields enough.
Today the snapshot is fetched once before the plan, so all of this lives in
host code where Rustev cannot budget, record, replay or evaluate it. This
spec lets a plan declare such lookups as finite, budgeted steps, identified
in `PlanId`, without making Rustev a workflow engine: no loops, no
recursion, no unbounded streaming, and no I/O in the core.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the new documents
  `rustev.acquisition-request/1` (identified, `AcquisitionRequestId`) and
  `rustev.acquisition-page/1`; the #76 track bundle version
  `rustev.replay/2`, introduced here; acquisition members added to the
  #76 track versions introduced by spec 032 (#76 B2): `rustev.source/1`,
  `rustev.definition/2`, `rustev.plan/3`, `rustev.evidence/2`,
  `rustev.run/3` and the registry `rustev.exact/2`; and the refusal
  categories
  `invalid_acquisition` and `source_mapping` (3.9.3).
- `crates/rustev-core/` (owner spec 002): compiler phases for acquisition,
  `compose_key@1`, the `skipped` outcome, staged evaluation of acquisitions
  (`pending_acquisitions`, `supply_page`, `supply_acquisition_failure`) and
  the `AcquisitionSource` seam trait. The core computes every request and
  performs no I/O.
- `crates/rustev-runtime/` (owner spec 003): dispatch of acquisitions
  through installed sources, deadline shares, retries, cost, cancellation,
  cache eligibility under spec 017, and run record 3.
- `crates/rustev-eval/` (owner spec 004): capture of pages and offline
  replay of acquisitions from replay bundle 2.
- `packages/rustev-pkg-lodging/` (owner spec 008): synthetic reference
  definitions, scripted sources and goldens for the acceptance tests. The
  package's existing plan and goldens are unchanged.
- `Makefile` (owner spec 001): 036 joins `make verify` at completion.

No new crate and no new dependency. `rustev-contract` and `rustev-core` stay
synchronous and take no async runtime; `make boundaries` is unchanged.
`ContextSource` (spec 003 3.2.6) is unchanged and still supplies the initial
snapshot. The CLI (spec 006), the bindings (spec 027) and the service and
remote context protocol (#76 A2) expose these constructs in their own
changes; until then they are reachable through the library only.

### 2.1 Binding principles of #76

- **Proposals, never grants.** An acquisition yields context values; it
  authorizes nothing. The principal handle is passed to the source opaquely,
  and whether a lookup is allowed is the source's decision (3.12.4). No
  type here is accepted by an executor and none is a `Permitted` value.
- **Derivation classes and lineage.** Acquired values carry a derivation
  class computed from the declared provenance of each response field and
  lineage to the acquisition step and every input and step its request read
  (3.4.6). Requests themselves are built only from exact-derived values.
- **Unresolved, never a default; no silent kind conversion.** A skipped
  acquisition is `skipped`, a failed one is unresolved with a typed reason,
  an empty answer is an empty list only when the source said so, and an
  item cut by a limit is reported as truncated. Response values decode
  strictly into declared types; a per-source mapping renames fields and
  never converts them.
- **Pure core; no async runtime in contract and core.** The core computes
  the next request and validates pages; the runtime calls the source. The
  core reads no clock: as-of checks use the supplied evaluation time.
- **New dependency classes only under `integrations/`.** None is added.
- **No dynamic plugin loading.** Sources are trait objects the host
  installs by explicit composition (D-06, R-38), bound by
  `SourceId`.
- **No identity, authority or knowledge store.** Rustev stores no acquired
  data. Caching uses the host-configured caches of spec 017 only; captured
  pages live in host-retained replay bundles (R-19).
- **Plan identities.** Every new construct is part of the definition and
  plan documents and so of `PlanId`; plans that use none keep byte-identical
  identities (3.16).

## 3. Behavior

### 3.1 Relation to OD-3 and to specs 002, 003 and 004

1. Specs 002, 003 and 004 are amended, not superseded. Every rule of theirs
   holds for plans that use no construct of this spec, and for every other
   step of a plan that does.
2. The initial snapshot is still obtained before the plan and validated as
   spec 002 3.7 states; `SnapshotId` and `rustev.snapshot/1` are unchanged.
   Acquired data are step values, never snapshot entries.
3. The **constructs of this spec** are: `acquire` steps (3.4), `widen` steps
   (3.5), `compose_key@1` (3.7), the `is_constructed` predicate (3.7.4),
   `skipped` in handler sets (3.8) and step field references (3.4.5). They
   are #76 track constructs: a definition that uses any of them is a
   `rustev.definition/2` document (introduced by spec 032), and the members
   this spec adds to the track documents are optional and omitted from
   canonical bytes when unused, under spec 032 3.1.2's amendment of spec
   002 3.3.1. A `rustev.definition/2` document that uses no #76 construct at
   all is refused by spec 032 3.1.3 (category 11, `use_definition_1`); this
   spec's constructs count as #76 constructs for that rule.
4. **Spec 032 relation.** This spec adds to the #76 track versions spec 032
   introduces and never redefines their members: spec 032's grade model,
   value metadata (`ValueMeta`), source binding and refusals apply to
   acquired values as stated in 3.2, 3.4.6 and 3.5.4. Where this text and
   spec 032 both name a member, spec 032's meaning holds.

### 3.2 Source descriptors (`rustev.source/1`)

1. This spec adds one member, `operations`, to the source descriptor
   `rustev.source/1` that spec 032 3.3.1 introduces and owns (identified
   as `SourceId`, parsed under `DESCRIPTOR_V1`, with `name`, `version` and
   the optional `facts`). `operations` is optional, 1 to 64 entries with
   unique names, each as 3.2.2, and omitted when a descriptor declares none
   (spec 032 3.1.2), so a descriptor without operations keeps its bytes and
   `SourceId`. One descriptor may declare facts, operations or both; there
   is no second descriptor type.
2. An operation declares:
   - `name` (1 to 64 bytes of `[a-z0-9_-]`);
   - `cancellation`: `acknowledges` (the source reports whether it stopped)
     or `none` (the source never reports);
   - `request`: 0 to 32 fields, each `{name, type, required}`, with `type`
     a spec 002 3.7.2 scalar type, `enum`, or `list{item, max_items}` of a
     scalar, and `max_request_bytes` (1 to 65 536);
   - `item`: the response item record, 1 to 64 fields, each `{name, type,
     provenance, max_grade, reliability, freshness}`, where `type` is a
     spec 002 3.7.2 type, `provenance` one of the closed classes of spec 002
     3.7.1, `max_grade` and `reliability` are as spec 032 3.3.1 declares
     them for facts (so a `known_unreliable` field's ceiling is `hint`), and
     `freshness` is `max_age_ms{value}` or `not_required`;
   - `max_page_items` (1 to 10 000) and `max_page_bytes` (1 to 4 MiB);
   - `paging`: `none`, `cursor{max_cursor_bytes}` (1 to 1 024) or `offset`;
   - `cost`: `bounded{max_units}` per call, `estimated`, or `unknown`, as
     spec 003 3.2.6 `cost_model`;
   - `idempotent`: a boolean;
   - `cache`: `not_cacheable` or `cacheable{max_ttl_ms}` (1 to 86 400 000).
3. The descriptor is static. A source states nothing per call that changes
   meaning: what an item field means, its provenance class and its grade
   ceiling are fixed by the descriptor and therefore by
   `SourceId` and `PlanId`. Grade propagation, requirements, hard
   gates and corroboration are spec 032's and apply to acquired values
   unchanged.

### 3.3 Acquisition requests and pages

1. An **acquisition request** (`rustev.acquisition-request/1`, identified
   as `AcquisitionRequestId`, parsed under `DESCRIPTOR_V1`) has: `source`
   (`SourceId`), `operation`, `fields` (every declared request field in
   declared order, each `set{value}` or `unset`), and `page`: `first`,
   `cursor{text}` or `offset{integer}`. Its canonical bytes are at most the
   operation's `max_request_bytes`.
2. The identity excludes decision id, step id, tier, attempt, timing and
   scope. Scope is an isolation key applied by capture (spec 004 3.1) and by
   cache keys (3.13.7), never part of what was asked.
3. An **acquisition page** (`rustev.acquisition-page/1`, parsed under
   `SNAPSHOT_V1` and at most the operation's `max_page_bytes`) has:
   `request` (the `AcquisitionRequestId` it answers), `as_of_ms`, `items`
   (each a record with exactly the declared item fields) and `next`:
   `none`, `cursor{text}` or `offset{integer}`.
4. A page is valid only when: `request` equals the pending request's
   identity; the item count is at most `max_page_items`; every item decodes
   strictly against the item record (no coercion, no default, no unknown
   field); `next` matches the operation's paging kind (`none` for `none`,
   a cursor within `max_cursor_bytes`, or an offset equal to the request's
   offset plus the item count, where `page: first` stands for offset 0); and `as_of_ms` is at most the evaluation
   time plus the step's `as_of_skew_ms` (3.4.2). Any other page is
   `invalid_backend_output` for that call (3.11.4).
5. Item order in a page is the source's rank, recorded as `rank` from 0.
   Rustev does not reorder items except as 3.5.6 states.

### 3.4 The `acquire` step

1. Shape (definition 2):

   ```json
   { "id": "subareas", "body": { "acquire": {
       "source": "geo", "operation": "list_subareas",
       "request": { "parent": { "ref": "step:resolved_parent/id" } },
       "when": { "present": "step:resolved_parent" },
       "limits": { "max_pages": 1, "max_items": 200, "deadline_share": "0.1",
                   "max_attempts": 2, "retry_on": ["transient", "timed_out"],
                   "delay": { "fixed": { "ms": 50 } }, "as_of_skew_ms": 5000 },
       "stop": "none",
       "on_failure": "unresolved"
   } } }
   ```

2. Members, all required in canonical form:
   - `source` and `operation`: a bound descriptor's `name` and one of its
     operations;
   - `request`: one entry per request field to set, each a spec 002 3.8
     typed literal or exact expression; required fields must be set;
   - `when`: `always` or a spec 002 3.8 exact predicate;
   - `limits`: `max_pages` (1 to 64; exactly 1 when paging is `none`),
     `max_items` (1 to 100 000), `deadline_share` (a decimal in (0, 1]),
     `max_attempts` (1 to 8), `retry_on` and `delay` (spec 003 3.2.2's
     retry policy, whose `on` member is named `retry_on` here: `retry_on`
     empty exactly when `max_attempts` is 1, classes from 3.13.4), and
     `as_of_skew_ms` (0 to 60 000);
   - `stop`: `none` or `items_at_least{count}`, with `count` an integer
     literal of at least 1 or an exact integer expression; a count below 1
     at evaluation time is `invalid_input`. Under spec 034 (#76 B4) the
     stop `count` of `acquire` and `widen` is a parameter site admitting an
     integer parameter whose bounds exclude values below 1; no other member
     of this spec is a parameter site;
   - `on_failure`: `unresolved`, the only member in this version.
3. The guard is evaluated first. If it is false, the step's outcome is
   `skipped{guard}` and nothing is requested. If it reads an unresolved
   value, the step is unresolved with that reason (spec 002 3.10.4); if it
   reads a skipped value, the step is skipped.
4. Otherwise the request is built in declared field order. A request
   expression reading an unresolved value makes the step unresolved, one
   reading a skipped value makes it skipped, and a fault (overflow, a value
   over `max_request_bytes`) makes it `invalid_input`; in each case nothing
   is requested.
5. The step's value, of type `acquisition<item>`, is a record:
   `items` (a list of item records), `stopped_by` (`exhausted`: the source
   reported no further page; `stop_condition`; `page_limit`; `item_limit`),
   `truncated` (the number of items received but not retained because of
   `max_items`), `as_of_ms` (the oldest retained page's) and
   `request_constructed` (whether any request field read a constructed
   value, 3.7). Each item field's provenance class and grade are those its
   operation declares. Downstream references use `step:<id>/<member>`
   and `item:<field>` inside spec 002 list operators; this field reference
   form is a construct of this spec.
6. Derivation and lineage: the static class is `model-derived` when any
   item field's declared provenance is `model-derived`, otherwise
   `exact-derived`; the runtime class follows spec 002 3.12. Lineage is the
   step itself plus everything its guard and request read, transitively.
   Freshness: an item field whose `max_age_ms` is exceeded by the page's
   age, `max(0, T - as_of_ms)` where `T` is the supplied evaluation time
   (never a clock read), after the skew check of 3.3.4, makes the step
   `stale_evidence{fields: [<step>/<field>]}`; equal is fresh.
   Value metadata (spec 032 3.2.2): each item field carries `grade` equal
   to its ceiling (3.2.2), `provenance` its declared class, `sources` the
   descriptor's `name`, `as_of_ms` the page's, and basis
   `acquired{step}`, a basis this spec adds to spec 032's vocabulary; the
   step value's metadata is the combination of its items' (spec 032 3.2.2).
   Spec 032 3.9 requirements and hard gates then apply to every read of an
   acquired value.
7. A truncated result carries a `truncation` notice naming the step. An
   empty result with `stopped_by: exhausted` is a resolved empty list: the
   source answered that nothing matched.

### 3.5 The `widen` construct

1. Shape (definition 2):

   ```json
   { "id": "pool", "body": { "widen": {
       "item": { "fields": [ { "name": "id", "type": { "text": { "max_bytes": 64 } } },
                             { "name": "price", "type": "decimal" } ] },
       "sources": [
         { "source": "tenant_catalogue", "operation": "search",
           "map": { "id": "listing_id", "price": "total" } },
         { "source": "shared_catalogue", "operation": "search",
           "map": { "id": "id", "price": "price" } } ],
       "base_request": { "dates": { "ref": "input:trip.dates" },
                         "party_size": { "ref": "input:trip.party_size" },
                         "amenities_optional": { "ref": "input:trip.nice_to_have" } },
       "groups": { "dates": ["dates"], "party_size": ["party_size"],
                   "optional_amenities": ["amenities_optional"] },
       "protected": ["dates", "party_size"],
       "tiers": [
         { "name": "specific", "source_index": 0,
           "request_patch": { "area": { "ref": "step:specific_key" } },
           "when": { "present": "step:specific_key" }, "relax": [] },
         { "name": "broad", "source_index": 0,
           "request_patch": { "area": { "ref": "input:trip.area" } },
           "when": "always", "relax": [] },
         { "name": "broad_shared_relaxed", "source_index": 1,
           "request_patch": { "area": { "ref": "input:trip.area" } },
           "when": "always", "relax": ["optional_amenities"] } ],
       "max_tiers": 8,
       "dedupe_key": "id", "order": "tier_then_source_rank",
       "stop": { "unique_at_least": { "ref": "input:pool_target" } },
       "on_tier_failure": "continue",
       "limits": { "max_pages": 2, "max_items": 200, "deadline_share": "0.3",
                   "max_attempts": 2, "retry_on": ["transient"],
                   "delay": "none", "as_of_skew_ms": 5000 }
   } } }
   ```

2. Members:
   - `item`: the pool's item record (spec 002 record fields);
   - `sources`: 1 to 4 entries, each a source, an operation and `map`, a
     total mapping from every pool item field to a response item field.
     A mapping is exact: the response field's declared type must equal the
     pool field's type (spec 002 type equality, including bounds no wider
     than the pool's), and the pool field takes the response field's
     declared provenance, grade and freshness. Nothing else is mapped;
     unmapped response fields are dropped and listed in the plan;
   - `base_request`: field values as 3.4.2, shared by every tier; tiers on
     different sources use the request fields of their own operation, so
     every base and patch field must exist in every operation a tier using
     it names;
   - `groups`: named, disjoint sets of request field names (1 to 16
     groups, each 1 to 8 fields);
   - `protected`: group names never relaxed or patched;
   - `tiers`: 1 to `max_tiers` entries with unique names, each naming
     `source_index`, a `request_patch` (fields set on top of the base
     request; top-level fields only), a `when` guard and a `relax` list of
     group names; `max_tiers` is at most 8;
   - `dedupe_key`: a pool item field of scalar type;
   - `order`: `tier_then_source_rank`, the only member in this version;
   - `stop`: `unique_at_least{count}` with `count` as 3.4.2, or `none`;
   - `on_tier_failure`: `continue` or `stop`;
   - `limits`: as 3.4.2, with `max_pages` applying per tier and `max_items`
     to the pool.
3. A tier's request is the base request with its patch applied, then each
   field of each relaxed group set to `unset`. Relaxing a group removes its
   constraints; it never substitutes a value.
4. **Protected constraints.** The compiler refuses (`invalid_acquisition`) a
   definition in which a tier relaxes a protected group, patches a field of
   a protected group, relaxes a group containing a request field the tier's
   operation declares `required`, or names an undeclared group. So every
   request a widening can send carries each protected field exactly as the
   base request computes it. A protected field is a hard gate in the sense
   of spec 032 3.9.2: a graded value it reads has the implicit requirement
   `{min: estimated}` and a read whose ceiling is `hint` is refused with
   spec 032's `uncorroborated_hint` unless a corroboration step raises it.
5. **Execution.** Tiers run strictly in declared order, one at a time:
   - a tier whose guard is false, or whose guard or request reads a
     skipped value, is recorded `skipped` and the next tier runs; a guard or
     request reading an unresolved value makes the tier `failed` with that
     reason;
   - otherwise its pages are requested as 3.6 states, and the tier is
     `ok` (at least one item), `empty` (the source answered with none) or
     `failed{reason}` (3.11.4);
   - each page is checked for freshness as 3.4.6 states, through the
     mapped fields; a stale page fails its tier with `stale_evidence`;
   - after each page, items are pooled (3.5.6); when the stop condition
     holds, no further page or tier is requested and the remaining tiers are
     recorded `not_needed`;
   - when the pool holds `max_items` items (3.6.2), no further page or
     tier is requested either: `stopped_by` is `item_limit`, the remaining
     tiers are recorded `not_needed`, and items of the last page beyond the
     limit are counted as truncated (3.5.6);
   - after a failed tier, `on_tier_failure: stop` ends the widening and
     `continue` moves to the next tier.
6. **Pooling.** Each received item is mapped to the pool item and given
   `origin`: `tier` (name), `tier_index`, `relaxation` (the number of groups
   the tier relaxes), `source` (descriptor name), `operation`, `page`,
   `rank` and the page's `as_of_ms`. An item whose `dedupe_key` value is already pooled is not added
   and is counted as a duplicate of its tier; the first occurrence, in tier
   order then page order then rank, wins. Items beyond the pool's
   `max_items` are counted as truncated and the step carries a truncation
   notice.
7. **Result.** The step's value, of type `pool<item>`, is a record:
   `items` (a list of `{item, origin}`), `stopped_by` (`stop_condition`,
   `item_limit`, `tiers_exhausted` or `tier_failure`), `truncated`,
   `request_constructed` (true when any requested tier's request read a
   constructed value, 3.7.4), and `tiers` (per declared tier: name,
   outcome, items received, unique items added, duplicates, and that
   tier's own `request_constructed`). Its outcome is:
   - `skipped{guard}` when every tier was skipped;
   - unresolved with the first failed tier's reason when the stop
     condition did not hold and at least one tier failed, because a pool
     that a failure may have shortened is not a complete answer;
   - resolved otherwise, including an empty pool when every attempted tier
     was `empty`. A resolved pool after a failed tier carries a
     `fallback` notice naming each failed tier.
8. A widening is a single step: it has no loop, its tiers are a finite
   declared list, and no tier can reference the step itself (spec 002
   category 4 applies to every reference).

### 3.6 Paging

1. A step requests pages one at a time, the first with `page: first` and
   each next one with the `next` value of the previous page.
2. Paging stops at the first of: the source's `next: none` (`exhausted`);
   the stop condition (`stop_condition`); `max_pages` pages received
   (`page_limit`); `max_items` items retained (`item_limit`). For a widening
   these apply per tier, except `max_items`, which applies to the pool.
3. A page counts once whatever its attempts. A failed page fails its step
   (`acquire`) or its tier (`widen`); items from earlier pages of a failed
   `acquire` step are not used. Likewise a failed `widen` tier contributes
   no items: items pooled from its earlier pages are removed from the pool
   (and from its duplicate counts), and the tier's record counts them as
   received but not added. A tier cannot fail after the stop condition
   holds, since no further page is then requested.
4. There is no streaming: a page is supplied to the core whole, after
   validation, and every page is recorded (3.14) and captured (3.15)
   separately.

### 3.7 Constructed keys (`compose_key@1`)

1. `compose_key@1` joins `rustev.exact/2` (3.10.3). Arguments: `parts`
   (2 to 8 exact expressions of type `text`, `integer` or `enum`), each with
   a `normalize` of `exact` or `ascii_token`, and a `separator`, one of `:`,
   `/`, `|` or `.`.
2. Normalization: `exact` keeps the text (an integer as its decimal digits,
   an enum as its option name). `ascii_token` lowercases ASCII letters,
   trims ASCII whitespace, replaces each run of ASCII whitespace, `-` and `_`
   with one `-`, and then requires every byte to be in `[a-z0-9-]`.
3. The result is `text` of at most 512 bytes. An empty normalized part, a
   byte outside the allowed set, a part containing the separator, or a
   result over 512 bytes is `invalid_input`; nothing is stripped silently.
4. The result is **constructed**. A value computed by an exact step from a
   constructed value is constructed; a page answered for a request built
   from one is not, but its step reports `request_constructed: true`. The
   predicate `is_constructed{ref}` is true for a constructed value, so a
   guard or policy condition can tell a guessed key from a verified one.
   The step's `rustev.evidence/2` entry records `constructed: true`. Its
   value metadata grade is `hint` (spec 032 3.2.1, basis `computed`) until a
   spec 032 corroboration step raises it.

### 3.8 Skipped values and policy handling

1. `skipped{guard}` is a step outcome distinct from a value and from every
   `Unresolved` reason. It records the step whose guard was false.
2. An exact step reading a skipped value is skipped, except that the
   predicates `present` and `is_constructed` evaluate to `false` for a
   skipped value instead of propagating it. A semantic step
   whose projection reads a skipped value is skipped and issues no request.
3. In definition 2 a handler's reason set may name `skipped`, and `any`
   covers it. Every value the policy reads that can be skipped (computed
   statically) must have a handler covering `skipped`, or the definition is
   refused with category 8 (`unhandled_unresolved`). A `propagate` handler
   covering `skipped` on such a value is refused with category 8 and
   requirement `skipped_not_propagatable`: skipping is a declared path, not
   a failure, and its handler is `as_unmet` or `escalate`. A read through
   `present` or `is_constructed` cannot be skipped (3.8.2), so a value the
   policy reads only through those predicates needs no `skipped` handler.
4. The policy's possible reasons for an acquisition step are computed from
   3.4 and 3.5: `missing_evidence` (denied lookups and uncaptured replay
   requests), `stale_evidence`, `invalid_input`, `backend_unavailable`,
   `invalid_backend_output`, `budget_exhausted`, `deadline_exceeded`,
   spec 032's `insufficient_grade` where a requirement reads an acquired
   value, and `skipped` when the step or a tier has a guard other than
   `always`.

### 3.9 Compilation, refusals and budget

1. The source descriptors that spec 032 3.3.4 passes to the compiler also
   bind acquisition sources. Phases of spec 002 3.9.4, with spec 032's
   phase (e2), apply with these additions:
   - (b) structure: the bounds of 3.4.2 and 3.5.2 and the protected rules
     of 3.5.4 that need no descriptor;
   - (c) resolve: every source an acquisition step names to exactly one
     supplied descriptor with that `name` (spec 032 3.3.4's binding and its
     `source_binding` refusals, applied to steps), and every operation
     within it;
   - (e) types: request fields against the operation's request schema,
     guards as bool, stop counts as integer, `dedupe_key` as a scalar pool
     field, mappings (3.5.2), relaxation of required fields (3.5.4), and
     the derivation rule of 3.9.2;
   - (g) budget: 3.9.4.
2. **Exact requests.** Every reference read by a request field, a guard, a
   stop count or a `compose_key@1` part that feeds them must have static
   derivation class `exact-derived`. A `model-derived` or `mixed-derived`
   reference is refused with `invalid_acquisition`, requirement
   `exact_request`. A
   later spec that defines declared acceptance rules (OD-2, #76 B3 and B6)
   may admit such a value only through one, and must amend this rule by
   name.
3. New and extended categories. The new categories are 20
   `invalid_acquisition` and 21 `source_mapping`, the next free after spec
   035's 18 and 19:

| Category | Phase | Refused when |
|---|---|---|
| 20 `invalid_acquisition` (new) | b, e | a bound of 3.4.2 or 3.5.2 is violated; duplicate tier names; a protected-group rule of 3.5.4; a required request field unset; a request field not in its operation; a reference that is not exact-derived (3.9.2); a non-idempotent operation with `max_attempts` above 1 |
| `source_binding` (spec 032, applied to steps) | c | as spec 032 3.3.4 for a named source not supplied or two supplied descriptors with one name, naming the step; and, added here, a named operation the descriptor does not declare |
| 21 `source_mapping` (new) | e | a pool field with no mapped response field, a mapped field that does not exist, or a type that differs |

   Existing categories apply to the new constructs as they do to others:
   1 for shape, 3 for types, 4 for cycles, 7 for budget, 8 for handling,
   11 for references and `use_definition_1`, and spec 032's grade
   categories for grade requirements and hard gates.
4. **Budget (phase g).** Definition 2 limits gain the optional member
   `acquisition`: `{max_calls, max_items, max_bytes}`, required when the
   definition has an acquisition step and omitted otherwise; `max_calls` is
   1 to 4 096 (category 1 otherwise), so every call of a compiled plan fits
   one replay bundle (3.15.2). For each acquisition step, its
   worst-case calls are `max_pages * max_attempts` (`acquire`) or the sum
   over tiers of `max_pages * max_attempts` (`widen`); guarded steps and
   tiers count as if taken. The plan is refused with category 7 when:
   - total worst-case calls exceed `max_calls`;
   - the sum of steps' `max_items` exceeds `max_items`;
   - the sum over calls of the operation's `max_page_bytes` exceeds
     `max_bytes`;
   - the sum of `deadline_share` over all acquisition steps exceeds 1;
   - compiled with an execution policy whose cost is `hard{max_units}`, any
     reachable operation's cost is not `bounded`, or the sum over calls of
     `bounded.max_units` exceeds `max_units`.
5. Compilation still stops at the first refusal, earliest phase first, then
   the first step in declaration order.

### 3.10 Plan identity

1. This spec adds to `rustev.plan/3` (introduced by spec 032): every
   source an acquisition step names is recorded in spec 032's `sources`
   member (`name`, `version` and `SourceId`, in name order), and an
   optional member `acquisitions`, omitted when the plan has no
   acquisition step, holds per acquisition step its static derivation
   class, possible outcomes (3.8.4), worst-case calls and the unmapped
   response fields of each widening source. `PlanId` is the plan's
   identity, so it changes when any acquisition member, limit, guard, tier,
   mapping, group or bound descriptor changes.
2. Supplying an unrelated descriptor that no step or input names does not
   change the `PlanId`. Supplying a different descriptor under a bound
   name does.
3. This spec adds `compose_key@1` and the predicate `is_constructed` to
   the registry `rustev.exact/2` (introduced by spec 032). Every definition
   1 plan records `rustev.exact/1` as today.
4. A `rustev.definition/1` document compiles exactly as today to
   `rustev.plan/2`. A `rustev.definition/2` document compiles only to
   `rustev.plan/3`.

### 3.11 Staged evaluation in the core

1. `Evaluation::start` computes every exact step and every acquisition
   guard and request it can. `pending_acquisitions()` lists, for plan 3,
   each requested page as `{step, tier_index, page_index, source, operation,
   request, request_id, deadline_share}` in step order then tier order;
   `pending()` keeps listing semantic requests exactly as spec 002 3.10.1.
   At most one page per acquisition step is pending at a time.
2. `supply_page(step, tier_index, page_index, page)` validates the page
   (3.3.4), pools or appends its items, and either lists the next page, the
   next tier, or completes the step; completing a step may make further
   exact steps, acquisitions and semantic requests computable.
3. `supply_acquisition_failure(step, tier_index, page_index, reason)` takes
   one of the reasons of 3.11.4; any other reason, or a page not pending, is
   refused. Accepting it rolls back what earlier `supply_page` calls added
   for the failing unit: an `acquire` step's items from earlier pages are
   discarded and the step fails (3.6.3); a `widen` tier's items are removed
   from the pool and the tier fails (3.5.5). Earlier pages stay recorded
   and captured.
4. Reasons for a page: `missing_evidence{fields: [<step>]}` (the source
   denied the lookup, or replay has no capture), `backend_unavailable{class:
   detail}` (a source failure, detail prefixed `source:<name>`),
   `invalid_backend_output` (3.3.4), `budget_exhausted{acquisition_calls |
   cost}` and `deadline_exceeded`. These are existing `Unresolved` reasons,
   so no existing handler vocabulary changes.
5. `finish` refuses while any page or semantic request is pending.
6. Order independence: each step's value depends only on its own pages,
   which a step requests strictly in sequence, so the judgment and evidence
   are the same for every interleaving of independent steps' supplies.

### 3.12 The acquisition seam

1. `rustev-core` defines, beside the seams of spec 003 3.2.6:

   ```rust
   pub trait AcquisitionSource: Send + Sync {
       fn descriptor(&self) -> &SourceDescriptor;
       fn cost_bound(&self, request: &AcquisitionRequest) -> CostBound;
       fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport>;
   }
   ```

   `AcquireCall` carries the operation name, the canonical request document
   and its `AcquisitionRequestId`, the attempt id (its idempotency key), a
   child `CancelSignal` and the `CallContext` (remaining milliseconds, trace
   id, opaque principal handle). `AcquireReport` carries either a page
   document's bytes or a failure class (`transient`, `overloaded`,
   `permanent`, `denied`), a charge (`observed`, `estimated` or `unknown`,
   as spec 003 3.5.4) and a cancellation acknowledgement (spec 003 3.7.3).
2. Everything that crosses the seam except the signal and the call context
   is a canonical document of this spec, so a remote protocol (#76 A2,
   `rustev.remote-context/1`) can carry it verbatim and adds only
   transport concerns. A remote source is an ordinary installed
   `AcquisitionSource` (R-38).
3. The seam is in `rustev-core` as types and a trait only, with the
   std-only `BoxFuture` the other seams use; the core never calls it.
4. `denied` means the source refused the lookup for this principal. It is
   never retried and is supplied as `missing_evidence`. Rustev does not
   judge whether the lookup should have been allowed.

### 3.13 Runtime execution

1. The runtime refuses to prepare a plan 3 unless every bound source is
   installed with exactly the `SourceId` the plan binds, as spec
   003 3.6.5 does for backends. The sink requirement for `rustev.run/3` is
   spec 032 3.11.2's (`sink_version_unsupported`).
2. Acquisitions are polled on the decision's task (no task per request,
   spec 003 3.4.3), share the decision's `max_parallel_requests`, and each
   installed source has its own attempt concurrency limit.
3. **Deadlines.** A step's acquisition window opens when its first page is
   listed and closes at `min(D, open + W)`, where `D` is the decision
   deadline (spec 003 3.3.2) and `W` is the step's `deadline_share` (3.4.2)
   times the definition-level `limits.deadline_ms` (spec 002 3.9.1, the
   plan's deadline, not a member of the step's own `limits`), rounded down
   to a whole millisecond. No call is
   dispatched at or after the window's close; a pending page for which
   neither a page nor a failure has been supplied by then is supplied as
   `deadline_exceeded`.
4. **Retries.** An attempt ends as a page, `invalid_page` (the page failed
   3.3.4's validation: the attempt class that corresponds to spec 003
   3.6.1's `invalid_output`, and is supplied to the core as the
   `Unresolved` reason `invalid_backend_output`, 3.11.4), `transient`,
   `overloaded`, `permanent`, `denied`, `timed_out` (the attempt's own timeout
   passed while the step's window is still open, as spec 003 3.6.1 defines
   it), `adapter_fault` (the source panicked; contained as spec 003 3.4.4),
   `deadline` (the step's window or the decision deadline closed; never
   retried) or `cancelled`. Retryable classes are `transient`, `overloaded`
   and `timed_out`, only when named in `retry_on`, with spec 003 3.6.3
   delays; a retry whose delay would pass the window's close is not
   attempted. There is no runtime fallback between sources: a widening's
   later tier is the declared way to try another source (R-10).
5. **Cost.** Each attempt reserves and settles in the decision's ledger as
   spec 003 3.5 states, using the source's `cost_bound`. A runtime counter
   enforces `acquisition.max_calls`; a call that would exceed it, or whose
   reservation does not fit, is not dispatched and is supplied as
   `budget_exhausted`.
6. **Cancellation.** As spec 003 3.7. With `cancellation: none` the runtime
   does not wait for an acknowledgement and records remote state
   `possibly_continuing`.
7. **Caching.** A page is eligible for spec 017's caches only when its
   operation is `cacheable` and the runtime's spec 017 cache policy enables
   acquisition pages. The key is the `AcquisitionRequestId`, the
   `SourceId` and the complete authorized scope (spec 017 3.2; tenant and
   principal can never be omitted), under spec 017's key schema version. An
   entry's lifetime is the smaller of `max_ttl_ms` and the policy's expiry;
   freshness is still checked against the page's own `as_of_ms` (3.4.6), so
   a cache never makes a page fresher. Without such a policy nothing is
   cached.
8. Judgment equivalence (spec 003 3.8) extends to pages: the runtime
   supplies the core only through `supply_page` with the source's page
   bytes unchanged, or through `supply_acquisition_failure`.

### 3.14 Evidence

1. This spec adds to `rustev.evidence/2` (introduced by spec 032) a member
   `acquisitions`, omitted when the plan has no acquisition step, with one
   entry per acquisition step
   in step order: step, kind, outcome (`resolved`, `skipped{guard}` or the
   unresolved reason), `stopped_by`, truncated count, and per tier (one tier
   for `acquire`) its name, outcome (`ok`, `empty`, `failed{reason}`,
   `skipped`, `not_needed`), relaxation, source, operation, and per page the
   `AcquisitionRequestId`, the page digest (tagged SHA-256 of its canonical
   bytes) or `none`, the item count and the unique items added. Every
   declared tier appears, including those never attempted. Per item, values
   appear only as spec 032 value digests with their `ValueMeta`, as spec
   032 3.10.1 records step values.
2. This spec adds to `rustev.run/3` (introduced by spec 032 3.10.3, the
   run record of every plan 3 decision) an optional member `acquisitions`,
   omitted when the plan has no acquisition step, with one entry per page
   in listing order with step, tier
   and page indexes, `AcquisitionRequestId`, its attempts (attempt id
   `<decision id>/<step>/<tier index>,<page index>/<n>`, escaped as spec
   003 3.9.2; dispatch and end milliseconds; end class; cost bound,
   reservation and charge; cancellation; remote state), its cache use
   (`none`, `hit{age_ms}` or `stored`) and its result. Latency is the
   difference of recorded milliseconds, never a remeasurement.
3. A run that would need both spec 020's `remote_evidence` and
   `acquisitions` is not yet specified; spec 032 3.11.2 already requires a
   plan 3 sink to accept `rustev.run/3`.
4. Bounds: a run 3 record holds at most `acquisition.max_calls` attempts
   beyond spec 003's bounds; free-text details are cut as spec 003 3.9.3.
   Page contents are never embedded in evidence or run records.

### 3.15 Capture and offline replay

1. **Plan 3 decisions are capturable.** This spec introduces the #76 track
   bundle version `rustev.replay/2` and admits every plan 3 decision to
   capture (spec 004 5.3) and replay, whatever #76 track constructs its
   plan uses (spec 032's grades, presence, joins and corroboration, spec
   033's rules and `rustev.judgment/2`, this spec's acquisitions, and the
   additions of later #76 specs to the same versions). From this spec on,
   spec 032's interim refusal `capture_unsupported` no longer applies. A
   plan 2 decision is still captured into `rustev.replay/1`, unchanged.
2. `rustev.replay/2` is `rustev.replay/1` with these differences, under the
   same bounds and the same 16 MiB case bound:
   - the retained plan is a `rustev.plan/3` document and the retained
     dependencies include every bound source descriptor with its
     `SourceId`;
   - the retained snapshot is a `rustev.snapshot/1` or `/2` document;
   - the retained run record is the run version the decision produced
     (`rustev.run/3` for every plan 3 decision), and its core evidence is
     `rustev.evidence/2`; the expected judgment is the `rustev.judgment/1`
     or `/2` it contains, and reproduction compares record-canonical bytes
     of that judgment (spec 004 3.3.4);
   - a member `acquisition_pages` (3.15.3), omitted when the plan has no
     acquisition step, at most `acquisition.max_calls` entries (itself at
     most 4 096, 3.9.4).
   Later #76 specs add members to `rustev.replay/2` rather than introducing
   another version.
3. Capture of a decision with acquisitions also records every page
   supplied to the core: supply sequence number (one sequence shared with
   semantic supplies), step, tier and page indexes,
   `AcquisitionRequestId`, producing attempt id or none, and a retention
   item holding the page document or a `rustev.runtime-reason/1` document
   with one of the reasons of 3.11.4. Page bytes count toward the capture
   byte limit (spec 004 5.7). Retention defaults to digest-only (R-19); a
   digest-only page cannot be replayed and the case is
   `incomparable{content-not-retained}`, as for outputs.
4. **Historical reproduction** follows the recorded supply sequence: each
   pending page is matched to its entry by step, tier, page and
   `AcquisitionRequestId` (recomputed, never trusted), and the retained page
   or reason is supplied. Zero acquisition calls and zero backend calls
   occur.
5. **Missing captures.** A pending page with no matching entry is supplied
   as `missing_evidence{fields: [<step>]}` and evaluation continues to a
   judgment, which is returned for inspection. The case verdict is
   `incomparable{acquisition-not-captured}` naming the step, tier, page and
   request identity, never `reproduced` or `diverged`.
6. **Candidate comparison** (spec 004 3.4): a candidate's pending page is
   served from a retained page whose `AcquisitionRequestId` is equal,
   whatever its step or tier in the original; retained pages with one
   identity must have equal canonical bytes or the case is
   `incomparable{ambiguous-output}`. A page with no equal identity is
   supplied as `missing_evidence` as in 3.15.5, and the case is
   `incomparable{acquisition-not-captured}` for metrics. Historical failures
   are never reused for a candidate (spec 004 3.4.5).
7. Given identical retained pages and outputs, replay is deterministic
   (3.11.6) and a reproduced plan 3 case has byte-identical judgment bytes.
   Grades, presence and value metadata are recomputed by the core from the
   retained plan, snapshot and pages; nothing about them is read from the
   retained evidence.

### 3.16 Compatibility

1. `rustev.definition/1`, `rustev.plan/2`, `rustev.exact/1`,
   `rustev.evidence/1`, `rustev.run/1`, `rustev.run/2`, `rustev.replay/1`,
   `rustev.request/1` and `rustev.snapshot/1` keep their bytes and meaning.
2. Every existing definition and plan golden, with its `DefinitionId` and
   `PlanId`, is byte-identical; a definition 1 never produces a plan 3, an
   evidence 2, a run 3 or a replay 2.
3. Plan 3 documents, source descriptors, evidence 2 and run records of
   plan 3 plans that use no construct of this spec are byte-identical with
   and without this spec, because every member it adds is omitted when
   unused.
4. `compile` and `compile_with`, `Evaluation::pending`, `supply`, `finish`,
   `decide`, `decide_with_capture` and the `ContextSource` trait keep their
   signatures and behavior for plan 2. Acquisition sources arrive through
   spec 032's source descriptor parameter; the acquisition calls are new
   entry points, not changes to existing signatures.
5. Parse limits of existing documents are unchanged.
6. A reader of an earlier version is never given a later version's bytes.

## 4. Out of scope

Acquisitions with a `for_each` fan-out (a request takes a list field
instead); runtime failure fallback between sources outside widening tiers;
unbounded or streamed responses; writes or side effects through a source
(operations are lookups; an operation that changes state is not admitted by
this spec and must not be declared as one); the remote context protocol and
service (#76 A2); CLI, Node and WebAssembly exposure (spec 006 amendment,
#76 A1); grade semantics, presence predicates, typed joins and corroboration
(spec 032, applied here unchanged); acceptance rules for model-derived values (#76 B3, B6); live
re-execution of acquisitions (spec 026, #76 E3); persistent caches and the
decision-result cache (#76 F1); quality claims (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A tier relaxes a protected group | Refused, `invalid_acquisition`, naming the step, tier and group. |
| A tier patches a field of a protected group | Refused, `invalid_acquisition`. |
| An offset-paged first page of 10 items whose `next` is `offset{10}` | Valid: `page: first` stands for offset 0. |
| A widening tier whose second page fails after its first page pooled 5 items | The 5 items are removed from the pool; the tier is `failed` and counts them received, not added. |
| A tier relaxes a group holding a `required` request field | Refused, `invalid_acquisition`. |
| A protected field reads a `hint`-ceiling value with no corroboration | Refused, `uncorroborated_hint` (spec 032). |
| A request field reads a semantic step, or an input accepting `model-derived` provenance | Refused, `invalid_acquisition`, requirement `exact_request`. |
| A step names a source no supplied descriptor has, two descriptors share its name, or the operation is absent | Refused, `source_binding`, naming the step. |
| A widening source whose response lacks a pool field, or has it as `integer` where the pool has `decimal` | Refused, `source_mapping`. |
| A widening whose worst-case calls exceed `acquisition.max_calls` | Refused, category 7. |
| `acquisition.max_calls` of 4 097 | Refused, category 1. |
| Under `hard` cost, an operation with `estimated` cost | Refused, category 7. |
| `max_attempts` 2 on a non-idempotent operation | Refused, `invalid_acquisition`. |
| Steps whose `deadline_share` sums to more than 1 | Refused, category 7. |
| A definition 2 that uses no #76 track construct | Refused, category 11, `use_definition_1`. |
| A spec 032 descriptor without operations | Same bytes and `SourceId` as without this spec. |
| A policy reads a guarded step with no `skipped` handler | Refused, category 8. |
| `propagate` covering `skipped` | Refused, category 8, `skipped_not_propagatable`. |
| A guard reads an absent value | The step is `skipped{guard}`; nothing is requested. |
| A page answering another request identity, with an extra field, or with more than `max_page_items` items | `invalid_backend_output` for that page. |
| A page `as_of_ms` beyond evaluation time plus `as_of_skew_ms` | `invalid_backend_output`. |
| A page older than a field's `max_age_ms` | `stale_evidence{<step>/<field>}`. |
| The source answers `denied` | `missing_evidence{<step>}`; no retry. |
| A source panics | `backend_unavailable{adapter_fault}`; the decision continues. |
| The window closes before a page | `deadline_exceeded`; no later call is dispatched. |
| Specific tier skipped, broad tier `empty`, no failures | Resolved empty pool, `stopped_by: tiers_exhausted`. |
| Specific tier failed, broad tier meets the stop | Resolved pool with a `fallback` notice naming the specific tier. |
| Specific tier failed, no tier meets the stop | Unresolved with the specific tier's reason. |
| Items beyond `max_items` | Truncated count and a `truncation` notice; never silently dropped. |
| `compose_key@1` part containing the separator, or a byte outside `[a-z0-9-]` under `ascii_token` | `invalid_input`. |
| Replay of a bundle with one page entry removed | That step `missing_evidence`; verdict `incomparable{acquisition-not-captured}`; zero source calls. |
| Two retained pages with one request identity and different bytes, under candidate comparison | `incomparable{ambiguous-output}`. |
| A plan with acquisitions prepared with a source of another `SourceId` | Prepare refused. |
| A plan 3 decision using only spec 032 constructs, submitted with capture | Captured into `rustev.replay/2` without `acquisition_pages`; no `capture_unsupported`. |
| A plan 2 decision submitted with capture | `rustev.replay/1`, byte-identical to today. |
| Any existing definition compiled after this change | Byte-identical plan and `PlanId`. |

## Acceptance

- **Specific-first widening** (lodging package, synthetic data, scripted
  sources): the specific tier succeeds and the broad tier is `not_needed`;
  the specific tier is empty and the broad tier fills the pool; the specific
  tier is skipped by its guard. In every case each pooled item's `origin`
  names its tier and relaxation, and no request sent by any tier differs
  from the base request in a protected field (asserted on the scripted
  source's received requests). A definition relaxing a protected group is
  refused with a golden refusal.
- **Corroboration:** a guarded `list_subareas` acquisition runs only when
  the parent resolved, and `member_of@1` over its keys tests a
  `compose_key@1` key; a run with the parent absent records `skipped` and
  the policy's handler path. A constructed key is `hint` and a spec 032
  corroboration step over the acquired keys raises it; these grade
  assertions need spec 032's implementation and are reported as externally
  blocked until it lands, never as passed.
- **Replay:** a decision with five acquisitions (two paged) is captured,
  assembled into `rustev.replay/2` and reproduced with byte-identical
  judgment bytes and zero source calls; removing one page entry yields
  `missing_evidence` for that step and `incomparable{acquisition-not-captured}`.
- **Plan 3 replay generally:** a decision using spec 032 constructs with a
  `rustev.snapshot/2`, and (once spec 033 is implemented) one producing a
  `rustev.judgment/2`, are captured into `rustev.replay/2` and reproduced
  byte-identically; a plan 2 decision's bundle is byte-identical
  `rustev.replay/1`.
- **Budget:** a widening whose worst case exceeds each ceiling of 3.9.4
  (calls, items, bytes, hard cost) is refused with category 7, one golden
  per ceiling.
- **Evidence:** every declared tier appears in `rustev.evidence/2` with its
  outcome, including `skipped`, `failed`, `empty` and `not_needed`, and every
  page in `rustev.run/3` with its attempts, cost and latency.
- **Source switching:** a two-source widening pools items from both, each
  with its source and tier; duplicates across sources are counted, first
  occurrence kept; an unmappable source is refused with `source_mapping`.
- **Paging:** a paged acquisition stops at its stop condition, at
  `max_pages` and at `max_items` (with the truncation count), and at
  `next: none`; replay reproduces each page by page.
- **Runtime:** deadline-share expiry, retry on `transient` only when named,
  `denied` never retried, a contained source panic, cancellation with each
  `cancellation` kind, and a cache hit recorded with its age, under the
  manual clock and scripted sources (R-09).
- **Order independence:** the same pages supplied in every interleaving of
  three independent acquisition steps give byte-identical judgments and
  evidence.
- **Compatibility:** every existing definition, plan, run, evidence, replay
  and CLI golden is byte-identical with its identities.
- Every row of section 5 has a test, and seeded defects in the core and
  runtime harnesses (a protected field patched, a skipped value treated as
  empty, an uncaptured page dispatched in replay, a retry on `denied`, a
  truncation without notice, tier order reversed) are each detected.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
contract vocabulary: source operations and the request and page documents;
the `acquire` and `widen` definition members, `compose_key@1`, `skipped`
and the refusal categories; the plan, evidence, run 3 and replay 2
members; (2) core compilation, budgets and staged evaluation with the
`AcquisitionSource` seam (3.4 to 3.12); (3) runtime dispatch (3.13); (4)
capture and offline replay (3.15); (5) the lodging package's reference
definitions and acceptance goldens, mutation seeds, declared verification
and closure.

**Increment 1, part 1 (2026-10-24): source operations, requests and
pages.** `rustev.source/1` gains the optional `operations` member of 3.2:
1 to 64 uniquely named operations with their request fields, item record
(type, provenance, grade ceiling, reliability, freshness), page bounds,
paging kind, cost, idempotence and cache terms, refused at parse outside
their bounds; a `known_unreliable` item field's ceiling is `hint`. A
descriptor without operations keeps its bytes and `SourceId` (3.16.3).
New documents `rustev.acquisition-request/1`, identified as
`AcquisitionRequestId` with no decision, step, tier, attempt, timing or
scope member (3.3.1, 3.3.2), and `rustev.acquisition-page/1` (3.3.3), with
a pure page validator for each rule of 3.3.4
(`crates/rustev-contract/src/acquisition.rs`,
`tests/acquisition_wire.rs`). Nothing produces or consumes them yet.

**Increment 1, part 2 (2026-10-24): the `acquire` and `widen` definition
members.** `rustev.definition/2` gains the `acquire` (3.4.1, 3.4.2) and
`widen` (3.5, with the paging limits of 3.6) step bodies
(`crates/rustev-contract/src/acquisition_step.rs`), checked at parse for
what is local to the document: bounds, closed enums, `retry_on` empty
exactly when `max_attempts` is 1, tiers, groups, protected groups and
source maps. The predicate `is_constructed{ref}` (3.7.4) and the handler
reason `skipped` (3.8.1, 3.8.3) join the vocabulary; `compose_key@1`'s
arguments travel as the registry's untyped operator arguments, so its
checks land with the core increment. Either step body in a
`rustev.definition/1` is refused at compile, category 11, detail
`requires-definition-2`, and a `rustev.definition/2` using only them is
admitted to the version check; until the core increment compiles them,
compilation refuses them with `invalid_definition` detail
`acquisition-not-compiled`, as it refuses `is_constructed`
(`crates/rustev-core/tests/acquisition_structure.rs`). Definitions without
these members keep their bytes and identities
(`crates/rustev-contract/tests/acquisition_steps.rs`).

**Increment 1, part 3 (2026-10-24): plan, evidence, run and replay
members.** `rustev.plan/3` gains the optional `acquisitions` member of 3.10
(each acquisition and widening with its bound source descriptors and
mapped response fields); `rustev.evidence/2` the acquisition and widening
evidence of 3.14, with per-tier outcomes including `skipped`, `failed`,
`empty` and `not_needed`, pooled item origins and the `constructed` marker;
and `rustev.run/3` the per-page and per-attempt records of 3.13 and 3.14,
each omitted when unused and refused in earlier versions
(`crates/rustev-contract/src/acquisition_record.rs`). `rustev.replay/2`
(3.15) is introduced as a document version that retains source
descriptors and captured pages keyed by request identity;
`rustev.replay/1` keeps its bytes and meaning (3.16.1), and capture and
replay behavior land with their increment. Redaction classes are declared
for the new members. Documents without them keep their bytes and
identities (`tests/acquisition_records.rs`).

**Increment 2, part 1 (2026-10-24): compiling acquisitions.** The
temporary `acquisition-not-compiled` refusal is gone: `acquire`, `widen`
and `is_constructed` compile (`crates/rustev-core/src/acquisition.rs`).
New refusal categories 20 `invalid_acquisition` and 21 `source_mapping`
(3.9.3), with the protected-group rules of 3.5.4, re-check under their
own category what parse already refuses on wire documents; category 8
covers `skipped` handlers, including `skipped_not_propagatable` (3.8.3);
category 7 refuses each budget ceiling of 3.9.4 (calls, items, bytes,
deadline share, hard cost). The plan records bound sources in spec 032's
`sources` member, fills `acquisitions` (3.10.1) and gains plan step
details for `acquire` and `widen`; `LimitsDecl` gains the optional
`acquisition` budget. Tests: `crates/rustev-core/tests/acquisition_compile.rs`
with refusal goldens for 20 and 21 (and a CLI golden for 20, harvested
into the shared conformance suite), budget
goldens, and plan goldens for a guarded acquire and a two-source
widening. Remaining for part 2: `compose_key@1` (3.7) and the CLI golden
for category 21, which needs source descriptors supplied to the CLI's
compile.

**Increment 2, part 2a (2026-10-26): `compose_key@1` evaluation.**
`compose_key@1` evaluates with `exact` and `ascii_token` normalization,
the separator and 512-byte bound and `invalid_input` faults (3.7.1 to
3.7.3); the constructed flag is carried through exact steps and read by
`is_constructed`, which is never a gate read, and a composed key's grade
ceiling is `hint` (3.7.4). Screening provenance treats a composed key as
copying its parts' text. Acquisition evaluation still refuses at start and
is part 2b. Tests: `crates/rustev-core/tests/compose_key.rs`.

**Increment 2, part 2b (2026-10-26): staged acquisition and the seam.**
`compose_key@1`'s compile-time argument checks and `request_constructed`
join part 2a's evaluation (3.7). Staged evaluation (3.11) gains
`pending_acquisitions`, `supply_page` and `supply_acquisition_failure`:
guards, exact request building, the `skipped{guard}` outcome and its
propagation (3.4.3, 3.4.4, 3.8.1, 3.8.2), page validation through the
contract's validator, paging and stop conditions (3.6), freshness and
value metadata (3.4.6) and the truncation notice (3.4.7), with skipped
steps recorded in evidence as `StepStatus::Skipped{guard}`. The
`AcquisitionSource` seam of 3.12 is defined in `rustev-core` with the
std-only `BoxFuture` the other seams use; the core never calls it.
Widening evaluation (3.5.5 to 3.5.8) still refuses at start and is part 3.
Tests: `crates/rustev-core/tests/acquisition_eval.rs`, including
byte-identical judgments and evidence across all 90 valid supply
interleavings of three two-page acquisitions.

**Increment 2, part 3 (2026-10-24): widening evaluation.**
`crates/rustev-core/src/widening_eval.rs` evaluates `widen` steps
(3.5.3 to 3.5.8): tiers strictly in declared order, one at a time, a tier
skipped by a false guard or a skipped read and failed by an unresolved
read under `on_tier_failure`; every request built from the base request
and the tier's patch with each protected field exactly as the base
request computes it; relaxation and source switching through each
source's field map; pooling by `dedupe_key` with duplicates counted and
the first occurrence kept, `tier_then_source_rank` order, the
`unique_at_least` stop with later tiers `not_needed`; pooled item origins
and per-tier outcomes in evidence (3.14). Tests in
`crates/rustev-core/tests/acquisition_eval.rs`: specific-first widening
(specific succeeds and broad is `not_needed`, specific empty and broad
fills, specific skipped), protected fields asserted on every request,
two-source pooling, and supply-order independence.

**Increment 3 (2026-10-24): runtime dispatch.** `crates/rustev-runtime/src/acquisition.rs`
installs acquisition sources through new entry points (existing
signatures unchanged, 3.16.4) and dispatches pending acquisitions through
them (3.13): acquisition windows from each step's `deadline_share`,
retries for `transient`, `overloaded` and `timed_out` only when named in
`retry_on` and never past the window's close, `denied` never retried and
supplied as `missing_evidence`, spec 003's attempt timeout applied to
each acquisition attempt, cost reserved and settled in the decision's
ledger with `acquisition.max_calls` enforced, cancellation for each
`cancellation` kind, contained source panics, paging and widening across
sources, and every page and attempt recorded in `rustev.run/3`. Pages are
cached under spec 017 only when the operation is `cacheable` and the
cache policy opts in through its new optional `acquisition_pages` member
(absent means off, so existing policies keep their bytes), keyed by the
request identity, `SourceId` and the full authorized scope, with a hit
recorded with its age (3.13.7). Tests:
`crates/rustev-runtime/tests/acquisition.rs` under the manual clock with
scripted sources, and `crates/rustev-contract/tests/acquisition_cache.rs`.

**Increment 4 (2026-10-24): capture and offline replay.** Every page is
captured by request identity in the decision's shared supply order, and
`rustev.replay/2` bundles admit every plan 3 decision, lifting spec 032's
interim `capture_unsupported` refusal (3.15,
`crates/rustev-eval/src/acquisition_replay.rs`). Offline replay supplies
captured pages in recorded order and never dispatches; an uncaptured
request is `missing_evidence` for its step and the comparison is
`incomparable{acquisition-not-captured}`, with the judgment kept for
inspection. A plan 2 decision's bundle stays byte-identical
`rustev.replay/1`. Replay 2 validation admits gaps in the supply sequence
(ordering and uniqueness still checked), an absent `sources` member and
an empty page list, so a removed page reaches evaluation; replay 1
validation is unchanged. Tests: `crates/rustev-eval/tests/acquisition_replay.rs`
(a paged decision reproduced byte-identically with zero source calls, a
removed page, a spec 032 plan 3 decision with `rustev.snapshot/2`, a
`rustev.judgment/2` decision, replay 1 compatibility).

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #59 leaves the points below open, and each is
decided within the binding principles, as written:

1. **Shared #76 track versions, chosen by content.** Following the #76
   coordination rule, the constructs are added to the track versions spec
   032 introduces (`rustev.source/1`, `rustev.definition/2`,
   `rustev.plan/3`, `rustev.evidence/2`, `rustev.run/3`,
   `rustev.exact/2`), and this spec
   introduces the track version `rustev.replay/2`, which later #76 specs
   extend. Members it adds to `rustev.run/3` (also spec 032's) are
   optional. Every member added is omitted when
   unused, so plans with no #76 construct keep their version 1 documents
   and identities, and plan 3 documents that use none of this spec's
   constructs keep theirs. The new refusal categories are 20 and 21, the
   next free after spec 035.
2. **Acquired data are step values.** The snapshot stays immutable and
   `SnapshotId` unchanged; OD-3's "initial snapshot plus declared, recorded
   acquisitions" is realized as steps.
3. **Reasons.** Acquisition failures map onto existing `Unresolved` reasons
   (3.11.4), so no handler vocabulary changes; the evidence names the
   source. A denied lookup is `missing_evidence`: the context is not
   available to this principal.
4. **Skipped.** A distinct outcome that the policy must handle and cannot
   propagate (3.8), so a skipped lookup is neither an empty list nor a
   failure.
5. **Failed tiers.** A pool that did not reach its stop after a failed tier
   is unresolved, because a failure may have shortened it; a pool that
   reached its stop is resolved with a visible `fallback` notice.
6. **No runtime source fallback.** Retries are declared per step; trying
   another source is a declared widening tier, keeping one mechanism.
7. **Relaxation** unsets the group's request fields and never substitutes
   values; the relaxation level is the number of relaxed groups.
8. **Mapping** is a total rename with equal types; no conversion, so value
   kinds never change across sources.
9. **Deadline share** is a fraction of the definition-level
   `limits.deadline_ms` (the plan's deadline), opened when
   a step first requests, and shares sum to at most 1.
10. **As-of skew** is declared per step (`as_of_skew_ms`, at most 60 s),
    because a live page can be newer than the caller's evaluation time;
    spec 002's zero-skew rule for snapshot entries is unchanged.
11. **Replay of a missing capture** supplies `missing_evidence`, as #59
    requires, and also reports the case incomparable, so spec 004's rule
    that no invented absence counts as a comparable result still holds.
12. **No fan-out acquisitions** in this version; a request takes a list
    field for joins with live data.
13. **Run record.** Page attempts, cost and latency are an optional
    `acquisitions` member of spec 032's `rustev.run/3`. Combining it with
    spec 020's `rustev.run/2` is left to a later change (3.14.3).
14. **Grades** follow spec 032: response fields declare a ceiling like
    spec 032 facts, acquired values carry its value metadata with basis
    `acquired{step}`, and protected fields are hard gates. Model-derived
    request values wait for the acceptance rules of OD-2 (3.9.2).
15. **The seam** is a separate trait, `AcquisitionSource`, whose payloads
    are canonical documents, so `ContextSource` is unchanged and #76 A2 can
    carry it remotely without redefining meaning.
16. **One descriptor.** Acquisition operations are a member of spec 032's
    `rustev.source/1`, as spec 032 anticipates, rather than a second
    descriptor type, so one source `name` binds both snapshot facts and
    operations.
17. **Replay of every plan 3 decision.** `rustev.replay/2` admits plan 3
    decisions whatever track constructs they use, because grades, presence,
    rules and judgments are recomputed by the pure core from retained
    inputs; this lifts spec 032's interim `capture_unsupported`.
18. **Parameter sites** (spec 034): only the stop `count`; limits, tiers,
    groups and mappings stay literal so the compile-time budget and the
    protected-group check are fixed per plan.

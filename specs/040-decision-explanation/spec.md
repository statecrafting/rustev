---
id: "040-decision-explanation"
title: "Decision explanation API and `rustev explain` (amends 004 and 006)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved specs 004 and 006 (R-16), for #76 child D2.
  A pure function over recorded evidence (a delivered `rustev.run/1`
  record, or a `rustev.replay/1` bundle, with the plan document when
  supplied) answers seven structured questions: why an item was selected,
  why one was excluded, why one item precedes another, why the outcome is
  what it is, where text came from, which models answered at what cost,
  and where two decisions first diverge. Answers are written as a
  versioned `rustev.explanation/1` document in which every fact cites the
  evidence path it was copied from and carries the derivation class and
  lineage its source records; a fact the evidence does not hold, or holds
  only as a digest, or that a supplied redaction policy withholds, is
  `not_available` with a typed reason, never a guess. Nothing is
  re-executed, compiled or dispatched and no model is called. Adds a
  deterministic Markdown rendering, `rustev explain` in the CLI, and the
  binding surface for the Node and WebAssembly packages and the service.
  No existing document, identity or CLI output changes. Approved (A-37);
  implementation is pending.
amends:
  - "004-evaluation-and-replay"
  - "006-cli-surface"
extends:
  # `rustev.explanation/1`, the query type and the evidence path type
  # (3.2, 3.3). Purely additive: no existing document or byte changes.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: additive }
  # The explainer, the diff and the Markdown renderer (3.4 to 3.14).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `rustev explain` and its exit statuses (3.16, 3.17).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Adds 040 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "009-remote-adapter-protocol"
  - "020-remote-protocol-refinements"
  - "027-node-and-wasm-bindings"
  - "028-opentelemetry-evidence-sink"
  - "029-evaluation-repeatability"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Explaining performs no re-execution: it starts no evaluation, compiles or recompiles no plan, supplies no value, constructs or calls no backend, calls no model, performs no I/O and reads no clock; the CLI and the bindings read files and resolve bundle items before the call."
    anchor: "3-4-the-explainer"
  - id: "I-2"
    kind: invariant
    text: "Every available fact in an explanation is a copy of, or a comparison stated over, values at the evidence paths it cites, and every cited path resolves in the supplied inputs; a fact the inputs do not hold is not_available with a typed reason, never a default, an inference or a recomputed value."
    anchor: "3-5-facts-availability-derivation-and-lineage"
  - id: "I-3"
    kind: invariant
    text: "Every fact carries the derivation class and lineage its source records, or an upper bound marked as such; a statement computed over cited values is exact-derived only when every compared value is exact-derived, and is never less derived than a value it reads."
    anchor: "3-5-facts-availability-derivation-and-lineage"
  - id: "I-4"
    kind: invariant
    text: "Under a supplied redaction policy, no value whose effective action is other than allow appears in the explanation or its rendering, raw or transformed; it is not_available with reason redacted and the policy identity."
    anchor: "3-13-redaction"
  - id: "I-5"
    kind: invariant
    text: "No existing document, schema, canonical byte, plan identity, CLI output or exit code changes; every existing plan, definition, run, replay and CLI golden stays byte-identical."
    anchor: "3-18-compatibility"
  - id: "R-1"
    kind: requirement
    text: "diff names as its first divergence the earliest difference in the fixed comparison order of 3.11, and for two replayed records that differ in one backend output names that step and instance with reason backend-output-changed."
    anchor: "3-11-diff"
  - id: "R-2"
    kind: requirement
    text: "The same evidence, query and policy produce byte-identical explanation documents and renderings through the library, the CLI, the native binding and the WebAssembly package."
    anchor: "3-15-library-surface-bindings-and-service"
---

# 040: Decision explanation API and `rustev explain` (amends 004 and 006)

Approved by the owner (A-37, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child D2 (#71). The approved texts of
specs 004 and 006 are not edited; this spec records what changes, as 029
does. Approval authorizes no release or publication, no live provider call
or spend, no real-user data, and no change to any existing document, plan
identity or CLI output.

## 1. Purpose

The questions people ask of a decision are few and repeatable: why was this
item in the result, why was that one excluded, why this order, which rule
produced the outcome, why was the request refused or escalated, did text
fall back, which model answered and what did it cost, and what changed
between two runs. Today each answer means reading code, plan documents and
run records by hand. This spec makes the answers a pure, versioned function
of the evidence Rustev already records, so that a host, a reviewer or a
regression triage (spec 029, 3.10.3) gets the same answer every time, with
the path of every fact it relies on.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002), additive: the document
  `rustev.explanation/1`, the query type, the evidence path type and the
  closed vocabularies of 3.3 to 3.11.
- `crates/rustev-eval/` (owner spec 004): the explainer, the diff and the
  Markdown renderer, as pure functions over parsed documents and resolved
  bundle items. Eval already parses run records, plans and bundles and
  resolves retention items through a host resolver (spec 004, 3.1.7); its
  normal dependencies stay contract, core, `serde` and `serde_json`.
- `crates/rustev-cli/` (owner spec 006): `rustev explain` (3.16) and the
  status of 3.17.
- `crates/rustev-surface/` and `tools/rustev-conformance/` (planned by
  spec 027): the `explain` command for the native and WebAssembly packages
  and its conformance cases (3.15). Those directories are not yet written,
  so this spec declares no edge on them; the change that adds `explain`
  there after spec 027 lands declares the extension then.
- `Makefile` (owner spec 001): 040 joins `make verify` at completion.

No change to `rustev-core`, `rustev-runtime`, any backend or integration,
any plan, definition, descriptor, snapshot, run record or bundle schema, or
any dependency. The explainer reads the evidence a run already produced; it
does not ask the runtime to record more. Facts that later specs add to the
record are read through the facet mechanism of 3.12.

### 2.1 Binding principles of #76

- **Proposals, never grants.** An explanation describes recorded evidence.
  It is not a judgment, a proposal, an input or an authorization; no member
  of it is accepted by an executor, and it defines no `Permitted` value.
  "Selected" means "appears in the proposal's recorded parameters", never
  "allowed".
- **Derivation classes and lineage.** Every fact carries the derivation
  class and lineage of its source, or a stated upper bound (3.5). A
  comparison over model-derived scores is not presented as exact.
- **Unresolved, never a default; no silent kind conversion.** A fact the
  evidence does not hold is `not_available` with its reason (3.5.3). Scores,
  distributions, labels and ranks are copied with their recorded kind; the
  explainer converts none of them, and compares scores only within one
  recorded ranking (3.7).
- **Pure core; no async runtime in contract and core.** `rustev-core` is
  untouched. The explainer in eval is synchronous, performs no I/O, reads
  no clock (bundle expiry uses the caller's `now_ms`) and calls no model.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and still passes.
- **No dynamic plugin loading.** Facet sources are compiled into the
  explainer (3.12); nothing is loaded at run time.
- **No identity, authority or knowledge store.** Rustev stores no record,
  bundle or explanation; the host passes them in and keeps the outputs.
- **Plan identities.** No plan construct is added. Every existing plan and
  definition golden stays byte-identical, with its `PlanId`.

## 3. Behavior

### 3.1 Evidence sets

1. An **evidence set** is the explainer's input for one decision, in one of
   two forms:
   - **record form**: a run record (spec 003, 3.9), as delivered by a
     live run or embedded by a failed delivery, and optionally the plan
     document it names;
   - **bundle form**: a replay bundle (spec 004, 3.2) with the
     caller's trusted scope, `now_ms` and a resolver for external items,
     exactly as reproduction takes them. The bundle's retained plan is the
     plan; a separately supplied plan is refused (`plan-ambiguous`).
   The explainer reads `rustev.run/1`, `rustev.evidence/1`,
   `rustev.replay/1` and `rustev.plan/2` as they are today, and each later
   version the contract parses: the #76 track versions `rustev.run/3`,
   `rustev.evidence/2`, `rustev.replay/2` and `rustev.plan/3`, which the
   lowest-numbered merged #76 spec introduces and later specs add optional
   members to (the #76 coordination rule). A track member that a record
   omits is `not-recorded`; this spec claims no version of its own except
   `rustev.explanation/1`.
2. Each document is parsed under its own limits (`RECORD_V1` for run
   records, `PLAN_V1` for plans, `REPLAY_V1` for bundles). A supplied plan
   is accepted only when its `PlanId` (spec 002, 3.3) equals the record's
   `plan_id`; otherwise `invalid_input` with reason `plan-mismatch`. A plan
   is parsed and identified, never compiled or recompiled.
3. In bundle form, the scope is checked before any item is resolved (spec
   004, 3.1.5); a difference is `invalid_input` with `scope-mismatch`. Item
   availability follows spec 004, 3.1.7, at the caller's `now_ms`:
   - an `available` item is read;
   - `missing`, `expired`, `erased`, `inaccessible` and `digest_only`
     items are not read; every fact that needs one is `not_available` with
     reason `retention` and the availability (3.5.3). The item's digest
     remains citable;
   - a `corrupt` or `mismatched` item refuses the evidence set
     (`invalid_input`, `evidence-integrity`, naming the item): evidence
     that fails integrity is not explained.
   If the run record item itself is unavailable, the explanation is still
   produced, and every fact needing the record is `not_available`.
4. The capture status of a bundle (spec 004, 3.2) is a fact of every
   explanation from it. A capture that is not `complete` does not refuse
   explanation: the run record it retains is still evidence of what was
   recorded, and the status is shown beside every answer.
5. A run record whose core evidence is `not_produced` (a cancelled or
   abandoned decision, spec 003, 3.9.1) has no judgment: queries over the
   judgment answer `not_available` with reason `not-produced`; `models`
   and `diff` still answer from the request records.

### 3.2 Evidence paths

1. An **evidence path** is `<role>:<pointer>`, where `pointer` is an RFC
   6901 JSON Pointer into the record canonical form (spec 004, 5.6) or
   canonical form of the named document, and `role` is one of:
   `record` (the run record), `plan` (the plan document), `bundle` (the
   bundle document), `snapshot` (the resolved snapshot item), and
   `supply/<n>` (the resolved value document of the bundle supply with
   sequence number `n`). In a diff every path is prefixed `a.` or `b.`.
2. A path cites a value only if it resolves in the supplied, available
   documents. A digest of an unavailable item is cited by the path of the
   digest inside the bundle, never by a path into the item.
3. Paths are compared and sorted as byte strings. A fact lists its paths in
   that order without duplicates.

### 3.3 The explanation document, `rustev.explanation/1`

1. A record canonical document (spec 004, 5.6; it holds binary64 numbers
   copied from records), parsed under `REPLAY_V1`, unknown fields and
   duplicate keys refused. Its identity is the record digest of its bytes.

   ```text
   explanation = { schema: "rustev.explanation/1",
     explainer: "rustev-explain/1",
     facet_sources: [ facet ],
     sides: [ side ],                 // one; two for a diff ("a", "b")
     redaction: null | { policy: RedactionPolicyId },
     query: query,
     answer: answer,
     facts: [ fact ] }
   side = { name: "a" | "b", form: "record" | "bundle",
            record: digest-ref, plan: null | PlanId, bundle: null | digest,
            capture: null | capture-status, decision_id: string | withheld,
            plan_id: PlanId | withheld }
   withheld = { not_available: unavailable }  // reason `redacted` (3.13)
   item = string | withheld
   digest-ref = { digest } | { not_available: unavailable }
   query = { why_selected: { item } } | { why_excluded: { item } }
         | { why_order: { a: item, b: item } } | "why_outcome"
         | "text_origin" | "models" | "diff"
   fact = { id: "f<n>", claim: claim, value: json | null,
            availability: "available" | { not_available: unavailable },
            derivation: null | { class, bound: "exact" | "judgment" | "plan" },
            lineage: null | { inputs: [string], steps: [string],
                              bound: "exact" | "judgment" | "plan" },
            truncated: null | { policy: RedactionPolicyId, bytes },
            cites: [path] }
   facet = { name: string, spec: string, path: path }  // 3.12.2
   ```

2. `explainer` names the rules of 3.4 to 3.14. Any change to how an answer
   is built, other than adding a facet source (3.12), is a new explainer
   name; `rustev-explain/1` never changes meaning.
3. `item` is 1 to 256 bytes of UTF-8 (the contract's `Handle` rules); in
   `why_order`, `a` and `b` differ. Otherwise the query is refused
   (`invalid_query`).
4. `facts` are numbered `f1`, `f2` and so on in the order the answer
   construction of 3.6 to 3.11 creates them; `answer` refers to facts only
   by id. `value` is null exactly when the fact is not available.
5. Bounds: at most 10 000 facts, and the whole document within every
   limit of `REPLAY_V1` (16 MiB, 1 000 000 values, depth 48). A list a query
   would emit beyond either bound is cut in its stated order at the last
   fact for which the complete document, answer included, stays within
   both, and the answer states `truncated: true` and the total. Copied
   values come from bounded documents, but several of them together can
   exceed 16 MiB, so the explainer checks the bound while it builds and
   never relies on the sources' own limits.

### 3.4 The explainer

1. `explain(evidence_set, query, options) -> Result<Explanation,
   ExplainError>` and `explain_diff(a, b, options)` in `rustev_eval::explain`.
   `options` holds the optional redaction policy (3.13). The function is
   total over valid inputs: every refusal is an `ExplainError` with a code
   from 3.17, never a panic.
2. The explainer reads only: the run record, the plan document, the bundle
   and its resolved items, the redaction policy, and the query. It does not
   start an `Evaluation`, call `supply`, compile, load (spec 004, 5.2),
   recompute a `RequestId` or call any seam. A value it does not find in
   those documents is not available; the explainer never derives a step
   value from the snapshot and the plan.
3. The only computations are: locating values by id; comparing two copied
   numbers or strings with the ordering the source operator declares
   (3.7); matching an item id against the declared id field of a recorded
   input list (3.7.1); selecting the first element of a recorded ordered
   list; counting
   and summing recorded integers (3.10); and the static graph closure of
   3.5.5. Each is stated where it is used.
4. Determinism: equal inputs and query give equal document bytes on every
   target. No binary64 arithmetic is performed; recorded binary64 values
   are copied and compared, never combined.

### 3.5 Facts, availability, derivation and lineage

1. A fact has a **claim** from a closed vocabulary per query (3.6 to 3.11)
   and a **value** copied verbatim from the cited path, or a comparison
   result stated in 3.7.
2. **Plan-linked facts.** The judgment's `trace` (spec 002, 3.11) names the
   handlers, the matched rule and the adjustments as `on_unresolved[i]`,
   `rule:<id>`, `rule:<id>:unhandled` and `adjustment:<id>[:unhandled]`.
   With a plan, a rule id resolves to its declaration in the plan, and an
   outcome parameter resolves to the step its rule's `propose` reads
   (`step:<id>`). Without a plan these facts are `not_available` with
   reason `plan-not-supplied`; the trace strings themselves remain
   available from the record.
3. **Unavailable reasons**, a closed set:

   | Reason | Members | When |
   |---|---|---|
   | `not-recorded` | `facet` (when a facet) | The record's schema carries no such fact (3.12). |
   | `plan-not-supplied` | none | The fact is a plan declaration and no plan was supplied. |
   | `retention` | `item`, `availability` | The item holding it is not available (3.1.3). |
   | `not-produced` | none | Core evidence is `not_produced` (3.1.5). |
   | `redacted` | `policy`, `field_class` | A supplied redaction policy withholds it (3.13). |
   | `not-in-record` | none | The record holds the containing value, but not this item or member. |

   A fact of 3.6 to 3.11 whose cited path names an item or member absent
   from a containing value that is present (for example an item missing
   from a recorded ranking's entries, or an attempt member a request record
   lacks) is `not-in-record`. When the containing value itself is absent,
   the reason is that value's own: `not-recorded` when the schema has no
   such member, else `retention` or `not-produced`.

   The Markdown rendering of each is fixed by 3.14.
4. **Derivation.** A copied step value takes the class its step evidence
   records (`steps[i].status.resolved.derivation`), with `bound: "exact"`;
   this is available when the value is linked to its step through the plan
   (3.5.2). A value of an outcome parameter not linked to its step takes
   the judgment's class with `bound: "judgment"`: the judgment is computed
   from every value it reads, so its class is never less derived than the
   parameter's (spec 002, 3.12.2). An input fact takes `model-derived` when
   its recorded provenance is `model-derived`, else `exact-derived`. A
   comparison fact (3.7) is `exact-derived` when every compared value is
   `exact-derived` and `mixed-derived` otherwise, with the bound of its
   least exact operand. Plan declarations, trace strings, attempt records
   and costs are runtime or plan observations, not decision values, and
   have `derivation: null`.
5. **Lineage.** The judgment's lineage is copied with `bound: "exact"`.
   A step's lineage is the transitive closure of the input and step
   references its plan declaration makes, with `bound: "plan"`: the static
   set it may read, never narrower than what it did read. Without a plan,
   a parameter value takes the judgment's lineage with `bound: "judgment"`.
   Lineage lists are sorted and deduplicated.

### 3.6 `why_selected(item)`

1. **Selected** means the item id appears in a recorded outcome parameter
   of a `propose` judgment: in `eligible` of a `filtered` value, in `ids`
   of a `shortlist`, or as `candidate` of a `ranking` entry. Parameters are
   visited in key byte order; a value nested in a `list` or `record`
   parameter is visited in its order. Each place found is a **scope**.
2. Facts, per scope in order: `membership` (the parameter name and kind and
   the item's index or entry); for a ranking, `rank_position`, `score` and
   `components` (copied), and, with a plan, `weights` and the `operator`
   of the step (`weighted_rank@1` and its declared components); for a
   shortlist, `shortlist_position` and, with a plan, the ordering key the
   `top_k@1` step declares (its per-item key values are not recorded:
   `not-recorded`); for a filtered value, `filters_passed`: with a plan,
   the rule names of the `filter_with_reasons@1` step, all of which the
   item passed, and without one `plan-not-supplied`.
3. Then, once: `proposing_rule` (the `rule:<id>` trace entry and, with a
   plan, its declaration), `adjustments` (trace entries in order),
   `judgment_derivation` and `judgment_lineage`, and the facets of 3.12
   that apply to a selected item: `tier`, `relaxation` and `origin`.
4. Answer: `{ kind: "selected", scopes: [ { param, value_kind, facts:
   [fact ids] } ], rule: fact id, adjustments: fact id, facets: [fact ids]
   }`, one `scopes` entry per scope in the order of 3.6.1 holding that
   scope's facts of 3.6.2 in creation order, `value_kind` being
   `filtered`, `shortlist` or `ranking`; or
   `{ kind: "not_selected", see: "why_excluded" }` when no scope contains
   the item; or `{ kind: "no_proposal", outcome: fact id }` when the
   outcome is not `propose`.

### 3.7 `why_excluded(item)` and `why_order(a, b)`

1. `why_excluded(item)`, over the same parameters in the same order:
   - an `excluded` entry of a `filtered`, `shortlist` or `ranking` value
     naming the item gives `exclusion` with `first_failing` (the first
     element of its `reasons`, which are in rule order, spec 002, 3.8),
     `all_reasons` (copied), and `reason_kind`: `rule` for a rule name,
     `unevaluable` for `unevaluable:<rule>`, `unresolved_component` for a
     ranking's unresolved component; with a plan, the failing rule's
     declaration;
   - a shortlist whose `truncated` count is non-zero, when the item is
     eligible upstream (it is in an `eligible` list of another parameter)
     but absent from `ids` and `excluded`, gives `cut_by_k`: the declared
     `k` (plan) and the truncated count; whether the item was among the
     truncated is stated only from those recorded values;
   - otherwise, `pool`: with a bundle whose snapshot is available and a
     plan whose filter step reads an input list directly
     (`input:<name>`), whether an element of that recorded list has the
     declared id field equal to the item (`in_pool: true | false`, citing
     the snapshot path); in every other case `not_available` with
     `not-recorded` (the pool is computed, not recorded). The facets
     `presence` and `tier` of 3.12 are added.
   Answer, the first that applies:
   - `{ kind: "no_proposal", outcome: fact id }` when the outcome is not
     `propose`;
   - `{ kind: "excluded", scopes: [ { param, value_kind, exclusion: fact
     id } ], cuts: [ { param, cut_by_k: fact id } ] }` when at least one
     `exclusion` fact exists (`cuts` lists any `cut_by_k` facts too);
   - `{ kind: "cut", cuts: [ { param, cut_by_k: fact id } ] }` when no
     exclusion but at least one `cut_by_k` fact exists;
   - `{ kind: "selected", see: "why_selected" }` when the item is
     selected in at least one scope and excluded or cut in none;
   - `{ kind: "not_in_pool", pool: fact id, facets: [fact ids] }` when the
     `pool` fact is available with `in_pool: false`;
   - `{ kind: "unknown", pool: fact id, facets: [fact ids] }` otherwise,
     including a `pool` fact with `in_pool: true` for an item no parameter
     names (the record does not say what happened to it).
2. `why_order(a, b)`:
   - Both items must be entries of one recorded `ranking` or ids of one
     `shortlist`: the **ordering scope**. Scores are compared only within
     one scope; items in different scopes give `{ kind: "incomparable",
     reason: "different-scope" }`, and an item in no scope gives
     `{ kind: "incomparable", reason: "not-ranked", item }`.
   - In a ranking: facts `position_a`, `position_b`, `score_a`, `score_b`,
     `components_a`, `components_b`, with a plan `weights`, and the
     comparison fact `order_by`: `score` when the scores differ (stating
     which is greater, compared as binary64 with `<`), or `tie_break` when
     they are equal, stating the declared tie rule (`weighted_rank@1`:
     candidate id ascending by byte order, spec 002, 3.8) and the byte
     comparison of the two ids. The fact `consistent` states whether the
     recorded positions agree with that order; `false` is reported, never
     corrected.
   - In a shortlist: `position_a`, `position_b`, and `order_by` as
     `not_available` (`not-recorded`): the key values are not recorded; the
     declared key and tie rule (`top_k@1`: id ascending) are cited from
     the plan.
   - The facet `tier` (3.12) is added: once tiers are recorded, scores
     across tiers are a different scope.
   Answer: `{ kind: "ordered", first: item, by: "score" | "tie_break" |
   "position", facts: [ids] }` or `incomparable`.

### 3.8 `why_outcome()`

1. Facts: `outcome` (the outcome variant and its payload: `propose` with
   action and parameter names, `escalate` with reason, or `unresolved` with
   its reason, fields and detail), `matched` (the trace entry that decided
   it: a handler `on_unresolved[i]` or a rule), with a plan the handler or
   rule declaration, `adjustments`, `notices` (copied: uncalibrated
   thresholds, truncation, fallbacks), `judgment_derivation`,
   `judgment_lineage`, and the record's `termination`.
2. **Cause chain** for an `unresolved` or handler-decided outcome: the
   inputs and steps the outcome names (`fields`, or with a plan the
   handler's subject) are traced to their evidence: an input's recorded
   status; a step's recorded status; for a semantic step, its request
   records (result, attempts with failure class and detail, retry and
   fallback transitions, spec 003, 3.6); for `budget_exhausted` and
   `deadline_exceeded`, the cost summary and timing. Each link is one fact,
   in the order the outcome names them, then request order.
3. Facets `addressed_to`, `remediation` and `caused_by` (3.12).
4. Answer: `{ kind: "proposed" | "escalated" | "unresolved", outcome:
   fact id, matched: fact id, adjustments: fact id, notices: fact id,
   judgment_derivation: fact id, judgment_lineage: fact id, termination:
   fact id, causes: [fact ids], facets: [fact ids] }`, each keyed member
   naming its fact of 3.8.1, `causes` the links of 3.8.2 in their order,
   and `facets` the facet facts of 3.8.3.

### 3.9 `text_origin()`

1. For every `text` value among the outcome parameters (in the order of
   3.6.1), a fact `text_field` names the parameter and the facet
   `field_origin` (3.12) gives its per-field origin and reasons.
2. Until a record schema carries per-field origin (#76 B5), every
   `field_origin` is `not_available` with `not-recorded`, and the step
   linked through the plan, if any, is cited with its derivation: a text
   from an exact step is shown as `exact-derived`, which is a recorded
   fact, not an origin claim.
3. Answer: `{ kind: "text_fields", fields: [ids] }`, or `{ kind:
   "no_text" }` when the outcome has no text parameter.

### 3.10 `models()`

1. Per request record, in record order: `request` (step, instance,
   result), and per attempt `attempt` (attempt id, target, backend id,
   artifact, dispatched and ended times, end, cancellation, remote state,
   cost bound, reservation and charge), then `transitions`.
2. **Requested and served identity.** The requested identity is the
   attempt's backend id and artifact, with, given a plan, the bound
   descriptor and target index (primary or fallback). The facet
   `served_identity` (3.12) reports what the backend disclosed about the
   model that served it: in bundle form from the retained value document
   when the remote protocol (spec 009, 3.7, as refined by spec 020)
   recorded it, else `not_available` with `not-recorded`. A served identity
   is never inferred from the requested one.
3. **Cost.** The record's cost summary is copied, and per backend id the
   sums of observed, estimated and unknown charges are counted (integer
   addition over the attempt charges, in record order; `unknown` charges
   are counted, never summed as zero). Costs are logical units; no money
   figure is stated.
4. Answer: `{ kind: "models", backends: [ { backend, attempts: [ids],
   served: id, cost: id } ], summary: id }`, backends in byte order of id.
   A record with no request records gives `{ kind: "no_model_calls" }`.

### 3.11 `diff()`

1. `explain_diff(a, b)` compares two evidence sets, each validated by 3.1.
   They may be different decisions, plans or forms; differences in plan or
   snapshot are themselves reported.
2. **Comparison order.** The explainer walks this fixed sequence and
   records every difference as a fact `difference` with both sides' values
   and paths:
   1. `plan_id`;
   2. `snapshot_id`, then `evaluation_time_ms`;
   3. inputs, by field name in byte order: presence, then recorded status;
   4. steps, keyed by (step, instance), in side `a`'s record order and then
      side `b`'s keys absent from `a`: for a semantic step, first the
      supplied value (bundle form: the supply entry's item digest, then
      producing target and attempt; record form: the request result), then
      the step's recorded status (kind and derivation, or unresolved
      reason);
   5. the judgment: outcome, derivation, lineage, notices, trace.
   Runtime observations (decision id, timing, attempt timings and ids,
   cost) are compared separately as `observation_difference` facts and are
   never a first divergence.
3. **First divergence** is the first difference in that sequence. Its
   `why` is from a closed set: `plan-changed`, `snapshot-changed`,
   `evaluation-time-changed`, `input-changed`, `backend-output-changed`
   (supply digests differ with equal backend and artifact),
   `backend-changed` (the producing backend id or artifact differs),
   `request-result-changed` (an output on one side and a failure or no
   supply on the other, or different failures), `step-changed`,
   `outcome-changed`. When both value documents are available, the
   differing values are cited; when either is digest-only, the digests are
   cited and the values are `not_available` (`retention`).
4. **Follows-from.** With a plan on both sides and equal `plan_id`, each
   later difference lists the earlier differences among its static
   references (3.5.5) as `follows_from`; a step or outcome difference with
   none is marked `unexplained`, which identifies a change the evidence
   cannot attribute (for example a compiler change, spec 004, 3.3.2).
   Without plans, `follows_from` is `not_available` (`plan-not-supplied`).
5. At most 10 000 differences are listed in order; the total and
   `truncated` are stated. Answer: `{ kind: "identical" }` when the
   sequence holds no difference, otherwise `{ kind: "diverged", first:
   fact id, why, differences: total, truncated }`.
6. This is the per-case decision diff that spec 029, 3.10.3, names: its
   changed cases list both sides' run record digests, which select the two
   evidence sets.

### 3.12 Facets supplied by later specs

1. A **facet** is a named fact that a later #76 spec adds to the record.
   The names are reserved here with their source child: `tier` and
   `relaxation` (B1), `presence` (B2), `addressed_to`, `remediation` and
   `caused_by` (B3), `field_origin` (B5), `origin` (C2), and
   `served_identity` (spec 009 and 020 disclosures, C1, C3).
2. A facet whose source is not in the record schema the explainer reads is
   `not_available` with `not-recorded` and `facet`. The spec that adds the
   recorded fact states the facet's evidence path and value as an
   amendment of this spec; the explainer then lists the facet in
   `facet_sources` as `{ name, spec, path }`: the reserved name of 3.12.1,
   the adding spec's id and the facet's evidence path (3.2), in the order
   of 3.12.1. Adding a facet source does not change the explainer
   name, because explanations of records without the new fact keep their
   bytes; the change of `facet_sources` is visible in every document.
3. A facet value is copied verbatim from its source path, with the
   derivation and lineage its source records. No facet is computed from
   other facts.

### 3.13 Redaction

1. `options.redaction` takes a `rustev.redaction/1` policy (spec 028,
   3.4). Without one, the explanation copies record content as the record
   holds it, and the host handles it like the record.
2. With one, every copied value is classed by spec 028, 3.3 (field classes
   and the lineage rule for values): ids (decision, item, candidate,
   attempt) as `id`, free text written at run time as `free-text-detail`,
   parameter and score values by the lineage rule, plan declarations and
   names as `plan-metadata`, backend and artifact ids as `model-identity`.
   A value's action is its class's entry in the policy, else the class
   default (spec 028, 3.4.3); the policy's `fields` overrides, which name
   telemetry catalogue fields, do not apply to explanation facts. A
   value whose action is `allow` is copied; `truncate{bytes}` copies
   the cut text and sets the fact's `truncated` to the policy identity and
   byte count; `drop` and `digest` withhold it
   as `not_available` with `redacted`, the policy's `RedactionPolicyId`
   and the class. The explainer computes no keyed digest and takes no key.
3. The query's item ids and each side's decision id are withheld the same
   way; the policy identity is recorded in `redaction`. Comparisons over
   withheld values (3.7.2) are still made, and their result is shown when
   the comparison's own class (`enum`) is allowed, without the values.
4. Records that a later spec writes with a value already replaced by a
   policy digest are read as withheld with that policy's identity.

### 3.14 Markdown rendering

1. `render_markdown(&Explanation) -> String` renders from the document
   only, deterministically: a title with the query; a sides table (form,
   decision, plan, capture); the answer in one sentence per answer kind
   from fixed templates; then one table of facts (claim, value, derivation,
   lineage, cites). It is a rendering, never a second source.
2. An unavailable fact renders as `not available: ` followed by: `not
   recorded` (and `(facet <name>)`), `plan not supplied`, `retention
   <availability> (item <path>)`, `not produced`, `redacted by policy
   <RedactionPolicyId>`, or `not in record`. An available fact whose
   `truncated` is set renders its value as in 3.14.3 followed by ` (truncated
   by policy <RedactionPolicyId> to <bytes> bytes)`, distinct from the
   display cut of 3.14.3.
3. Values render as their canonical JSON in a code span, cut at 256 bytes
   at a character boundary with `...` appended; the JSON document is
   complete.

### 3.15 Library surface, bindings and service

1. **Library.** `rustev_eval::explain::{explain, explain_diff,
   render_markdown}` and the contract types. The facade crate of spec 031
   re-exports them with eval, under its existing rules.
2. **Bindings (spec 027, 3.7).** Through `rustev-surface`, both packages
   gain `engine.explain(evidence, query, { nowMs?, scope?, resolve?,
   redaction? })`, resolving to `{ explanation, markdown }`, where
   `evidence` is `{ record, plan? }` or `{ bundle }` as bytes and `query`
   is the query of 3.3 or `{ diff: { other: evidence } }`. It is pure, so
   it is available in the WebAssembly package; refusals map to spec 027,
   3.4, as `InvalidInput` with the code of 3.17. The shared conformance
   suite gains every golden of the acceptance for both packages, with
   comparison `bytes`. This joins the bindings when spec 027 is
   implemented; no binding exposes a placeholder before.
3. **Service.** The decision service of #76 A2 (#57) owns the route
   `POST /v1/explain`: its transport, authentication, bounds and envelope
   belong to the spec implementing A2, which is not part of this corpus
   yet (it is proposed separately). This spec only states what any such
   route must satisfy, as requirements on that spec: it takes this spec's
   evidence and query, with evidence as documents or host-resolved
   references; it returns the explanation's record canonical bytes
   unchanged and the Markdown rendering of 3.14, computed by the same
   library function; it applies the redaction policy the host configures
   for the caller (3.13); and it never explains evidence outside the
   caller's authorized scope (spec 010, 3.7.2). Until this spec is
   implemented, no service explains anything.

### 3.16 CLI surface (amends spec 006)

Grammar, bounded file access, no-clobber writes and stdout rules of spec
006, 3.1 and 3.3, apply unchanged.

1. `rustev explain <query>` with subcommands `why-selected` and
   `why-excluded` (each `--item <id>`), `why-order` (`--a <id>`, `--b
   <id>`), `why-outcome`, `text-origin`, `models` and `diff`.
2. Evidence flags: exactly one of `--record <file>` or `--bundle <file>`;
   `--plan <file>` only with `--record`; with `--bundle`, the scope flags
   of spec 006, 3.1.4, `--now-ms` (required) and optional `--store` with
   the store resolver of spec 006, 3.7.1. `diff` also takes exactly one of
   `--other-record` or `--other-bundle`, `--other-plan` only with
   `--other-record`. The scope flags, `--now-ms` and `--store` are given
   once and serve every bundle side. When either side is a bundle
   (including a record on one side and a bundle on the other), `--now-ms`
   is required and the scope flags follow spec 006, 3.1.4; when neither
   is, all of them are refused.
3. Outputs: `--out <file>` (required) receives the explanation's record
   canonical bytes; `--markdown <file>` (optional) the rendering; all or
   none. Optional `--redaction <file>` is a `rustev.redaction/1` policy,
   read under `DESCRIPTOR_V1`.
4. Usage errors (exit 2), before any file is read: a missing or extra
   evidence flag, `--plan` with `--bundle`, `--other-plan` with
   `--other-bundle`, a bundle on either side without `--now-ms`, a scope
   flag, `--now-ms` or `--store` with no bundle on either side, an
   `--item`, `--a` or `--b` that is empty or longer than 256 bytes, `--a`
   equal to `--b`, and an `--other-*` flag outside `diff`.
5. Stdout: `{ command: "explain <query>", status: "explained",
   explanation: <record digest>, answer: <answer kind>, not_available:
   <count> }`. Explanations are files, never embedded in stdout.

### 3.17 Exit codes and refusal codes

Spec 006, 3.2's table is extended; no existing code or status changes
meaning.

| Code | Status | Meaning |
|---|---|---|
| 0 | `explained` | An explanation was written, including one whose answer is `not_available`, `unknown`, `incomparable` or `diverged`. |
| 2 | (stderr only) | As spec 006: a usage error of 3.16.4, before any file is read. |
| 3 | `io_error` | As spec 006. |
| 4 | `invalid_input` | The evidence set, plan, policy or query was refused, with one of the codes below. |

Refusal codes: `plan-mismatch`, `plan-ambiguous`, `scope-mismatch`,
`evidence-integrity`, `invalid_query`, and the document's own parse
refusal (`parse`). Each names the file or item.

### 3.18 Compatibility

1. This spec adds no member to any schema: `rustev.run/1`,
   `rustev.evidence/1`, `rustev.judgment/1`, `rustev.replay/1`,
   `rustev.plan/2`, `rustev.redaction/1` and every eval document keep
   their bytes and identities. Every plan and definition golden keeps its `PlanId`, and
   every run, replay and CLI golden its bytes.
2. Spec 004, 5.6's list of record canonical documents gains
   `rustev.explanation/1`.
3. No existing command, flag, stdout object or exit code changes;
   `explain` is a new command name.
4. Explaining a record changes nothing about it: the explainer takes its
   inputs by shared reference, and a test asserts the input bytes are
   unchanged.

## 4. Out of scope

Recording more evidence at run time (later specs add facts, 3.12);
explaining from `rustev.trace/1` telemetry lines (spec 028), which are a
redacted projection rather than the record; natural-language or
model-written explanations; counterfactuals ("what would have been chosen
if"); re-running a decision or a candidate (spec 004, 3.4, and spec 026);
storing explanations; the service transport (#76 A2); authorization of any
kind.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `--plan` whose `PlanId` differs from the record's | `invalid_input`, `plan-mismatch`; no file written. |
| `--plan` together with `--bundle`, or `--other-plan` with `--other-bundle` | Usage error, exit 2. |
| A bundle under another tenant's scope | `invalid_input`, `scope-mismatch`; no item resolved. |
| A bundle with a corrupt supply item | `invalid_input`, `evidence-integrity`, naming the item. |
| A bundle whose run record item is `digest_only` | Explained; every record fact `not available: retention digest_only`. |
| An expired supply item in a diff | The difference cites both digests; values `not available: retention expired`. |
| A cancelled run (`not_produced`) and `why-outcome` | Explained; answer facts `not available: not produced`; `models` still answers. |
| `why-selected` with no plan | Explained; `filters_passed` and `proposing_rule` declaration `plan not supplied`; membership and scores available. |
| `why-order` across two different rankings | `incomparable`, `different-scope`. |
| `why-order` with equal scores | `tie_break`, citing the declared rule and the id byte order. |
| `why-order --a x --b x` | Usage error, exit 2. |
| `why-excluded` for an id absent from every parameter, record form | `unknown`; `pool` `not available: not recorded`. |
| `text-origin` on today's records | Every `field_origin` `not available: not recorded (facet field_origin)`. |
| `models` with an `unknown` charge | Counted as unknown, never added as zero. |
| A redaction policy with `id` as `digest` | Item and decision ids `not available: redacted by policy <id>`; no keyed digest computed. |
| A redaction policy that drops `third-party` | Ranking scores derived from third-party inputs withheld; the order comparison still stated. |
| Two identical replayed records | `identical`. |
| Two replayed records with one changed backend output | `diverged`, first divergence that step and instance, `backend-output-changed`. |
| Two records of different plans | First divergence `plan_id`, `plan-changed`; `follows_from` not available. |
| Any explanation | Input bytes unchanged; output bytes identical on a second call. |

## Acceptance

- Golden explanations (`explanation.json` and `explanation.md`) for both
  reference plans (support routing and lodging) cover every query, from
  record form with and without the plan and from bundle form, including a
  lodging fixture with an excluded candidate, a ranking tie and a
  truncated shortlist, and a support-routing fixture whose outcome is
  unresolved through a backend failure and one that escalates.
- `diff` on two replayed bundles of the same decision that differ in one
  supplied backend output identifies that step and instance with
  `backend-output-changed`; a second case changes an input and is
  `input-changed`, with the step difference `follows_from` it.
- A record explained under a redaction policy that withholds a class
  renders `not available: redacted by policy <RedactionPolicyId>` for
  every such value, and a test scans the document and the rendering for
  every withheld value's bytes and finds none.
- A property test over generated records asserts that every cited path
  resolves, every available value equals the value at its path, and no
  fact is available whose source is absent.
- Derivation: a test asserts that no fact's class is less derived than its
  source step's or the judgment's, and that a comparison of model-derived
  scores is `mixed-derived`.
- Purity: `rustev_eval::explain` is checked by the existing eval purity
  tests (no I/O, no clock), and a test with scripted seams asserts zero
  backend calls, zero `supply` calls and no compile.
- Every row of section 5 has a test, CLI rows through the binary.
- Compatibility: every existing plan, definition, run, replay, eval and
  CLI golden is byte-identical.
- Seeds in the eval and CLI mutation harnesses (first failing reason taken
  from the end, tie broken descending, a judgment-bound class reported as
  exact, an unknown charge summed as zero, an observation difference taken
  as first divergence, a withheld value rendered, a plan accepted with
  another `PlanId`) are each detected.
- Binding conformance cases of 3.15.2 pass on the native and WebAssembly
  packages. This item depends on spec 027's implementation; until then it
  is reported as externally blocked, never as passed.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #71 leaves the points below open, and each is
decided within this spec, as written:

1. **Where it lives.** The document and query types in `rustev-contract`
   (the issue's "contract (new document)"), the explainer in `rustev-eval`,
   which already parses records, plans and bundles and resolves retention
   items without I/O. `rustev-core` is unchanged.
2. **Inputs.** A run record (live) or a replay bundle (replayed), with the
   plan. The plan is read for declarations only; nothing is recompiled.
   Telemetry lines are not an input: they are a redacted projection.
3. **No new recording.** Today's records do not hold per-item filter
   values, shortlist keys, tiers or per-field origin. Rather than amend
   specs 002 and 003 here, those facts are `not-recorded` facets that the
   specs adding them (B1, B2, B3, B5, C2) fill, so explanations of today's
   records stay stable and honest about what they cannot say.
4. **Evidence paths.** RFC 6901 JSON Pointers into the canonical forms,
   prefixed by a document role, so a reader can check every fact
   mechanically.
5. **Derivation bounds.** Where the record does not link a parameter to
   its step, the judgment's class and lineage are used as a stated upper
   bound rather than omitted or guessed lower.
6. **Redaction.** Explanations reuse spec 028's policy and classes rather
   than a second vocabulary. `digest` is treated as withholding because
   the explainer takes no key; the issue's wording "not available:
   redacted by policy <digest>" uses the policy identity.
7. **Diff order and attribution.** A fixed comparison order with decision
   values before runtime observations, and attribution only through
   declared plan references; anything else is `unexplained`, not guessed.
8. **CLI shape.** One `explain` command with query subcommands, outputs to
   files, one new status `explained` with exit 0; an answer that is
   unknown or not available is still a successful explanation, since the
   command's job is to describe the evidence.
9. **Service endpoint.** `POST /v1/explain` belongs to the spec
   implementing #76 A2, which owns the transport and amends spec 010; this
   spec supplies the function, document and rendering the route returns,
   states the requirements of 3.15.3, and does not amend spec 010 itself.

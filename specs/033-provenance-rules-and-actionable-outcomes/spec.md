---
id: "033-provenance-rules-and-actionable-outcomes"
title: "Provenance-conditional rules and actionable outcomes (amends 002 and 006)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved specs 002 and 006, for #76 child B3. A
  definition may declare which provenance classes the caller controls,
  input checks whose consequence on failure depends on the provenance of
  the value tested (refuse, drop the field, relax a named constraint
  group, downgrade its grade, abstain, escalate, or continue with a
  notice), exhaustive over the classes the input accepts, and policy
  conditions over provenance. Every unresolved outcome, refusal and notice
  of such a plan carries who can act on it, a typed remediation, its
  causes with provenance, and whether retrying can succeed; who can act is
  computed from the causes, and an author may only move it away from the
  caller, never toward it. Adds the `needs_input` outcome with declared,
  literal questions and a continuation digest, and documents how hosts map
  outcomes to transport semantics. Adds to the #76 track versions
  `rustev.definition/2`, `rustev.plan/3` and `rustev.evidence/2`, and
  introduces `rustev.judgment/2`; plans using no construct of this spec
  keep their bytes and `PlanId`. Approved (A-30); implementation is
  pending.
amends:
  - "002-decision-contract-and-pure-core"
  - "006-cli-surface"
extends:
  # Definition, plan, evidence and judgment members; outcome, notice and
  # action types; the two refusal categories (spec 002, 3.5 to 3.12).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Check evaluation, consequences, the actionability compilation phase,
  # the party computation and the needs-input outcome.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Backend registration origin passed into failure supply (3.9); no
  # existing runtime behavior changes.
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  # `plan show` and `run` output for plans using this spec (3.11).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The evaluation case outcome of a judgment 2 outcome (an abstention by
  # its own name); no evaluation document changes.
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list (spec 021, 3.5).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Adds 033 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "032-evidence-quality"
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A definition that uses no construct of this spec compiles to the same plan bytes and PlanId as before this spec, and its decisions produce the same rustev.judgment/1, evidence and run record bytes; no existing refusal changes category or order."
    anchor: "3-12-compatibility"
  - id: "I-2"
    kind: invariant
    text: "An outcome or notice is addressed to the caller only when every cause is an input whose provenance class the definition declares caller-controlled; no declaration, override or remediation can address the caller otherwise, and the compiler refuses any that would."
    anchor: "3-5-actionable-outcomes"
  - id: "I-3"
    kind: invariant
    text: "A check over an input that accepts several provenance classes covers each accepted class with a branch or a default; otherwise the definition is refused naming the uncovered classes."
    anchor: "3-2-checks"
  - id: "I-4"
    kind: invariant
    text: "A dropped field is absent, read as missing_evidence by every dependent; no consequence substitutes a value, and no check result is a default."
    anchor: "3-3-consequences"
  - id: "R-1"
    kind: requirement
    text: "Every question of a needs_input outcome is a declared literal template, byte-identical to its declaration; no model output or input value enters a question's text."
    anchor: "3-7-the-needs-input-outcome"
  - id: "R-2"
    kind: requirement
    text: "transient is computed from the fixed table of 3.6 and is true only for backend_unavailable, deadline_exceeded, and invalid_backend_output from a backend whose determinism is not bitwise."
    anchor: "3-6-remediations-and-transience"
---

# 033: Provenance-conditional rules and actionable outcomes (amends 002 and 006)

Approved by the owner (A-30, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child B3 (#61). The approved texts of
specs 002 and 006 are not edited; this spec records what changes. It builds
on spec 032 (#76 B2, evidence quality), which introduces the #76 track
versions `rustev.definition/2`, `rustev.plan/3` and `rustev.evidence/2`,
value metadata and grades; this spec adds members to those versions and
does not restate them. The #76 children are specified as specs 032 to
044, one per child; spec 038 (#76 B6, structured extraction) builds on the
actionable outcomes and the `needs_input` outcome defined here.

## 1. Purpose

The same field can arrive from different places. A trip window typed by the
caller and a trip window inferred by a model from behavioral data fail a
"dates must be in the future" check for different reasons and need
different consequences: refuse and tell the caller to send future dates in
the first case; drop the inferred window and continue undated in the
second, because the caller sent nothing and cannot fix anything. Today a
host writes this as ad hoc mode checks, and an unconditioned rule produces
advice its recipient cannot act on. More generally, a host maps outcomes to
transport statuses and messages; without knowing who caused an outcome and
who can fix it, hosts misreport dependency failures as client errors and
fallbacks as failures.

This spec lets checks and policy conditions depend on the provenance of the
value they test, gives checks field-level consequences, and makes every
unresolved outcome, refusal and notice say who can act on it and how,
computed from its causes so that advice the caller cannot act on cannot be
authored.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the definition 2 members of
  3.1, the plan 3 `actionability` member, the evidence 2 check records, the
  document `rustev.judgment/2` with the outcome, notice and action types of
  3.5 to 3.7, and the two refusal categories of 3.8.
- `crates/rustev-core/` (owner spec 002): check evaluation, consequences,
  the provenance condition, the party computation, remediations, the
  `needs_input` outcome and the actionability compilation phase.
- `crates/rustev-runtime/` (owner spec 003): the backend registration
  origin of 3.9, passed with every failure the runtime supplies.
- `crates/rustev-cli/` (owner spec 006): the output of 3.11.
- `Makefile` (owner spec 001): 033 joins `make verify` at completion.

No dependency is added and no crate boundary changes. The Node and
WebAssembly bindings (spec 027) return the judgment as the Rust result
holds it, so they carry the new fields without change of their own; their
registration of host-implemented backends with origin `host` (3.9) is part
of spec 027's implementation.

### 2.1 Binding principles of #76

- **Proposals, never grants.** `addressed_to` and a remediation state who
  can act and how; neither is a permission, and a caller-addressed outcome
  authorizes nothing. `refused` and `needs_input` are outcomes, not
  grants. No `Permitted` type is defined.
- **Derivation classes and lineage.** Checks read values and change none of
  their classes or lineage. A judgment decided by a check carries the
  derivation and lineage of the check's reads (3.4.6). `caused_by` records
  provenance; it never changes it.
- **Unresolved, never a default; no silent kind conversion.** A dropped
  field is absent and read as `missing_evidence`; a check that cannot be
  evaluated makes its subject unresolved with the reason; no consequence
  supplies a value. No value kind changes.
- **Pure core; no async runtime in contract and core.** Checks are exact
  predicates over supplied inputs and the supplied evaluation time `now`;
  the core reads no clock. Nothing new is asynchronous.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged and still passes.
- **No dynamic plugin loading.** Remediation codes and questions are
  declared data in the definition; nothing is loaded as code.
- **No identity, authority or knowledge store.** The continuation digest
  (3.7) is computed, never stored; at-most-once questioning across turns
  stays with the host, which owns session identity.
- **Plan identities.** Definitions without any construct of this spec keep
  byte-identical plans and `PlanId`s (3.12).

## 3. Behavior

### 3.1 Documents and versions

1. **Definition** (adds to `rustev.definition/2`, introduced by spec 032):
   an optional member `actionability`, omitted when unused, holding
   `caller_controlled`, `remediations`, `questions`, `constraint_groups`
   and `checks` (each a list, possibly empty), and the policy constructs
   of 3.4.7 and 3.7. Any of these is a construct of this spec. A
   construct of this spec inside a `rustev.definition/1` is refused,
   category 11 `invalid_definition`, detail `requires-definition-2`.
2. **Plan** (adds to `rustev.plan/3`, introduced by spec 032): an optional
   member `actionability`, present exactly when the definition has one,
   holding the canonical declarations and the static table of 3.8.3 (per
   check branch and per policy site: the static causes and static party).
   `PlanId` therefore changes with any declaration of this spec.
3. **Judgment** (introduces `rustev.judgment/2`, a #76 track version, under
   `RECORD_V1`, record canonical, spec 004 5.6): `rustev.judgment/1` with
   the outcome of 3.5.1 and notices of 3.5.4. A plan with `actionability`
   produces `rustev.judgment/2`; every other plan produces
   `rustev.judgment/1` exactly as before. Later #76 specs add members to
   version 2 rather than introducing a third.
4. **Evidence** (adds to `rustev.evidence/2`, introduced by spec 032): per
   check its record (3.4.8), per input its dropped or relaxed status, and
   the judgment as `rustev.judgment/2`. The run record carries the evidence
   document as spec 032 and the #76 track's run version (`rustev.run/3`,
   spec 036) state. Capture and replay of plan 3 decisions follow the rules
   in force for plan 3: spec 032 refuses capture until spec 036 (#76 B1)
   admits plan 3 decisions into `rustev.replay/2`; from then on, a decision
   using this spec's constructs is captured and replayed like any plan 3
   decision, and replay compares its `rustev.judgment/2` bytes. This spec
   adds no capture or replay rule of its own.

### 3.2 Checks

1. `caller_controlled` is a sorted, duplicate-free list of provenance
   classes (spec 002, 3.7.1) the host declares the caller controls: values
   of these classes come from the party that sent the request and can be
   changed by it. Only `user-supplied`, `authenticated-app-field` and
   `attributed-claim` may be listed. `third-party`, `system-of-record` and
   `model-derived` are refused, category `caller_cannot_act` (3.8), detail
   `not-caller-controllable`: the caller cannot change a third party's
   data, a system of record or a model's inference.
2. A **check** is `{ id, subject, test, on_fail }`:
   - `id` is unique among checks, steps and policy items (spec 002, 3.9.1
     naming rules);
   - `subject` names one declared input: the value whose provenance the
     check branches on;
   - `test` is a predicate of the exact registry the plan uses (spec 002,
     3.8; `rustev.exact/2` under spec 032) that reads only inputs (`input:`
     references, including record fields), `now`, and literals, and has
     type `bool` (category 3 `kind_mismatch` otherwise). It reads no step:
     checks run before any step;
   - `on_fail` is `{ by_provenance, default }`, where `by_provenance` maps
     provenance classes to consequences (3.3) and `default` is a
     consequence or `none`.
3. **Branches.** Every key of `by_provenance` must be a class the subject
   accepts (otherwise category 11, detail `unreachable-branch`).
   **Exhaustiveness:** every class the subject accepts must be a key of
   `by_provenance` or covered by a `default` that is not `none`; otherwise
   the definition is refused, category `uncovered_provenance` (3.8), naming
   the check and every uncovered class in class order. A subject accepting
   one class is exhaustive with one branch or a default.
4. Example (the future-window check of #61, in the definition's JSON
   shape): the input `trip.window` is a record with `start`, accepting
   `user-supplied` and `model-derived`, and `caller_controlled` lists
   `user-supplied`.

   ```text
   { "id": "future_window", "subject": "trip.window",
     "test": { "compare": { "op": "gt",
               "left": { "ref": "input:trip.window/start" },
               "right": { "ref": "now" } } },
     "on_fail": {
       "by_provenance": {
         "user-supplied": { "refuse": { "remediation": "send_future_window" } },
         "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
       "default": "none" } }
   ```

   `send_future_window` is declared in `remediations` (3.6.3) `for:
   caller`.

### 3.3 Consequences

A consequence is one of the following. `refuse`, `abstain` and `escalate`
are **terminal**; the others are **field-level**.

| Consequence | Effect |
|---|---|
| `refuse { remediation, address }` | The judgment is `refused` (3.5.1), listing this check. |
| `drop_field { notice }` | The subject becomes absent: every reader sees `missing_evidence{fields: [subject]}`, exactly as if no accepted entry had been supplied (spec 002, 3.7.3); a `field_dropped` notice is emitted. |
| `relax { group, notice }` | Every check of the named constraint group is not applied in this evaluation (status `relaxed`); a `constraints_relaxed` notice is emitted. |
| `downgrade_grade { to, notice }` | The subject's effective grade (spec 032, 3.2) becomes the lower of its grade and `to`; a `grade_downgraded` notice is emitted. Never raises a grade. |
| `abstain` | The judgment is `unresolved: abstained{rule: <check id>}`. |
| `escalate { reason }` | The judgment is `escalate{reason}`. |
| `continue_with_notice { notice }` | The value is kept unchanged; a `check_notice` notice is emitted. |

1. `notice` and `reason` are codes matching `[a-z][a-z0-9_]{0,63}`; a
   notice's `reason` is that code. `remediation` names a declared or
   built-in remediation (3.6) with its parameters. `refuse` and every
   notice-emitting consequence also take `address`: `computed` (when
   omitted) or an override `{ to, reason }` (3.5.3).
2. `drop_field` drops the subject only. A field-level consequence that
   would apply to a value inside a record is expressed by checking that
   record's input as the subject.
3. A constraint group is `{ name, checks }`: a unique name and a non-empty
   list of check ids. A group may name only checks declared after every
   check whose consequence relaxes it (category 11, detail
   `relax-order`), so evaluation order alone decides what is relaxed.
   Relaxing a group never relaxes a step, operator or policy rule.
4. `downgrade_grade` requires a graded subject (spec 032, 3.3); otherwise
   category 11, detail `ungraded-subject`. `to` is a grade below
   `authoritative`.

### 3.4 Check evaluation

1. Checks run in declaration order after input validation (spec 002,
   3.7.3, and spec 032's admission and presence rules) and before any step.
2. A check whose subject is unresolved or absent is `not_evaluated` with
   that reason; the subject's status is unchanged. A check of a relaxed
   group is `relaxed{by: <check id>}`.
3. A check whose test reads another unresolved or absent value, or whose
   test faults (overflow, invalid comparison), is `unevaluable` with that
   reason, and its subject becomes unresolved with the reason (merging
   fields as spec 002 3.10.4), so no failure is mistaken for a pass.
4. A test that is `true` passes. A test that is `false` fails, and the
   consequence applied is the branch of the subject's accepted entry's
   provenance class, else `default`. Exhaustiveness (3.2.3) guarantees one
   exists.
5. Field-level consequences apply immediately, so a later check reading a
   dropped subject is `not_evaluated` or `unevaluable` with
   `missing_evidence`. Terminal consequences are collected.
6. **Terminal outcome.** If any failed check has a terminal consequence,
   the outcome is decided by the first such check in declaration order:
   `refused` lists every failed check whose consequence is `refuse`, in
   declaration order; `abstain` and `escalate` are that check's alone. No
   step is evaluated, `pending()` is empty and no semantic request is
   issued, and the policy is not run. The judgment's derivation and
   lineage are those of the deciding checks' reads (spec 002, 3.12.2).
   Otherwise evaluation continues as spec 002 states, with field-level
   consequences in force.
7. **Policy over provenance.** Policy conditions gain `provenance_within {
   ref, classes }`: for an input reference, true when its accepted entry's
   class is in `classes`; for a step reference under spec 032, true when
   every class of the value's `provenance` set is in `classes` (an empty
   set is within). It reads its reference like any atomic condition, so
   spec 002 3.11.2's handler coverage applies. Policy rules stay ordered
   with a final `always`, so they are total by construction.
8. **Evidence.** Each check records `{ check, subject, status }` with
   status `passed`, `failed{class, consequence}`, `not_evaluated{reason}`,
   `unevaluable{reason}` or `relaxed{by}`; a dropped input records
   `dropped{check}` and keeps its entry digest, source and provenance in
   evidence; values appear only as digests (spec 032, 3.1.4).

### 3.5 Actionable outcomes

1. **Outcome** in `rustev.judgment/2`, externally tagged:
   - `propose{action, params}` and `escalate{reason}` as in version 1;
   - `unresolved{reason, action}`: the spec 002 reason with its action;
   - `refused{refusals, addressed_to}`: each refusal is `{check, subject,
     action}`; `addressed_to` is the combination (3.5.2) of the refusals'
     effective parties;
   - `needs_input{fields, questions, continuation, action}` (3.7).
2. **Action** is `{ addressed_to, computed, override, remediation,
   caused_by, transient }`, every member always written:
   - `caused_by` is the list of causes, duplicate-free and sorted by the
     bytes of each cause's canonical JSON encoding (spec 002), so its
     order never depends on evaluation order. An input cause is `{ field,
     provenance }` where `provenance` is `observed{class}` when an
     accepted entry existed (including a dropped or invalid one) and
     `accepted{classes}` (the declared classes) when none did. Other
     causes are `backend{step, origin}` (origin `upstream` or `host`,
     3.9), `budget{resource}`, `deadline`, `capability{name}` and
     `policy{site}`. A **site** names where a declaration produced an
     outcome: `check:<id>`, `rule:<id>` or `handler:<n>` (`n` the
     zero-based position in `on_unresolved`).
   - `computed` is the party computed from `caused_by`. Each cause has a
     party: an input cause whose class (every class, for `accepted`) is in
     `caller_controlled` is `caller`; an input cause of class
     `third-party` or `system-of-record` is `upstream`; any other input
     cause (`model-derived`, or a class the host did not declare
     caller-controlled) is `host`; `backend` is its origin; `budget`,
     `deadline` and `capability` are `host`; `policy` is `none`. An
     `accepted` cause listing classes of several parties takes the
     combination of their parties. The **combination** of a set of
     parties is `upstream` if any is `upstream`, else `host` if any is
     `host`, else `caller` if any is `caller`, else `none`. So `computed`
     is `caller` only when every cause is a caller-controlled input.
   - `addressed_to` is the effective party: the override's party when an
     override applies (3.5.3), else `computed`.
   - `override` is `none` or `{ to, basis, reason }`: basis `declared`
     with the author's `reason` from `address` (3.5.3), or basis
     `remediation` with `reason` the remediation's code.
   - `remediation` and `transient` are 3.6.
3. **Overrides.** An author may move an outcome only toward a party less
   able to act than the caller: `address: { to, reason }` on a `refuse`
   consequence, a notice-emitting consequence, or a policy handler (3.5.5),
   with `to` one of `host`, `upstream`, `none` and `reason` a code
   matching `[a-z][a-z0-9_]{0,63}` (category 11 otherwise), copied into
   the action's `override`. An override naming
   `caller` is refused, category `caller_cannot_act`, detail
   `override-to-caller`. A remediation declared `for` a party other than
   `computed` applies with basis `remediation`; it is never `for: caller`
   unless the site's static party is `caller` (3.8.3). An explicit
   override wins over a remediation's party. The computed party is always
   recorded, so an override is visible.
4. **Notices** in `rustev.judgment/2` are `{ kind, subject, reason,
   action }`. `NoticeKind` adds `field_dropped`, `constraints_relaxed`,
   `grade_downgraded` and `check_notice`. A check notice's causes are the
   check's input reads with observed provenance; the existing kinds have
   causes `capability{step}` (`uncalibrated_threshold`, `fallback`) or
   `budget{semantic_requests}` (`truncation`). The future-window drop of a
   model-derived window is therefore addressed to `host`.
5. **Handlers.** Policy handler actions (spec 002, 3.11.1) gain
   `propagate_with { remediation, address }`, which is `propagate` with a
   declared remediation or override, and `needs_input` (3.7). `propagate`
   keeps its meaning and yields the computed action.
6. **Causes of each reason.** `missing_evidence`, `stale_evidence`,
   `invalid_input`, `conflict` and spec 032's `insufficient_grade`: one
   input cause per named field or fact (a record path names its input);
   `conflict` lists each conflicting entry's observed class.
   `abstained{rule}`: `policy{site}`, with site `check:<id>` when a
   check's `abstain` consequence produced it and `rule:<rule>` otherwise. `backend_unavailable` and
   `invalid_backend_output`: `backend{step, origin}` of the failed request
   (the first in step and instance order when several). `budget_exhausted`:
   `budget{resource}`. `deadline_exceeded`: `deadline`. `unsupported`:
   `capability{name}`. A `refused` refusal's causes are its check's input
   reads, excluding `now` and literals. `missing_evidence_from{step}`
   names the step's listed fields.

### 3.6 Remediations and transience

1. A **remediation** is `none` or `{ code, params }`: a stable code
   (`[a-z][a-z0-9_]{0,63}`) and a map of named exact values (`OutValue`,
   spec 002), canonical. It says what the addressed party can do; the
   party is `addressed_to`, never the code.
2. **Computed remediations**, used when no remediation is declared:

   | Reason or notice | Code and parameters |
   |---|---|
   | `missing_evidence` | `provide_fields{fields}` |
   | `stale_evidence` | `refresh_fields{fields}` |
   | `invalid_input` | `correct_fields{fields}` |
   | `conflict` | `resolve_conflict{facts}` |
   | `insufficient_grade` (spec 032) | `improve_evidence{fields}` |
   | `backend_unavailable`, `deadline_exceeded` | `retry` |
   | `invalid_backend_output` | `retry` when transient, else `report_backend{step}` |
   | `budget_exhausted`, `truncation` notice | `raise_budget{resource}` |
   | `unsupported`, `uncalibrated_threshold` and `fallback` notices | `provide_capability{name}` |
   | `abstained`, check notices | `none` |

   These codes, and `retry_after{ms}` (an integer of milliseconds, for
   authored use), are built in and cannot be redeclared.
3. **Declared remediations** are `{ code, for, params }`: `for` is
   `caller`, `host` or `upstream`; `params` lists `{ name, type }` with a
   scalar type of spec 002 3.7.2. A use names the code and supplies every
   parameter as a literal or an `input:` reference of that type (category 3
   otherwise; an undeclared code or a missing or extra parameter is
   category 11). A reference parameter that is unresolved at evaluation
   makes the remediation `none` with the action unchanged otherwise, so a
   remediation never carries a default.
4. **`transient`** says whether retrying the same request unchanged can
   succeed. It is `true` for `backend_unavailable` and `deadline_exceeded`,
   and for `invalid_backend_output` when the failed request's bound
   backend descriptor declares determinism other than `bitwise`; it is
   `false` for every other reason, for `refused`, for `needs_input` and
   for every notice. For `refused`, the outcome-level transience is
   `false`.

### 3.7 The needs-input outcome

1. `needs_input { fields, questions }` is a policy rule outcome and a
   handler action. It states that the decision could proceed if the caller
   supplied the named fields; it is distinct from `missing_evidence`,
   which states only that evidence is missing.
2. Every field must be a declared input accepting at least one
   caller-controlled class; otherwise category `caller_cannot_act`, detail
   `field-not-caller-supplied`, naming the field. Its action is addressed
   to `caller` by construction, with `caused_by` listing each field with
   `accepted{classes}` restricted to its caller-controlled classes, and
   remediation `provide_fields{fields}`.
3. `questions` names declared question templates. A template is `{ id,
   fields, text }`: `fields` a non-empty subset of the outcome's fields,
   `text` a literal UTF-8 string of 1 to 1024 bytes. The outcome carries
   each named template as `{ id, fields, text }`, with `text`
   byte-identical to its declaration: no interpolation, no model output,
   no input value. A host renders or localizes by question id. A later
   spec (#76 B6) may let an extraction step propose which declared
   questions to ask, through declared acceptance rules; the set stays
   declared.
4. `continuation` is `tagged_digest("rustev.continuation/1", canonical
   bytes of { plan_id, snapshot_id, evaluation_time_ms, site, fields,
   questions })`, where `site` (3.5.2) is the rule or handler that
   produced it.
   Rustev stores nothing. A host may echo it as the `source` of the
   snapshot entries answering it (spec 002, 3.7.3), which evidence
   records; Rustev neither verifies nor deduplicates it. At-most-once
   policies across turns are the host's responsibility.

### 3.8 Compilation: the actionability phase

1. Structural rules of 3.1 to 3.7 (unique ids, known subjects and groups,
   unreachable branches, relax order, codes, parameters, templates) are
   phase (b) category 11; type errors are phase (e) category 3.
2. A new phase (h2), after (h) and before (i), checks exhaustiveness and
   caller addressing. Two categories are added, 16 and 17, after spec 032's 13 to
   15:

   | Category | Phase | Refused when |
   |---|---|---|
   | 16 `uncovered_provenance` | h2 | a check leaves an accepted class of its subject without a branch or default (3.2.3) |
   | 17 `caller_cannot_act` | b, h2 | a non-caller-controllable class in `caller_controlled` (b); an override to `caller` (b); a `needs_input` field accepting no caller-controlled class (h2); a remediation `for: caller` at a site whose static party is not `caller` (h2) |

3. **Static party.** For each check branch, the static causes are the
   subject with the branch's classes (all classes `default` covers) and
   every other input the test reads with its accepted classes; for each
   handler, the static causes of each reason it covers (3.5.6, with
   accepted classes). The static party is their combination (3.5.2), and
   is recorded in plan 3 `actionability`. Because runtime causes are
   observed classes within the accepted ones, a site whose static party is
   `caller` can only produce `caller` at runtime, which is what makes the
   check of 3.8.2 sound.
4. Earliest phase wins, then declaration order, as spec 002 3.9.4. A
   definition reaching phase (h2) without `actionability` passes it
   unchanged.

### 3.9 Runtime origin (adds to spec 003)

1. A backend is registered with an origin, `upstream` (the default) or
   `host`. A host registers a backend it implements itself, including a
   host-language backend through spec 027's binding, with `host`.
2. The runtime passes the registered origin with every failure it supplies
   for that backend's requests (`backend_unavailable`,
   `invalid_backend_output`); the core records it as the `backend` cause.
   Failures the runtime produces itself (`budget_exhausted`,
   `deadline_exceeded`) carry their own causes (3.5.6). For plans without
   `actionability` the origin is ignored, and run records keep their bytes.
3. A host-language implementation may report a failure it relays from its
   own upstream service as `upstream`; it can never report `caller`.

### 3.10 Host status guidance

Documentation, not enforcement. The contract's `Outcome` and `Action`
types carry it in their rustdoc, and spec 027's and 031's API
documentation reproduce it. A host maps outcomes to its transport this
way:

| Outcome | Transport meaning |
|---|---|
| `propose`, with or without notices (including fallback notices and #76 B5 generation backstops) | Success with notices; never an error. |
| `refused` or `unresolved` addressed to `caller` | Client error: the request must change (for example HTTP 400 or 422). |
| `needs_input` | The caller is asked for the named fields: a client-addressed response, not a service failure. |
| addressed to `upstream` | Dependency failure (for example HTTP 502 or 503 when `transient`, 424 otherwise). |
| addressed to `host` | Host-side failure: configuration, budget, deadline or a host-implemented seam (for example HTTP 500, or 503 and 504 when `transient`). |
| addressed to `none` (an abstention by declared policy) | Success without a proposal. |
| `escalate` | Success routed to the host's escalation path. |

### 3.11 CLI surface (amends spec 006)

1. `plan check` and `plan compile` accept a `rustev.definition/2` with
   `actionability` (and refuse spec 032 constructs whose inputs the CLI
   cannot supply as spec 032 states); refusals print categories 16 and 17
   like every category, so spec 006 3.2's "categories 1 to 12" becomes
   every category the core defines.
2. `plan show` of a plan with `actionability` adds `caller_controlled`,
   and per check its `id`, `subject`, each branch's classes, consequence
   kind and static party, then `constraint_groups`, `remediations` and
   question ids. Other plans print exactly as before.
3. `run` embeds the judgment as its record canonical bytes as before, so a
   `rustev.judgment/2` appears whole. For such a judgment the output adds
   top-level `addressed_to` and `transient` keys: the outcome's action
   (or, for `refused`, its combined party and `false`), and `null` for
   `propose` and `escalate`. The status stays `judged` and the exit code 0
   for every outcome, including `refused` and `needs_input` (spec 006,
   3.2). Output for `rustev.judgment/1` keeps its bytes.
4. Refusal goldens of the CLI gain fixtures for categories 16 and 17 and
   for `run` outputs with each new outcome; existing goldens are
   unchanged.

### 3.12 Compatibility

1. `rustev.definition/1`, `rustev.plan/2`, `rustev.judgment/1`,
   `rustev.evidence/1` and the run record's existing versions, every
   refusal category of specs 002, 003 and 032 with its phase and order,
   and every operator and condition's meaning are unchanged.
2. A `rustev.definition/2` without `actionability` compiles to the same
   `rustev.plan/3` bytes and `PlanId` as under spec 032, and its decisions
   produce `rustev.judgment/1` and evidence bytes as under spec 032.
3. Every committed definition, plan, judgment, evidence, run and CLI
   golden, including both reference plans, keeps its bytes and `PlanId`; a
   test recompiles each through the amended compiler and compares bytes.
4. `PlanId` of a plan with `actionability` changes with any declaration of
   this spec, including `caller_controlled`; adding an unused remediation
   or question also changes it, since both are declarations.
5. The outcome variants, notice kinds, handler actions, condition and
   categories extend closed sets in new versions or with new names; no
   existing name moves.

## 4. Out of scope

Relaxing steps, operators or filter rules (constraint groups hold checks;
#76 B1's tiered widening covers widening searches); checks reading steps;
interpolated or localized question text; storing continuations or
enforcing at-most-once questioning; multi-turn decision chains (#76
future work, building on spec 019); model-proposed question selection
(#76 B6); a remediation catalogue beyond 3.6; transport status
enforcement; capture and replay rules for plan 3 decisions (specs 032 and
036); CLI flags for spec 032's source descriptors; quality claims from
synthetic fixtures (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Future-window check, `user-supplied` window in the past, `user-supplied` caller-controlled | `refused`; refusal addressed `caller`, computed `caller`, remediation `send_future_window`, cause `trip.window` observed `user-supplied`, `transient` false; no semantic request issued. |
| Same check, `model-derived` window in the past | `trip.window` dropped; `field_dropped` notice `inferred_window_dropped` addressed `host`; the decision continues and readers see `missing_evidence{trip.window}`. |
| Same check with no `model-derived` branch and no default | Category 16, naming `model-derived`. |
| A branch for a class the subject does not accept | Category 11, `unreachable-branch`. |
| `caller_controlled` listing `third-party` | Category 17, `not-caller-controllable`. |
| An override `to: caller` on an outcome citing a `third-party` input | Category 17, `override-to-caller`. |
| A remediation `for: caller` on a branch whose test also reads a `system-of-record` input | Category 17, naming the input and class. |
| `refuse` with `user-supplied` not declared caller-controlled, remediation `for: caller` | Category 17. |
| `needs_input` naming an input accepting only `model-derived` | Category 17, `field-not-caller-supplied`. |
| A test reading an input that is stale | Check `unevaluable`; the subject is `stale_evidence` naming that input. |
| A check whose subject is missing | `not_evaluated`; the judgment follows the policy's handling of `missing_evidence`. |
| Two failed checks, first `continue_with_notice`, second `refuse`; a third `refuse` fails too | `refused` listing the second and third; the notice of the first is kept. |
| `relax` naming a group containing an earlier check | Category 11, `relax-order`. |
| `downgrade_grade` on an ungraded subject | Category 11, `ungraded-subject`. |
| `downgrade_grade{to: estimated}` on a `hint` value | Grade stays `hint`. |
| A declared remediation parameter reading an unresolved input | Remediation `none`; the action otherwise unchanged. |
| A `third-party` missing field and a `user-supplied` missing field in one `missing_evidence` | Computed `upstream`. |
| `backend_unavailable` from a backend registered `host` | Addressed `host`, `transient` true, `retry`. |
| `invalid_backend_output` from a `bitwise` backend | `transient` false, `report_backend{step}`. |
| `abstain` consequence of check `c` | `unresolved: abstained{rule: c}`, cause `policy{site: check:c}`, addressed `none`. |
| An override `{ to: host, reason: "Bad Reason" }` (not a valid reason code) | Category 11. |
| An override `{ to: host, reason: "partner_outage" }` (a valid reason code) | Recorded as `override{to: host, basis: declared, reason}` beside `computed`. |
| A `needs_input` judgment | Questions byte-identical to their templates; `continuation` reproducible from its stated inputs. |
| A `rustev.definition/1` with a check | Category 11, `requires-definition-2`. |
| Every existing golden recompiled and rerun | Byte-identical, same `PlanId`, `rustev.judgment/1`. |

## Acceptance

- The future-window example: refused for a caller-supplied past window,
  dropped and continued for a model-derived one, both with the
  `addressed_to`, computed party, remediation, causes and transience of
  section 5, as judgment 2 and evidence 2 goldens over synthetic fixtures.
- A check missing a provenance branch is refused at compile time with the
  class named (category 16), with a golden refusal fixture and a passing
  neighbour differing only in the missing branch.
- A caller-addressed override on an outcome citing a third-party input is
  refused at compile time (category 17); so is every other category 17
  case of 3.8.2, each with a refusal fixture and passing neighbour.
- Computed `addressed_to` is golden-tested for each cause class: each
  provenance class declared and not declared caller-controlled, observed
  and accepted causes, `backend` with each origin, `budget`, `deadline`,
  `capability` and `policy`, and the combination of mixed causes; and
  `transient` for every reason and determinism value.
- Each consequence of 3.3, check statuses of 3.4.2 and 3.4.3, terminal
  ordering of 3.4.6, and `provenance_within` on inputs and on spec 032
  step values are tested.
- `needs_input`: questions byte-identical to templates, the continuation
  digest recomputed independently in the test from 3.7.4's definition.
- CLI: `plan show`, `run` and refusal goldens for 3.11; the Node binding
  returns the new fields unchanged, tested in spec 027's conformance suite
  once spec 027 is implemented (until then reported as externally
  blocked, never as passed).
- Compatibility: every existing golden is byte-identical (3.12.3).
- Seeds in the core mutation harness (party combination preferring
  `caller`, an override to `caller` accepted, `accepted` causes ignoring a
  non-caller class, a dropped value replaced by its entry, an unevaluable
  check treated as passed, a terminal check not stopping semantic
  requests, `transient` true for `invalid_input`) are each detected.
- Every row of section 5 has a test. `make code`, declared verification,
  and `make gate` pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it. Plan:

1. Contract: the wire types of 3.1 to 3.7 and their version gate.
2. Compilation: phases (b), (e) and (h2), categories 16 and 17, and the
   plan 3 static table (3.8).
3. Evaluation: checks, consequences and statuses, terminal outcomes and
   `provenance_within` (3.3, 3.4), evidence 2 records, and the actions of
   3.5 to 3.7, which every judgment 2 outcome and notice carries, so they
   land together.
4. The runtime origin (3.9) and the CLI output and goldens (3.11).
5. Compatibility goldens (3.12), mutation seeds, every section 5 row,
   verification.

**Increment 1 (contract).** `rustev-contract` carries every member of
3.1: `actionability` in definition 2 (`caller_controlled`, remediations,
questions, constraint groups, checks with `on_fail` branches by provenance
and a `default` that is `none` or a consequence), the consequences of 3.3
with their optional `address` (omitted when computed), the condition
`provenance_within`, the handler actions `propagate_with` and
`needs_input`, the rule outcome `needs_input`, plan 3's static table
(`PlanActionability`), evidence 2's check records and dropped inputs, and
`rustev.judgment/2`: `Outcome` gains `UnresolvedAction`, `Refused` and
`NeedsInput`, and both unresolved forms share the wire key `unresolved`
(version 1 `{"unresolved": <reason>}`, version 2 `{"unresolved":
{"reason", "action"}}`), which never overlap because no reason is named
`reason` or `action`. `Notice` gains an optional `action`, omitted in
version 1, and `NoticeKind` the four kinds of 3.5.4. `Action` writes every
member; causes, parties (ordered so their combination is the greatest),
overrides and remediations (a subset of `OutValue` in the same wire form)
are typed. Every new member is omitted when unused, so every existing
document keeps its bytes. `rustev-core` refuses every construct of this
spec at phase (b): in a version 1 definition, category 11 with detail
`requires-definition-2` (3.1.1); in a version 2 definition, until the
compilation phase of increment 2 lands, so nothing is compiled from one.
The evaluation crate maps the new outcomes to abstentions by name. The two
new source files join spec 021's reviewed archive list.
`crates/rustev-contract/tests/actionability_wire.rs` round-trips each
form, including the future-window example of 3.2.4 and a judgment 2
document; `crates/rustev-core/tests/actionability_versions.rs` covers the
version 1 refusal of each construct (section 5, last but one row) and the
version 2 interim refusal.

**Increment 2 (compilation).** `rustev-core`'s `actionable` module adds the
actionability phase of 3.8 to compilation. Phase (b), after the policy's
existing structure: `caller_controlled` holds only caller-controllable
classes (category 17 `not-caller-controllable` otherwise) sorted and
duplicate-free; declared remediations have unique codes that are not
built in, a party other than `none` and scalar parameters; questions have
unique ids, declared fields and 1 to 1024 bytes of text; constraint
groups name declared checks; each check has an id unique among checks,
steps and policy items, a declared subject, branches only for classes the
subject accepts (`unreachable-branch`), and consequences whose notice and
reason codes, remediations (declared or built in, with exactly their
parameters), groups (`relax-order`) and downgrades (`ungraded-subject`,
below `authoritative`) are valid; an override names `host`, `upstream` or
`none` with a code as reason (category 17 `override-to-caller` for
`caller`); `propagate_with`, `needs_input` (declared fields and questions
whose fields it names) and `provenance_within` (an input or step, sorted
non-empty classes) are checked where the policy declares them. Phase (e):
a check's test is a `bool` predicate reading inputs, `now` and literals
only, and remediation parameters have their declared types (category 3).
Phase (h): a check's subject can also be unresolved with every reason of
the inputs its test reads, `invalid_input` for a test that faults, and
`insufficient_grade` after a downgrade, so spec 002's handler coverage
sees them. `needs_input` names fields for the caller to supply and reads
none, so it adds no coverage requirement. Phase (h2): exhaustiveness
(category 16 naming the uncovered classes in class order), a `needs_input`
field accepting no caller-controlled class (category 17
`field-not-caller-supplied`), and a remediation `for: caller` at a site
whose static party is not `caller` (category 17, naming each input and
class the caller cannot act on). Plan 3's `actionability` records, per
check branch, its classes, consequence kind, static causes and static
party, and per handler and `needs_input` rule its site, static causes and
party; a backend's registration origin is unknown before the runtime, so
its static cause is the stronger `upstream`. The CLI names
categories 16 and 17 `uncovered_provenance` and `caller_cannot_act`
(3.11.1). A `rustev.definition/2`
whose only #76 construct is `actionability` is a track definition. Until
increment 3 evaluates checks, `Evaluation::start` refuses a plan with
`actionability` rather than run a decision that would ignore them.
`crates/rustev-core/tests/actionability_compile.rs` covers the
future-window example and its static parties, and each section 5
compile-time row with its passing neighbour: categories 16 and 17 (every
case of 3.8.2), `unreachable-branch`, `relax-order`, `ungraded-subject`,
an invalid and a valid override reason, a step read in a test, an unknown
remediation or parameter, and a `PlanId` that changes with an unused
question.

**Increment 3 (evaluation and actionable outcomes).** A plan with
`actionability` evaluates its checks after input validation and before any
step (3.4.1), in declaration order: a check of a relaxed group is
`relaxed{by}`; an unresolved or absent subject leaves the check
`not_evaluated` with that reason; a test whose read is unresolved, or which
faults, is `unevaluable` and makes the subject unresolved with the reason
(merged as spec 002 3.10.4), so no failure passes; a `true` test passes;
a `false` one applies the branch of the subject's accepted entry's class,
else the default. `drop_field` makes the subject absent for every reader
(`missing_evidence`) while its entry stays in evidence with
`dropped{check}`; `relax` relaxes the named group's later checks;
`downgrade_grade` lowers every grade of the subject's metadata, never
raising one; `continue_with_notice` keeps the value. Each emits its notice.
The first failing terminal check decides (3.4.6): `refused` lists every
failing refusing check in declaration order, `abstain` and `escalate` are
that check's; no step is evaluated, no semantic request is issued, the
policy does not run, and derivation and lineage are the deciding checks'
reads. `provenance_within` reads its reference like any atomic condition:
an input's accepted entry's class, or every class of a plan 3 step value's
provenance set (3.4.7). The policy reports the site that decided
(`rule:<id>` or `handler:<n>`); `propagate_with` carries its declared
remediation and address, and `needs_input` (rule outcome or handler) its
fields and question ids. Such a plan produces `rustev.judgment/2`: each
unresolved outcome becomes `{reason, action}`; a refusal carries one action
per refusing check and `addressed_to` is their combination; `needs_input`
carries each named template byte-identical, its continuation
`tagged_digest("rustev.continuation/1", ...)` over the plan, snapshot,
evaluation time, site, fields and question ids, and an action addressed to
the caller with `provide_fields`; every notice, the existing kinds
included, carries an action. Causes follow 3.5.6 (input causes observed
when an accepted entry existed, accepted classes otherwise, one per class
for a conflict; `policy{site}` for an abstention; the first failed step,
in step and instance order, for a backend reason, with origin `upstream`
until the runtime supplies one in increment 4); `computed` is their
combination; an explicit override, or a declared remediation `for` another
party, is recorded beside it; remediations follow 3.6.2's table or the
declared one, with parameters evaluated and `none` when one reads an
unresolved input; `transient` follows 3.6.4, with the bound backend's
declared determinism. Evidence 2 records every check and each dropped
input. `crates/rustev-core/tests/actionability_eval.rs` covers the
future-window example both ways (section 5 rows 1 and 2), a passing check,
a missing subject, a stale read making the check unevaluable and its
subject stale, terminal precedence with a kept notice, an abstention
addressed to no one, a declared override beside the computed party,
`needs_input`'s literal questions and an independently recomputed
continuation, `provenance_within` both ways, and a remediation parameter
reading an unresolved input.

**Increment 4 (runtime origin and CLI).** `RuntimeBuilder::host_backend`
registers a backend the host implements itself, origin `host`;
`RuntimeBuilder::backend` keeps registering with origin `upstream`, the
default (3.9.1). Before supplying a failure the runtime notes the
backend of the request's last attempt to the core
(`Evaluation::note_failure` with `FailureContext`: its registered origin
and its descriptor's declared determinism), so the `backend` cause carries
that origin and `invalid_backend_output`'s transience follows that
backend's determinism (3.6.4, 3.9.2); a plan without `actionability` reads
neither, so run records keep their bytes. `rustev_core::transient` exposes
the table of 3.6.4. The CLI names categories 16 and 17 (3.11.1); `plan
show` of a plan with `actionability` adds `caller_controlled`, per check
its id, subject and each branch's classes, consequence kind and static
party, then `constraint_groups`, `remediations` and question ids, and
prints every other plan as before (3.11.2); `run` prints a
`rustev.judgment/2` whole and adds top-level `addressed_to` and
`transient` (a refusal's combined party and `false`; `null` for a
proposal or escalation), status `judged` and exit code 0 for every
outcome (3.11.3). So that a plan 3 runs, the CLI's record file sink
accepts `rustev.run/3` as well as `rustev.run/1`; capture of a plan 3
decision is refused before any decision runs (invalid input), since spec
032 3.11 defers it to spec 036. Tests:
`crates/rustev-core/tests/actionability_origin.rs` (section 5's backend
rows: a host backend's `backend_unavailable` addressed `host`, transient,
`retry`; `upstream` by default; `invalid_backend_output` from a `bitwise`
backend not transient with `report_backend{step}`, from others transient
with `retry`; the transience table for every reason and determinism),
`crates/rustev-runtime/tests/actionability_origin.rs` (the runtime
carries the registered origin), and `crates/rustev-cli/tests/actionability.rs`
with goldens `refusal-16.json`, `refusal-17.json`, `actionable.show.json`
and `actionable.run-{propose,refused,needs,unresolved}.json`; every
existing CLI golden is unchanged.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #61 leaves the points below open, and each is
decided here within the binding principles:

1. **Versioning.** Under the #76 track rule, this spec adds an optional
   `actionability` member to `rustev.definition/2` and `rustev.plan/3` and
   members to `rustev.evidence/2`, and introduces `rustev.judgment/2` as
   the track's judgment version. Actionable outcomes are opt-in per
   definition: computing `caller` needs the host's declaration of
   caller-controlled classes, and opting in keeps every existing judgment
   byte-identical (version 1 judgments are compared byte for byte by
   replay, spec 004).
2. **What a rule is.** Provenance-conditional validation rules are
   **checks** on one subject input, run before steps; policy rules branch
   on provenance through the `provenance_within` condition. A check
   branches on its subject's provenance only; other inputs it reads count
   as causes.
3. **Caller-controllable classes** are limited to `user-supplied`,
   `authenticated-app-field` and `attributed-claim`; the host declares
   which of them apply. `model-derived` input is addressed to `host`: the
   caller sent no such value.
4. **Party combination** is `upstream` over `host` over `caller` over
   `none`, so a mixed cause is never addressed to the caller. A missing
   field's cause uses its accepted classes: the caller is addressed only
   when only the caller could have supplied it.
5. **Overrides** may name `host`, `upstream` or `none`, never `caller`, and
   the computed party is always recorded beside the effective one.
6. **Terminal precedence.** The first failing terminal check decides;
   a refusal lists every failing refusing check, so a caller can fix all
   of them at once. A terminal check issues no semantic request.
7. **`relax`** relaxes named groups of later checks only; widening steps
   or filter rules is #76 B1's tiered widening.
8. **`escalate` carries no action.** It is a proposal to the host's
   escalation path, unchanged from version 1.
9. **Questions** are literal templates; hosts render and localize by id.
   No interpolation, so no input or model text can enter a question.
10. **Continuation** binds plan, snapshot, evaluation time, site, fields
    and questions; the host echoes it through snapshot entry `source`.
11. **Host-implemented seams** are identified by registration origin, the
    one place the runtime knows who implemented a backend.
12. **Refusal category numbers** are 16 `uncovered_provenance` and 17
    `caller_cannot_act`, the next free after spec 032.

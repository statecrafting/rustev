---
id: "037-generation-steps"
title: "Generation steps with deterministic backstops and per-field origin (amends 002, 003 and 004)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002, 003 and 004 (R-16), for #76 child B5
  (#63), under owner decisions R-41 (OD-2) and R-42 (OD-4). Defines the
  bounded model step envelope that later model steps reuse, and its first
  kind, `generate`: a declared, finite step that runs after selection and
  asks a backend for schema-bounded text fields per selected item, with a
  versioned prompt template bound into `PlanId`, a deterministic backstop
  for every required field written in a logic-less template language
  implemented in `rustev-core`, optional bounded repair, ownership checks,
  and per-field origin (`generated`, `repaired`, `backstop{reason}`) in the
  text result, the evidence, every capture and every cached copy. Generated
  text is model-derived and can never reach an exact operator, a policy
  control, an acquisition request or another model step. Generate steps
  add to the shared #76 versions `rustev.definition/2`, `rustev.plan/3`
  and `rustev.evidence/2`; plans without any #76 construct keep
  byte-identical identities and documents. Approved
  (A-34); implementation is in progress; approval authorizes no live provider
  call, spend, release or real-user data.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
extends:
  # Step, binding, output and descriptor variants, the evidence member; the prompt
  # template and text result documents (3.2 to 3.10, 3.17).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Compiler passes, the backstop language, validation, repair scheduling,
  # origin assembly and the ownership check (3.3 to 3.10).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # The generation stage, deadlines, streaming and final verification
  # (3.12 to 3.14).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Replay of generate steps and the text metrics (3.15, 3.16).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # Adds 037 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Exhaustive matches over the new step body and kinds, and regenerated
  # document types of the bindings; no behavior.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: directory, path: "integrations/rustev-otel/" }, nature: additive }
  # Clones and unsupported-kind arms where `OutputKind` stops being `Copy`.
  - { spec: "005-reference-backends", unit: { kind: directory, path: "backends/rustev-backend-rules/" }, nature: additive }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: additive }
  # Match arms for the generation stage in the service.
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  # The CLI refusal goldens for 22 to 24 join the shared conformance suite.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # New contract and core source files join their reviewed archive lists.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "017-general-runtime-optimization"
  - "032-evidence-quality"
  - "033-provenance-rules-and-actionable-outcomes"
  - "034-parameterized-definitions"
  - "035-screening-seam"
  - "036-mid-plan-acquisition"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A definition without a generate step is unaffected by this spec: a version 1 definition compiles to the same plan bytes and PlanId under the same compiler identity, and evaluates to the same judgment, evidence, run record, capture and report bytes, as before."
    anchor: "3-18-compatibility-and-identity"
  - id: "I-2"
    kind: invariant
    text: "No value produced by a generate step is readable by an exact step, a semantic or model step, a policy condition, outcome, parameter or adjustment, an execution policy, an acquisition request, a template parameter or a backstop; every such reference is refused at compile time."
    anchor: "3-6-isolation-from-decisions"
  - id: "I-3"
    kind: invariant
    text: "Every required generated field has a backstop that compiles, reads only exact-derived values, and renders within the field's bounds; its output is exact-derived."
    anchor: "3-4-backstop-language"
  - id: "I-4"
    kind: invariant
    text: "Per-field origin never changes the judgment: the judgment is final before the generation stage starts, a proposal whose text fell back is a successful proposal, and origin is never an Unresolved reason."
    anchor: "3-10-per-field-origin-and-the-text-result"
  - id: "I-5"
    kind: invariant
    text: "A cached copy of generated output is served with its stored origin and with `served_from_cache` naming the original decision; a backstop or repaired field is never served as freshly generated."
    anchor: "3-14-caching"
  - id: "R-1"
    kind: requirement
    text: "An output carrying a member that is not a declared generated field is rejected as an invalid output, and a carried context field or judgment that differs from its source at final verification is restored and reported with an `ownership_restored` notice."
    anchor: "3-9-ownership"
  - id: "R-2"
    kind: requirement
    text: "A streamed field is provisional; a streamed value that is not the field's final value is followed by exactly one `superseded` event, and the final result, evidence and cache entry equal those of the same run without streaming."
    anchor: "3-13-streaming"
  - id: "R-3"
    kind: requirement
    text: "A prompt template is an identified document embedded in the plan; any change to it changes `PlanId` and therefore every request identity and cache key derived from the plan."
    anchor: "3-3-prompt-templates"
---

# 037: Generation steps with deterministic backstops and per-field origin (amends 002, 003 and 004)

Approved by the owner (A-34, under the 2026-10-16 delivery work order) as
its own reviewable change before any code (R-16); implementation is
pending. This spec implements #76 child B5 (#63) and is the first spec that
depends on owner decisions OD-2 and OD-4 of #76, recorded as R-41 and R-42.
The approved texts of specs 002, 003 and 004 are not edited; this spec
states what changes. Screening follows spec 035 (#76 B7); this spec names the points where screening verdicts enter
(3.11) and does not define screening.

## 1. Purpose

Recommendation results usually pair a decision (which items, in what order)
with generated text about those items. When generation fails, good pipelines
fall back to deterministic text, and two things go wrong: the fallback is
invisible (the caller cannot tell generated from fallback text, and partial
fallback is not logged), and fallback is confused with failure (a page with
fallback text is reported as degraded or as an error). Caches then replay a
page with fallback text as if it were fresh.

Rustev already separates outcomes from notices and records runtime
fallbacks. This spec extends that to generated text: a declared, budgeted,
identified generation step with a deterministic backstop for every required
field and a per-field origin carried everywhere the text goes. It also
defines the envelope shared by every bounded model step, so structured
extraction (#76 B6) and model-backed ranking (#76 C2) reuse one set of
bounds, identity and isolation rules.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the `generate` step body,
  its plan binding, the `text_record` output, the descriptor capability,
  the `generation` evidence member, `rustev.prompt-template/1`, `rustev.generate-request/1`
  and `rustev.text-result/1`. Definitions, plans and evidence use the
  shared #76 versions (3.18); other existing documents gain only new enum
  variants.
- `crates/rustev-core/` (owner spec 002): compilation of generate steps,
  the backstop language (3.4), output validation, repair scheduling, origin
  assembly and the ownership check. No dependency is added: the template
  language is implemented in the crate.
- `crates/rustev-runtime/` (owner spec 003): the generation stage, its
  deadlines and concurrency, streaming and final verification.
- `crates/rustev-eval/` (owner spec 004): replay of generate steps and the
  text metrics.
- `Makefile` (owner spec 001): 037 joins `make verify` at completion.

The Node binding (#76 A1, spec 027) and the service (#76 A2) expose the
text result and the stream; exposing them is their specs' work. A backend
mapping of `generate` onto a provider wire is the backend's spec: for
Bedrock, spec 030 3.12 admits the reserved `generate` value under its own
change.

### 2.1 Binding principles of #76

- **Proposals, never grants.** Generated text is presentation attached to
  a proposal. It is never an authorization, never a `Permitted` value, and
  never an input to the decision (3.6).
- **Derivation classes and lineage.** A generated or repaired field is
  `model-derived` with lineage to its step, template, projection and
  attempt; a backstop field is `exact-derived` with lineage to the paths
  its template read (3.10).
- **Unresolved, never a default; no silent kind conversion.** A backstop
  is not a default for missing evidence: it is a declared, authored
  rendering of exact values, visible as `backstop{reason}` on every copy.
  Missing, stale or invalid evidence still makes decision values
  `Unresolved`; generated text is never converted into any decision value
  kind, and no text field is ever silently empty or silently replaced.
- **Pure core.** Validation, the template language, origin and ownership
  live in `rustev-core` with no I/O, clock, `unsafe` or async runtime; all
  dispatch and time are the runtime's (3.12).
- **New dependency classes only under `integrations/`.** None is added.
- **No dynamic plugin loading.** Generators are ordinary backends composed
  explicitly by the host (D-06, R-38).
- **No identity, authority or knowledge store.** Caches follow spec 017
  and the host's store; retention of generated text is the host's (3.15).
- **No open-ended agent loops.** A generate step is finite in items,
  fields, bytes, attempts, time and cost, fixed at compile time (3.1).

## 3. Behavior

### 3.1 The bounded model step envelope

A **model step** is a step whose value comes from a backend under a
declared output schema rather than from a closed option set. Every model
step kind shares this envelope; `generate` is the first kind, and later
kinds (`extract`, #76 B6) add only what their own spec states.

1. **Declared and finite.** A model step declares: at most one `for_each`
   list with a compile-time `max_items` (1 to 256); an output record of 1
   to 32 declared fields, each with byte, item and vocabulary bounds; a
   prompt template (3.3); a projection; `repair` with `max_attempts` 0 to
   2; and `limits` (3.12). It has no loop, recursion or step that a model
   output can create. A step without a list (`for_each: none`) has exactly
   one item, and `max_items` reads as 1 wherever this spec uses it.
2. **Budgeted at compile time.** Its worst-case requests are
   `max_items * (1 + repair.max_attempts + k)`, where `k` is the number
   of further requests per item its kind declares (for `generate`,
   output screening: the number of text fields when `screen.output` is
   `screener{backend}`, else 0; 3.11 item 5); they count against
   `max_semantic_requests` (spec 002, 3.9.5 category 7) and against the
   execution policy's worst-case attempts (spec 003, 3.2.3). Its
   worst-case projection counts against `max_projection_bytes` and the
   bound backend's input limit.
3. **Identified.** Its declaration, template documents, vocabularies and
   bound backend are in the plan document, so any change changes `PlanId`.
4. **Model-derived.** Every value a backend supplies to it is
   `model-derived`.
5. **Isolated except through acceptance rules.** A model step's output
   reaches a selection-policy control, an exact operator, an acquisition
   request or another model step only through acceptance rules its kind's
   spec declares (R-41). `generate` declares none: its output reaches
   nothing but the text result (3.6).
6. **Accounted.** Its requests use the runtime's attempts, retries,
   fallback, cost ledger, cancellation, evidence and capture exactly as a
   semantic step's do (spec 003), plus the stage rules of 3.12.

### 3.2 The generate step declaration

A `rustev.definition/2` (the shared #76 version, introduced by spec 032)
admits a new step body `generate` beside `exact` and `semantic`; this
spec adds that variant to it. A generate step in a `rustev.definition/1`
is refused, category 11 `invalid_definition`, detail
`requires-definition-2`. Illustrative form (the
contract types are normative):

```json
{ "id": "item_text", "body": { "generate": {
  "when": { "actions": ["recommend"] },
  "for_each": { "over": "proposal:selected", "bind": "item",
    "items": { "list": { "list": "input:candidates", "id_field": "id" } },
    "max_items": { "literal": 4 } },
  "template": { "id": "item-text", "digest": "sha256:<64 hex>",
    "params": { "style": "input:text_style" } },
  "language": "en-GB",
  "projection": ["bind:item/name", "bind:item/summary_facts", "input:preferences"],
  "carry": ["bind:item/name", "bind:item/price"],
  "vocabularies": { "tag_vocabulary": ["quiet", "central", "family"] },
  "output": { "fields": [
    { "name": "headline", "type": { "text": { "max_bytes": 120, "lines": "single" } }, "required": true },
    { "name": "why", "type": { "text": { "max_bytes": 400, "lines": "multi" } }, "required": true },
    { "name": "tags", "type": { "list": { "vocabulary": "tag_vocabulary", "max_items": 4 } }, "required": false } ] },
  "backstop": { "locale": "en-GB", "escape": "text", "overflow": "refuse", "fields": {
    "headline": { "text": "Option: {{item.name}}" },
    "why": { "text": "{{#item.matches_required}}Matches your required features.{{/item.matches_required}}{{^item.matches_required}}A well-reviewed option nearby.{{/item.matches_required}}" } } },
  "repair": { "max_attempts": 1, "only_missing_fields": true },
  "screen": { "input": "untrusted_fields", "output": "none" },
  "limits": { "deadline_share": "0.4", "per_item_deadline_ms": { "literal": 1500 },
    "concurrency": 4, "max_cost_units": "none" },
  "backend": { "min_determinism": "nondeterministic", "artifact": "any", "binding": "any" }
} } }
```

Members, all always present in canonical form:

| Member | Meaning and bounds |
|---|---|
| `when` | `any_propose`, or `actions{[...]}`: the step runs only when the judgment is `propose` with one of these actions. |
| `for_each` | `none` (one record, instance `[]`) or one list: `over` is `proposal:<param>` (a list parameter of every matching `propose` outcome) or `step:<id>` (an exact step yielding an id list, filter result or shortlist); `bind` and `items` as spec 002's `ForEach`; `max_items` is an integer literal in 1 to 256 or a profile parameter as spec B4 (#62) defines, with its declared maximum. Items are taken in list order. |
| `template` | Prompt template id, digest and parameter bindings (3.3); in the example, `style` is an `enum` parameter and `text_style` an `enum` input. |
| `language` | BCP 47 tag the text is requested in; a literal or a profile parameter. |
| `projection` | References the backend sees, as spec 002 3.9.1. |
| `carry` | Exact references copied into the text result as context fields (3.9); 0 to 16. |
| `vocabularies` | Named literal option lists (each 1 to 256 options of 1 to 64 bytes) or profile parameters. |
| `output.fields` | 1 to 32 fields: `name` (`[a-z][a-z0-9_]{0,63}`, unique), `type` `text{max_bytes 1..=16384, lines single or multi}` or `list{vocabulary, max_items 1..=64}`, and `required`. |
| `backstop` | `locale` (3.4.6), `escape` (`text` or `html`), `overflow` (`refuse` or `truncate`), and per field a template: `text{<template>}` for text fields, `list{[options]}` for list fields. Required fields must have one; optional fields may. |
| `repair` | `max_attempts` 0 to 2 and `only_missing_fields` (3.8). |
| `screen` | `input`, a closed value whose only member is `untrusted_fields` (any other value is out of bound, phase a `parse`), and `output` (`none` or `screener{backend}`), 3.11. |
| `limits` | `deadline_share` (decimal in (0, 1], at most four fractional digits), `per_item_deadline_ms` (1 to 600 000, literal or profile parameter), `concurrency` (1 to 64), `max_cost_units` (`none` or `units{n}`, n >= 1). |
| `backend` | As a semantic step's `BackendReq`. |

A definition holds at most 8 generate steps.

### 3.3 Prompt templates

1. A prompt template is an identity-bearing document
   `rustev.prompt-template/1` (parsed under the definition limits):
   `schema`, `id` (`[a-z0-9][a-z0-9._-]{0,127}`), `version`
   (`MAJOR.MINOR.PATCH`), `instructions` (a template in the language of
   3.4, at most 16 KiB) and `params` (0 to 32 of `{name, type}`, types
   `bool`, `integer`, `decimal`, `text{max_bytes <= 1024}`, `enum`).
   Its identity `PromptTemplateId` is spec 002 3.3.2's tagged digest.
2. Instructions read only `param.<name>`. Item data and caller text never
   enter the instructions; they travel in the projection, a separate
   channel the backend mapping keeps apart from instructions.
3. Template parameters bind to literals, exact references whose static
   class is `exact-derived` (`input:`, `step:` of an exact step,
   `proposal:`), or profile parameters (B4). A `text` parameter binds only
   to a literal or a profile parameter, which the definition's author
   controls; a reference binds only to a `bool`, `integer`, `decimal` or
   `enum` parameter, and to an `enum` parameter only when the referenced
   value is of an `enum` type whose options are all among the
   parameter's. So no caller-supplied text reaches the instructions
   (3.3.2). A reference to a generate step is refused as
   `model_output_flow` (3.6); a model-derived or mixed-derived value, and
   any other binding outside these rules, is a bad binding, refused as
   `template` (3.5 phase c).
4. The compiler receives template documents beside calibrations. It
   refuses a generate step whose template is not supplied, whose supplied
   document's identity differs from the declared digest, or whose
   parameter bindings are missing, extra or of another type. The plan's
   generate binding embeds the full template document, so
   `Compiled::load` recompiles from the plan's own embedded templates (as
   it does with the execution policy), and replay needs no separate
   template retention. At most 16 distinct templates per plan.
5. A template change changes the definition's declared digest, hence
   `DefinitionId` and `PlanId`, hence every request identity (spec 004,
   3.4) and every cache key bound to the plan (3.14).
6. The core renders the instructions when it lists a request; rendering
   uses `escape: text` and has no overflow (the worst case is bounded by
   the parameter types and checked against 16 KiB at compile time).

### 3.4 Backstop language

A logic-less subset of Mustache syntax, versioned `rustev.template/1`,
implemented in `rustev-core` with no external dependency, pure and
`wasm32`-compatible.

1. **Grammar.** `ws` is zero or more U+0020; `name` is
   `[a-z][a-z0-9_]{0,63}`, the same pattern as generated field names
   (3.2).

   ```
   template  = { text | tag }
   text      = one or more characters not starting "{{"
   tag       = interp | raw | section | inverted | helper | comment
   interp    = "{{" ws path ws "}}"
   raw       = "{{{" ws path ws "}}}"
   section   = "{{#" ws path ws "}}" template "{{/" ws path ws "}}"
   inverted  = "{{^" ws path ws "}}" template "{{/" ws path ws "}}"
   helper    = "{{" ws hname 1*( " " arg ) ws "}}"
   hname     = "number" | "money" | "date" | "list_join"
   arg       = path | name "=" ( path | literal )
   literal   = 1*[A-Za-z0-9_-]
   comment   = "{{!" any text not containing "}}" "}}"
   path      = name *( "." name )
   ```

   A closing path must equal its opening path. Partials (`{{>`),
   delimiter changes (`{{=`), `{{&`, lambdas, recursion and any other
   sigil are refused. Section nesting is at most 4 deep; a backstop
   template is at most 4 KiB.
2. **Roots.** A path's first segment is the step's `bind` name (the item
   record), `input`, `step` (exact steps only), `proposal` (parameters of
   the matching outcome), `param` (prompt templates only) or `this`
   (inside a list section, the current element; the innermost section
   wins). There is no context-stack lookup: an unqualified name is refused.
   A generate step's `bind` must match `name` and must not be one of the
   other roots; otherwise the step is refused (`invalid_generation`). An
   input, field, step or parameter whose name does not match `name` cannot
   be addressed by a template; a template that needs it is refused
   (`template`).
3. **Typing.** Templates are parsed and type-checked at compile time
   against the declared types. An unknown path, a path whose static
   derivation class is not `exact-derived`, and any path to a generate
   step are compile errors. Interpolation accepts text, enum, integer and
   decimal (canonical text); `raw` accepts only enum and bool (`true`,
   `false`). A section accepts bool (true renders), `maybe` (present
   renders, and inside it the path has its inner type) and list
   (non-empty iterates with `this`); an inverted section renders once
   when false, absent or empty. A section or interpolation of any other
   type, a timestamp or list interpolated directly, and a helper argument
   of the wrong type are compile errors.
4. **Escaping.** Literal template text is emitted as written. Interpolated
   values pass through the backstop's `escape`: `text` replaces every
   Unicode `Cc` character with U+0020, except U+000A in a `lines: multi`
   field; `html` does the same and then writes `&`, `<`, `>`, `"`, `'` as
   `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`. A `raw` tag bypasses
   `escape` and emits the value as written; this is sound because `raw`
   accepts only `bool` (`true`, `false`) and `enum`, whose texts the
   definition declares, and the compiler refuses (`template`) a `raw` tag
   over an enum with an option containing a `Cc` character or, under
   `escape: html`, any of those five characters. The check covers every
   option of the enum as resolved for the compiled plan, including
   options a profile parameter supplies (B4), since profiles are resolved
   at compile time.
5. **Helpers** (closed set `rustev.template-helpers/1`): `number <path>
   scale=<0..9>` (integer or decimal, rounded half-even with spec 002's
   decimal arithmetic, locale separators, ASCII `-`); `money <path>
   currency=<[A-Z]{3} or enum path> scale=<0..9>` (as `number`, then
   U+0020 and the code); `date <path> style=iso|numeric` (a timestamp's
   UTC calendar date, `YYYY-MM-DD` or the locale pattern); `list_join
   <path>` (a list of text or enum: empty is empty, one item is itself,
   otherwise items joined by the locale separator with the final
   conjunction before the last). Named arguments are literals, except
   `currency`, which also accepts a path: `scale` is a literal digit 0 to
   9 and `style` the literal `iso` or `numeric`, and a path in either is a
   compile error (`template`). A `currency` path must be of an `enum` type
   whose every option (as resolved for the compiled plan, B4) matches
   `[A-Z]{3}`, or the template is refused (`template`), so no value
   outside these forms can reach a helper at run time.
6. **Locales** (closed table, part of `rustev.template-helpers/1`; a
   Rustev convention, not a claim of CLDR conformance; grouping is by
   three digits; `<nbsp>` is U+00A0, `<nnbsp>` U+202F):

   | Locale | Decimal | Group | Numeric date | Separator | Conjunction |
   |---|---|---|---|---|---|
   | `und` | `.` | none | `YYYY-MM-DD` | `, ` | `, ` |
   | `en-US` | `.` | `,` | `MM/DD/YYYY` | `, ` | ` and ` |
   | `en-GB` | `.` | `,` | `DD/MM/YYYY` | `, ` | ` and ` |
   | `de-DE` | `,` | `.` | `DD.MM.YYYY` | `, ` | ` und ` |
   | `fr-FR` | `,` | `<nnbsp>` | `DD/MM/YYYY` | `, ` | ` et ` |
   | `es-ES` | `,` | `.` | `DD/MM/YYYY` | `, ` | ` y ` |
   | `pl-PL` | `,` | `<nbsp>` | `DD.MM.YYYY` | `, ` | ` i ` |

   Adding a locale or helper is `rustev.template-helpers/2`; the version
   is recorded in the plan binding.
7. **Bounds.** The compiler computes each backstop's worst-case UTF-8
   length from literal bytes, declared maxima (text `max_bytes`, longest
   enum option, 20 bytes per integer, 41 per decimal, helper outputs, list
   `max_items` per section), multiplied by 6 for interpolations under
   `html`. With `overflow: refuse`, a worst case above the field's
   `max_bytes` is a compile error. With `overflow: truncate`, rendering
   stops at the last character boundary at or below `max_bytes` that is
   not inside an `html` entity, and the field records `truncated: true`.
   A list backstop is a literal list of distinct options of the field's
   vocabulary, at most its `max_items`, checked at compile time. A
   backstop for a required field can never render empty: a required list
   field's backstop holds at least one option, and for a required text
   field the compiler also computes the minimum rendered length and
   refuses a minimum of zero (`template`). The minimum counts, outside
   sections, literal bytes and each interpolation's minimum (0 for text
   and `list_join`, the shortest option for an enum, 1 for an integer, a
   decimal and the `number`, `money` and `date` helpers); for a section
   and an inverted section on the same path, the smaller of their bodies'
   minimums, since exactly one of them renders; and 0 for any other
   section. Under `overflow: truncate` a required text field also needs
   `max_bytes` of at least 8, so truncation always keeps a first
   character or entity. So the `empty` reason of 3.7.4 never applies to a backstop.
8. **Determinism.** Rendering is a pure function of the template, the
   locale table version and exact values; output bytes are identical in
   native and WebAssembly builds. Backstop output is `exact-derived`.

### 3.5 Compilation

Generate steps go through spec 002's phases; the new checks belong to
these phases and refuse with three new categories: 22
`invalid_generation`, 23 `template` and 24 `model_output_flow`, the next
free after spec 036's 20 and 21.

| Phase | Refused when | Category |
|---|---|---|
| a | a member out of its bound in 3.2 or 3.3 | 1 `parse` |
| b | a required field without a backstop; a backstop for an undeclared field or of the wrong form; a field name colliding (3.9.1); a vocabulary unknown; more than 8 generate steps or 16 templates; `for_each.over` naming a non-list parameter, or any step that is not an exact step yielding an id list, filter result or shortlist (3.2), including a semantic or other model step whatever its output, except a generate step (a generate step there is a flow, refused in phase e as 24 `model_output_flow`, 3.6.1); `when` naming an action no rule proposes; until spec 035 is implemented, a projection that can carry untrusted text or a `screen.output` other than `none` (3.11) | 22 `invalid_generation` |
| c | a template missing, with another identity, with bad bindings, or failing to parse; a backstop failing to parse | 23 `template` |
| e | any reference to a generate step from anywhere but the text result (3.6), including from another generate step's `for_each.over` (the case phase b excludes); a template or backstop path that is unknown, ill-typed or not `exact-derived`; a backstop worst case over `max_bytes` under `refuse` | 24 `model_output_flow` for flows, 23 `template` otherwise |
| f | no candidate backend satisfies 3.17, with shortfalls listed | 5 `no_capable_backend` |
| g | worst-case requests or projection bytes over the limits of 3.1.2 without `truncate_visible` | 7 `budget` |

A refusal's category is fixed by the row its construct matches, never by
the order in which an implementation runs the phases. In particular a generate
step named in `for_each.over` matches only phase e (24
`model_output_flow`), never phase b, and is refused even if phase b runs
first.

A generate step has no compile-time capability fallback: an unsatisfied
step is refused, never compiled as `unsupported`, because the backstop
already covers runtime failure.

The plan (`rustev.plan/3`, the shared #76 version introduced by spec 032,
to which this spec adds one variant) records each generate step as a new
`PlanStepDetail` variant `generate` holding the bound backend id,
artifact, `DescriptorId`, the embedded template documents with their
identities, the resolved vocabularies, `max_items`, worst-case requests and
projection bytes, the helpers version and static derivation `model-derived`.

### 3.6 Isolation from decisions

1. A generate step's value is not addressable as a decision value. The
   compiler refuses (`model_output_flow`) any `step:<id>`, `bind:` or path
   reference to a generate step from: exact operator arguments and
   predicates; the projection, `for_each` (`over` and `items`),
   candidates, template parameters and backstops of any semantic or model
   step, generate steps included (so another generate step named in a
   generate step's own `for_each.over` is a flow); policy handlers,
   conditions, thresholds, outcomes, parameters and adjustments; execution
   policy entries other than the step's own retry and fallback; and any
   acquisition request a later spec defines (spec 036, #76 B1).
2. Generation runs after the judgment is computed and reads it only to
   select items (`when`, `for_each`, `proposal:` references). Nothing the
   generation stage does changes the judgment's bytes (3.10.5).
3. This is stricter than R-41 allows: `generate` declares no acceptance
   rule, so no path from its output to a decision exists at all.

### 3.7 Requests, outputs and validation

1. **Requests.** For each item the core lists one request in
   `pending()`, after every decision request has resolved. The request
   document `rustev.generate-request/1` (record) holds: step, instance
   (the item id), template id and identity, rendered instructions,
   language, canonical projection, the output schema (fields, types,
   bounds, vocabularies) and `repair` (`none` or `{round, fields:
   [{name, reason}]}`). Its `RequestId` (spec 004, 3.4) covers all of
   these through a new operation variant of `rustev.request/1`; existing
   request documents keep their bytes.
2. **Output.** `rustev.backend-output/1` gains the `text_record` variant:
   a map from field name to `text(string)` or `list([string])`.
3. **Record-level check** (spec 003's pre-supply check): the output is an
   `invalid_output` attempt when it is not a `text_record`, when it has a
   member that is not a declared field, or, for a repair request, a
   member not requested in that round. An `invalid_output` attempt is one
   whose request the runtime supplies as spec 002's runtime failure
   `invalid_backend_output{detail}`: this spec uses the shorter name for
   the attempt, and `invalid_output` is also the origin reason it maps to
   (3.10.3). Record-level failures follow the step's retry and fallback
   policy.
4. **Field-level check** on an accepted record, per declared field, with
   reason codes: `missing`; `wrong_type`; `empty` (a text of zero bytes,
   or an empty list for a required list field);
   `too_long` (UTF-8 bytes over `max_bytes`); `control_character` (any
   `Cc`, except U+000A under `lines: multi`); `not_in_vocabulary`;
   `duplicate_item`; `too_many_items`. A valid field is accepted as
   written: no trimming, normalization or truncation. An invalid field
   goes to repair (3.8) or, when repair is exhausted or not declared, to
   its backstop with reason `invalid_output`.
5. **Optional fields.** An optional field that is missing or invalid with
   no backstop is `absent{reason}`; with a backstop it is
   `backstop{reason}`.

### 3.8 Repair

1. With `repair.max_attempts` k > 0, after an accepted record with
   invalid fields the core lists a repair request for the same item with
   instance `[<item id>, "repair:<round>"]`. It carries the invalid
   fields' names and reason codes, never the invalid text.
2. `only_missing_fields: true` requests only those fields; a repair output
   may contain only them. `false` requests the full record; each field
   then takes the first valid value in round order, so a field valid in
   an earlier round is never replaced.
3. A field first made valid by a repair round has origin `repaired`.
4. Repair rounds run while invalid fields remain, up to k rounds, within
   the item's deadline (3.12). A repair request that fails gives the
   remaining invalid fields the reason of that failure (3.10.3).
5. Retries of a repair request are transport retries under the execution
   policy; they are not repair rounds.

### 3.9 Ownership

1. **Namespaces.** Each item in the text result has `context` (carried
   exact values) and `generated` (declared fields). The compiler refuses
   a generated field name equal to a carried field's last path segment, a
   projected reference's last segment, an input name, a step id, or a
   parameter name of any `propose` outcome. It also refuses two `carry`
   references with the same last path segment (for example
   `bind:item/price` and `input:price`), since that segment is the
   context field's name (`invalid_generation`).
2. **Output ownership.** A backend output member named like any of those
   is an undeclared member, so the attempt is `invalid_output` (3.7.3),
   and the text result and evidence record an `ownership_violation`
   notice naming the step, item and member.
3. **Final verification.** Before the result is returned, the evidence
   delivered or any cache entry written, the runtime verifies that every
   carried context value equals its source value in the validated
   snapshot or exact step value, and that the returned judgment's
   record-canonical bytes equal the bytes the core produced at policy
   time. The core performs the same check in `finish`. A difference, from
   any cause including a defective host-language seam, is restored from
   the source, and the text result records an `ownership_restored`
   notice naming the item and field (or `judgment`). The evidence always
   holds the source values' digests, never the mutated value. The bindings
   (#76 A1, A2) call the same verification before handing results to the
   host.

### 3.10 Per-field origin and the text result

1. **Origin** of each field slot (item by declared field): `generated`,
   `repaired`, `backstop{reason}` or `absent{reason}`, with `truncated`
   (backstop only) and `source`: `fresh` or `served_from_cache{original_decision_id}`
   (3.14).
2. **Reasons:** `screen_blocked`, `screen_unavailable`, `deadline`,
   `retries_exhausted`, `invalid_output`, `backend_unavailable`,
   `budget_exhausted`, `not_attempted`.
3. **Mapping** from what happened to the last request for the field:
   an `invalid_output` attempt (supplied as `invalid_backend_output`,
   3.7.3), or a field invalid after the last repair round, is
   `invalid_output`; `deadline_exceeded`, or the item or stage
   deadline (3.12), is `deadline`; `budget_exhausted` (ledger, step
   `max_cost_units` or `truncate_visible`) is `budget_exhausted`;
   `backend_unavailable` is `retries_exhausted` when the request made two
   or more attempts (retries or fallbacks, as the run record lists them)
   and `backend_unavailable` otherwise; an item beyond `max_items` is
   `not_attempted`; screening gives `screen_blocked` or
   `screen_unavailable` (3.11).
4. **The text result** `rustev.text-result/1` (record, `RECORD_V1`
   limits, record canonical form of spec 004 5.6): decision id, `PlanId`,
   per generate step its state (`ran`, or `skipped{reason}` with
   `outcome_not_matched` or `source_unresolved{<unresolved reason>}`) and
   items in list order (`id`, `context`, `generated` with value and
   origin per field, absent fields listed with no value), the aggregate
   `text_origin` and counts, and notices (`ownership_violation`,
   `ownership_restored`, `backstop_truncated`). With `for_each: none` the
   items are the one record (instance `[]`, 3.2). With a list they are
   every item of the `for_each` list, including those beyond `max_items`:
   such an item is listed with its `context`, no request is made for it,
   and each of its fields is `backstop{not_attempted}` when the field
   declares a backstop and `absent{not_attempted}` otherwise.
   `text_origin` is `none` when no slot holds a value (no field slot
   exists, or every slot is `absent`), `generated` when every slot is
   `generated` or `repaired`, `backstop` when every slot is `backstop` or
   `absent` and at least one is `backstop`, and `partial` otherwise;
   counts are per origin and per reason.
5. **Origin is not an outcome.** The judgment is final when the stage
   starts. A proposal whose every field fell back is still `propose`;
   text origin never adds an `Unresolved` reason, an escalation, a
   judgment notice or a failure termination. The judgment document of a
   plan with generate steps is byte-identical to the judgment the same
   plan would produce with its generate steps removed, apart from the
   `PlanId` it names.
6. **Derivation.** `generated` and `repaired` values are `model-derived`
   with lineage to the step, template identity, projection references and
   producing attempt; `backstop` values are `exact-derived` with lineage
   to the paths their template read. The item list's own lineage is the
   judgment's.

### 3.11 Screening (B7)

Screening is defined by spec 035 (#76 B7). Following its section 3.17,
generation:

1. applies spec 035's `unscreened_text` check to the projection of
   every generate step unconditionally: whether or not the definition
   declares a spec 035 `screening` block, and whatever the step's
   `screen.output` is (`none` included). `screen.input` is always
   `untrusted_fields` and has no setting that disables the check. A
   projection that can carry untrusted text no screening entry covers is
   refused;
2. never dispatches a request whose projection holds a withheld value
   (spec 035 3.8): the item's fields take their backstops with
   `screen_blocked`;
3. under `fail_closed` or `skip_generation` with the screener unavailable
   (spec 035 3.9.2, 3.9.3), takes backstops with `screen_unavailable`;
4. never places withheld or unscreened text in a request except under a
   declared `fail_open`;
5. with `screen.output` set to `screener{backend}` (a backend serving
   spec 035's `screen` operation) rather than `none`, screens each valid
   generated field before acceptance, one text field per request: a
   verdict other than `pass` gives that field `screen_blocked` with no
   repair, and an unavailable screener gives `screen_unavailable`. Since
   each request carries exactly one field, the per-item isolation spec 035
   requires of some screeners always holds, whatever the screener
   declares. Each text field is screened at most once per item: only
   when it first becomes valid, and its verdict is final (a field that
   passes is accepted; any other verdict ends the field with no repair),
   while repair never replaces a valid field (3.8.2) and asks again only
   for fields that never became valid. These requests therefore count in
   the step's worst-case requests (3.1.2) as one per text field per item
   (the `k` of 3.1.2), whatever the repair rounds.

Until spec 035 is implemented, each of the following is refused
(`invalid_generation`) on its own: a generate step whose projection can
carry untrusted text, whatever its
`screen.output`; and a generate step whose `screen.output` is other than
`none`. Either condition alone refuses the step, so generation fails
closed, and the screening acceptance items are reported as externally
blocked.

The untrusted-text criterion is spec 035 3.1.2's, which needs no part of
spec 035's implementation: it is a compile-time rule over the inputs'
declared provenance classes (spec 002 3.7.1). A projected text value is
untrusted when its input accepts any provenance class other than
`system-of-record`, and every text field of a record or list input
shares its input's classes.

### 3.12 Runtime: the generation stage (amends 003)

1. **Start.** The core exposes the judgment as soon as the last decision
   request resolves. If it is `propose` and a generate step's `when`
   matches, the stage starts at that instant `T_s` (runtime clock); spec
   003 3.3.5's unbounded policy evaluation is unchanged.
2. **Deadlines.** Step g's stage deadline is `G = min(D, T_s +
   floor(s * L))`, where `s` is the step's `limits.deadline_share` (3.2)
   and `L` is the definition's own `limits.deadline_ms` (spec 002 3.9.1,
   the duration behind `D`, spec 003 3.3.2), computed with spec 002's
   decimal arithmetic. An item's deadline is `min(G, T_i +
   per_item_deadline_ms)`, where `T_i` is when its first request is
   dispatched or refused. No attempt is dispatched at or after the item's
   deadline; an unresolved request at its deadline is supplied as
   `deadline_exceeded`. This amends spec 003 section 4, which deferred
   per-step deadline shares.
3. **Concurrency.** At most `limits.concurrency` items of a step are in
   progress, within the decision's `max_parallel_requests` and backend
   permits. Items start in list order.
4. **Cost.** Generation attempts reserve in the decision ledger (and the
   shared ledger) exactly as spec 003 3.5 states, plus, under
   `max_cost_units: units{n}`, a step sub-ledger of n units reserved
   first. A refused reservation is `budget_exhausted`.
5. **Retries and fallback.** An execution policy may list generate steps
   (spec 003 3.2.3 no longer refuses them as non-semantic); fallback
   targets must satisfy 3.17 for the step. Repair requests use the same
   step policy.
6. **Termination.** Each generate step ends when all its items are
   resolved or its own deadline `G` (item 2) is reached; the stage ends
   when every generate step has ended, and the decision then terminates
   `judged` with the text result. Cancellation behaves as spec 003 3.7: a
   cancelled decision returns no judgment and no text result. The
   decision result gains an optional text result member that is omitted
   (not present and empty) for plans without generate steps, so no
   document or record of such a plan gains a member or changes a byte
   (3.18).
7. **Order.** Final verification (3.9.3), then evidence delivery under the
   sink policy, then any cache write, then return.

### 3.13 Streaming

1. When the host requests streaming and the bound backend's descriptor
   declares `streaming: true`, the runtime yields events while the stage
   runs: `field{step, item, field, value, attempt_id, provisional: true}`
   when a field of a streamed output passes its field-level check (3.7.4);
   `superseded{step, item, field, attempt_id, origin}`;
   `lagged{count}` when `field` events were dropped (item 3); and a
   terminal `complete{text_origin}` after final verification. These four
   are the only event kinds.
2. Every provisional value that was queued and is not the field's final
   value (a later record-level failure, output screening, a repair, a
   backstop, or a restoration) is followed, before `complete`, by exactly
   one `superseded` event for that field and attempt. A provisional value
   dropped under item 3 was never delivered and gets no `superseded`
   event.
3. Only `field` events are droppable. A step yields at most `max_items *
   fields * (1 + repair.max_attempts)` `field` events (3.1); let `n` be
   that count summed over the decision's generate steps, fixed at compile
   time. When streaming starts, the runtime allocates one queue of
   capacity `stream_buffer + 2n + 1` (`stream_buffer`, 1 to 4096 events,
   runtime configuration), whose first `stream_buffer` slots are the only
   ones `field` events may occupy; other events never count toward or
   occupy them. A `field` event that finds `stream_buffer` `field` events
   already queued is dropped and counted, and a `lagged{count}` event is
   queued when the number of queued `field` events next falls below
   `stream_buffer`. `superseded`, `lagged` and `complete` events are never
   dropped and always find a slot: there are at most `n` `superseded`
   events (one per queued `field` event), at most `n` `lagged` events (one
   per run of drops) and one `complete`, so the reserved `2n + 1` slots
   always suffice. The decision never waits for the consumer. A consumer
   that sees no `lagged` event has seen every provisional value.
4. Streaming never changes the decision, the evidence, the capture or the
   cache entry; all are written only from the final result. A field served
   from cache is never streamed as provisional.
5. The Node binding exposes the stream as an async iterator and the
   service as server-sent events; specifying them is #76 A1's and A2's.

### 3.14 Caching

1. **Request outputs.** Generate and repair requests are semantic requests
   for spec 017: their equivalence includes the rendered instructions,
   template identity, output schema, repair round, projection, backend
   artifact and descriptor, and the authorized scope; the plan binding
   adds profile (B4) through `PlanId`. A hit supplies the stored output,
   which is validated again, and every field derived from it records
   `served_from_cache{original_decision_id}`, the decision id of the
   source attempt 017 records. Within the decision that gets the hit, the
   stored output stands in for the attempt it serves through all of that
   attempt's later processing (validation, repair, screening, origin): a
   field that fails validation again takes the
   same path as an invalid field of a fresh output (3.7), so it goes to
   repair (3.8) while rounds remain and otherwise to its backstop, and the
   runtime never re-dispatches the attempt the hit served.
2. **Results.** Any Rustev cache of whole results (the decision-result
   cache of #76 F1) stores the text result with its per-field origins and
   serves them unchanged, adding `served_from_cache{original_decision_id}`
   to every slot; it never stores a result without origins and never
   relabels a `backstop`, `absent` or `repaired` slot. Its key includes
   `PlanId` (hence template identities and profile), each bound backend
   artifact, each item's projection digest and the tenant. Whether to
   store results containing backstop slots is that cache's declared
   policy; when stored, they are served as backstop.
3. **Invalidation.** A template change changes `PlanId`, so every key
   bound to the old plan misses (3.3.5).

### 3.15 Evidence, capture and replay (amends 004)

1. **Evidence.** For a plan with generate steps the core evidence record is
   `rustev.evidence/2` (the shared #76 version, introduced by spec 032), to
   which this spec adds the optional member `generation`, omitted when the
   plan has no generate step: per generate step its state, `text_origin`
   and counts, and per item one entry listing every field slot's origin, reason, source,
   `truncated`, UTF-8 byte length and value digest (`sha256:` over the
   field's record-canonical value), and the carried context values'
   digests. Generated and backstop text itself is not in evidence.
2. **Run record.** Generate, repair and output-screening requests appear
   in the run record's existing `requests` list with their attempts,
   transitions and costs. This spec adds no member to any run record
   version; every plan 3 decision's record, and so every decision with a
   generate step, is `rustev.run/3` (the shared #76 version, introduced
   by spec 032 3.10.3), never an older run-record version.
3. **Capture** (spec 004 5.3): supplied `text_record` outputs and runtime
   reasons of generate requests are captured as any supplied value,
   subject to the host's retention policy (R-19). A supplied output served
   from the request cache is captured with its hit and source attempt, as
   spec 017 already requires (its 3.8 evidence records each hit's source
   attempt, and its capture keeps that attempt as the hit's origin). That
   attempt's decision id is the `original_decision_id` replay reproduces,
   so 3.15.4 needs no cache access. A digest-only output makes the bundle
   `incomparable` for reproduction, as today. No replay member is added;
   bundles of plan 3 decisions are `rustev.replay/2` (introduced by spec
   036).
4. **Reproduction.** For a plan with generate steps, `reproduced` also
   requires the `generation` evidence member's record-canonical bytes to
   equal the retained evidence, so origins, reasons, digests and
   `served_from_cache` are reproduced exactly; the attempt counts of
   3.10.3 come from the retained run record. Notices of 3.9.3 are runtime
   observations and are reported, not reproduced. Replay never dispatches
   and never consults a cache.

### 3.16 Evaluation metrics (amends 004)

For datasets whose plan has generate steps, the report adds, per
generate step and in total: field slots; `text/backstop_rate` = (backstop
plus absent slots) / slots, unknown when there are no slots; counts per
origin and per reason; per-case `text_origin` counts; and the count of
slots served from cache. Reports over other plans are byte-identical.
Rubric scorers over generated fields are #76 E1's; this spec requires that
a scorer that is itself a model is identified and its metric labeled
`model-derived`, never pooled with an exact metric.

### 3.17 Backend capability

1. `OperationSupport` admits operation `generate` with the new output
   kind `text_record{structure, max_output_bytes, languages, streaming}`:
   `structure` is `json_schema`, `tool_call` or `grammar`; languages are
   BCP 47 tags; `max_output_bytes` is a content capacity: the most UTF-8
   bytes of field values (text values and chosen list options) one
   response can carry, excluding the structure that encodes them (keys,
   quotes, brackets, separators, escapes, tool-call framing), which the
   backend's mapping must provide on top of it. `max_options` is not a
   member of `text_record`: it is `OperationSupport`'s existing member
   beside `output` (spec 002, the most options, levels or candidates one
   request may carry), and for `generate` it is the most fields per
   record. The determinism tier is the descriptor's existing
   `determinism`. Existing descriptors keep their bytes and `DescriptorId`s.
2. A descriptor satisfies a generate step when it supports `generate`,
   its `max_options` is at least the field count, `max_output_bytes` is at
   least the sum of text `max_bytes` plus, per list field, `max_items`
   times its longest option (both content bytes, as item 1 defines), the
   step's `language` is listed, and its determinism and artifact meet
   `backend`. Shortfalls are listed as spec 002 3.9.5 category 5 does.
3. `rank`, `classify`, `proposition` and `rubric` are unchanged; R-05's
   name set gains `generate` and `extract` (R-42). This spec defines only
   `generate`.

### 3.18 Compatibility and identity

1. **Shared versions.** A generate step requires `rustev.definition/2`
   and yields `rustev.plan/3` and `rustev.evidence/2`, the #76 track
   versions introduced by spec 032; this spec adds to them the step
   variant, the plan step detail variant and the optional evidence member
   `generation`, omitted when unused. It introduces no further version of
   any existing document. Descriptors, requests and backend outputs gain
   only new enum variants (3.7, 3.17); judgments are unchanged.
2. **Unaffected plans.** A version 1 definition compiles exactly as
   before; a document without the new constructs keeps its bytes, and
   under the same compiler identity every committed plan and definition
   golden keeps its bytes and `PlanId`.
3. A plan with generate steps has a `PlanId` that changes with any change
   to its generate declaration, template, vocabulary, locale, helpers
   version or bound backend.
4. Spec 004 5.6's record-canonical documents gain
   `rustev.generate-request/1` and `rustev.text-result/1`;
   `rustev.prompt-template/1` is identity-bearing.
5. Older readers refuse the new variants and versions as unknown, as every
   strict parser of these documents already does; no reader is given a
   text result it did not ask for.

## 4. Out of scope

Defining screening (spec 035); `extract` and its acceptance rules (#76 B6);
model-backed ranking (#76 C2); the decision-result cache itself (#76 F1);
rubric scorers (#76 E1); Node, WebAssembly and service exposure (#76 A1,
A2); provider wire mappings, including Bedrock's (spec 030 3.12); the
remote adapter protocol mapping of `generate` (a later change of spec
009); free-form or unbounded generation, chat, tool use or agent loops;
generated text feeding any decision; prompt rendering of untrusted text
into instructions; templates with partials, lambdas or logic; quality
claims about generated text (R-04, R-37); live provider calls or spend.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A required field without a backstop | `invalid_generation`, naming step and field. |
| A backstop path to an unknown field | `template`. |
| A backstop path to a model-derived input or a semantic step | `template` (not `exact-derived`). |
| A selection rule, exact step, threshold or another step reading `step:<generate id>` | `model_output_flow`. |
| A template parameter bound to a model-derived value | `model_output_flow`. |
| `{{> partial}}`, `{{=<% %>=}}`, `{{#if x}}`, an unclosed section | `template`, parse. |
| `{{_name}}`, or a `bind` of `_item` or `input` | `template` (parse) or `invalid_generation` (bind). |
| `{{{item.name}}}` on a text field | `template` (raw needs enum or bool). |
| A required text field whose backstop is `{{item.name}}` alone, or a required list field whose backstop is `list{[]}` | `template` (may render empty). |
| Backstop worst case over `max_bytes` with `overflow: refuse` | `template`; with `truncate`, compiles and records `truncated`. |
| Generated field named `price` while `carry` holds `bind:item/price` | `invalid_generation`. |
| `carry` holding `bind:item/price` and `input:price` | `invalid_generation`. |
| A `text` template parameter bound to an `input:` reference | `template`. |
| A supplied template whose identity differs from the declared digest | `template`. |
| Worst-case requests over `max_semantic_requests` with `refuse` | category 7 `budget`. |
| Output with a member `name` not declared | attempt `invalid_output`; `ownership_violation` notice; fields per policy. |
| A field over `max_bytes` with repair 0 | `backstop{invalid_output}`; judgment still `propose`. |
| Repair output containing a field not requested | attempt `invalid_output`. |
| Every item deadline passes | every slot `backstop{deadline}`; `text_origin: backstop`; termination `judged`. |
| Two attempts fail `transient` | `backstop{retries_exhausted}`. |
| One attempt fails `permanent` | `backstop{backend_unavailable}`. |
| Ledger refuses the reservation | `backstop{budget_exhausted}`. |
| Item beyond `max_items` | listed in the text result with no request; `backstop{not_attempted}` per field with a backstop, `absent{not_attempted}` otherwise. |
| Judgment `escalate` | stage not run; text result empty, `text_origin: none`. |
| A carried field mutated after `finish` (fault injection) | restored; `ownership_restored`; evidence holds the source digest. |
| A streamed field later replaced by its backstop | `superseded`; final result equal to the unstreamed run. |
| Request cache hit | stored output revalidated; `served_from_cache{original_decision_id}`. |
| Template edited by one byte | new `PlanId`; cache miss. |
| A plan without generate steps | byte-identical plan, judgment, evidence and run record. |
| A projection carrying unscreened untrusted text | `unscreened_text` (spec 035); `invalid_generation` before 035 is implemented. |
| A generate step in a `rustev.definition/1` | category 11, `requires-definition-2`. |

## Acceptance

- Forced tests with scripted backends (R-09) for each reason (screen
  blocked, screen unavailable, deadline, retries exhausted, invalid
  output, backend unavailable, budget exhausted, not attempted) assert
  per-field origin, aggregate `text_origin` and counts, and that the
  judgment stays `propose` and byte-identical to the plan's judgment
  without generation. The screening items are externally blocked until
  spec 035 is implemented and are never reported as passed before then.
- Repair: a field invalid in the first output and valid in round 1 is
  `repaired`; a field valid in round 0 is never replaced under
  `only_missing_fields: false`; repair requests never carry invalid text.
- A request cache hit preserves origin and records `served_from_cache`
  with the original decision id; a result-cache conformance test for #76
  F1 is committed as a pending fixture that F1 must pass.
- A definition routing generated text into a selection rule, an exact
  operator, a threshold, a template parameter or another model step is
  refused at compile time with `model_output_flow`.
- Ownership: an output member named like a context field is rejected with
  `ownership_violation`; a post-hoc mutation of a carried field and of the
  judgment, injected through a test-only hook, is restored with
  `ownership_restored`.
- A one-byte template change alters `PlanId` and misses the cache.
- Backstop language: a grammar test corpus (accepted and refused
  templates, every helper, every locale row, escaping, nesting, `maybe`
  and list sections); unknown path and model-derived access are compile
  errors; rendering respects `max_bytes` and truncation boundaries; the
  same corpus yields identical bytes compiled for `wasm32-wasip1` under a
  WASI runtime, which depends on #76 A1's toolchain and is externally
  blocked until it lands.
- Streaming: a streamed field later replaced by its backstop emits
  `superseded`; a full queue drops only `field` events and emits
  `lagged`, never drops a `superseded` event, and emits none for a
  dropped value; the final result, evidence, capture and cache entry
  equal a non-streamed run.
- Replay reproduces a decision with generated, repaired and backstop
  slots, including a cache-served slot, with zero backend calls.
- Text metrics are golden-tested; every existing eval report golden is
  byte-identical.
- Compatibility: every existing definition, plan, descriptor, run, replay
  and eval golden is byte-identical, and every committed `PlanId` is
  unchanged.
- Every row of section 5 has a test.
- Seeded defects are each detected: a missing backstop accepted, a
  model-derived backstop path accepted, a generated value readable by a
  rule, origin dropped on a cache hit, repair overwriting a valid field,
  a `superseded` event missing, a mutation not restored,
  `retries_exhausted` and `backend_unavailable` swapped.
- `make code`, `spec-spine verify 037-generation-steps` and `make gate`
  pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
prompt and backstop template languages as a pure core module, built on
`main` while specs 035 and 036 land; (2) the contract vocabulary: the
envelope, the `generate` declaration, plan, evidence and text result
members; (3) compilation, isolation and the `generate` lowering; (4)
requests, validation, repair, ownership and per-field origin in the core;
(5) the runtime generation stage, streaming and caching; (6) evidence,
capture, replay and metrics; (7) seeds, declared verification and closure.

**Increment 1 (2026-10-24): the template languages.**
`crates/rustev-core/src/template/` (`parse.rs`, `check.rs`, `render.rs`)
implements the prompt template rules of 3.3 and the logic-less backstop
language of 3.4 as a self-contained module, not yet wired into
compilation: grammar, nesting and roots, typed parse errors, static type
and derivation checks, escaping and raw restrictions, helpers for every
locale, output bounds, required fields and truncation, canonical source
digests, and deterministic rendering. Decisions within the spec: authored
source bytes, including comments and whitespace, are identity input;
prompt helpers use the `und` locale and prompt text keeps LF; version
syntax is three decimal components without leading zeros; static schema
nesting is capped at 64; required HTML truncation conservatively refuses a
possibly empty leading entity fragment. Tests:
`crates/rustev-core/tests/template.rs`, including the spec's examples and
1,500 bounded generated inputs that never panic.

**Increment 2, part 1 (2026-10-24): generate declarations and plan
bindings.** `crates/rustev-contract/src/generation.rs` adds the bounded
model step envelope of 3.1 and the `generate` step declaration of 3.2 to
`rustev.definition/2`, with the prompt and backstop templates carried as
their authored source (the template module validates them), document-local
parse checks, the prompt template document and its identity (3.3.4,
3.3.5), and the plan bindings for generate steps in `rustev.plan/3`.
A `generate` step in a `rustev.definition/1` is refused, category 11
`requires-definition-2`; a `rustev.definition/2` using only it passes the
version check, and compilation refuses it with `invalid_definition`
detail `generation-not-compiled` until the compile increment. Documents
without these members keep their bytes and identities
(`tests/generation_wire.rs`, `crates/rustev-core/tests/generation_compile.rs`).
Remaining for part 2: the text result and per-field origins (3.10), the
evidence members (3.15) and the backend capability additions (3.7, 3.17).

**Increment 2, part 2 (2026-10-24): text result, origins, evidence and
capability.** `crates/rustev-contract/src/generation_result.rs` adds the
text-record output and required kind of 3.7, the descriptor capability of
3.17, the text result with per-field origin (`generated`, `repaired`,
`backstop{reason}`), lineage and counts of 3.10, and the digest-only
`rustev.evidence/2` generation members of 3.15, each appended last and
omitted when unused, with redaction classes. `OutputKind` is no longer
`Copy`, because its text-record variant carries a language list; the
clones this needs elsewhere change no behavior. Documents without these
members keep their bytes and identities (`tests/generation_wire.rs`).

**Increment 3 (2026-10-24): compiling generate steps.**
`crates/rustev-core/src/generation.rs` replaces the temporary
`generation-not-compiled` refusal: generate steps compile with their
prompt and backstop templates validated through the template module and
their identities bound into `PlanId` (3.3.4, 3.3.5, 3.5), a backstop for
every required field, generated text isolated from exact operators,
policy controls, acquisition requests and other model steps (3.6),
static ownership checks (3.9), backend capability matching (3.17), and
the plan members filled. New refusal categories 22 `invalid_generation`,
23 `template` and 24 `model_output_flow` (3.5), with surface names, the
serve refusal's category bound, core and CLI golden refusals and their
harvested conformance cases. Until spec 035's compiler is integrated, the
phase-b rule of 3.5 applies: a generate step whose projection can carry
untrusted text is refused as 22, naming the step and field; spec 035's
`unscreened_text` check replaces it. Profile-parameter substitution in
generate steps is refused at compile for now. Tests:
`crates/rustev-core/tests/generation_compile.rs` with plan and refusal
goldens.

**Increment 4 (2026-10-24): requests, validation, repair, ownership and
origins in the core.** `crates/rustev-core/src/generation_eval.rs` builds
one generation request per selected item with the rendered prompt
(`rustev.generate-request/1`, 3.7) and exposes them as pending requests;
supplied outputs are validated against the field schema (3.7), repaired
within the declared bound (3.8), checked for ownership (3.9), and every
missing, invalid, unrepaired or withheld required field takes its
rendered backstop with `backstop{reason}` (3.4, 3.10). The text result
carries per-field origins, lineage and counts (3.10); generated text
never reaches a decision input (3.6), and evidence holds digests only
(3.15). Tests: `crates/rustev-core/tests/generation_eval.rs` (12 scripted
tests: failures, repair, ownership, budgets, determinism, judgment
isolation).

**Increment 5, part 1 (2026-10-26): the runtime generation stage.**
`crates/rustev-runtime/src/generation.rs` dispatches the core's pending
generation requests after selection (3.12) through the existing retry,
fallback, deadline, cost and cancellation machinery, with repair round
trips and step costs. Tests: `crates/rustev-runtime/tests/generation.rs`
(11, manual clock, scripted backends). A generation invariant failure has
no run-record termination (3.12.6): the runtime returns
`GenerationFailed`, the live API `LiveRefusal::GenerationFailed`, and the
decision service refuses with spec 042's `internal_error` (500, gRPC
`INTERNAL`; owner decision 2026-10-26), never a request refusal.
Streaming (3.13) and caching (3.14) follow in part 2.

**Increment 5, part 2 (2026-10-26): streaming and caching.** Streaming
(3.13) goes through an additive, default-compatible path on the core's
backend seam: fields are delivered in order and only after validation,
with lag bounded and the stream's waker woken only after its lock is
released; a backend without the path behaves exactly as before. Generated
values are cached under spec 017 per 3.14, keyed with the template
identities and the authorized scope, preserving per-field origins and the
cached source in every copy; a template change invalidates. Tests:
`crates/rustev-runtime/tests/generation.rs` grows to 20.

**Increment 6 (2026-10-24): evidence, capture, replay and metrics.**
Generation outputs are captured into `rustev.replay/2` with their
per-field origins and digest-only evidence, through the runtime's
capture hooks; offline replay supplies the captured outputs and never
dispatches, an uncaptured output is handled as 3.15 states, and evidence
tampering is detected (3.15). Replay 2 admits a generation-only bundle
without source descriptors. The evaluation metrics of 3.16 are computed
in `crates/rustev-eval/src/metrics.rs` and reported. Tests:
`crates/rustev-eval/tests/generation_replay.rs` (ten: byte-identical
offline replay with zero backend calls, cached origins, origins kept
through repair, a backstop, an absent value and a cached invalid output,
missing outputs,
an unrecorded origin, tampering, a captured runtime failure, retries,
multiple steps, every metric, and the report's text metrics and
generation artifact).

**Increment 7 (2026-10-24): acceptance audit, missing tests and seeds.**
Every Acceptance item and section 5 row is mapped to its proving test in
`crates/rustev-eval/tests/acceptance-037.md`; the missing tests are added
(a private test module,
`crates/rustev-runtime/src/generation_acceptance.rs`, and additions to the
core, runtime and eval generation suites). The private module's fixtures
(`generate-step.json`, the generation helpers and the shared scripted
backend) live under `crates/rustev-runtime/src/testdata/`, which the
reviewed archive carries, so its unit tests also run from the unpacked
archive (spec 021 3.7). Eight `spec 037` mutation seeds
(missing required backstop accepted, model-derived backstop path accepted,
generated value readable by a rule, origin dropped on cache hit, repair
overwriting a valid field, superseded event missing, carried mutation not
restored, retries exhausted and backend unavailable swapped) are each
detected (`seeds.sh "spec 037"` with `SEEDS_JOBS=1`: 8/8); `spec 037`
joins the harness's `OWN_RUNS`. Not yet met, so `implementation` stays
`in-progress`: the screening items wait for spec 035's `unscreened_text`
and screening outcomes to replace this branch's interim category-22 rule
once both chains are on `main`; the result-cache origin item waits for #76
F1 (spec 044), recorded as an ignored test and a pending fixture, never as
passed; WASI byte parity of the template module waits for the A1 toolchain
integration.

**Owner decision A-49 (2026-10-23): dependencies.** Spec 030 is no longer
a declared dependency of this spec. No behavior or acceptance item here
needs it: provider wire mappings, "including Bedrock's (spec 030 3.12)",
are out of scope (section 4), and admitting Bedrock's reserved `generate`
value is its own change under 030 3.12, made only after spec 030 is
complete.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #63 leaves the points below open, and each is
decided within the binding principles, as written:

1. **The text is beside the judgment, not in it.** The judgment is final
   before generation and keeps its bytes; text and origins live in
   `rustev.text-result/1` and in the `generation` evidence member. This makes "origin
   never changes the outcome" structural and keeps judgment equivalence
   (spec 003, 3.8) and every existing judgment golden intact. Text notices
   are in the text result, not the judgment.
2. **Shared #76 versions.** Generate steps add to `rustev.definition/2`,
   `rustev.plan/3` and `rustev.evidence/2` as the track's coordination
   rule requires, and add nothing to `rustev.run/3` or `rustev.replay/2`;
   plans without #76 constructs are byte-identical (I-1).
3. **Mustache sections only.** The issue's example used `{{#if}}` and
   `{{else}}`, which are Handlebars, not Mustache; the language keeps
   Mustache's section and inverted section and refuses `if`/`else`.
   Unqualified names are refused (no context stack) so typing is static.
4. **Prompt instructions read only exact parameters.** Rustev renders the
   instructions with the same language; item data and caller text travel
   only in the projection, which keeps untrusted text out of
   instructions and makes requests backend-neutral.
5. **Templates are embedded in the plan,** like the execution policy, so
   replay and `Compiled::load` need no separate template retention.
6. **Locales and helpers are a small closed table** owned by Rustev, not
   CLDR, so rendering is pure and identical across targets; money is the
   amount followed by the ISO code in every locale.
7. **Origin vocabulary extended.** `screen_unavailable` is added to the
   issue's reasons, matching spec 035's reason, so a screener outage
   (including `skip_generation`) is not reported as a block;
   `absent{reason}` covers optional fields without backstops; the
   aggregate adds `none` for results with no field slots.
8. **`retries_exhausted` versus `backend_unavailable`:** two or more
   attempts (retries or fallbacks) versus one, as the run record lists
   them, so replay derives the same reason.
9. **Whole-record versus field-level failure.** An undeclared member
   invalidates the attempt (ownership); a declared field's invalidity is
   repaired or backstopped per field.
10. **Deadline share is a cap, not a reservation:** the decision phase is
    not shortened; generation gets at most its share of the decision's
    deadline, and never past `D`.
11. **Caching** reuses spec 017's request-level cache for generation
    outputs and states the obligations a result cache (F1) must meet.
12. **Ownership verification** runs in the core and again in the runtime
    and bindings; a restoration is a notice in the text result, since the
    evidence already holds the source values.
13. **Envelope for later model steps** (3.1): spec 038 (`extract`, #76 B6)
    and spec 039 (model-backed `rank`, #76 C2) reuse it and add their own
    acceptance rules; `generate` declares none.
14. **Refusal category numbers** are 22 to 24, the next free after spec
    036.

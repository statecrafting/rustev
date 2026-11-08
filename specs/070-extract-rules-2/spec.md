---
id: "070-extract-rules-2"
title: "Extraction rules 2: token extractors, exact-value conditions, declared date formats and rules first (amends 038 and 032)"
status: approved
implementation: pending
created: "2026-11-08"
summary: >
  An amendment of approved specs 038 (3.3.3, 3.3.5, 3.5, 3.7, 3.11, 3.12,
  3.14) and 032 (3.2.1, 3.9.3), for #572, ratified by the owner on
  2026-11-08 (A-90) for release 0.7.0. A host that runs per-sender
  templates over forwarded booking emails cannot express them in
  `rustev.extract-rules/1`, so it runs them as a host backend and every
  templated field becomes `model-derived`. This spec adds the program
  schema `rustev.extract-rules/2`, a superset of /1: the value extractors
  `token_after` (a token from a declared ASCII charset after a term),
  `token_matching` (a token of a small structured shape, never a regular
  expression), `date` (a closed set of declared formats with month names
  from a table the program declares) and `vocabulary_after` (a token that
  is a member of a declared vocabulary, graded `hint` under spec 032); the
  conditions `equals` and `one_of` over an exact input value; and the
  extract step's `fill: rules_first`, which evaluates the program before
  the request is listed, skips the request when every required field is
  filled, and otherwise asks the backend only for the fields the rules left
  absent. Everything stays pure, bounded and evaluated in `rustev-core`
  with no new dependency. A rules match is evidence that this program
  produced this value from this text, never that the value is true. /1
  programs, every existing plan and every `PlanId` keep their bytes.
amends:
  - "038-structured-extraction"
  - "032-evidence-quality"
extends:
  # The /2 program document, its JSON Schema, the `rules_first` fill and
  # its `required` member, the plan 3 fallback detail and the evidence
  # request outcome `skipped` (3.1, 3.7, 3.9).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # The extractors, conditions, binding checks, grades and rules-first
  # evaluation and listing (3.2 to 3.8).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # A skipped extract request reserves, dispatches and captures nothing;
  # the partial request goes through the existing driver (3.7.6).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  # The generated TypeScript declarations gain the /2 members.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/types/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/types/" }, nature: additive }
  # Conformance cases for rules first, and the WebAssembly gate value if
  # an increment crosses it (3.11).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
  # Adds 070 to `make verify` at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Places 070 in CI's `integrations-base` acceptance group at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "005-reference-backends"
  - "027-node-and-wasm-bindings"
  - "032-evidence-quality"
  - "035-screening-seam"
  - "038-structured-extraction"
  - "057-wasm-raw-budget-for-spec-039"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every rustev.extract-rules/1 document keeps its bytes, its identity and its meaning; every committed definition, plan, evidence and judgment golden keeps its bytes and PlanId; a /1 document holding any /2 construct is refused at parse, never read as /2."
    anchor: "3-10-compatibility-and-identity"
  - id: "I-2"
    kind: invariant
    text: "A value extractor yields exactly the bytes it matched, or nothing: a token longer than its declared bound, outside its charset or shape, or a calendar-invalid date makes the rule not apply; nothing is cut short, case-changed, trimmed, padded beyond the declared date rendering, or defaulted, and the produced value is validated by spec 038 3.4 like any fallback value."
    anchor: "3-3-value-extractors"
  - id: "I-3"
    kind: invariant
    text: "An `equals` or `one_of` condition reads only an input whose accepted provenance excludes `model-derived`, compares its core value exactly, and is never evaluated as false because its source is unresolved: the field stays absent with `source_unavailable`."
    anchor: "3-4-exact-value-conditions"
  - id: "I-4"
    kind: invariant
    text: "Under `fill: rules_first`, a field the rules filled is never requested from or replaced by the backend; a request skipped because every required field was filled is recorded `skipped{rules_filled}` with no attempt, no reservation and no cost; and compile-time budgets count the request as before."
    anchor: "3-7-rules-first"
  - id: "I-5"
    kind: invariant
    text: "Every present value of an extract field that a `vocabulary_after` rule can fill carries grade `hint`, its static ceiling is `hint`, and only a spec 032 corroboration step can raise it; a hard gate or requirement reading it otherwise is refused with category 14 `uncorroborated_hint`."
    anchor: "3-8-grade-of-vocabulary-capture"
  - id: "R-1"
    kind: requirement
    text: "The /2 evaluator is pure, bounded and in rustev-core, gives byte-identical results on every target including wasm32, uses no regular expression engine, Unicode table, locale data, floating point or new dependency, and stays within the per-byte work bounds of 3.6."
    anchor: "3-6-evaluation-bounds-and-determinism"
  - id: "R-2"
    kind: requirement
    text: "The optimized WebAssembly module stays within spec 057's gated uncompressed budget; an increment of this spec that crosses 5,242,880 bytes raises the gate to 6,291,456 bytes in the same change, as 3.11 amends 027 3.10.4."
    anchor: "3-11-webassembly-budget"
---

# 070: Extraction rules 2: token extractors, exact-value conditions, declared date formats and rules first (amends 038 and 032)

Approved: ratified by the owner on 2026-11-08 (A-90) for release 0.7.0
("we do need it in 0.7.0 ... proceed with specs/build"); the
implementation is pending. The approved texts of 038 and 032 are not
edited; this spec records what it adds to and changes in them. Section and
item numbers such as "038 3.7.3" refer to those specs.

## 1. Purpose

A host that ingests forwarded booking emails runs per-sender templates
before any model call: it pulls a confirmation code, a flight number,
dates and a place from a known supplier layout. Those templates are
deterministic, so their fields should be `exact-derived` and live in the
plan as identified configuration. `rustev.extract-rules/1` (038 3.7) has
only `literal`, `integer_after`, `date_iso` and `record` values and
text-only conditions, so the host runs its templates as a host backend,
which makes every templated field `model-derived` (spec 005, 3.14) and
puts it behind the same acceptance rules as the model.

This spec closes four gaps, in the issue's order of value: value
extractors for tokens, shapes and declared date formats (3.3); a
condition on an exact input value, so a template can be selected by the
sender's DKIM `d=` domain (3.4); a rules-first fill mode that asks the
backend only for what the rules left absent and skips it when nothing
required is left (3.7); and a vocabulary capture that carries a `hint`
grade (3.3.5, 3.8).

What a match means is fixed here (3.12, A-90): a rules match is evidence
that this program produced this value from this text, never evidence that
the value is true. Qualifying a rules program against supplier layouts is
the host's evaluation work, not Rustev's.

## 2. Territory

`crates/rustev-contract/` (002, amending): the `rustev.extract-rules/2`
document and its JSON Schema, the `rules_first` fill with `required` in
`rustev.definition/2`, the plan 3 fallback detail, and the evidence 2
request outcome `skipped`; the regenerated schemas. `crates/rustev-core/`
(002, amending): the extractors, conditions, binding checks, grades and
rules-first listing and merge. `crates/rustev-runtime/` (003, additive):
tests that a skipped request is never reserved, dispatched or captured,
and that a partial request runs through the existing driver.
`integrations/rustev-node/types/` and `integrations/rustev-wasm/types/`
(027, additive): regenerated declarations; neither binding's code
changes. `tools/rustev-conformance/` (027, amending): runtime conformance
cases for rules first and, only if 3.11.3 applies, the gated value in
`js/wasm-check.mjs`. At completion, `Makefile` and `scripts/acceptance/`
(001, additive).

Unchanged: `rustev.extract-rules/1` and its evaluator's results;
`rustev.extract-request/1` (its shape; 3.7.4 states which schema it
carries); `rustev.definition/1`, `rustev.plan/2`, `rustev.backend-output/1`
and the descriptor documents; the fill modes `request_failure` and
`absent_fields`; the model-value gate (038 3.9); every refusal category
and its phase order; spec 005's rules backend, whose language is separate.

### 2.1 Binding principles

- **Proposals, never grants.** An extracted value, however produced, is a
  decision input; nothing here authorizes an action.
- **Derivation and lineage.** A rules-filled field is `exact-derived`
  only when every source the program read for it is `exact-derived`
  (038 3.5.2); condition sources count as reads (3.4.4). Its lineage names
  the program identity and its sources.
- **Unresolved, never a default.** An unreadable source leaves the field
  absent `source_unavailable`; a condition is never false because its
  source is unresolved (I-3); a skipped request is recorded, never
  silently omitted.
- **No silent conversion.** Captured bytes are kept exactly; a date is
  rendered to `YYYY-MM-DD` only by the declared format's fixed mapping
  and validated by 038 3.4; a vocabulary capture is a `hint`, never
  corroborated by its own match (3.8).
- **Pure core.** No I/O, no clock, no locale data, no regular expression
  engine, no Unicode tables, no floating point.

## 3. Behavior

### 3.1 The document

1. `rustev.extract-rules/2` is parsed under `DESCRIPTOR_V1` with unknown
   fields and duplicate keys refused, identified by its tagged digest as
   /1 is. Every /1 construct is valid in /2 with the same meaning. Added
   (contract types normative):

```text
program   = { schema: "rustev.extract-rules/2", name, version,
              sources: [source], month_tables?: { <table>: months },
              fields: [field_rules] }
months    = [ [name] ] (exactly 12 entries, January first)
condition = ... /1 conditions ...
          | { equals: { source, value: scalar } }
          | { one_of: { source, values: [scalar] } }
value     = ... /1 values ...
          | { token_after: { source, terms: [string], charset: [class],
                             min_len, max_len } }
          | { token_matching: { source, shape: [element], occurrence? } }
          | { date: { source, format, months?, occurrence } }
          | { vocabulary_after: { source, terms: [string], vocabulary } }
class     = "upper" | "lower" | "digit" | { range: [lo, hi] }
element   = { repeat: { class: [class], min, max } }
          | "optional_space"
          | { byte: string }
scalar    = JSON string | JSON integer | JSON boolean
```

2. Document-local bounds, refused at parse: `month_tables` holds 1 to 4
   tables named `[a-z][a-z0-9_]{0,15}`; each table has exactly 12
   entries, each 1 to 4 names of 1 to 16 bytes of UTF-8 holding no ASCII
   control byte, space, digit, `,`, `.`, `/` or `-`, and no two names of
   one table are equal after ASCII folding. `terms` of the new extractors
   hold 1 to 64 terms of 1 to 64 bytes (/1's bound). `charset` and a
   `repeat` class hold 1 to 8 entries; a `range` is two one-byte strings
   in `!` (0x21) to `~` (0x7E) with `lo <= hi`. `min_len` and `max_len`
   satisfy `1 <= min_len <= max_len <= 64`. A `shape` holds 1 to 16
   elements; a `repeat` has `0 <= min <= max` and `1 <= max <= 16`; a
   `byte` is one byte in 0x21 to 0x7E; `optional_space` is neither the
   first nor the last element; the shape's minimum length (the sum of
   every `repeat`'s `min` and one per `byte`) is at least 1. `occurrence`
   is 1 or 2 and defaults to 1 for `token_matching` only. `one_of` holds
   1 to 256 distinct values; a text `value` is at most 256 bytes. /1's
   bounds (038 3.7.4) apply unchanged to both versions.
3. A `rustev.extract-rules/1` document holding a /2 member or variant is
   refused at parse as an unknown member, never read as /2 (I-1).
4. **Naming.** 038 3.3.3's `rules_program` names a `<name>@<version>` of
   a supplied /1 or /2 document; the supplied document's `schema` member
   decides its version.

### 3.2 Shared text rules

1. **Terms** match as /1's do: ASCII `A` to `Z` fold to lower case in both
   text and term, other bytes compare exactly (spec 005, 3.4.4; 038
   3.7.3). An extractor that takes `terms` uses the earliest occurrence of
   any term, ties going to declared term order, and only that occurrence:
   when the text after it does not yield a value, the rule does not apply
   and no later occurrence is tried (as `integer_after`).
2. **Skipping after a term.** From the end of the term occurrence: any
   number of ASCII spaces (0x20) and tabs (0x09), then at most one
   separator byte from `:`, `#` and `-`, then any number of spaces and
   tabs. Nothing else is skipped; a line break ends the skip.
3. **Classes.** `upper` is `A` to `Z`, `lower` is `a` to `z`, `digit` is
   `0` to `9`, and `{range: [lo, hi]}` is every byte from `lo` to `hi`
   inclusive. A byte is in a class list when it is in any entry. Classes
   compare bytes exactly: no case folding. No class contains a space, a
   control byte or a byte above 0x7E, so every captured token is printable
   ASCII.
4. **Token boundaries.** A position `p` in a text of length `n` is a
   start boundary when `p = 0` or byte `p - 1` is not an ASCII letter or
   digit, and an end boundary when `p = n` or byte `p` is not an ASCII
   letter or digit. Bytes above 0x7F are not letters or digits, so a
   non-ASCII character (including a no-break space) bounds a token.

### 3.3 Value extractors

1. **`token_after{source, terms, charset, min_len, max_len}`.** After the
   term occurrence and the skip of 3.2.2, the token is the maximal run of
   bytes in `charset`. The rule applies when the run's length is within
   `[min_len, max_len]`; a shorter or longer run (including an empty one)
   makes it not apply. A longer run is never cut short. When the charset
   contains the separator byte that the skip consumed, the skip still
   consumes it first. The value is a JSON string of exactly the run's
   bytes.
2. **`token_matching{source, shape, occurrence}`.** A match is found at a
   start boundary by applying the shape's elements in order, each
   possessively, with no backtracking:
   - `repeat{class, min, max}` takes the longest run of up to `max` bytes
     in `class`; fewer than `min` fails the match at this start;
   - `optional_space` takes one ASCII space when the next byte is one,
     and nothing otherwise;
   - `byte` takes that exact byte or fails the match at this start.
   After the last element the position must be an end boundary, or the
   match fails at this start. Starts are tried left to right; the
   `occurrence`-th match counts, and a second match is searched from the
   end of the first. The value is a JSON string of exactly the matched
   bytes. Possessive means a shape such as `[upper|digit]{2,3}`
   followed by `[digit]{1,4}` does not match `AB1`: the first element
   takes all three bytes. Authors bound each class to what it must take;
   for a flight number:

```json
{ "token_matching": { "source": "body", "shape": [
    { "repeat": { "class": ["upper", "digit"], "min": 2, "max": 2 } },
    "optional_space",
    { "repeat": { "class": ["digit"], "min": 1, "max": 4 } },
    { "repeat": { "class": ["upper"], "min": 0, "max": 1 } } ] } }
```

3. **`date{source, format, months, occurrence}`.** `format` is one of the
   closed set below. `D` and `M` are a maximal run of 1 or 2 ASCII digits
   (a run of 3 or more fails the match at this start), `DD` and `MM`
   exactly 2 digits, `YYYY` exactly 4 digits, `Mon` one name of the named
   month table, and a space is exactly one ASCII space.

| `format` | Pattern | Examples | Layouts covered (`D` and `M` are 1 or 2 digits) |
|---|---|---|---|
| `ymd_dash` | `YYYY-MM-DD` | `2026-11-12` | YYYY-MM-DD |
| `dmy_name` | `D Mon YYYY`, optional `.` after `Mon` | `12 Nov 2026`, `2 November 2026`, `02 Nov. 2026` | every combination of any name in the table (`Mon`) with and without the `.`: e.g. D Mon YYYY, D Mon. YYYY |
| `mdy_name` | `Mon D YYYY`, optional `.` after `Mon`, optional `,` directly after `D` | `Nov 12, 2026`, `November 2 2026` | every combination of any name in the table (`Mon`), with and without the `.`, and with and without the `,`: e.g. Mon D YYYY, Mon. D, YYYY |
| `dmy_slash` | `D/M/YYYY` | `12/11/2026`, `2/3/2026` | D/M/YYYY |
| `mdy_slash` | `M/D/YYYY` | `11/12/2026` | M/D/YYYY |
| `dmy_dot` | `D.M.YYYY` | `12.11.2026` | D.M.YYYY |

   A match starts at a start boundary and ends at an end boundary; starts
   are tried left to right and the `occurrence`-th match counts. A month
   name matches when the text's bytes equal the name under ASCII folding
   (other bytes exactly) and the byte after it is the pattern's next byte
   (space, or `.`), so `Mar` and `March` in one table never both match at
   one position. The matched date is **calendar-valid** when its month is
   1 to 12 and its day is 1 to the month's length in the proleptic
   Gregorian calendar (February has 29 days in a year divisible by 4 and
   not by 100, or divisible by 400). A calendar-invalid match makes the
   rule not apply; later matches are not tried. A valid date yields the
   JSON string `YYYY-MM-DD` with the month and day zero-padded, which 038
   3.4 then validates as a `date` value, so a valid date before
   1970-01-01 leaves the field absent `fallback_invalid` (038 3.4.4,
   3.5.3). `months` names a table in `month_tables` and is required for
   `dmy_name` and `mdy_name` and refused for the other formats. No month
   name is built in. `date{format: ymd_dash}` differs from /1's
   `date_iso`, which keeps its meaning: `date_iso` matches without
   boundaries and leaves a calendar-invalid date to 3.4
   (`fallback_invalid`).
4. **Value types.** `token_after` and `token_matching` fill a `text`
   field or a `text` record subfield; `date` fills a `date` field or
   subfield; `vocabulary_after` fills a `text` field or subfield, or an
   `enum{vocabulary}` field or subfield whose vocabulary is the one it
   names. None of them may be a rule value of a `list` field (038 3.7.3
   keeps list rules `literal`). Each produced value is validated by 038
   3.4 like every fallback value; a failing one leaves the field absent
   `fallback_invalid` (038 3.5.3), for example a token longer than the
   field's `max_bytes`.
5. **`vocabulary_after{source, terms, vocabulary}`.** After the term
   occurrence and the skip of 3.2.2, the token is the maximal run of ASCII
   letters and digits. The rule applies when the run is 1 to 64 bytes and
   equals, byte for byte, an option of the extract step's vocabulary named
   `vocabulary` (038 3.3.2: a literal, a profile parameter or an input);
   no folding is applied. An input vocabulary that is unavailable at
   evaluation (038 3.3.2) makes the rule not apply; when the field then
   ends with no applying rule and at least one rule was skipped for that
   reason, its absence reason is `vocabulary_unavailable` (038 3.3.2 and 3.4.2's
   existing reason, already in evidence 2's field-absence enumeration and
   `rustev_contract::extraction`, so no schema changes) instead of
   `fallback_no_match`. Its value's grade is 3.8's.

### 3.4 Exact-value conditions

1. `equals{source, value}` holds when the source's value equals `value`;
   `one_of{source, values}` when it equals any of `values`.
2. **Source.** A program source read by `equals` or `one_of` must be an
   input, or a record field of an input, whose declared accepted
   provenance excludes `model-derived`, of type `text`, `integer`, `bool`
   or `enum{options}` (not nullable). Otherwise the compiler refuses,
   category `invalid_extraction`. Each `value` must be of the source's
   type (a string for `text` and `enum`, an integer for `integer`, a
   boolean for `bool`), and for an `enum` source one of its declared
   options; otherwise `invalid_extraction`. Such a source need not be in
   the step's `projection`, because only the core reads it; a source
   read by any text condition or text extractor must be, as 038 3.7.2
   requires.
3. **Comparison.** The core value is compared exactly: text and enum by
   bytes with no folding, trimming or normalization; integers by value;
   booleans by value.
4. **Reads.** A condition source is a source read under 038 3.5.2 and
   enters the field's lineage, whether or not its condition held, as /1's
   condition reads do.
5. **An unreadable source.** An unresolved condition source (missing,
   stale, conflicting or invalid), or one withheld from exact steps by
   screening (035), ends the evaluation of the field that reached it: the
   field is absent `source_unavailable` (038 3.7.5), no later rule of that
   field is tried, and the condition is never evaluated as false, so a
   `not` around it cannot turn it true. This is how /1 already treats an
   unreadable text source.

### 3.5 Binding (amends 038 3.7.2)

At compile time, in addition to 038 3.7.2's checks, each refused with
category `invalid_extraction`:

1. a source read by a text condition or a text extractor is not in the
   step's `projection`, or its type is not `text`, for a /2 extractor (a
   /1 construct keeps /1's binding);
2. a condition source breaks 3.4.2;
3. an extractor's value type does not fit its field or subfield (3.3.4);
4. a `date` names a month table the program does not declare, omits
   `months` for a name format, or gives it for another format;
5. a `vocabulary_after` names a vocabulary the step does not declare, or
   fills an `enum` field whose vocabulary is another one;
6. `required` (3.7.1) breaks its bounds.

### 3.6 Evaluation bounds and determinism

1. Evaluation keeps 038 3.7.3's order (first applying rule for scalar and
   record fields, rule-order accumulation for list fields), its
   short-circuiting and its reads, and 038 3.7.4's bounds, including that
   a source over `max_projection_bytes` is not read.
2. **Worst-case work per source byte**, with `n` the source's length:
   term search as /1 (at most 64 terms of 64 bytes at each position);
   `token_after` and `vocabulary_after` then examine at most `n` bytes
   once, and `vocabulary_after` compares one token of at most 64 bytes
   with at most 4096 options; `token_matching` examines at most 257 bytes
   per start (the shape's longest match, at most 16 elements of 16 bytes,
   plus the end-boundary byte), each tested against at most 8 class
   entries, so at most `257 n` byte examinations; `date` examines at most
   10 bytes per start for a numeric format and at most 12 plus 48 names
   of 16 bytes per start for a name format. No step depends on more than
   the bounded state above; there is no backtracking.
3. The evaluator uses integer and byte operations only: no regular
   expression engine, no Unicode table, no locale data, no floating
   point, and no dependency the core does not already have. Its results
   are byte-identical on every target, including `wasm32` (038 3.7.6).

### 3.7 Rules first

1. **Declaration (amends 038 3.3.3).** `fallback.fill` gains
   `rules_first`, and `fallback` gains the optional member `required`: 1
   to 32 distinct names of declared top-level fields, allowed only with
   `rules_first` (`invalid_extraction` otherwise). Omitted, every declared
   top-level field is required. For example:

```json
"fallback": { "rules_program": "acme-booking@2", "fill": "rules_first",
              "required": ["confirmation", "flight", "depart_on"] }
```

2. **Evaluation before listing (amends 038 3.3.5).** When 038 would list
   the step's request in `pending()`, the core first evaluates the rules
   program over the step's sources (3.6, 038 3.7.5) and validates each
   produced value (038 3.4). Rules-filled fields are present with origin
   `fallback` and the derivation of 038 3.5.2.
3. **Skipped request.** When every required field is present from the
   rules, no request is listed. The step's evidence records the request
   outcome `skipped{reason: rules_filled}`; there is no attempt, no cost
   reservation, no step deadline and no captured output. Every field the
   rules left absent keeps its rules absence reason (`fallback_no_match`,
   `fallback_invalid`, `source_unavailable`, `vocabulary_unavailable`).
   Compile-time accounting is unchanged: the request still counts against
   `max_semantic_requests`, the worst-case attempts, `max_projection_bytes`
   and the backend's input limit (038 3.3.4), so skipping never makes a
   definition admissible that would otherwise be refused.
4. **Partial request.** Otherwise the core lists one
   `rustev.extract-request/1` whose `output` is the subset of the declared
   schema holding exactly the fields the rules left absent, required or
   not, in declared order, and whose `schema_digest` is computed over that
   subset (038 3.3.5). The other members are as 038 states (all declared
   vocabularies, in declared order). The document's shape and schema name
   are unchanged, and its `RequestId` covers the subset through its
   members; when the rules filled nothing the request is byte-identical to
   the one `absent_fields` lists.
5. **Merge.** The output is checked against the requested subset (038
   3.4.2): a member naming a field that was not requested, including a
   rules-filled field, is a record-level `invalid_output` fault and
   follows the retry and fallback policy. Requested fields take the
   model's validated values or absence reasons (`extracted`, or `absent`
   with 038 3.4.3's reasons, `request_failed{reason}` on a request
   failure, `screen_blocked` or `screen_unavailable` under 038 3.11.4). A
   rules-filled field is never requested and never replaced. The rules are
   not evaluated again after the request: there is no second fallback
   pass.
6. **Runtime (amends 038 3.11).** A skipped request is never admitted,
   reserved, dispatched, cached or captured; the decision continues. A
   listed partial request goes through the existing driver unchanged
   (038 3.11.1 to 3.11.5). Replay (038 3.11.6) re-evaluates the rules, so
   it lists the same subset and `RequestId` and supplies the captured
   record, or none for a skipped request.
7. The `fill` mode and `required` enter the plan 3 fallback detail and so
   the `PlanId` (038 3.14.4).

### 3.8 Grade of vocabulary capture

This section amends 032 3.2.1 and 3.9.3.

1. 032 3.2.1's `hint` gains a third kind: a token that a rules program
   captured because it is a member of a declared vocabulary. A
   vocabulary match says the text contains a known code, not that the
   code names what the field means.
2. **Dynamic.** Every present value of an extract field (top-level, and
   every subfield of a record field) that any `vocabulary_after` rule of
   the program can fill carries value metadata with grade `hint`,
   whatever its origin, so a model value in the same field is `hint` too
   and the static and dynamic grades agree. Its provenance and sources are
   the combination of what it read (032 3.2.2); its basis is `computed`.
   Assigning `hint` to a value that would otherwise be `ungraded` is not
   a raise (032 3.2.3: `ungraded` is not a point of the order), as for
   036's constructed keys.
3. **Static.** In phase e2 an extract step's tree, today one `ungraded`
   leaf, gives each such field the ceiling `hint`; other fields keep
   `ungraded`. An accept step over such a field takes its ceiling (032
   3.9.3 as 038 implements it). A requirement or hard gate reading it is
   therefore refused with category 14 `uncorroborated_hint` unless a
   corroboration step raises it (032 3.7, 3.9.3), and only a
   corroboration step can.

### 3.9 Evidence and judgment (amends 038 3.12)

1. The evidence 2 extraction member's request outcome gains
   `skipped{reason: rules_filled}` (3.7.3); `rules_filled` is the only
   reason in this version.
2. Field evidence is 038 3.12.1's. Rules-filled fields record origin
   `fallback` under every fill mode; the name is kept, and the step's
   `fill` in the plan says whether the rules ran first. A `hint` field's
   value metadata records its grade (3.8.2).
3. The judgment's `fallback` notice for a rules-filled field (038 3.12.3)
   is unchanged under `rules_first`.

### 3.10 Compatibility and identity

1. Every `rustev.extract-rules/1` document parses, binds and evaluates
   as before, with the same identity and results. A plan whose program is
   /1 and whose `fill` is `request_failure` or `absent_fields` has the same
   plan 3 bytes and `PlanId` (the `required` member is omitted when
   absent). Every committed definition, plan, evidence, judgment and run
   golden, including both reference plans, is byte-identical (038 3.14.2),
   and a golden check recompiles the /1 extraction goldens and compares
   bytes.
2. A /2 program's identity and canonical document enter the plan 3 step
   detail and the `PlanId` exactly as a /1 program's do (038 3.7.2,
   3.14.4); a change to any member of the program, including a month
   table, changes the `PlanId`.
3. New variants and members extend closed enums and optional members; no
   canonical name moves. An earlier release refuses a /2 program,
   `rules_first` or a `skipped` outcome as unknown (category 1 or parse),
   never reads them as something else.
4. The JSON Schemas of `rustev.extract-rules/2` (new), `rustev.definition/2`,
   `rustev.plan/3` and `rustev.evidence/2` regenerate; the TypeScript
   declarations of both bindings regenerate through `gen-types` (027).
   Adding members neither removes nor narrows an export (031 3.6.5).

### 3.11 WebAssembly budget

1. Measured on the optimized module in CI (027 3.10.6) with the pinned
   `wasm-bindgen` and `wasm-opt`: 4,822,101 bytes uncompressed (gzip
   1,612,354, recorded), merge-queue run 42514479928 over `main` at
   59cc7c32 with #569 (a Node-only change). Spec 057's gate is 5,242,880
   bytes, so the headroom is 420,779 bytes (about 411 KiB).
2. **Projection.** The /2 contract types with their serialization and
   the core evaluator are expected to add about 30 to 90 KiB, well within
   the headroom. 057's gate is kept; each increment's `bindings` run
   records its size.
3. **Pre-approved amendment of 027 3.10.4 (after 057).** If an increment
   of this spec measures more than 5,242,880 bytes, the owner's approval
   of 2026-11-08 (A-90) applies: that increment raises the gated
   uncompressed value in `tools/rustev-conformance/js/wasm-check.mjs` to
   6,291,456 bytes (6 MiB), recording the measured size here, and
   everything else of 027 3.10.4 as 057 left it is unchanged (gzip and
   Brotli recorded, not gated; instantiation budgets; procedure; spec
   050's exclusion). Otherwise the gate stays at 5,242,880 bytes.

### 3.12 What a match means

A rules match is evidence that this program produced this value from this
text, never evidence that the value is true. A supplier layout change can
make a template capture the wrong token while every check here passes, so
qualifying a rules program against real layouts belongs to the host's
evaluation set (spec 004's evaluation over the host's own cases), not to
Rustev. `exact-derived` states how a value was computed, not that it is
correct.

## 4. Out of scope

General regular expressions or backtracking shapes; Unicode classes,
case-insensitive classes or normalization; built-in month names or
locales; two-digit years, ordinal day suffixes, times and time zones;
skipping line breaks after a term; resolving free-text place names (the
host's); `vocabulary_after` over lists or with folding; a second rules
pass after the request; a quality claim for any program (R-04, R-37);
spec 005's rules backend; remote and Bedrock mappings, which carry the
partial request unchanged.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A /1 document with a `token_after` value or `month_tables` | Refused at parse, unknown member; never read as /2. |
| `token_after` with `max_len: 65`, or `min_len > max_len` | Refused at parse. |
| `charset` with 9 entries, or `{range: ["a", " "]}` | Refused at parse. |
| A shape of 17 elements, a `repeat` with `max: 17`, or `optional_space` first | Refused at parse. |
| A shape whose minimum length is 0 | Refused at parse. |
| A month table with 11 entries, or `Mar` and `MAR` in one table | Refused at parse. |
| `token_after` over `Ref: ABC1234567` with `max_len: 8` | Rule does not apply; never cut to 8 bytes. |
| `token_after` with term `Ref` over `Ref: abc12`, charset `upper`, `digit` | Skip takes `:` and the space; the run is empty at `a`; rule does not apply; nothing case-folded. |
| `token_after` with term `Ref` over `Ref: # ABC12` | Skip takes `:` and the space and stops at `#` (one separator only); the run is empty; rule does not apply. |
| `token_after` value of 12 bytes into `text{max_bytes: 10}` | Field absent `fallback_invalid`. |
| `token_matching` flight shape over `XBA123` | No match at offset 1 (not a start boundary). |
| `token_matching` flight shape over `BA12345` | No match (digit run takes 4, end is not a boundary). |
| `date{format: dmy_name}` over `31 Feb 2026` | Calendar-invalid; rule does not apply; next rule tried. |
| `date_iso` over `2026-02-31` | Unchanged /1 meaning: field absent `fallback_invalid`. |
| `date` over `12 Nov 1969` | Date produced; field absent `fallback_invalid` (038 3.4.4). |
| `date{format: dmy_name}` without `months`, or `months` naming an undeclared table | Refused `invalid_extraction`. |
| `date{format: dmy_slash}` with `months` | Refused `invalid_extraction`. |
| A `token_after` rule in a `list` field | Refused `invalid_extraction`. |
| `token_after` filling an `integer` or `enum` field | Refused `invalid_extraction`. |
| `equals` over an input accepting `model-derived` | Refused `invalid_extraction`. |
| `equals` with an integer value over a `text` source, or an `enum` value not among its options | Refused `invalid_extraction`. |
| `equals` source not in `projection` | Compiles (core-only read). |
| `token_after` source not in `projection` | Refused `invalid_extraction`. |
| `equals` over `Example.com` against value `example.com` | False; no folding. |
| `not: {equals}` over a missing input | Field absent `source_unavailable`; later rules not tried; never true. |
| `vocabulary_after` capturing `LHR` with vocabulary `[lhr]` | Rule does not apply (bytes differ). |
| `vocabulary_after` with an input vocabulary unavailable, no other rule applies | Field absent `vocabulary_unavailable`. |
| A hard gate reading an accept step over a `vocabulary_after` field | Refused, category 14 `uncorroborated_hint`. |
| The same field extracted by the model | Value metadata grade `hint`. |
| `required` with `fill: absent_fields` | Refused `invalid_extraction`. |
| `required` naming an undeclared field, or a duplicate | Refused `invalid_extraction`. |
| `rules_first`, every required field filled | No request listed; outcome `skipped{rules_filled}`; no attempt, no cost. |
| `rules_first`, rules fill 2 of 5 fields | One request whose output lists the other 3 in declared order; its digest over the subset. |
| `rules_first`, rules fill nothing | Request byte-identical to the `absent_fields` request. |
| Model returns a member for a rules-filled field | Record-level `invalid_output`; retry and fallback policy applies; rules value kept. |
| `rules_first` partial request fails | Requested fields `request_failed{reason}`; rules-filled fields kept; no second pass. |
| `rules_first` definition whose worst case exceeds `max_semantic_requests` | Refused, category 7, as without rules first. |
| Every committed golden recompiled | Byte-identical, same `PlanId`. |

## Increments

Each a separately reviewed pull request of about 1,000 changed lines or
fewer (generated files excluded), in this order:

1. **Contract.** The /2 document types with their parse-time bounds
   (3.1), `rules_first` and `required` (3.7.1), the plan 3 fallback
   detail, the `skipped` request outcome (3.9.1), the new and regenerated
   JSON Schemas and binding declarations (`gen-types`), and contract tests
   for round trips, refusals and unchanged bytes.
2. **Core.** The extractors and conditions (3.2 to 3.4), the binding
   checks (3.5), the bounds (3.6), the `hint` grade (3.8), goldens for a
   /2 program and its `PlanId`, and section 5's evaluation and refusal
   rows. It may land in two parts: (2a) `token_after`, `token_matching`,
   `date` and month tables; (2b) `equals`, `one_of`, `vocabulary_after`
   and its grade.
3. **Rules first.** Listing, skipping, the partial request and the merge
   in the core (3.7), evidence `skipped` (3.9), runtime tests (3.7.6),
   runtime conformance cases for a skipped and a partial request, 070 in
   `make verify` and in CI's `integrations-base` acceptance group, and
   closure.

Any increment whose `bindings` run crosses 5,242,880 bytes carries
3.11.3's gate change.

## Acceptance

- Contract: /2 documents round-trip canonically; every parse-time bound
  of 3.1.2 and section 5 is refused; a /1 document with a /2 construct is
  refused; regenerated schemas and declarations match their sources
  (`crates/rustev-contract/tests/schemas.rs`, `gen-types --check`).
- Core: every extractor's examples and section 5 rows; possessive shape
  matching; boundaries with non-ASCII bytes; every date format with valid,
  calendar-invalid and pre-1970 dates; exact-value conditions with each
  source type, including an unresolved source under `not`; binding
  refusals with category and phase; derivation and lineage including
  condition reads.
- Grade: a `vocabulary_after` field is `hint` statically and dynamically;
  a hard gate over it is refused with category 14; a corroboration step
  raises it.
- Rules first: skipped, partial and full requests with their outcomes,
  subsets, digests and `RequestId`s; the record-level fault on an
  unrequested member; request failure; replay listing the same subset; no
  reservation or attempt for a skipped request.
- Determinism: a native byte-identity test over every extractor stands in
  for `wasm32` parity, as 038's Verification records.
- Compatibility: every committed golden and reference `PlanId`
  byte-identical (I-1).
- The WebAssembly module within the gated budget (3.11).
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

## Decisions at approval

Fixed by the owner's design of 2026-11-08 and decided in this text, with
the reason beside each:

1. **A superset document.** `rustev.extract-rules/2` keeps every /1
   construct with its meaning, so /1 programs, plans and `PlanId`s keep
   their bytes (3.1, 3.10).
2. **Structured shapes, never regular expressions,** matched
   possessively (3.3.2), so the work per source byte is bounded and
   stated (3.6.2) and no Unicode table can change a `PlanId`'s meaning
   between releases (the issue).
3. **Declared month names.** Month names come from a table the program
   declares, compared with ASCII folding; no locale data is built in
   (3.3.3). The issue's "DD Mon YYYY" and "D Month YYYY" are one format,
   `dmy_name`, because the table holds both short and long names and the
   day takes 1 or 2 digits.
4. **A calendar-invalid date does not apply** in the new `date` extractor,
   so a later rule can try another format; `date_iso` keeps /1's meaning
   (3.3.3).
5. **Exact-value conditions read only non-model inputs,** compare
   exactly, and treat an unreadable source as /1 treats one: the field is
   absent `source_unavailable` and the condition is never false (3.4.5).
   Their sources need not be projected, because only the core reads them.
6. **Rules first lists only what is missing,** keeps
   `rustev.extract-request/1`'s shape with a subset schema, records a
   skipped request, and keeps compile-time budgets unchanged (3.7).
7. **Vocabulary capture is a `hint`** for every value of the field it can
   fill, whatever the origin, so static and dynamic grades agree and spec
   032's existing refusals apply (3.8).
8. **What a match means** (3.12): a rules match is evidence that this
   program produced this value from this text, never evidence that the
   value is true; qualifying a rules program belongs to the host's
   evaluation set.
9. **WebAssembly.** 057's 5 MiB gate is kept with the measured headroom;
   the owner pre-approved raising it to 6 MiB in the increment that
   crosses it, should one (3.11).

Ratified by the owner on 2026-11-08 (A-90: "we do need it in 0.7.0 ...
proceed with specs/build"), for release 0.7.0.

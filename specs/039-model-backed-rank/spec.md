---
id: "039-model-backed-rank"
title: "Model-backed rank and deterministic backfill (amends 002 and 005)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved specs 002 and 005 (R-16), for #76 child C2.
  Adds a `rank` step kind with three declared modes (listwise, pointwise,
  pairwise) over a bounded candidate pool, a distinct `ModelRanking` value
  kind that records partial rankings, dropped ids with reasons, scores
  scoped to one ranking request and pairwise win counts, optional
  identity-bearing position-bias controls driven by a named seeded
  generator, and a `select` step that fills k positions from a model
  ranking and, when it is short or its backend failed, from a declared
  exact ordering, with per-position origin and derivation. Pairwise
  schedules are deterministic and bounded by `max_comparisons`; cycles are
  resolved by win count, then an exact tie-break, and counted. The
  constructs join the #76 track versions (`rustev.definition/2`,
  `rustev.plan/3`, `rustev.evidence/2`); the backend descriptor gains an
  optional `ranking` member. The rules backend (spec 005) answers
  pointwise `rank` for the swap test. Bedrock (spec 030) admits its
  reserved `rank` value under 030 3.12. Jev (specs 012 and 022) keeps
  R-28: rankings over Jev answers stay composed in plans and feed `select`
  unchanged. Plans that use none of this keep byte-identical identities.
  Approved (A-36); implementation is in progress.
amends:
  - "002-decision-contract-and-pure-core"
  - "005-reference-backends"
extends:
  # `rank` and `select` step kinds, the descriptor `ranking` member, the
  # `ranking` backend output, the `selection` parameter value and the
  # evidence entries of 3.13.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Compilation, the request schedules, the permutation generator, the
  # ranking and selection values and their evaluation.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Pointwise `rank` tasks in the rules program (3.14).
  - { spec: "005-reference-backends", unit: { kind: directory, path: "backends/rustev-backend-rules/" }, nature: amending }
  # Exhaustive matches over the new output and parameter variants only.
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  # Adds 039 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Exhaustive matches and `ranking: None` initializers for the new step
  # kinds and descriptor member; regenerated binding types.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "028-opentelemetry-evidence-sink", unit: { kind: directory, path: "integrations/rustev-otel/" }, nature: additive }
  - { spec: "030-bedrock-backend", unit: { kind: directory, path: "integrations/rustev-bedrock/" }, nature: additive }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: additive }
  # The new source file in the published contract crate's archive list.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "036-mid-plan-acquisition"
  - "037-generation-steps"
  - "038-structured-extraction"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A `ModelRanking` and a `Selection` are distinct value kinds with no conversion to or from scores, distributions, calibrated probabilities, labels or ordinal levels; no probability threshold, label condition, expectation or `weighted_rank@1` component reads them, and a ranking score is compared only with scores of the same ranking request."
    anchor: "3-9-the-model-ranking-value"
  - id: "I-2"
    kind: invariant
    text: "Every position of a selection carries its origin (`model` or `backfill`), its source position and its derivation class; a backfill position comes only from a declared ordering whose static class is exact-derived, never repeats a selected id, and never introduces an id outside the ranked candidate pool."
    anchor: "3-10-the-select-step-and-deterministic-backfill"
  - id: "I-3"
    kind: invariant
    text: "A model ranking whose backend failed is never treated as an empty or partial ranking: `select` either fills every position from its backfill with a `fallback` notice (`backfill_all`) or is unresolved with the failure's reason (`propagate`), and evidence reasons (missing, stale, conflicting, invalid) always propagate."
    anchor: "3-10-the-select-step-and-deterministic-backfill"
  - id: "I-4"
    kind: invariant
    text: "A pairwise ranking issues its comparisons in the schedule's deterministic order, never more than `max_comparisons`, and the same answers yield the same order, win counts and cycle count on every run."
    anchor: "3-7-pairwise-mode"
  - id: "I-5"
    kind: invariant
    text: "Position-bias controls are off unless declared; when declared, their kind, order count and seed enter the plan identity, and every presented order is a pure function of the seed, the step, the instance and the request index through `rustev.permute/1`."
    anchor: "3-8-position-bias-controls"
  - id: "I-6"
    kind: invariant
    text: "A definition that uses no construct of this spec, and a descriptor without a `ranking` member, keep byte-identical documents, `DescriptorId`s and `PlanId`s, proven by the existing goldens."
    anchor: "3-18-compatibility"
  - id: "R-1"
    kind: requirement
    text: "Listwise answers drop unknown ids, ids outside the pool and duplicates with recorded reasons; a list shorter than the pool is a partial ranking, not an error."
    anchor: "3-5-listwise-mode"
  - id: "R-2"
    kind: requirement
    text: "The rules backend answers pointwise `rank` tasks deterministically, so the same plan can be run with the rules backend and with a model backend with no change to the policy, the outcomes or any authority path."
    anchor: "3-14-rules-backend-pointwise-rank-amends-005"
---

# 039: Model-backed rank and deterministic backfill (amends 002 and 005)

Approved by the owner (A-36, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is in progress. It implements #76 child C2 (#67). The approved texts of specs
002 and 005 are not edited; this spec records what changes. Spec 030's
reserved `rank` value is admitted under 030 3.12, which anticipates exactly
this change, so 030 is not amended. Approval authorizes paid Bedrock smoke
calls for `rank` only within the work order's scope and R-29's cap (3.15),
and authorizes no release, publication, real-user data or quality claim.

## 1. Purpose

Selection is often "pick the best k of n candidates for this request". A
model reads the request against each candidate well; code guarantees
constraints and always returns k. Hosts usually let a model choose and
fill gaps in code, with nothing recording which positions came from where.
Rustev's `rank` operation exists (spec 002, 3.9.1), but only as one request
returning a full score map, no backend declares it, and nothing completes a
short or failed ranking visibly. This spec adds:

1. a `rank` step with listwise, pointwise and pairwise modes over a bounded
   candidate pool, validated into a distinct ranking value that keeps
   partial results and the reasons for every dropped id;
2. declared, identity-bearing position-bias controls;
3. a `select` step that fills k positions from a ranking and, when needed,
   from an exact ordering, recording each position's origin;
4. the backends to exercise it: Bedrock (spec 030) natively, the rules
   backend (spec 005) pointwise for deterministic comparison, and Jev
   (specs 012, 022) through the composition R-28 already prescribes.

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner spec 002): the `rank` and `select` step
  declarations in `rustev.definition/2`, their plan details in
  `rustev.plan/3`, the descriptor's optional `ranking` member, the
  `ranking` variant of `RawOutput`, the `selection` variant of `OutValue`,
  the evidence entries of 3.13, and one refusal category (3.12).
- `crates/rustev-core/` (owner spec 002): compilation of both step kinds,
  the request schedules of 3.5 to 3.7, the generator `rustev.permute/1`,
  the `ModelRanking` and `Selection` value kinds, `select` evaluation and
  derivation.
- `backends/rustev-backend-rules/` (owner spec 005): pointwise `rank`
  tasks (3.14).
- `crates/rustev-runtime/`, `crates/rustev-eval/`, `crates/rustev-cli/`
  (additive): exhaustive matches over the new variants only; no behavior
  changes. The runtime already re-reads `pending()` after every supply
  (spec 003), which pairwise rounds rely on.
- `integrations/rustev-bedrock/`: the `rank` admission of 3.15 is a change
  to that crate made under spec 030 3.12, as that section provides.
- `Makefile` (owner spec 001): 039 joins `make verify` at completion.

**Amendment of spec 002.** Spec 002 3.10.4 (an exact step reading an
unresolved value is unresolved) applies to `select` except for the backend
failure reasons of 3.10.3, which `select` handles by its declared policy;
every other unresolved reason of `from` propagates as 3.10.4 states. Spec
002 3.9.3 gains the rows of 3.12. Nothing else in 002 changes for version 1
documents.

**Amendment of spec 005.** Spec 005 3.1.1 and 3.3 ("`rank` is refused")
change for pointwise `rank` tasks only (3.14). The rules backend still
returns `logits` for every other task.

**What does not change.** No new dependency, no I/O, clock or randomness
in the core, no change to spec 003's runtime behavior, spec 009's
protocol, spec 012's or 022's Jev adapter, spec 004's evaluation documents,
or `rustev.exact/1` and `/2` (no exact operator is added). Spec 037 (#76
B5) bounds model steps that produce text; a `rank` step produces no text
and needs nothing from it.

### 2.1 Binding principles of #76

- **Proposals, never grants.** A ranking and a selection are step values.
  A selection may be a proposal parameter like a shortlist; neither is
  authorization, and no `Permitted` type is defined.
- **Derivation classes and lineage.** A ranking is `model-derived`. A
  selection is `mixed-derived` and carries per position the class and
  lineage of the value that supplied it (3.11).
- **Unresolved, never a default; no silent kind conversion.** A failed
  backend is never an empty ranking; a short ranking is visible as partial;
  evidence reasons propagate through `select`; rankings, scores and
  selections never become probabilities, labels or each other (3.9).
- **Pure core.** Schedules, permutations, aggregation and backfill are pure
  functions in `rustev-core`; the seed is plan data, not entropy.
- **New dependency classes only under `integrations/`.** None is added.
- **No dynamic plugin loading.** Backends are constructed by the host.
- **No identity, authority or knowledge store.** Candidates and their
  records come from the host's snapshot; Rustev stores nothing.
- **Plan identities.** Plans that use none of this keep byte-identical
  `PlanId`s (3.18).

## 3. Behavior

### 3.1 Documents and versions

Under the #76 coordination rule, the constructs of this spec are added to
the shared track versions, with members omitted when unused:

- adds to `rustev.definition/2` (introduced by spec 032): the step kinds
  `rank` (3.2) and `select` (3.10);
- adds to `rustev.plan/3` (introduced by spec 032): the plan details of
  3.2.4 and 3.10.6;
- adds to `rustev.evidence/2` (introduced by spec 032): the ranking and
  selection entries of 3.13;
- `rustev.replay/2` (introduced by spec 036) and `rustev.run/3` are used
  unchanged (3.13.3);
- `rustev.backend/1`: adds the optional member `ranking` (3.3), omitted
  when absent, so existing descriptors keep their bytes and
  `DescriptorId`s.
- `rustev.backend-output/1`: adds the variant `ranking` (3.5.2); no
  existing document changes.
- `rustev.judgment/1`: adds the parameter variant `selection` (3.10.7); no
  existing document changes.

A definition that uses no #76 construct still compiles to `rustev.plan/2`
with a byte-identical `PlanId` (3.18).

A `rank` or `select` step in a version 1 definition is refused, category 11
`invalid_definition`, detail `requires-definition-2`.

### 3.2 The `rank` step

1. **Declaration.** A step body `rank` with every member required:

   ```json
   { "rank": {
       "task": "catalogue-fit", "question": "Which items best fit the request?",
       "candidates": "step:eligible",
       "items": { "list": { "list": "input:catalogue", "id_field": "id" } },
       "bind": "candidate",
       "project": ["input:request"],
       "mode": { "listwise": { "scores": "optional" } },
       "bias": "none",
       "tie_break": "id_ascending",
       "backend": { "min_determinism": "unspecified", "artifact": "any", "binding": "auto" },
       "fallback": "none" } }
   ```

   - `candidates`: a step whose value is an id list, a filter result
     (its eligible ids) or a shortlist. Its order is the **pool order**;
     its ids are the **pool**. Protected constraints are applied there,
     before ranking.
   - `items`: an item source as for `for_each` (spec 002) or `none`;
     `bind` names how a candidate's item appears in projections.
   - `mode`: `{"listwise": {"scores": "none" | "optional" | "required"}}`,
     `"pointwise"`, or `{"pairwise": {"schedule": "round_robin@1" |
     "merge_sort@1", "max_comparisons": 1..4096}}`.
   - `bias`: `"none"`, `{"shuffle": {"seed"}}` or
     `{"permutation_average": {"orders", "seed"}}` (3.8).
   - `tie_break`: `"id_ascending"` or `{"order_of": "step:<id>"}`, an
     ordered id list of static class `exact-derived`; ids it does not hold
     follow, by id ascending.
   - `fallback`: `none` or `unsupported`; a kind fallback is refused
     (category 11).
2. **No fan-out.** A `rank` step has no `for_each`; its instance key is
   empty, and one evaluation holds one **ranking request** per `rank` step.
3. **Static bounds.** The pool bound is the candidate step's static id
   bound (the bound phase g already uses for fan-out), at most 1 024.
   `max_requests` is 3.5.4, 3.6.3 or 3.7.5 for the mode and counts against
   `max_semantic_requests` under spec 002 3.9.5 category 7.
4. **Plan detail.** `rustev.plan/3` records for a `rank` step its bound
   backend (id, artifact, `DescriptorId`), mode with its parameters, bias
   with kind, orders and seed, the generator name `rustev.permute/1`, the
   tie-break, the pool bound and `max_requests`. All of it is in `PlanId`.

### 3.3 The descriptor `ranking` member

`ranking`, when present, is:

```json
{ "modes": ["listwise", "pointwise", "pairwise"],
  "max_candidates": 50,
  "listwise_scores": "never" | "always" | "optional",
  "listwise_partial": "possible" | "never" }
```

`modes` is a non-empty, duplicate-free subset in that order;
`max_candidates` (1 to 1 024) is the most candidates one request carries
(1 suffices for pointwise and 2 for pairwise). Projection limits and the
determinism tier are the descriptor's existing `input` limit and
`determinism`. The existing `operations` entry for `rank` with `scores`
keeps its spec 002 meaning and serves only `semantic` rank steps.

### 3.4 Requests and projections

Every request of a `rank` step is an ordinary pending request (spec 002,
3.10.1) whose projection is the canonical envelope of spec 002 with
`operation: "rank"` and two members added for this step kind:

- `mode`: `listwise`, `pointwise` or `pairwise`;
- `presented`: the presented candidates in presentation order, each
  `{id, item}` with `item` the bound record or `null` when `items` is
  `none`.

`candidates` holds the presented ids in the same order; `options` is `[]`
except for pairwise, `["first", "second"]`. Request instance keys extend the
empty step instance with one element: `perm/<j>` (listwise), `cand/<id>`
(pointwise) or `pair/<c>/<o>` (pairwise). In a pairwise key, `<c>` is the
**comparison key**: the pair's index from 0 in the `round_robin@1` order,
or `<w>.<m>.<n>` for `merge_sort@1` (merge level w from 0, merge index m
from 0 within that level, comparison number n from 0 within that merge),
all in decimal; `<o>` is the presentation index, `0` for the first or only
presentation and `1` for the reversed presentation of 3.8.3. Every request
of a step therefore has a distinct key. **Comparison order** is ascending
`<c>` (numeric, or lexicographic over `(w, m, n)`), then `<o>`. Request
identity, capture and replay treat them as any request (3.13). An empty
pool issues no request and yields an empty, complete ranking.

### 3.5 Listwise mode

1. **Request.** One request per presented order (`orders`, 1 without
   averaging), each presenting the whole pool. The pool bound must not
   exceed the backend's `max_candidates`.
2. **Output.** `RawOutput::Ranking{ids: [string], scores: none |
   [number]}` in `rustev.backend-output/1`. `scores`, when present, has one
   finite value per returned id, in the same order; otherwise the whole
   output is `invalid_backend_output`. `scores: required` in the step
   requires them and `none` refuses them (`invalid_backend_output`);
   `optional` accepts either.
3. **Validation.** Ids are read in returned order. An id not in the pool is
   dropped as `outside_pool` when the candidate step lists it among its
   excluded or truncated ids, and as `unknown_id` otherwise; a repeated id
   is dropped as `duplicate` (first occurrence kept). The remaining ids, in
   returned order, are the entries; positions are 1-based. Pool ids not
   returned are `unranked`. A ranking with unranked ids is **partial**; it
   is a result, not an error. Scores never reorder entries; a score that
   disagrees with list order is kept as received, and the count of adjacent
   disagreements is recorded.
4. **Count.** `max_requests` = `orders`.
5. **Scores** are `ModelScore`s scoped to the ranking request (plan, step,
   instance; spec 002 3.5.3), the issue's `Scores{scope: request}`. Scores
   of different ranking requests are never compared.

### 3.6 Pointwise mode

1. **Requests.** One per pool id in pool order, presenting that candidate
   alone. A backend's batch capability (spec 017, spec 020) may dispatch
   them together; nothing here changes batching.
2. **Output.** `RawOutput::Scores` keyed by exactly the presented id, with
   one finite value. Any other key set is `invalid_backend_output` for that
   candidate.
3. **Count.** `max_requests` = pool bound.
4. **Order.** Entries by score descending, ties by `tie_break`. A
   candidate whose request failed (any supplied failure or invalid output)
   is dropped with `failed{reason}`; the ranking is then partial. If the
   pool is non-empty and every candidate failed, the step is unresolved
   with the first failure's reason in pool order.
5. **Scope.** All pointwise scores of one ranking share its scope: they
   answer one question under one binding, which declares one scale for its
   pointwise mode. They are never compared with scores of another step,
   instance or plan.

### 3.7 Pairwise mode

1. **Requests.** Each comparison presents two candidates and expects
   `RawOutput::Label` of `first` or `second`; any other answer is
   `invalid_backend_output`.
2. **`round_robin@1`.** Every unordered pair of the pool, in lexicographic
   order of pool positions, as one round: all comparisons are pending at
   once, and under `permutation_average` both presentations of every pair
   (3.8.3) are pending together. Order: wins descending, then `tie_break`.
   Every **cyclic triple** (a beats b, b beats c, c beats a) among answered
   pairs is counted and recorded; the order is the win-count resolution,
   never an arbitrary choice.
3. **`merge_sort@1`.** Bottom-up merge sort over the pool order (after the
   shuffle of 3.8): width 1, 2, 4, ...; within one width the merges of
   adjacent runs proceed in parallel, and each merge has at most one
   comparison pending, issued after the previous comparison is answered
   (under `permutation_average`, after both of its presentations are). The
   winner of each comparison is placed first. Each pair is compared at most
   once, so no cycle is observable; the cycle count is recorded as
   `not_observable`. Win counts are recorded.
4. **Failures.** A failed or invalid comparison makes the step unresolved
   with the first failure's reason in comparison order; no comparison is
   ever replaced by a tie-break.
5. **Bound.** The worst case for pool bound n is `n(n-1)/2` for
   `round_robin@1` and, for `merge_sort@1`, the sum over every merge of
   `|left| + |right| - 1`, computed exactly for n; both are multiplied by
   `orders` (3.8). A worst case above `max_comparisons` is refused at
   compilation (category 7). The core never issues comparison
   `max_comparisons + 1`; if a defect would, the step is unresolved
   `budget_exhausted{comparisons}`.
6. **Determinism.** The comparison schedule is a pure function of the
   pool, the seed and the supplied answers, so the same inputs yield the
   same comparisons in the same order on every run and in replay.

### 3.8 Position-bias controls

1. **Off by default.** `bias: "none"` presents pool order, and for
   pairwise presents the earlier pool member first.
2. **`shuffle{seed}`.** Listwise presents a permutation of the pool;
   `merge_sort@1` starts from a permutation; pairwise chooses each
   comparison's presentation order. `seed` is 1 to 64 bytes of
   `[A-Za-z0-9._-]`.
3. **`permutation_average{orders, seed}`.** Listwise: `orders` 2 to 8
   requests, request j presenting permutation j. The aggregate is Borda: an
   entry at position p of a list of length l scores `l - p + 1` points;
   entries are ordered by total points descending, then `tie_break`; ids
   returned by no request are unranked. Scores of different requests are
   never combined; entries carry points, not scores, and each request's
   scores stay in its evidence. Any failed request makes the step
   unresolved with the first failure's reason. Pairwise: `orders` must be
   2; each pair is asked in both presentation orders, presentation index 0
   being the order 3.8.1 or 3.8.2 gives and index 1 its reverse (3.4); both
   count against `max_comparisons`; a pair wins only when
   both answers agree, and a disagreement gives neither a win and is
   counted as an inconsistent pair. For `merge_sort@1` a disagreement is
   decided by `tie_break` and counted. Pointwise refuses any bias control
   (category 11): one candidate per request has no position.
4. **Generator `rustev.permute/1`.** Fisher-Yates from the last index down:
   for index i, v is the first 8 bytes, big-endian, of SHA-256 over
   `rustev.permute/1 || 0x00 || canonical JSON [seed, step, instance, j, i,
   attempt]`, built as spec 002 3.3.2 builds an identity: the 16 ASCII
   bytes of the tag with no length prefix or terminator, one 0x00 byte,
   then the canonical bytes (spec 002 3.3.1) of that array. With
   `m = floor(2^64 / (i + 1)) * (i + 1)` computed without overflow (so
   `m = 2^64` and nothing is rejected when `i + 1` is a power of two), a
   value `v >= m` is rejected and the draw repeats with the next attempt
   (attempt counts from 0); otherwise r in `0..=i` is `v mod (i + 1)`. A
   pairwise presentation bit uses the same construction with `i = 1` and
   `j` the comparison key of 3.4 as a string. Otherwise `j` is the
   permutation index: 0 under `shuffle` (one permutation, for listwise or
   the `merge_sort@1` start) and the request index 0 to `orders - 1` under
   listwise `permutation_average`; pairwise `permutation_average` draws no
   permutation (3.8.3). The `PlanId` is not an input, so swapping the
   backend keeps every presented order.
   Known-answer vectors are committed.
5. **Identity.** Kind, `orders` and `seed` are in the definition and so in
   `PlanId`; the evidence records the permutations used (3.13).

### 3.9 The model ranking value

1. `ModelRanking` is a distinct type in `rustev-core` with no `From` or
   `Into` to or from `ModelScore`, `Distribution`, `CalibratedProbability`,
   `SelectedLabel`, `OrdinalLevel` or `RankPosition`, and a compile-fail
   doctest for each.
2. It holds the mode, the pool in pool order, entries
   `{candidate, position, score: none | ModelScore, wins: none | integer,
   points: none | integer}`, `unranked` ids, `dropped` ids with reasons
   (`unknown_id`, `outside_pool`, `duplicate`, `failed{reason}`), `partial`,
   the count of score and order disagreements (listwise), and for pairwise
   `{issued, max_comparisons, cyclic_triples: integer | not_observable,
   inconsistent_pairs}`.
3. It is not a probability of being best and not a score of any other
   scope. Its only consumer is a `select` step's `from` (3.10). A policy
   condition, projection, `for_each`, exact operator or calibration that
   reads a `rank` step is refused, category 3 `kind_mismatch`.

### 3.10 The `select` step and deterministic backfill

1. **Declaration.** A step body `select`:

   ```json
   { "select": {
       "from": "step:model_ranking", "k": 5,
       "backfill": { "from": "step:exact_ranking", "exclude_selected": true },
       "on_backend_failure": "backfill_all" } }
   ```

   - `from`: a `rank` step, a `semantic` step of operation `rank`
     (ordered by score descending within its one scope, ties by id
     ascending), or a `weighted_rank@1` step (the composed ranking of
     3.16).
   - `k`: an integer 1 to 1 024.
   - `backfill`: `"none"` or `{from, exclude_selected: true}`, where
     `from` is a step whose value is an ordered id list (id list, filter
     result, shortlist or ranking) of static class `exact-derived`. A
     `weighted_rank@1` ranking qualifies when every component reads only
     `exact-derived` values (for example shortlist positions over an exact
     shortlist; spec 002 3.8 and 3.12.2); a `rank` step never does.
     `exclude_selected: false` is reserved and refused (category 11).
   - `on_backend_failure`: `backfill_all` or `propagate`; `backfill_all`
     without a backfill is refused (category 11).
2. **Pool.** The selection pool is the pool of `from`: the rank step's
   pool, the semantic rank step's candidates, or the `weighted_rank@1`
   candidates.
3. **Model failure.** `from` has **failed** when it is unresolved with
   `backend_unavailable`, `invalid_backend_output`, `budget_exhausted`,
   `deadline_exceeded` or `unsupported`, or when it is a `weighted_rank@1`
   ranking that is empty over a non-empty pool and excludes every candidate
   for one of those reasons (the first such reason is the failure). Any
   other unresolved reason of `from` (missing, stale, conflicting or
   invalid evidence, abstention) makes `select` unresolved with it, whatever
   the policy.
4. **Evaluation.**
   - `from` resolved: positions 1.. take its entries in order, up to k,
     origin `model`.
   - `from` failed and `propagate`: `select` is unresolved with the
     failure's reason.
   - `from` failed and `backfill_all`: no model positions; the judgment
     gains a `fallback` notice `{step, reason}` naming the failure.
   - While fewer than k positions are filled and a backfill is declared,
     the backfill ordering is walked in order: an id already selected is
     skipped (`skipped_selected`), an id outside the pool is skipped and
     counted (`skipped_outside_pool`), any other id fills the next position
     with origin `backfill`.
   - The backfill is read only when a position remains to fill. If it is
     then unresolved, `select` is unresolved with its reason; if not needed
     its state is recorded as `not_needed`.
   - Fewer than k filled positions after both sources is a **shortfall**,
     recorded as `k - filled`; it is a result, not an error.
5. **Value.** `Selection` is a distinct type (3.9.1 applies) holding k,
   entries `{position, id, origin: model | backfill, source_position,
   derivation}`, `shortfall`, `model: ranked{entries, partial} |
   failed{reason}`, and `backfill: none | not_needed | used{taken,
   skipped_selected, skipped_outside_pool} | unresolved{reason}`.
6. **Plan detail.** `from`, k, backfill and policy are definition data and
   so in `PlanId`.
7. **Consumers.** A selection reads as an ordered id list: `empty`,
   `nonempty`, `len`, `member` and `for_each` accept it, and it may be a
   proposal parameter, serialized as `OutValue::Selection` with every
   entry's origin, so evaluation can read per-position origin (3.17).
   Probability thresholds, `top_label`, expectations and `weighted_rank@1`
   components refuse it (category 3).
8. **Static reasons.** For spec 002 3.11.2, `select` can be unresolved
   with: every reason of `from` except the five failure reasons under
   `backfill_all`, or every reason of `from` under `propagate`; plus every
   reason of the backfill step.

### 3.11 Derivation, lineage and origin

1. A `rank` step value is `model-derived`; its lineage is its candidate
   step's lineage, the projected references and its tie-break step.
2. A `select` value is `mixed-derived` in the plan and at runtime, because
   it reads a model ranking even when every position is backfilled (spec
   002 3.12.3: never less derived than what it reads). Its lineage is the
   union of both sources.
3. Each position's `derivation` is the runtime class of the value that
   supplied it: the `from` value's class for `model` positions
   (`model-derived` for a rank step, `mixed-derived` for a composed
   ranking), the backfill step's class (`exact-derived`) for `backfill`
   positions. Lineage per position is that source's lineage.

### 3.12 Compilation and refusals

Spec 002 3.9.3 gains:

| Step kind | Satisfied by |
|---|---|
| `rank`, mode m | a descriptor whose `ranking.modes` contains m, with `max_candidates` at least the pool bound (listwise), 1 (pointwise) or 2 (pairwise), `listwise_scores` not `never` when the step requires scores and not `always` when it refuses them, and the existing determinism, artifact and input-limit checks |

Refusals, in spec 002's phases and order:

| Category | Phase | Refused when |
|---|---|---|
| 11 `invalid_definition` | b | a construct in a version 1 definition; a kind fallback; bias on pointwise; `orders` outside 2 to 8, or not 2 for pairwise; a bad seed; `k` or `max_comparisons` out of range; `exclude_selected: false`; `backfill_all` without a backfill; `candidates` not an id-producing step |
| 3 `kind_mismatch` | e | anything but `select.from` reads a `rank` step; `select.from` is not a ranking kind; a backfill or `order_of` step is not an ordered id list; a mass, label or expectation condition, a probability threshold, `top_label` or a `weighted_rank@1` component reads a selection (3.10.7) |
| 28 `exact_order_source` (new) | e | a backfill or `order_of` step's static class is not `exact-derived` |
| 5 `no_capable_backend` | f | no descriptor satisfies the mode; shortfalls `rank_mode`, `max_candidates`, `listwise_scores` |
| 7 `budget` | g | the pool bound exceeds 1 024; pairwise worst case exceeds `max_comparisons`; requests exceed `max_semantic_requests` without `truncate_visible` |

Category 28 is the next free number in merge order, after spec 036's 20
and 21, spec 037's 22 to 24 and the three categories spec 038 names
without numbers (`invalid_extraction`, `invalid_acceptance` and
`unaccepted_model_value`, 25 to 27 in the order its ownership list names
them). Spec 038 is on `main` and no spec that adds a category merges
between it and this one, so merge order fixes 25 to 27 for spec 038 and 28
here; a later spec takes 29 onward. Backend construction refusals (3.15.1)
are not categories. Under `truncate_visible`, requests not issued are
`budget_exhausted` failures under 3.5 to 3.7.

### 3.13 Evidence, capture and replay

1. `rustev.evidence/2` adds, for a `rank` step, the ranking of 3.9.2 with
   the presented order of every request (ids only) and the comparison
   answers in schedule order; for a `select` step, the selection of 3.10.5.
   Item records and the question are not copied; ids, positions, scores,
   counts and reasons are.
2. Bounds stay within `RECORD_V1`: at most 1 024 pool ids and 4 096
   comparisons per step, checked in phase g (category 7, detail
   `evidence-bound`).
3. Capture and replay use `rustev.replay/2` (introduced by spec 036),
   unchanged. Every rank request is a recorded request with its `RequestId`;
   historical reproduction supplies them in the recorded order, so
   pairwise rounds reproduce exactly. Until spec 036's implementation
   provides that replay version, a decision of a plan using these
   constructs with capture enabled is refused at submission with
   `capture_unsupported`, as spec 032 does, and the swap-test acceptance
   waits for it.

### 3.14 Rules backend pointwise rank (amends 005)

1. `rustev.rules/1` admits `operation: rank` for a task whose `options` are
   exactly `["score"]`. Conditions and contributions are as for other tasks
   and may read `bind:<name>` fields of the presented item.
2. Answer: base plus contributions for `score`, as a decimal, converted to
   the nearest `f64`, returned as `RawOutput::Scores{<presented id>:
   value}`. `on_no_match` applies unchanged.
3. Descriptor: when the program has a rank task, `ranking: {modes:
   ["pointwise"], max_candidates: 1, listwise_scores: "never",
   listwise_partial: "never"}`; `operations` does not list `rank`. A program
   without rank tasks keeps its descriptor bytes. Determinism `bitwise`.
4. A rank task in a listwise or pairwise request, or a request presenting
   other than one candidate, is refused as `capability` with no output.

### 3.15 Bedrock rank (admitted under spec 030 3.12)

1. `operations` admits `rank` with `mapping_version` `rank/1` (listwise
   ids without scores) or `rank-scored/1` (listwise ids with scores); both
   serve pointwise and pairwise. `max_options` is `max_candidates`, at
   most 100. A binding with `structured_output: none` and `rank` is
   inconsistent: construction refuses it, naming
   `rank_requires_structured_output`, and sends nothing (spec 030 3.2), so
   no descriptor exists and no plan compiles against it. It is a
   construction refusal of the binding, not a compile category of 3.12.
2. Schemas (`tool_use` or `json_schema`, spec 030 3.7.3): listwise
   `{ranking: [enum of presented ids]}`, plus `scores: [number]` under
   `rank-scored/1`; pointwise `{score: number}`; pairwise `{choice: first |
   second}`. Answers map to `RawOutput::Ranking`, `Scores` and `Label`; the
   adapter never repairs, reorders or completes an answer. A self-reported
   confidence is provider-reported only (030 3.7.4).
3. Descriptor: `ranking: {modes: [listwise, pointwise, pairwise],
   max_candidates, listwise_scores: never | always, listwise_partial:
   possible}`. Version 1 binding digests that do not use `rank` keep their
   bytes (030 3.12.2).
4. The fixtures 030 3.12.3 carries forward are acceptance here. Live smoke
   calls for `rank` follow 030 3.14.4: synthetic fixture content or spec
   023 admitted data only, reserved against the R-29 testing journal,
   refused unsent over the cap.

### 3.16 Jev and composed rankings (specs 012, 022)

R-28 stands: the Jev adapter declares no `rank` and synthesizes no scores,
and spec 022's production transport changes nothing about that. A plan
that ranks with Jev composes the ranking where it is visible, a fan-out
`rubric` or `proposition` answered by Jev plus `weighted_rank@1` (spec 012
3.3.2), and that ranking is a valid `select.from` (3.10.1), with failure
detected as 3.10.3 states. #67's "C1 or C3" dependency is met by Bedrock
natively and by Jev through composition; neither path changes the policy.

### 3.17 Evaluation

The per-position origin is evaluation input: a `selection` parameter
exposes origin per position, and spec 041 (#76 E1, drafted in parallel)
reports `backfill@p<n>` and the ranking metrics from it. This spec adds no
metric and no evaluation document. The swap test (Acceptance) runs one
plan with the rules backend and with Bedrock fixtures and compares the
spec 004 reports, and the E1 and E2 (spec 029) reports once those are
implemented.

### 3.18 Compatibility

1. Definitions without `rank` or `select`, descriptors without `ranking`,
   rules programs without rank tasks and Bedrock bindings without `rank`
   keep their bytes and identities; a test recompiles every committed
   golden and compares bytes.
2. The `semantic` rank step keeps its spec 002 meaning and bindings.
3. Version 1 readers refuse the new variants, so no older reader
   misreads a ranking output or a selection.

## 4. Out of scope

Implementation; top-k tournaments and other schedules (a later schedule is
a new versioned name); ties in pairwise answers; listwise requests larger
than one backend's `max_candidates` (split pools); `exclude_selected:
false`; a native Jev `rank` (R-28); training or tuning rankers; ranking
metrics themselves (spec 041); quality claims (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `rank` step in a version 1 definition | Category 11, `requires-definition-2`. |
| Policy rule reads a `rank` step | Category 3. |
| `mass_at_least` over a selection | Category 3. |
| Backfill from a shortlist over a model-derived input | Category 28 `exact_order_source`. |
| Listwise step, pool bound 60, backend `max_candidates` 50 | Category 5, shortfall `max_candidates`. |
| Pairwise `round_robin@1`, pool bound 10, `max_comparisons` 44 | Category 7 (worst case 45). |
| `bias: shuffle` on pointwise | Category 11. |
| Listwise answer `[b, b, zz, x, a]`, pool `[a, b, c]`, `x` excluded by the filter | Entries `b, a`; dropped `b` duplicate, `zz` unknown_id, `x` outside_pool; `c` unranked; partial. |
| Listwise `scores` of a different length | `invalid_backend_output`; step unresolved. |
| Pointwise, one candidate's backend times out | That candidate `failed{deadline_exceeded}`; partial ranking. |
| Pointwise, every candidate fails | Step unresolved with the first failure. |
| Round robin answers a>b, b>c, c>a | One cyclic triple counted; order by wins (all 1), then tie-break. |
| Pairwise `round_robin@1`, `permutation_average` orders 2, pool of 3 | Six requests `pair/0/0` to `pair/2/1`, all keys distinct, six counted against `max_comparisons`. |
| Merge sort, one comparison fails | Step unresolved; no tie-break replaces it. |
| `select` k 5, model returns 3 valid ids | Positions 1-3 `model`, 4-5 `backfill` from the exact ordering, skipping the 3. |
| Backfill ordering lists an id outside the pool | Skipped and counted; never selected. |
| Model backend fails, `backfill_all` | 5 `backfill` positions, `fallback` notice, class still `mixed-derived`. |
| Model backend fails, `propagate` | `select` unresolved with the failure reason. |
| Candidate input stale | `select` unresolved `stale_evidence`, even under `backfill_all`. |
| Model short, backfill exhausted | Shortfall recorded; no invented ids. |
| Same plan, seed changed | Different `PlanId`; presented orders change. |
| Same plan, backend swapped | Same presented orders; `PlanId` differs only by the binding. |
| Rules program with a listwise rank request | `capability`, no output. |
| Bedrock binding `rank` with `structured_output: none` | Construction refused, `rank_requires_structured_output`; no descriptor. |
| Every existing golden recompiled | Byte-identical, same `PlanId`. |

## Acceptance

- Fixtures with per-position origin asserted in each: full list, partial
  list, duplicates, unknown ids, ids outside the pool, backend failure
  under `backfill_all` and `propagate`, evidence failure, backfill
  exhausted, backfill unresolved when needed and when not needed.
- Pointwise, listwise with and without scores, and permutation averaging
  with Borda ties broken by each tie-break form.
- Pairwise: for pool sizes 0 to 12 and both schedules, the issued
  comparison count never exceeds `max_comparisons` or the computed worst
  case; an injected cycle is resolved by wins then tie-break and counted;
  the schedule and result are identical across runs and in replay for the
  same answers; both-order disagreements counted.
- `rustev.permute/1` known-answer vectors; permutations independent of the
  bound backend.
- Every refusal row of 3.12 with a passing neighbour; compile-fail
  doctests for 3.9.1.
- Derivation and lineage per position and per step (3.11).
- Rules backend pointwise rank tasks (3.14), including refusals.
- Bedrock `rank` fixtures on both wires for the three modes (3.15, 030
  3.12.3), with no network.
- Swap test: the same plan (pointwise rank, `select` with backfill) run
  with the rules backend and with Bedrock fixtures yields judgments whose
  policy trace and outcome kinds are produced by the same rules, and
  comparable spec 004 reports; E1 and E2 reports join when specs 041 and
  029 are implemented. No authority path differs.
- Compatibility goldens (3.18); `make code`, declared verification and
  `make gate` pass.

## Verification

Pending with implementation. The implementing change adds `039` to `make
verify` with repository-local deterministic tests for every acceptance
item; any live Bedrock smoke result is recorded with its journal total and
is not part of declared verification.

**Owner decision A-49 (2026-10-23): dependencies.** Specs 022 and 030 are
no longer declared dependencies of this spec; spec 012 remains one. Spec
022 changes nothing here: 3.16 states that its production transport
"changes nothing about" R-28's composed rankings. The Bedrock `rank`
admission of 3.15 becomes a later increment of this spec: a change to
`rustev-bedrock` made under spec 030 3.12 and 3.15 (as Decision 15 and
spec 035 3.16 already frame it), only after spec 030 is complete, with
3.15 binding it unchanged. These items move to that increment and are its
acceptance, not acceptance of the earlier increments:

- section 5's row "Bedrock binding `rank` with `structured_output: none`",
  refused `rank_requires_structured_output` with no descriptor;
- the acceptance item "Bedrock `rank` fixtures on both wires for the three
  modes (3.15, 030 3.12.3), with no network";
- the swap test, run with the rules backend and with Bedrock fixtures;
- 3.18 item 1 for Bedrock bindings without `rank`;
- any paid live Bedrock `rank` smoke call (Decision 18).

This spec's implementation is complete when every other acceptance item
is met, with the rules backend (3.14) and Jev composition (3.16) as the
backends exercising it; the Bedrock increment then lands as its own
change, as spec 035's `screen` admission does. No boundary, invariant,
value-kind rule or refusal changes.

## Implementation record

Planned increments, each a separately reviewed pull request: (1) the
contract vocabulary: the `rank` and `select` declarations and the
descriptor `ranking` member; (2) the remaining contract members: the plan 3
details of 3.2.4 and 3.10.6, the `ranking` backend output, the `selection`
parameter, the evidence entries of 3.13 and category 28; (3) requests,
schedules and `rustev.permute/1` in the core; (4) the model ranking value,
`select` and backfill; (5) compilation and refusals; (6) the rules backend
and Bedrock; (7) Jev composition, declared verification and closure.

**Increment 1 (2026-10-26): the contract vocabulary.**
`crates/rustev-contract/src/ranking.rs` adds the `rank` step body of 3.2.1
and the `select` step body of 3.10.1, reusing the item source and backend
requirement types, with the document-local refusals of 3.12's category 11
row that need no other step (ranges of `k`, `max_comparisons` and
`orders`, bias on pointwise, `orders` other than 2 for pairwise, seeds,
`exclude_selected: false`, `backfill_all` without a backfill, kind
fallbacks). Both kinds are version 2 constructs: in a
`rustev.definition/1` they are refused, category 11
`requires-definition-2`; in a `rustev.definition/2` compilation refuses
them with `invalid_definition` detail `ranking-not-compiled` until the
compile increment. `rustev.backend/1` gains the optional `ranking` member
of 3.3, omitted when absent. Existing definitions, descriptors and plans
keep their bytes, `DescriptorId`s and `PlanId`s
(`crates/rustev-contract/tests/ranking_wire.rs`).

**Increment 2 (2026-10-26): plan details, the ranking output, values,
evidence and category 28.** `crates/rustev-contract/src/ranking_result.rs`
adds the `rustev.plan/3` details of 3.2.4 and 3.10.6, the
`RawOutput::Ranking` variant of 3.5.2 with its scores-length check (the
pointwise and pairwise modes reuse the existing scores and label outputs,
3.6 and 3.7), the model ranking and `Selection` values of 3.9 and 3.10.5
with no conversion to or from any other value kind (I-1), the
`rustev.evidence/2` entries of 3.13.1 with the bounds of 3.13.2, the
`OutValue::Selection` parameter of 3.10.7 and category 28
`exact_order_source`; nothing produces them until the evaluation and
compile increments. Existing plans, outputs, evidence and judgments keep
their bytes and identities (`crates/rustev-contract/tests/ranking_results.rs`).

**Increment 3, part a (2026-10-26): the permutation generator and the
request bounds.** `crates/rustev-core/src/ranking/` adds `rustev.permute/1`
exactly as 3.8.4 states (Fisher-Yates from the last index down, rejection
above the largest multiple of `i + 1`, the pairwise presentation bit) and
the worst-case request counts of 3.5.4, 3.6.3 and 3.7.5 with checked
arithmetic, the `merge_sort@1` sum computed exactly. Known-answer vectors
are committed with an independent reimplementation that reproduces them
(`crates/rustev-core/tests/vectors/permute.py`, `permute.json`,
`crates/rustev-core/tests/permute_vectors.rs`,
`crates/rustev-core/tests/ranking_schedules.rs`). Nothing calls them until
the compile and evaluation increments.

**Increment 3, part b (2026-10-26): request planning and the pairwise
schedules.** `crates/rustev-core/src/ranking/schedules.rs` plans every
mode's requests with the instance keys and comparison order of 3.4
(`perm/<j>`, `cand/<id>`, `pair/<c>/<o>`), presenting orders from
`rustev.permute/1` under 3.8, and runs `round_robin@1` as one round with
cyclic-triple counting and `merge_sort@1` as a deterministic state machine
with at most one pending comparison per merge (3.7.2, 3.7.3). Presentation
disagreements follow 3.8.3, and a batch is reserved against
`max_comparisons` before it is issued, so comparison `max_comparisons + 1`
is never issued (3.7.5). The tie-break order is supplied by the caller
(`crates/rustev-core/tests/ranking_schedules.rs`).

**Increment 4 (2026-10-26): the model ranking value, `select` and
backfill.** `crates/rustev-core/src/ranking/value.rs` builds the model
ranking value of 3.9 from supplied outputs in each mode: listwise with
validation (3.5.3), partial rankings and adjacent score disagreements,
position-bias presentations aggregated by integer Borda points (3.8),
pointwise (3.6) and pairwise (3.7) through increment 3's schedules, with
win counts, cycle count and inconsistent pairs. Scores are kept as
received, scoped to their request (3.5.5), and never reorder entries.
`crates/rustev-core/src/ranking/select.rs` implements `select` (3.10)
with per-position origin, source position, derivation and lineage (3.11);
backfill reads only the declared exact-derived ordering, never repeats a
selected id and never leaves the ranked pool. Nothing calls these until
the compile increment (`crates/rustev-core/tests/ranking_value.rs`,
`ranking_select.rs`).

## Decisions at approval

Decided within the binding principles under the work order.

1. **A new step kind, not a new member of `semantic`.** Adding members to
   `semantic` would change every version 1 definition's bytes; `rank` and
   `select` are new kinds in the shared `rustev.definition/2`.
2. **Reference form.** The issue's `{"ref": ...}` is written in the
   repository's `step:<id>` form; `k` is a literal. Spec 034 (#76 B4,
   drafted in parallel) may make `select.k` a parameter position.
3. **Ranking request scope.** The issue's `Scores{scope: request}` is the
   existing `ScoreScope` (plan, step, instance) of one ranking request, so
   pointwise scores of one ranking compare and nothing else does.
4. **Pointwise and pairwise are composed in the core,** visibly, as
   pending requests, not inside an adapter, which spec 009 3.2.2 forbids.
5. **Pairwise failures are not tie-broken;** any failed comparison makes
   the ranking unresolved and `select`'s policy decides. Only a
   both-order disagreement under `merge_sort@1` is settled by the
   tie-break, and it is counted (3.8.3).
6. **Schedules.** `round_robin@1` (observes cycles) and `merge_sort@1`
   (bounded, cycles not observable, recorded as such). Tournaments are out
   of scope.
7. **Permutation averaging aggregates positions (Borda), never scores,**
   because scores of different requests are never compared.
8. **Backfill must be statically exact-derived,** enforced by a new
   category; backfill positions are checked against the pool at runtime,
   so protected constraints applied before ranking cannot be bypassed.
9. **Model failure versus evidence failure.** Only backend failures
   trigger `backfill_all`; evidence reasons always propagate.
10. **A backfilled selection stays `mixed-derived`** at step level (spec
    002 3.12.3), with exact-derived classes per position.
11. **Shortfall is a result.** The policy reads `len` to act on it.
12. **`exclude_selected: false` is reserved.** It has no meaning that
    avoids duplicate positions.
13. **Rules backend amended (spec 005)** so the issue's swap test with
    "the rules backend" exists; it serves pointwise only.
14. **Jev keeps R-28.** C3's path is composition plus `select`; no Jev
    `rank`, so no owner decision is reopened.
15. **Bedrock admission under 030 3.12,** not an amendment of 030, as
    spec 035 does for `screen`.
16. **Descriptor member, not a new version.** `ranking` is optional and
    omitted when absent, keeping `DescriptorId`s.
17. **Metrics.** Backfill rate per position is spec 041's metric over the
    origin this spec records; this spec adds none.
18. **Paid live calls** for Bedrock `rank` follow spec 030 Decision 18 and
    R-29's USD 5 total testing cap; no other spend is authorized.

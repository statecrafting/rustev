---
id: "002-decision-contract-and-pure-core"
title: "Decision contract and pure core"
status: draft
created: "2026-09-23"
summary: >
  Increment 1: the versioned wire contract (`rustev-contract`) and the pure
  core (`rustev-core`). Value kinds and their conversion rules, identities,
  bounded input validation with provenance and freshness, the exact operator
  registry, decision definitions, the plan compiler with capability matching
  and explicit refusal, selection policy evaluation, and typed unresolved
  outcomes. No I/O, no clock, no async executor, no model.
establishes:
  - { kind: directory, path: "crates/rustev-contract/" }
  - { kind: directory, path: "crates/rustev-core/" }
depends_on:
  - "000-bootstrap"
  - "001-boundaries-and-authority"
---

# 002: Decision contract and pure core

Rationale: `docs/design/001-decision-engine-architecture.md` sections 6, 7,
8 (trait shapes only), 13 and 14, and reference plans 16.1 and 16.2 as
fixtures.

## 1. Purpose

Make a decision definition compile into an identified plan, or be refused
with a reason, and make a plan's selection policy evaluate deterministically
over supplied step values, before any runtime or model exists.

## 2. Territory

This spec claims two crates it will create, as forward claims a draft may
hold (`W-001` until built):

- `rustev-contract`: serde types for definitions, plans, judgments,
  unresolved reasons, capability descriptors, evidence records and the
  evaluation report envelope; canonical serialization; schema version.
- `rustev-core`: everything else in section 3. Depends on `rustev-contract`
  only among Rustev crates.

## 3. Behavior

### 3.1 Purity

1. `rustev-core` performs no I/O, reads no clock and no environment, spawns
   nothing, and is `#![forbid(unsafe_code)]`. Time is an input value.
2. Seam traits (context source, decision backend, evaluator, evidence sink)
   are defined here as signatures only; nothing here calls them.

### 3.2 Value kinds

The kinds, their meanings and their conversion rules are the design's
section 6 table, with these obligations:

1. Each kind is a distinct type. No `From`/`Into` exists between
   `ModelScore`, `Distribution`, `CalibratedProbability`, `SelectedLabel`,
   `OrdinalLevel` and `RankPosition`.
2. `CalibratedProbability` is constructible only by applying a calibration
   artifact whose binding (artifact, task, question, dataset, method) matches
   the step.
3. A `Distribution` is validated: finite, non-negative, keyed exactly by the
   declared options, summing to 1 within a declared tolerance.
4. `Unresolved` is a closed enum with the reasons in the design's section 6.

### 3.3 Validation

1. Parsing is bounded: byte length, nesting depth, collection lengths and
   string lengths are checked before allocation proportional to them.
2. Duplicate keys and unknown fields are refused.
3. Construction is fallible; there is no defaulting path for an absent or
   invalid value.
4. Each input field is checked against its declared provenance class and
   maximum age, relative to a supplied evaluation time. A failure is
   `MissingEvidence` or `StaleEvidence` naming the fields.

### 3.4 Exact operators

A closed, versioned registry. The initial set is exactly what the two
reference plans need: count with predicate, window filter, timestamp
difference, comparison, arithmetic on decimals, currency conversion by a
supplied rate table, set membership, field presence, filter with per-item
exclusion reasons, and deterministic weighted aggregation for ranking. Each
operator states its behavior on empty input and on overflow.

### 3.5 Definitions and compilation

The compilation phases are the design's section 7.3. Each refusal is a typed
error naming the step and the requirement:

1. Parse or bound failure.
2. Unknown operator or version.
3. Value-kind mismatch across a dependency edge.
4. Cycle in the step graph.
5. No registered backend satisfies a semantic step, and no fallback is
   declared; the error lists each candidate backend's shortfall.
6. A probability threshold over a non-calibrated kind without a declared
   `uncalibrated_threshold`.
7. Declared ceilings that cannot cover declared maxima without declared
   truncation.

A successful compile yields a plan whose `PlanId` is the digest of its
canonical form, including every bound artifact and calibration identity and
the compiler version.

### 3.6 Selection policy

Evaluation is a pure function of the plan and the step values. It must
handle every `Unresolved` input explicitly; a policy that leaves one
unhandled is a compile error.

### 3.7 Fixtures

The definitions of reference plans 16.1 and 16.2 are compiled in tests
against stub capability descriptors (no inference), with golden canonical
documents and golden `PlanId`s.

## 4. Out of scope

Executing semantic steps, scheduling, caching, budgets as they are spent,
evidence sinks, evaluation metrics, the CLI, any backend implementation. These
are specs `003` to `006`.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A definition whose semantic step needs `Distribution`, bound only to a `Label` backend, no fallback | Compile refused, reason 5, naming the backend's output kind. |
| A routing threshold over `Distribution` with no calibration and no `uncalibrated_threshold` | Compile refused, reason 6. |
| A distribution with a NaN, a negative mass, or an undeclared key | `InvalidBackendOutput`. |
| `payments.events` older than its maximum age at evaluation time | `StaleEvidence{payments.events}`; no policy row is evaluated as if fresh. |
| Lodging request missing dates | Judgment `MissingEvidence{dates}`. |
| A request body over the byte bound, or nested past the depth bound | Refused before allocation proportional to its size. |
| The same definition compiled twice with the same bindings | Byte-identical plan and identical `PlanId`. |
| A binding changes only the calibration artifact | A different `PlanId`. |

---
id: "005-reference-backends"
title: "Reference backends"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-backend-rules`, a deterministic backend (rules, lookup
  tables, linear heads over exact features) used as reference and test
  double; and the first semantic backend candidate, frozen embeddings plus a
  task-specific linear head (R-01), with ONNX Runtime through `ort` preferred
  only after a bounded compatibility, licensing, operator, linking, packaging
  and resource check.
establishes:
  - { kind: directory, path: "backends/rustev-backend-rules/" }
depends_on:
  - "002-decision-contract-and-pure-core"
---

# 005: Reference backends

Draft. A proposal, not a claim about code. Rationale: design section 8.2;
owner decision R-01.

## 1. Purpose

Provide backends that disclose exactly what they return, so plans can be
exercised end to end and a backend swap can be measured without changing any
authority path.

## 2. Territory

`backends/rustev-backend-rules/` now. The semantic backend's crate
(`backends/rustev-backend-embed-linear/` or similar) is claimed only after
the bounded check in 3.2 records its result; no inference dependency is added
before then.

## 3. Behavior (to be made concrete before approval)

1. **Rules backend.** Deterministic (`Determinism::Bitwise`), returns labels,
   logits or scores from declared rules and exact features; its descriptor
   states exactly which. Used as the reference double in spec 003 and 004
   tests.
2. **Bounded check for `ort` (R-01).** Before a dependency is added: the
   chosen embedding checkpoint's compatibility and license, the ONNX
   operators it needs, static or dynamic linking and packaging on each target,
   and measured memory and latency on declared hardware. The result is
   recorded in the decisions record; a failed check selects another runtime
   without reopening R-01.
3. **Linear head.** Task-specific; its descriptor lists only the labels it was
   trained for and it is never represented as supporting new labels. No
   custom-model training and no paid inference are authorized.
4. **Numerical repeatability.** Declared as `Tolerance{...}` with measured
   bounds for a pinned artifact and configuration; not bitwise.

## 4. Out of scope

Remote adapters (spec 009, network side under `integrations/`), second
semantic backend (increment 3), accuracy claims (spec 004 with real data).

## Acceptance (draft)

- The rules backend satisfies spec 002's capability matching for both
  reference plans and is bitwise repeatable.
- The `ort` check is recorded before any inference crate is added.

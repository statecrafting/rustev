---
id: "011-semantic-backend"
title: "First semantic backend (deferred)"
status: draft
implementation: deferred
created: "2026-09-23"
summary: >
  Deferred roadmap item split out of spec 005 by owner decision R-13: the
  first semantic backend candidate of R-01, frozen embeddings plus a
  task-specific linear head, with ONNX Runtime through `ort` preferred only
  after a bounded compatibility, licensing, operator, linking, packaging and
  resource check. Claims no code and adds no dependency until that check is
  recorded and this spec is made concrete and approved.
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "005-reference-backends"
---

# 011: First semantic backend (deferred)

Draft, and deferred: a proposal, not a claim about code, and not offered as
work. Moved here from spec 005 by owner decision R-13 so that 005 can be
completed as the deterministic rules backend without carrying unfinished
semantic obligations. Rationale: design section 8.2; owner decisions R-01,
R-04 and R-17. The ordinal is the next free one after the design roadmap's
`007` to `010`; it is not a claim about build order relative to them.

## 1. Purpose

Provide the first backend whose outputs come from a model, disclosing exactly
what it returns and costs, so a backend swap against spec 005's rules
backend can be measured (spec 004) without changing any authority path.

## 2. Territory

None yet. A crate (`backends/rustev-backend-embed-linear/` or similar) is
claimed only after the bounded check of 3.1 records its result in the
decisions record. No inference dependency, checkpoint download, training or
paid inference is authorized before then (R-17).

## 3. Behavior (carried from spec 005's draft; to be made concrete)

1. **Bounded check for `ort` (R-01).** Before a dependency is added: the
   chosen embedding checkpoint's compatibility and license, the ONNX
   operators it needs, static or dynamic linking and packaging on each
   target, and measured memory and latency on declared hardware. The result
   is recorded in the decisions record; a failed check selects another
   runtime without reopening R-01.
2. **Linear head.** Task-specific; its descriptor lists only the labels it
   was trained for and it is never represented as supporting new labels. No
   custom-model training and no paid inference are authorized. Its
   cancellation is local: inference already handed to a thread pool may run
   to completion, so it acknowledges `Stopped` only when it can establish
   the stop and `Unconfirmed` otherwise.
3. **Numerical repeatability.** Declared as `Tolerance` with measured bounds
   for a pinned artifact and configuration; not bitwise.
4. **Identity.** The artifact identity covers the checkpoint, tokenizer,
   preprocessing, truncation, precision and head parameters, as spec 005's
   rules program identity covers every behavior-affecting input.
5. **Quality.** No quality, calibration or usefulness claim without the
   real, independently labeled data R-04 requires; synthetic fixtures
   establish mechanics only.

## 4. Out of scope

Remote adapters (spec 009), a second semantic backend (increment 3),
training, paid inference.

## Acceptance (draft)

- The `ort` check is recorded before any inference crate is added.
- The backend satisfies spec 002's capability matching for the reference
  plans' semantic steps it declares, runs through `rustev-runtime`, and its
  measured tolerance is stated with the conditions that measured it.

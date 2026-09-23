---
id: "005-reference-backends"
title: "Reference backends"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-backend-rules`, a deterministic backend (rules, lookup
  tables, linear heads over exact features) implementing the backend seam as
  amended by spec 003, used as a reference and in spec 004 and 006; and the
  first semantic backend candidate, frozen embeddings plus a task-specific
  linear head (R-01), with ONNX Runtime through `ort` preferred only after a
  bounded compatibility, licensing, operator, linking, packaging and
  resource check.
establishes:
  - { kind: directory, path: "backends/rustev-backend-rules/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
---

# 005: Reference backends

Draft. A proposal, not a claim about code. Rationale: design section 8.2;
owner decision R-01. Updated after spec 003 was delivered, to build on its
actual interfaces.

## 1. Purpose

Provide backends that disclose exactly what they return and cost, so plans
can be exercised end to end and a backend swap can be measured without
changing any authority path.

## 2. Territory

`backends/rustev-backend-rules/` now. Adding the `backends/*` member glob
extends spec 001's workspace manifest. The semantic backend's crate
(`backends/rustev-backend-embed-linear/` or similar) is claimed only after
the bounded check in 3.3 records its result; no inference dependency is
added before then. Spec 003's scripted backends remain test fixtures and
are not replaced by these.

## 3. Behavior (to be made concrete before approval)

1. **The seam, as delivered by spec 003.** A backend implements
   `rustev_core::seams::DecisionBackend`:
   - `descriptor()`: a `rustev.backend/1` descriptor whose `DescriptorId`
     plans bind; the runtime refuses to prepare a plan whose bound
     descriptor differs;
   - `cost_model()` and `cost_bound(projection)`: static and per-call
     disclosure (`bounded`, `estimated`, `unknown`), in integer units the
     deployment chooses, rounded up; only a `bounded` backend can serve a
     hard budget;
   - `infer(AttemptCall)`: receives the canonical projection, a stable
     attempt id usable as an idempotency key, a `CancelSignal` and a
     `CallContext` with `remaining_ms`; returns an `AttemptReport` with the
     output or a classified failure (`transient`, `overloaded`, `permanent`,
     `cancelled`), a `Charge` and a `CancelAck`.
   It never manufactures a distribution or a calibration claim; the core
   validates every output (spec 002, 3.10.2).
2. **Rules backend.** In-process and deterministic (`Determinism::Bitwise`);
   returns labels, logits or scores from declared rules and exact features;
   its descriptor states exactly which. Cost model `bounded`, with its
   declared per-call units (0 unless configured). It checks its cancellation
   signal before answering and acknowledges a stop it can establish
   (`Stopped`), since it has no remote work.
3. **Bounded check for `ort` (R-01).** Before a dependency is added: the
   chosen embedding checkpoint's compatibility and license, the ONNX
   operators it needs, static or dynamic linking and packaging on each
   target, and measured memory and latency on declared hardware. The result
   is recorded in the decisions record; a failed check selects another
   runtime without reopening R-01.
4. **Linear head.** Task-specific; its descriptor lists only the labels it
   was trained for and it is never represented as supporting new labels. No
   custom-model training and no paid inference are authorized. Its
   cancellation is local: inference already handed to a thread pool may run
   to completion, so it acknowledges `Stopped` only when it can establish
   the stop and `Unconfirmed` otherwise.
5. **Numerical repeatability.** Declared as `Tolerance{...}` with measured
   bounds for a pinned artifact and configuration; not bitwise.

## 4. Out of scope

Remote adapters (spec 009, network side under `integrations/`), second
semantic backend (increment 3), accuracy claims (spec 004 with real data).

## Acceptance (draft)

- The rules backend satisfies spec 002's capability matching for both
  reference plans, runs them through `rustev-runtime` under a hard budget,
  and is bitwise repeatable.
- Its cost and cancellation disclosures are tested against the runtime's
  ledger and cancellation records.
- The `ort` check is recorded before any inference crate is added.

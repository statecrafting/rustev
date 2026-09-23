---
id: "003-runtime-execution-and-evidence"
title: "Runtime execution and evidence emission"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-runtime` drives the pure core's staged evaluation
  against real seams on Tokio (R-03): deadlines, admission, budget
  reservation, bounded retries and declared runtime fallbacks, batching and
  duplicate suppression, caches keyed by artifact and projection, truncation,
  cancellation honesty, and evidence emission under a declared sink policy.
establishes:
  - { kind: directory, path: "crates/rustev-runtime/" }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
---

# 003: Runtime execution and evidence emission

Draft. A proposal, not a claim about code. Rationale: design sections 8, 9
and 10.

## 1. Purpose

Execute a compiled plan's semantic requests through `DecisionBackend`
implementations under explicit time, budget and concurrency bounds, and
emit an evidence record for every decision, without changing any outcome
the pure core would compute from the same supplied values.

## 2. Territory

`crates/rustev-runtime/`. Depends on `rustev-core`, `rustev-contract` and
Tokio (R-03). Not a dependency of `rustev-core` or `rustev-contract`
(spec 001, 3.4.1).

## 3. Behavior (to be made concrete before approval)

1. **Time.** A `Clock` boundary supplies the evaluation time and deadlines;
   the runtime reads time only through it, so tests use a manual clock. The
   core still receives time as a value.
2. **Deadlines.** One deadline per decision, subdivided by declared share; a
   request that cannot start before its share expires is supplied to the core
   as `deadline_exceeded` without dispatch.
3. **Admission and concurrency.** Bounded queues per backend; refusal to
   admit is a typed result, not a timeout.
4. **Budgets.** Reservation before dispatch, settlement after; a failed
   reservation is `budget_exhausted`.
5. **Retries and runtime fallback.** Bounded, only where the definition
   declares them, recorded in evidence. Open question: whether runtime
   fallback needs a definition field beyond spec 002's compile-time fallback.
6. **Batching and duplicate suppression.** Requests sharing a backend and
   canonical projection share one in-flight call.
7. **Caches.** Keyed by `ArtifactId`, canonical projection and an opaque
   tenant handle; byte-budgeted; purged by source invalidations.
8. **Cancellation honesty.** The evidence records when a cancelled remote
   call may still have run or been charged, as the backend declared.
9. **Evidence.** Every decision emits the core's `EvidenceRecord` plus
   runtime observations (latency, retries, cache hits) to an `EvidenceSink`
   whose policy is `FailDecision`, `Backpressure{max_pending}` or
   `DropCounted`; a drop is counted and the count reported.
10. **Determinism.** For identical supplied semantic values the judgment is
    byte-identical to the pure core's; runtime observations are recorded,
    never promised to repeat (principle XIII).

## 4. Out of scope

Backends (spec 005), evaluation (spec 004), the CLI (spec 006), hosting,
HTTP (integrations only), any executor other than Tokio.

## Acceptance (draft)

- A manual clock drives every deadline test; no test sleeps on wall time.
- Each failure path (deadline, admission, budget, backend error, sink
  failure under each policy) has a test.
- A property test: for random supplied outputs, the runtime's judgment equals
  the pure core's.

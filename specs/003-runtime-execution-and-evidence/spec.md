---
id: "003-runtime-execution-and-evidence"
title: "Runtime execution and evidence emission"
status: approved
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev-runtime` drives the pure core's staged evaluation
  through backend seams on Tokio (R-03) under explicit bounds: injectable
  monotonic time and one end-to-end deadline, bounded admission and
  concurrency, cost budgets reserved before dispatch with honest settlement,
  bounded retries and runtime fallback declared in an identified execution
  policy that is part of plan identity, cancellation that never claims more
  than the backend acknowledged, and evidence delivery under a declared sink
  policy. Amends spec 002: plan schema 2, the execution-policy and run-record
  documents, a pre-supply output check, and the backend and sink seams.
  Batching, duplicate suppression and caches are deferred by name.
establishes:
  - { kind: directory, path: "crates/rustev-runtime/" }
amends:
  - "002-decision-contract-and-pure-core"
extends:
  # Adds the runtime crate's dependencies (Tokio) to the lockfile.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds tokio to the workspace dependencies.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  # Adds 003 to the specs `make verify` runs.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # The contract and core amendments of section 3.2 (plan schema 2, the
  # execution-policy and run-record documents, compilation with a policy,
  # the pre-supply check, the seams) and the regenerated plan goldens. The
  # crates stay spec 002's; 003 extends them and amends 002.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
---

# 003: Runtime execution and evidence emission

Rationale: design sections 8, 9 and 10, which are not contract. The owner's
decisions R-07 to R-11 and approval A-03 in
`docs/decisions/00-founding-decisions.md` bound this spec.

## 1. Purpose

Execute a compiled plan's semantic requests through backend seams under
explicit time, admission, concurrency, cost and attempt bounds; deliver an
evidence record for every admitted decision; and never change the judgment
the pure core computes from the same validated step values.

## 2. Territory

- `crates/rustev-runtime/`: the runtime, its clock boundary, admission,
  budget ledger, attempt driver and evidence delivery. Depends on
  `rustev-contract`, `rustev-core` and `tokio` only; no other workspace
  crate depends on it (spec 001, 3.4). Its tests hold the scripted backends
  and sinks of section 3.12, which are test fixtures, not the production
  backends of spec 005 (R-09).
- Amendments to units spec 002 owns, declared by the `amends` and `extends`
  edges above and stated in section 3.2. Spec 002's text is not edited: it
  remains the record of what increment 1 delivered.

## 3. Behavior

### 3.1 Relation to spec 002 and compatibility

1. Spec 002 is amended, not superseded. Every rule of spec 002 holds except
   where section 3.2 states a replacement.
2. Definitions (`rustev.definition/1`) are unchanged. Their canonical bytes,
   `DefinitionId`s and the committed definition goldens do not change.
3. Plans move to `rustev.plan/2` (3.2.1). Adding a field changes canonical
   bytes, so under spec 002 3.3.1 this is a new schema version, and every
   `PlanId` changes, including both reference plans' golden `PlanId`s and
   plan documents. A `rustev.plan/1` document is refused by the parser with a
   schema error; recompiling its embedded definition yields a plan 2 whose
   `execution` is `none`.
4. A plan compiled without an execution policy has `execution: none`. The
   runtime then makes exactly one attempt per request on the bound backend,
   with no retry, no runtime fallback, no attempt timeout and an unlimited
   cost budget whose charges are still recorded. An old definition never
   acquires retries, fallback or any other effect by being run, and a backend
   that is installed but not named by the plan is never used.
5. Compile-time capability fallback (spec 002, 3.9.6) and runtime failure
   fallback (3.6) are different policies (R-10). The first is declared in the
   definition and chooses a binding; the second is declared in an execution
   policy and is taken per request after a failure.

### 3.2 Contract amendments to spec 002

1. **Plan schema 2.** `rustev.plan/2` is `rustev.plan/1` plus one field,
   `execution`, which is `none` or `declared` with the execution policy's
   identity (`ExecutionPolicyId`), the canonical policy document, and each
   runtime fallback target bound to its backend id, artifact, `DescriptorId`
   and output kind. The `PlanId` is the identity of that document, so it
   changes whenever the execution policy, a fallback target's descriptor or
   artifact, or anything spec 002 identified changes.
2. **Execution policy** (`rustev.execution/1`, identified, parsed under
   `DESCRIPTOR_V1`). Fields, all required:
   - `max_attempts_per_decision`: the worst-case number of backend attempts
     one decision may make, over all requests, retries and fallbacks;
   - `cost`: `unlimited`, `hard{max_units}` or `estimated{max_units}` (3.5);
   - `steps`: a list of step policies, each naming a semantic step once, with
     `retry{max_attempts, on, delay}`, `attempt_timeout` (`none` or
     `ms{value}`) and `fallback{backends, on}`. A semantic step not listed
     gets one attempt, no timeout and no fallback.
3. **Compilation with an execution policy.** `compile_with(definition,
   descriptors, calibrations, policy)` runs phases (a) to (f) of spec 002,
   then validates the policy after every step is bound, then (g) to (i).
   `compile` is unchanged and produces `execution: none`. `Compiled::load`
   recompiles with the policy embedded in the plan. A policy that is not
   valid is refused with a new category, 12 `invalid_execution`, in phase
   (f), naming `execution` or `step:<id>`, when:
   - the schema is not `rustev.execution/1` (category 1 `parse` instead);
   - a step policy names an unknown step, a non-semantic step, a step whose
     compile-time fallback `unsupported` was taken, or a step twice;
   - `retry.max_attempts` is outside `1..=8`; `on` is non-empty when
     `max_attempts` is 1 or empty when it is greater; `on` names a class that
     is not retryable (3.6.1) or repeats one;
   - a delay is outside its bounds (`fixed{ms}` in `1..=60000`;
     `exponential{initial_ms, max_ms}` with `1 <= initial_ms <= max_ms <=
     60000`), or an attempt timeout is 0;
   - a fallback backend is not supplied, is the step's bound backend, appears
     twice (no cycle can be expressed), or there are more than 4; `on` is
     empty while backends are declared or non-empty while none are, repeats
     a class, or names a class that is not a fallback trigger (3.6.2);
   - a fallback backend fails any shortfall check of spec 002 3.9.4 against
     the step's bound requirement, or its output kind differs from the
     primary binding's output kind (so normalization and validation are
     identical), or the step is bound to a calibration and the fallback's
     artifact is not the calibration's artifact;
   - `cost` declares `max_units` of 0;
   - the worst-case attempts exceed `max_attempts_per_decision`, where a
     step's worst case is its `max_requests` times `max_attempts` times one
     plus its fallback count, and an unlisted step's is its `max_requests`.
4. **Pre-supply check.** `Evaluation::check_output(step, instance, output)`
   returns what `supply` would record for that output without consuming the
   request: `Ok` or `invalid_backend_output`. It is a pure read; `supply`
   still validates what it is given.
5. **Run record** (`rustev.run/1`, not identified, parsed under
   `RECORD_V1`): the evidence the runtime delivers (3.9).
6. **Seams.** The seam traits of spec 002 3.1.2 had no caller in increment 1
   and are replaced where the runtime needs more than they said:
   - `CallContext` carries `remaining_ms` (time left at dispatch, by the
     runtime's monotonic clock) instead of a wall-clock `deadline`, plus the
     trace id and the opaque principal handle;
   - `DecisionBackend` gains `cost_model()` (static: `bounded`, `estimated`
     or `unknown`) and `cost_bound(projection)` (per call: `bounded{max}`,
     `estimated{units}` or `unknown`), and `infer` takes an attempt call
     (projection, attempt id, cancellation signal, context) and returns an
     attempt report: an output or a classified failure, a charge, and a
     cancellation acknowledgement;
   - `EvidenceSink::deliver` takes a run record and returns a receipt or an
     error;
   - `CancelSignal` is a std-only, clonable, one-way signal with child
     signals. The core still reads no clock, performs no I/O and spawns
     nothing.
   `ContextSource` and `Evaluator` keep their shapes apart from
   `CallContext`.

### 3.3 Time

1. Domain time and elapsed time are separate. The caller supplies the
   evaluation time (`Timestamp`) with each decision; the core receives it as
   a value, as in spec 002. Deadlines, delays, timeouts and every duration in
   the run record use the runtime's `Clock`: a monotonic millisecond counter
   with `now()` and `sleep_until(instant)`. The runtime reads time through
   this boundary only; the production clock is Tokio's, and tests use a
   manual clock that moves only when the test advances it.
2. A decision's deadline `D` is the instant the decision was submitted plus
   the plan's `limits.deadline_ms`, or a caller-supplied duration if it is
   smaller (a caller can only tighten it). Queue waiting, permit waiting,
   every attempt, every retry delay and every fallback count against `D`.
3. No attempt is dispatched at or after `D`. A request that reaches `D`
   without an accepted output is supplied as `deadline_exceeded`.
4. Race resolution. Each wait polls, in this order: the attempt's
   completion, then caller cancellation, then the deadline and the attempt
   timeout. A completion that is ready when the runtime observes an expired
   deadline or a cancellation in the same poll is accepted. Once the runtime
   has recorded a deadline, timeout or cancellation for an attempt, a later
   completion of that attempt is never supplied.
5. The deadline bounds semantic work. Policy evaluation after the last
   request resolves is pure and not bounded by `D`; evidence delivery is
   bounded by the sink policy's own timeouts (3.9).

### 3.4 Admission and concurrency

1. The runtime admits at most `max_in_flight` decisions and holds at most
   `max_queued` further decisions waiting for admission. A submission that
   finds the queue full is rejected at once as `overloaded`. A queued
   submission whose deadline passes before admission is rejected as
   `queue_deadline`; one whose caller cancels while queued is rejected as
   `cancelled`. A decision id outside 3.9.2's bounds, or a snapshot the core
   refuses to start (spec 002, `StartError`), is rejected as
   `invalid_request`. A rejection is a typed result, never a timeout, and a
   rejected decision has no judgment, no run record and spends nothing.
2. Within a decision at most `max_parallel_requests` requests are in
   progress; the rest wait in a local queue bounded by the plan's
   `max_semantic_requests`. Each registered backend has its own attempt
   concurrency limit; an attempt waits for a backend permit no later than
   `D`.
3. The runtime spawns no task per decision, request or attempt: a decision
   runs on the caller's task, and its requests are polled by that task. The
   only spawned task is the single evidence delivery worker of 3.9.3. The
   number of waiting futures is therefore bounded by the admission bounds
   times the per-decision bounds.
4. Admission and backend permits are released when their holder finishes,
   fails, is cancelled or is dropped, and when a backend adapter panics
   while polled: a panic inside `infer` is contained at the attempt, classed
   `adapter_fault`, and its cost is unknown (3.5.4). A panic inside the
   core is a defect and is not contained.
5. Dropping a decision's future is not cancellation: it releases every
   permit and reservation guard it holds, but no run record is built or
   delivered, and the runtime counts it as `abandoned` in memory.

### 3.5 Cost budgets

1. **Units.** Cost is a non-negative integer count of units the deployment
   chooses (for example micro-currency). There is no fractional unit and no
   rounding inside the runtime; an adapter that knows a fractional cost
   rounds a bound or a charge up to the next unit.
2. **Scope.** One ledger per decision holds the execution policy's `cost`
   for that decision; every attempt, retry and fallback of the decision draws
   from it. A runtime may also hold one shared ledger across decisions; an
   attempt reserves in the decision ledger and then the shared ledger, and a
   failure in the second releases the first. Each ledger reserves atomically
   under a lock, so concurrent attempts cannot together reserve beyond its
   limit.
3. **Reservation before dispatch.** Under `hard{max_units}`, an attempt
   reserves the adapter's per-call `bounded{max}`; the plan is refused when
   prepared if any backend its steps can reach declares a `cost_model` other
   than `bounded`, and an attempt whose per-call bound is not `bounded` is
   refused without dispatch. Under `estimated{max_units}` (the weaker policy)
   an attempt reserves the per-call bound or estimate, and `unknown` is
   refused without dispatch. Under `unlimited` nothing is refused, and the
   reservation is the bound or estimate, or 0 when unknown. A reservation that
   does not fit is `budget_exhausted{cost}` for that request, without
   dispatch.
4. **Settlement.** The attempt report's charge settles the reservation:
   - `observed{units}`: the reservation is released and `units` is added to
     observed spend, even when it exceeds the reservation; an observed charge
     above a `bounded` reservation is recorded as a bound violation;
   - `estimated{units}`: the reservation is released and `units` is added to
     estimated spend;
   - `unknown`, and every attempt the runtime stopped waiting for (deadline,
     timeout, cancellation without a confirmed stop, panic): the reservation
     is kept as outstanding liability and is never refunded because the local
     future ended.
5. **Reconciliation.** A later observed charge for an attempt id moves its
   liability to observed spend on a ledger the host still holds (the shared
   ledger); the delivered run record is never rewritten.
6. **Labels.** The run record reports the mode, the limit, observed spend,
   estimated spend, outstanding liability and whether the final cost is known.
   Only `hard` with no unknown liability and no bound violation is reported
   as within a guaranteed cap; `estimated` is never labeled a cap.

### 3.6 Retries and runtime fallback

1. **Failure classes.** An attempt ends as: an output; `invalid_output` (the
   pre-supply check refused it); `transient`, `overloaded` or `permanent`
   (the adapter's own classification); `timed_out` (the attempt timeout
   passed); `adapter_fault` (the adapter panicked); `deadline`; or
   `cancelled`. Retryable classes are `transient`, `overloaded` and
   `timed_out`, and a class is retried only when the step's `retry.on` names
   it. `invalid_output`, `permanent` and `adapter_fault` are never retried.
   Admission refusal, budget refusal, missing or stale evidence, deadline and
   cancellation are never retried and never trigger fallback.
2. **Fallback triggers.** When the current target's last attempt ends in a
   class named in `fallback.on` (from `transient`, `overloaded`, `timed_out`,
   `permanent`, `invalid_output`, `adapter_fault`), the request moves to the
   next declared backend, in declared order, with a fresh retry count and the
   same deadline and ledger. Each target is tried at most once per request.
3. **Delays.** `none`, `fixed{ms}`, or `exponential{initial_ms, max_ms}`
   (doubling per retry, capped at `max_ms`). No jitter. A retry whose delay
   would reach `D` is not attempted; the request moves to fallback if the
   class is a trigger, and otherwise resolves with the last failure.
4. **Supplied reasons.** A request resolves as the first accepted output, or
   as the failure it ended with: `invalid_output` as
   `invalid_backend_output`; `transient`, `overloaded`, `permanent`,
   `timed_out` and `adapter_fault` as `backend_unavailable{class: detail}`;
   deadline as `deadline_exceeded`; budget refusal as
   `budget_exhausted{cost}`. These are exactly the runtime reasons spec 002
   3.10.1 accepts, so no policy needs a new handler.
5. **Fallback outputs** pass the same pre-supply check as primary outputs.
   Their compatibility was established at compile time (3.2.3), and the
   runtime refuses to prepare a plan unless every installed backend's
   descriptor has exactly the `DescriptorId` the plan binds for it.
6. **Trace.** Every attempt and every transition (retry with its delay and
   class, fallback with its target and class, stop with its reason) is kept
   in the run record in order.

### 3.7 Cancellation

1. The caller cancels a decision through a `CancelSignal`. Each attempt
   receives a child signal, which the runtime also raises on deadline and
   attempt timeout. An adapter that supports cancellation observes it and
   reports an acknowledgement.
2. When it raises an attempt's signal, the runtime polls the attempt once
   more; a report returned then is recorded with its charge and
   acknowledgement. Otherwise the runtime stops waiting and drops the future.
3. The run record distinguishes, per attempt: cancellation not requested;
   requested and acknowledged as stopped; requested and answered without a
   confirmed stop; and requested with no answer observed. Remote state is
   `finished`, `stopped` (only on an acknowledged stop) or
   `possibly_continuing`. Dropping a future is never recorded as stopping
   remote work or charges.
4. A cancelled decision finishes no judgment. The caller receives the
   `cancelled` termination and the run record, which keeps every attempt and
   every retained liability. Outputs that completed before cancellation are
   recorded in their attempts and are not supplied.

### 3.8 Judgment equivalence

1. The runtime supplies the core only through `Evaluation::supply`, with the
   adapter's output unchanged or one of the reasons of 3.6.4. Supplied
   outputs therefore pass spec 002's validation unchanged.
2. For any plan, snapshot, evaluation time and sequence of validated step
   values, the runtime's judgment is identical to the one direct core
   evaluation produces from the same values.
3. Timeouts, failures and budget refusals change which values are supplied
   (an unresolved reason instead of an output). That changes the judgment as
   the core specifies; it is not a determinism violation (principle XIII).

### 3.9 Evidence

1. **Record.** A run record carries the decision id, the `PlanId`, the
   execution policy's identity or `none`, the termination (`judged` or
   `cancelled`), the core's `EvidenceRecord` or `not_produced`, the elapsed
   times (queued, total, deadline, whether it expired) in monotonic
   milliseconds from submission, each request with its result, attempts and
   transitions, and the cost summary of 3.5.6. Core judgment and runtime
   observations are separate members: the core record is exactly what
   `finish` returned.
2. **Identities.** The decision id is supplied by the caller, must be
   non-empty and at most 256 bytes, and is the delivery key. An attempt id is
   `<decision id>/<step>/<instance joined by ','>/<n>`, with `n` counting
   attempts of that request from 1 across targets; it is passed to the
   adapter as its idempotency key. Both are independent of timing.
3. **Bounds.** A record holds at most the plan's requests and
   `max_attempts_per_decision` attempts (or one attempt per request without
   an execution policy); every free-text detail the runtime writes, including
   an adapter's, is cut to 256 bytes at a character boundary. Pending
   deliveries are bounded by the sink policy.
4. **Sink policies.** The runtime holds one of:
   - `fail_decision{timeout_ms}`: the decision delivers inline and waits at
     most `timeout_ms`. On a receipt the caller receives the result with the
     receipt. On an error, a panic or the timeout the caller receives
     `evidence_not_delivered` with the record, the error and whether the sink
     may have received it (true after a timeout); no judgment is returned as
     a success.
   - `backpressure{max_pending, enqueue_timeout_ms, delivery_timeout_ms}`:
     the record is handed to the delivery worker's queue of `max_pending`
     slots; the decision waits for a slot at most `enqueue_timeout_ms`, and
     otherwise the caller receives `evidence_not_delivered` as above (never
     sent). Once queued, the caller receives the result with a ticket that
     resolves to the receipt or the failure.
   - `drop_counted{max_pending, delivery_timeout_ms}`: the record is queued if
     a slot is free and otherwise dropped and counted; the caller receives
     the result marked `dropped` with the running drop count.
   The worker delivers one record at a time, bounded by
   `delivery_timeout_ms`, and never retries. At most `max_pending` records are
   queued plus one in delivery.
5. **What cannot be undone.** Sink failure does not undo inference or
   spending: the record the caller receives still carries every attempt and
   charge.
6. **Acknowledgement and duplicates.** A receipt establishes only that the
   sink accepted the record for that decision id under its own durability
   terms; the runtime does not verify durability. The runtime delivers a
   record at most once. A caller that redelivers after an uncertain failure
   sends the same record, and sinks deduplicate by decision id.
7. **Counters are memory.** Drop, failure, rejection and abandonment counts
   live in memory, reset with the process, and are not durable loss
   accounting.

### 3.10 Deferred optimization contract

Automatic batching, cross-request duplicate suppression, reusable inference
caches and persistent caches are deferred to a separately specified
optimization increment (R-08). Nothing here implements them. That increment
must satisfy, at least:

1. **Request equivalence** covers every output-affecting input and isolation
   boundary: backend artifact and descriptor, operation, question, rubric
   levels or options, candidates, projection, preprocessing and tokenization,
   every output-affecting backend configuration, calibration binding where
   it applies after the backend, and the authorized scope (principal handle
   or tenant). Artifact plus projection is not a complete identity.
2. **Shared calls** define each waiter's own deadline and cancellation (one
   waiter's cancellation does not cancel others; the call is cancelled only
   when no waiter remains), who owns the reservation and charge (one ledger
   pays, the others record a shared reference, and liability is never
   double counted or dropped), and isolation (no sharing across authorized
   scopes).
3. **Caches** define invalidation on correction, revocation and erasure of
   any input that reached a cached projection, byte budgets and eviction, and
   the race where an in-flight result arrives after its invalidation: such a
   result is never stored and never served to a later request.
4. Evidence records whether a value came from a shared call or a cache
   entry, and the entry's identity.

These are requirements on a future spec, not claims of implemented behavior.

### 3.11 Partial adoption

`rustev-runtime` depends on no workspace crate other than `rustev-contract`
and `rustev-core`, and on no HTTP stack; `rustev-contract` and `rustev-core`
build and pass their tests without it.

### 3.12 Test fixtures

Scripted backends answer from a script per attempt (output, classified
failure, hang until cancelled or released, panic), declare a cost model and
per-call bounds, report a scripted charge and acknowledgement, and record
every dispatch, completion and cancellation observably. Scripted sinks
acknowledge, fail, hang or panic on demand. All are labeled synthetic and are
evidence of runtime mechanics only (R-04, R-09).

## 4. Out of scope

Batching, duplicate suppression and caches (3.10). Per-step deadline shares.
Production backends and any inference dependency (spec 005), evaluation and
replay (spec 004), the CLI (spec 006), hosting and HTTP (integrations only),
dynamic plugins, and any executor other than Tokio.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A submission when `max_queued` decisions already wait | Rejected `overloaded` at once; nothing spent. |
| A queued decision whose deadline passes | Rejected `queue_deadline`; nothing dispatched. |
| A request still waiting for a backend permit at `D` | `deadline_exceeded`, never dispatched. |
| Two concurrent attempts whose bounds together exceed the hard budget | One is dispatched; the other is `budget_exhausted{cost}` without dispatch. |
| A hard budget with a backend whose cost model is `estimated` | The plan is refused when prepared. |
| An attempt abandoned at the deadline with no charge reported | Its reservation stays as outstanding liability; the record says the final cost is unknown and remote work possibly continues. |
| A `permanent` failure with `retry.on: [transient]` | One attempt; no retry. |
| An output with an undeclared option | `invalid_backend_output`; not retried; fallback only if declared for `invalid_output`. |
| A fallback backend listing the primary, or a backend twice | Refused, category 12. |
| The same definition compiled with two different execution policies | Two different `PlanId`s. |
| A plan with `execution: none` whose backend fails transiently | One attempt, `backend_unavailable`, no fallback even if another capable backend is installed. |
| A sink that never answers under `fail_decision` | The caller receives `evidence_not_delivered` after the timeout, with the record and `uncertain: true`. |
| A full `drop_counted` queue | The record is dropped, the count increments, the caller is told. |

## Acceptance

- Every behavior in section 5 has a test, driven by the manual clock,
  barriers and scripted backends; no test sleeps on wall time.
- Queue and concurrency bounds hold under contention, and permits are
  released on success, failure, cancellation, drop and adapter panic.
- Deadline expiry is tested while queued, while waiting for a permit, while
  executing and during a retry delay; completion racing a deadline and a
  cancellation resolves as 3.3.4 states.
- Concurrent reservations never exceed a hard limit, on the decision ledger
  and on a shared ledger; retries and fallbacks draw from the same budget and
  deadline; unknown charges remain liability.
- Nonretryable classes are not retried; invalid output is refused through
  the core's validation; each fallback trigger and non-trigger is tested.
- Cancellation records the four states of 3.7.3 and never claims a remote
  stop without an acknowledgement.
- Each sink policy is tested under success, error, hang, panic and
  saturation; pending deliveries and waiting decisions stay within their
  bounds.
- For generated step values, the runtime's judgment equals direct core
  evaluation's, and both reference plans run through the runtime with
  scripted outputs to the judgments spec 002's tests expect.
- A definition compiled without a policy runs with one attempt and no
  fallback; its `DefinitionId` and definition golden are unchanged;
  `PlanId` changes when the execution policy or a fallback descriptor
  changes; a `rustev.plan/1` document is refused.
- Each invalid-execution case of 3.2.3 is refused with category 12, with a
  passing neighbour.
- Seeded defects in the runtime's most important guarantees are each detected
  by the tests (a bounded negative control).
- `rustev-runtime` depends only on `rustev-contract`, `rustev-core` and
  Tokio among checked families; `make code` passes.

## Verification

```verify:cli
# 3.2: contract and core amendments (plan 2, execution policy, category 12,
# pre-supply check), and the regenerated goldens.
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
# 3.3 to 3.9, 3.12: the runtime against scripted backends and sinks.
cargo test -p rustev-runtime --locked
# 3.11: partial adoption and dependency rules.
cargo run -p rustev-boundaries --locked --quiet
cargo build -p rustev-core --locked
sh -c 't=$(cargo tree -p rustev-runtime -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -E "^rustev-" | grep -qvxE "rustev-(contract|core|runtime)"; then exit 1; fi'
cargo clippy -p rustev-runtime --all-targets --locked -- -D warnings
cargo fmt -p rustev-runtime --check
# Negative control: seeded defects must each be detected.
sh crates/rustev-runtime/mutation/seeds.sh
```

## Decision history

- 2026-09-23: the owner approved this spec within the bounded requirements
  of their runtime brief (A-03), and decided R-07 to R-11. The concrete
  rules above were written by the agent within those bounds; engineering
  choices it made are recorded below as its own.

## Engineering choices

Made by the agent within A-03; open to the owner's review.

| Id | Choice | Reason |
|---|---|---|
| E-08 | The execution policy is a separate identified document embedded in the plan, not a definition field. | Definitions and their ids stay unchanged (3.1.2); one definition can be deployed under different policies, each with its own `PlanId`; fallback names concrete backends, which are bindings. |
| E-09 | One end-to-end deadline; no per-step shares. | The approved scope names deadlines, not shares; shares need their own declared schema. |
| E-10 | No task per decision or attempt; one delivery worker. | Makes every waiting future countable from the admission bounds (3.4.3). |
| E-11 | Completion wins a same-poll race with deadline or cancellation. | An output already available is not discarded, and the rule is deterministic under the manual clock. |
| E-12 | One extra poll after raising cancellation, then stop waiting; no grace period. | A cooperative adapter can acknowledge without the runtime extending past `D`. |
| E-13 | The runtime never retries a delivery; sinks deduplicate by decision id. | A retry after an uncertain timeout is the duplicate hazard; the caller decides. |
| E-14 | `invalid_output` can trigger fallback but never retry. | A deterministic backend repeats an invalid output; another backend may not. |

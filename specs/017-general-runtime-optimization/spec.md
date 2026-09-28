---
id: "017-general-runtime-optimization"
title: "General runtime batching, duplicate suppression, and caches"
status: approved
implementation: in-progress
created: "2026-09-26"
summary: >
  A separately reviewable amendment to runtime execution for general
  runtime-planned batching, concurrent duplicate suppression, bounded memory
  caches, and opt-in persistent caches. Optimized execution must be judgment
  equivalent to independent uncached execution, isolate authorized scopes,
  preserve each waiter's deadline and cancellation, attribute one charge and
  every liability exactly once, invalidate on correction, revocation and
  erasure, and record every shared-call or cache effect. Existing
  adapter-scoped batching remains a special case, not evidence that this
  general contract is implemented or qualified.
amends:
  - "003-runtime-execution-and-evidence"
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "For the same plan, snapshot, evaluation time and validated backend values, optimization produces the same judgment and core evidence as independent execution; it changes only runtime observations explicitly recorded by this spec."
    anchor: "3-1-equivalence-and-plan-identity"
  - id: "I-2"
    kind: invariant
    text: "No batch, shared call or cache entry crosses an authorized scope, tenant, principal, artifact, question, preprocessing, policy or other output-affecting identity boundary."
    anchor: "3-2-complete-request-equivalence"
  - id: "I-3"
    kind: invariant
    text: "Correction, revocation or erasure invalidates every affected cache entry and in-flight result; a result arriving after invalidation is neither stored nor served to a later request."
    anchor: "3-7-invalidation-erasure-and-races"
  - id: "I-4"
    kind: invariant
    text: "One external attempt has one charge owner and one liability record; sharing never duplicates, hides, refunds or reallocates an unknown charge by inference."
    anchor: "3-5-budgets-cost-and-liability"
---

# 017: General runtime batching, duplicate suppression, and caches

Approved (A-16, 2026-09-26) as its own reviewable change before any code
(R-16). Implementation is in progress in increments; see the implementation
record. Approval makes no latency, throughput,
cost, durability, cache-safety, or production claim.

## 1. Purpose

Define when the runtime may combine or reuse semantic work without changing a
decision's meaning, authority boundary, individual bounds, or honest evidence.
This makes spec 003 section 3.10 concrete for four mechanisms:

1. runtime-planned batching of independent requests;
2. concurrent duplicate suppression through one in-flight call;
3. reusable byte-bounded memory caches; and
4. opt-in persistent caches behind a host-supplied runtime store.

Specs 009 and 012 already implement adapter-scoped, same-decision batching.
That special case remains governed by those specs. It is neither replaced nor
accepted as qualification of the general mechanisms here.

## 2. Territory and compatibility

This spec amends spec 003's execution policy, runtime records, budget
settlement and dispatch behavior. It adds neutral optimization identities and
evidence fields to contract and core, runtime implementation under
`rustev-runtime`, and evaluation support for cold, warm and invalidation cases.
It does not change value kinds, capability matching, selection policy,
authorization, backend output validation, the remote protocol, or any
adapter's declared capabilities.

Optimization is disabled unless a plan's identified execution policy enables
a named mechanism and supplies every required bound. `none` preserves the
existing behavior and bytes. Enabling or changing an optimization policy
changes `PlanId`. Cache contents, queue timing, hit or miss, and batch
membership do not change `PlanId`; they are runtime observations.

## 3. Behavior

### 3.1 Equivalence and plan identity

An optimized request is supplied to the core only with the exact `RawOutput`
or unresolved reason that its independent logical request receives. Batching,
sharing and reuse cannot invent mass, merge judgments, alter ordering, extend
a deadline, bypass a budget, select fallback, or convert failure into a value.

The policy identifies enabled mechanisms, all bounds, key schema version,
cache namespace version, admission policy, expiry basis, persistence posture,
and invalidation contract. The compiler refuses incomplete policies.

### 3.2 Complete request equivalence

Two requests are equivalent only when canonical identity includes every
output-affecting and isolation member:

- authorized scope, including opaque tenant and principal handles;
- backend artifact and complete capability descriptor;
- operation, task, question, ordered options or levels, candidates and their
  order, projection, canonical inputs and provenance-relevant revisions;
- preprocessing, tokenization, truncation, precision, backend configuration,
  requested privacy and served-identity requirement;
- plan step semantics and any calibration binding applied after inference; and
- protocol, adapter and key-schema versions.

Unknown or noncanonical members make the request ineligible. Digest equality
is verified against canonical key material where retained; a hash collision or
schema mismatch is a refusal, never a hit.

### 3.3 Runtime-planned batching

Only a backend that explicitly declares compatible batch semantics may receive
a general batch. Every member is independently admitted, reserved, bounded,
identified and attributable. Batch size, canonical bytes, members per scope,
queue delay and backend work are hard bounded.

The batch dispatch deadline is the earliest remaining member deadline after
the declared queue allowance. A member that expires or cancels before send is
removed without dispatch. After send, each member retains its own outcome,
cancellation state, charge share and remote-state liability. One malformed or
missing member does not silently poison or fill another. Backend-wide failure
is copied as an attributable failure for each dispatched member.

### 3.4 Concurrent duplicate suppression

Equivalent concurrent requests may join one in-flight call. Each waiter keeps
its own deadline, cancellation, evidence and budget decision. One waiter
leaving does not cancel the call while another remains. When none remain, the
runtime requests cancellation and records the acknowledged or unconfirmed
effect under the existing contract.

A late joiner never receives work completed before its own admission unless
the result has entered an eligible cache. Joining cannot evade queue or
concurrency limits. The shared-call id is unique and every waiter record names
it and the external attempt id.

### 3.5 Budgets, cost and liability

Before dispatch, exactly one charge ledger owns the full reservation. Other
members reserve their declared maximum exposure or are refused, then record a
shared reference rather than a second charge. The policy defines a
deterministic settlement allocation whose integer shares sum exactly to the
observed charge.

Unknown charge remains one outstanding liability linked from all affected
members. Cancellation, timeout, cache admission failure, process loss or sink
failure never converts it to zero. A cache hit has zero new backend charge but
retains the source entry identity; it does not rewrite the historical charge.

### 3.6 Memory and persistent caches

Memory caches are bounded by total canonical bytes, entry count, per-entry
bytes and per-scope bytes. Admission occurs only after output validation.
Eviction is deterministic for a declared observation order and is recorded.
Expiry uses injected runtime time and cannot extend source freshness,
artifact validity, authorization scope or host retention.

Persistent caching is off by default and lives in runtime, never core. A
host-supplied store declares schema, namespace, durability, encryption,
atomicity, conflict, expiry, invalidation and erasure behavior. A stored entry
contains the complete key identity, validated raw output, derivation and
lineage references, creation and expiry inputs, and integrity digest. Missing,
corrupt, conflicting, stale or unverifiable entries are misses plus explicit
diagnostics, never values. Store failure follows a declared fail-closed or
continue-without-cache policy and never blocks evidence of actual inference.

### 3.7 Invalidation, erasure and races

Correction, revocation, erasure, artifact withdrawal, policy-version change
and scope revocation identify affected entries and in-flight generations.
Invalidation increments a namespace or key generation before removal. A result
may be stored only if its captured generation still matches at commit.

An in-flight pre-invalidation result may finish current waiters only when the
host's invalidation contract explicitly permits it; erasure and scope
revocation never do. It is never stored or served to a later request. Failed or
partial invalidation makes the affected namespace unavailable until reconciled.
Tombstones or equivalent generation state outlive any entry that could return.

### 3.8 Evidence, replay and evaluation

Every logical request records mechanism, key-schema version, hit or miss,
entry or shared-call id, batch id and member index where applicable, source
attempt, age, invalidation generation, charge ownership and allocation, and
every refusal or diagnostic. Evidence never includes secret key material or
unredacted cache content by default.

Offline replay does not consult a live cache or persistent store. It consumes
the retained logical outputs and optimization observations. Evaluation reports
cold and warm p50, p95 and p99 latency, throughput, hit and join rates, bytes,
evictions, invalidation lag, erasure completion, failures, unknown liability,
and judgment equivalence on named datasets and conditions. Missing measurement
is unknown.

## 4. Out of scope

Adapter-scoped batching changes; distributed caches;
cross-scope sharing; approximate or semantic cache keys; caching judgments or
authorizations; changing backend capabilities; provider qualification;
production enablement; performance or cost claims.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Two equal inputs have different authorized scopes | No batch, join or cache sharing. |
| One waiter cancels while another remains | The call continues only for the remaining waiter; records stay distinct. |
| All waiters leave after send | Cancellation is requested; charge and remote-state liability remain honest. |
| A result arrives after erasure invalidation | It is not stored or served later. |
| A persistent entry has a valid digest but old key schema | Miss and diagnostic, never migration by inference. |
| A batch omits one member response | That member is invalid output; siblings retain their own validated results. |
| Allocation rounding leaves a remainder | Deterministic shares still sum exactly to the observed charge. |
| Optimization is disabled | Existing unoptimized record bytes and behavior remain unchanged. |

## Acceptance

- One deterministic test covers each section 5 row without wall-clock sleeps.
- Differential tests compare optimized and independent execution across
  outputs, failures, cancellation, deadlines, fallback and evidence.
- Adversarial tests cover cross-scope keys, collision handling, invalidation
  races, process restart, corrupt persistent entries and unknown charge.
- Byte and entry bounds are enforced before admission, with negative controls.
- Existing spec 009 and 012 batching tests remain unchanged and are explicitly
  reported as special-case coverage only.
- Cold and warm benchmarks report measurements without converting them into an
  undeclared acceptance threshold.
- `make code`, declared verification and `make gate` pass.

## Verification

```verify:cli
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo clippy -p rustev-contract -p rustev-core -p rustev-runtime -p rustev-eval -p rustev-cli --all-targets --locked -- -D warnings
cargo test -p rustev-runtime --test optimization_benchmark --locked -- --ignored
```

## Implementation record

Settled question 1 lets the mechanisms land as separately reviewed
increments under this contract. Increment 1 implements runtime-planned
batching (3.3) and the bounded memory cache (3.6, memory part) with its
invalidation (3.7), their evidence (3.8) and evaluation support. Concurrent
duplicate suppression (3.4) and persistent caching (3.6, persistent part) are
increment 2 and are not implemented: the policy has no member for them, so no
plan can enable them. Increment 2 must settle how a shared call outlives an
owner that cancels while spec 003's rule that the runtime spawns no task per
request still holds.

- `rustev-contract` adds the identified optimization policy (key schema and
  namespace versions, admission, expiry basis, batch and memory-cache
  bounds) and the per-request `OptimizationRecord`, including every
  diagnostic up to a bound. Both members are optional and omitted when
  disabled; `an_execution_policy_parses_round_trips_and_is_identified` pins
  the exact pre-017 policy bytes and
  `disabled_optimization_adds_nothing_to_the_record` the record bytes.
  `SuppliedFrom::reused_from` names the source attempt of a cached output.
- `rustev-core` validates the policy: every bound positive, at least one
  mechanism, a batch of at least two members, and memory bounds that some
  entry can satisfy (`max_entry_bytes <= max_scope_bytes <= max_bytes`).
  `incomplete_or_unsatisfiable_optimization_policies_are_refused` has one row
  per refusal; `plan_identity_follows_the_optimization_policy` shows the
  policy is part of `PlanId`. Core stays free of I/O, clocks and cache state.
- The complete memory-cache key binds the plan, key-schema, namespace and
  runtime versions, the complete authorized-scope handle, the bound target's
  artifact and descriptor, the step, the instance and the canonical
  projection. A digest hit is compared with retained key material; a
  mismatch is a collision, refused or run independently as the policy says.
- Cache state is kept per plan namespace. Total-byte and count bounds evict
  that namespace's oldest entry; the per-scope bound evicts only the
  inserting scope's oldest entry, so one tenant's traffic never evicts
  another's and one plan's bounds never evict another plan's entries.
  Admission preconditions are checked before anything is evicted.
- Only a validated output of target 0 is cached, because the key names
  target 0; a fallback's output is supplied but never cached. A hit is
  revalidated by the core, records its entry, source attempt, age and
  generation, makes no attempt and has zero new charge.
- `Runtime::invalidate_plan` and `invalidate_scope` advance the namespace
  generation before removing entries. A request captures the generation
  before it runs; a result from an earlier generation is supplied to its own
  request but is never stored, so it is never served later. The memory cache
  dies with the process, so its generation state needs no durability.
- A general batch forms only among requests of one decision's admission
  wave whose steps allow one attempt, no timeout and no fallback, on a
  backend that declares compatible batch semantics, within every hard
  bound. Batch members count against `max_parallel_requests` until their
  batch ends, and no queue delay is taken because nothing can join.
- Each member is admitted and reserved, rechecked for deadline and
  cancellation after the permit wait, and concluded through the same code as
  an independent attempt: the same failure classes, `Unresolved` details,
  transitions, cancellation answers and remote states. A panic or top-level
  failure is attributed to every member. A missing or duplicate member
  response is that member's invalid output only.
- The charge owner (member 0) holds and settles the summed reservation
  against the batch total, so a share above one member's own bound is not a
  bound violation. A known charge is split evenly, remainder to the lowest
  indices, summing exactly. An unknown charge stays `Unknown` on every
  member, and every member links the one liability.
- Capture records a cache hit's source attempt as its origin, and replay
  accepts it; `a_run_served_from_the_memory_cache_reproduces_offline`
  captures a warm run and reproduces it without a backend.
- `rustev-eval::optimization` reports cold and warm p50, p95 and p99
  latency, throughput, hit and batch rates, failures and unknown liability,
  plus cache bytes, evictions, invalidation lag, erasure completion and
  judgment equivalence when the host supplies them. Anything unmeasured, and
  any metric over zero runs, is `unknown`.
- Section 5 rows in scope are covered in
  `crates/rustev-runtime/tests/optimization.rs`: different authorized
  scopes share nothing (`memory_cache_hits_expire_and_never_cross_scope`;
  a batch never spans decisions, so never scopes), a result arriving after
  erasure (`a_result_arriving_after_erasure_is_neither_stored_nor_served`),
  an omitted member (`a_missing_batch_member_fails_only_that_member`), the
  allocation remainder (`general_batch_is_one_dispatch_with_exact_member_evidence_and_allocation`
  and the driver's unit test) and disabled optimization. The waiter rows and
  the persistent-schema row belong to increment 2.
- Differential tests compare batched with independent execution for
  outputs, invalid output, permanent and transient failure, an unrequested
  cancel answer, deadline and cancellation after send, and warm with cold
  memory-cache execution; `an_output_from_a_fallback_target_is_not_cached`
  covers fallback. Adversarial coverage: cross-scope keys, collisions,
  cross-scope and cross-plan eviction, the invalidation race, and unknown
  charge. Existing specs 009 and 012 batching tests are unchanged and are
  special-case coverage only.
- `crates/rustev-runtime/tests/optimization_benchmark.rs` is an ignored local
  synthetic measurement of the memory cache with no performance assertion.
  On 2026-09-27, an Apple M1 Max, Darwin arm64, Rust 1.98.1 debug test run
  over 100 cases reported cold p50/p95/p99 909/1230/1391 microseconds and
  1043.30 runs per second, and warm 603/726/833 microseconds and 1584.78 runs
  per second, with 303 backend dispatches (300 cold, 3 to seed the warm
  entry, none warm). These numbers are not qualification, an acceptance
  threshold or a performance claim.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Which mechanisms, if any, should be approved together versus implemented in
   separately reviewed amendments under this one owning contract?
2. Is persistent caching justified by measured consumer need, and which host
   store contract can satisfy section 3.6 without adding I/O to core?
3. Which benchmark environments and thresholds may support later enablement?

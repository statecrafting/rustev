---
id: "017-general-runtime-optimization"
title: "General runtime batching, duplicate suppression, and caches"
status: approved
implementation: pending
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
(R-16); implementation is pending. Approval makes no latency, throughput,
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

Implementation; adapter-scoped batching changes; distributed caches;
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

Pending with implementation. Approval of this spec authorizes only the
contract. The implementation change must add repository-local verification for
every acceptance item before marking implementation complete.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Which mechanisms, if any, should be approved together versus implemented in
   separately reviewed amendments under this one owning contract?
2. Is persistent caching justified by measured consumer need, and which host
   store contract can satisfy section 3.6 without adding I/O to core?
3. Which benchmark environments and thresholds may support later enablement?

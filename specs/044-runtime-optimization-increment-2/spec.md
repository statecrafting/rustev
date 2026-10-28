---
id: "044-runtime-optimization-increment-2"
title: "Runtime optimization increment 2: duplicate suppression, persistent and decision-result caches, batching qualification (amends 017, 003, 002, 009, 012 and 020)"
status: approved
implementation: in-progress
created: "2026-10-17"
summary: >
  An amendment of approved spec 017 (and, through it, of spec 003's
  concurrency and cancellation rules) for #76 child F1, which also amends
  specs 002, 009, 012 and 020 where its members and batching rules reach
  them. Completes 017 increment 2 and adds a decision-result cache. Concurrent duplicate
  suppression keeps spec 003's rule that the runtime spawns no task per
  request: a shared attempt lives in a runtime-owned slot that is driven by
  whichever waiter is polled, a waiter that cancels returns at once and
  hands charge ownership to the oldest remaining waiter, and the last waiter
  to leave cancels the attempt under spec 003's existing rules. Persistent
  caches sit behind a host-implemented `RuntimeStore` seam with durable
  generations, conditional writes, byte budgets and integrity-checked
  `rustev.cache-entry/1` entries. The decision-result cache reuses a whole
  decision's semantic work under a compiler-derived key that always holds
  plan identity, the complete authorized scope, the profile and declared
  upstream-origin dimensions; on a hit the core recomputes the judgment and
  the runtime re-runs the compiler-derived dependency closure of every
  declared volatile source, with a declared policy when refresh fails that
  never serves stale evidence as a value. Capabilities may declare
  `isolation: per_item`, which every batching path honours, and
  adapter-scoped batching gains a qualification procedure and report
  (`rustev.batch-qualification/1`). Plans, descriptors and records that use
  none of this keep their bytes and identities. Approved (A-41);
  implementation is in progress.
amends:
  - "017-general-runtime-optimization"
  - "003-runtime-execution-and-evidence"
  - "002-decision-contract-and-pure-core"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "020-remote-protocol-refinements"
extends:
  # Policy members, the cache entry document and record members (spec 002
  # owns the contract crate; the isolation member is spec 035 3.2.2).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # Policy validation, the decision-cache key schema and refresh closure.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # Shared slots, the store seam, tiers, invalidation and the decision cache.
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Benchmarks and the batch qualification report.
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  # Per-item isolation in adapter-scoped batching (spec 009, 3.10.4).
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: amending }
  # The production batching posture and qualification harness.
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: amending }
  # The Node and WebAssembly declarations generated from the contract
  # schemas this spec's members change (spec 027, 3.2.3).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  # Archive policy entries and byte budgets for the new source files.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # The CLI reports every `PrepareError`, including `store_missing` and
  # `store_contract_mismatch` (3.4.1).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: additive }
  # Adds 044 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
  - "017-general-runtime-optimization"
  - "020-remote-protocol-refinements"
  - "023-real-data-quality-qualification"
  - "027-node-and-wasm-bindings"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "034-parameterized-definitions"
  - "035-screening-seam"
  - "036-mid-plan-acquisition"
  - "037-generation-steps"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Duplicate suppression spawns no task: a shared attempt is polled only from the tasks of its waiters, progresses whenever any remaining waiter is polled, and a waiter that cancels, times out or is dropped returns or drops without waiting for the attempt while another waiter remains."
    anchor: "3-2-concurrent-duplicate-suppression"
  - id: "I-2"
    kind: invariant
    text: "One shared attempt has exactly one charge owner at every instant and one settlement; a handoff moves ownership to a waiter that already holds the full reservation, and an unknown charge stays one liability on the final owner's record, linked by shared-call id from every waiter."
    anchor: "3-2-4-charge-reservation-and-liability"
  - id: "I-3"
    kind: invariant
    text: "Every cache key, request-level or decision-level, memory or persistent, is derived by Rustev from the plan's compiled key schema and always contains the plan identity, the complete authorized scope, the profile identity or `none`, and the key-schema and namespace versions; no host API accepts a key, and a request that cannot supply a declared key dimension is never served from or admitted to a cache."
    anchor: "3-5-key-discipline"
  - id: "I-4"
    kind: invariant
    text: "A persistent entry is served only after its schema, complete key material, integrity digest, generations, expiry and output validation all check; anything else is a miss with a diagnostic, never a value, and a write commits only if the store's generations still equal those captured before the work began."
    anchor: "3-4-runtime-store-and-persistent-entries"
  - id: "I-5"
    kind: invariant
    text: "A decision served from the decision-result cache has its judgment recomputed by the core from the new request's snapshot and evaluation time; every request in the refresh closure of a declared volatile source is dispatched again; and a failed refresh never supplies a value outside its declared freshness."
    anchor: "3-6-decision-result-cache"
  - id: "I-6"
    kind: invariant
    text: "A capability that declares `isolation: per_item` is never dispatched as part of any batch, runtime-planned or adapter-scoped, whatever the batching configuration."
    anchor: "3-7-per-item-isolation"
  - id: "R-1"
    kind: requirement
    text: "Plans, descriptors, run records and policies that use none of this spec's members keep byte-identical documents and identities, proved by the existing goldens."
    anchor: "3-10-compatibility"
  - id: "R-2"
    kind: requirement
    text: "Adapter-scoped batching on a production binding is constructible only with a cited `rustev.batch-qualification/1` report whose verdict is `qualified`; enabling it remains the owner's act under R-28."
    anchor: "3-8-adapter-scoped-batching-qualification"
  - id: "R-3"
    kind: requirement
    text: "The decision-result cache (3.6) is built on the versions it adds to, `rustev.plan/3` and `rustev.run/3` (both introduced by spec 032), and is not implemented before those versions are; specs 032, 034, 035, 036 and 037 are in this spec's `depends_on`."
    anchor: "3-1-policy-members-and-plan-identity"
---

# 044: Runtime optimization increment 2 (amends 017, 003, 002, 009, 012 and 020)

Approved by the owner (A-41, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements #76 child F1 (#75) and covers backlog
RUSTEV-005. The approved texts of specs 017 and 003 are not edited; this
spec records what changes, as 013 does for 003. It also amends spec 002
(contract members, the cache entry document, compiled key schemas and
refresh closures, section 2) and specs 009, 012 and 020 (per-item
isolation in their batching, 3.7, and the production batching posture,
3.8), without editing their approved texts. Approval makes no latency,
throughput, cost, durability, cache-safety or production claim, and enables
no adapter-scoped batching in production (R-28).

## 1. Purpose

Spec 017 increment 1 shipped runtime-planned batching and a bounded memory
cache. Its implementation record left increment 2 open on one question: how
a shared call outlives an owner that cancels while spec 003 3.4.3 says the
runtime spawns no task per request. A pre-split attempt at increment 2
kept the dispatching decision alive to drive the call after its caller
cancelled, which the review of PR #46 found in conflict with spec 003.

This spec settles that question without relaxing spec 003 and completes the
increment:

1. concurrent duplicate suppression (017 3.4);
2. persistent caches behind a host-implemented store (017 3.6, persistent
   part);
3. a decision-result cache with refresh of declared volatile sources, which
   017 section 4 excluded as "caching judgments" and #75 now requires;
4. compiler-derived keys for every cache;
5. per-item isolation and the qualification of adapter-scoped batching
   (RUSTEV-005); and
6. cold and warm measurements for every mechanism (017 3.8).

## 2. Territory

When delivered:

- `crates/rustev-contract/` (owner 002): new optional members of
  `OptimizationPolicy`, `OptimizationRecord`, `RunRecord` and the capability
  descriptor; the documents `rustev.cache-entry/1` and the store contract
  inside the policy.
- `crates/rustev-core/` (owner 002): validation of the new policy members,
  the decision-cache key schema and the refresh closure, both derived at
  compile time and placed in the plan. Core stays free of I/O, clocks,
  cache state and async runtimes.
- `crates/rustev-runtime/` (owner 003): shared slots, the `RuntimeStore`
  seam, the persistent tier, invalidation across tiers and the
  decision-result cache.
- `crates/rustev-eval/` (owner 004): benchmarks and
  `rustev.batch-qualification/1`.
- `integrations/rustev-remote-http/` (owner 009) and
  `integrations/rustev-jev/` (owner 012): per-item isolation in
  adapter-scoped batching and the production batching posture.
- `integrations/rustev-node/` and `integrations/rustev-wasm/` (owner 027):
  only their generated `types/rustev-documents.d.ts`, regenerated from the
  contract schemas the new members change.
- `tools/rustev-release/` (owner 031): the archive policy entry of each
  new source file in the crates above, and their archive byte budgets.
- `Makefile` (owner 001): 044 joins `make verify` at completion.

No new dependency family. The store seam is a Rust trait; the Node binding
exposes it as a host-implemented seam under spec 027 3.6, which binds every
later seam, and a remote store belongs to the protocol of #76 child A2.
Neither is specified here.

### 2.1 Binding principles of #76

- **Proposals, never grants.** Cached and shared values are raw outputs,
  never authorizations. No entry, store, key or record is accepted by an
  executor, and no `Permitted` type exists. Spec 017 section 4's exclusion of
  caching authorizations stands.
- **Derivation classes and lineage.** A reused output keeps its derivation
  class and names its source attempt (017 `SuppliedFrom::reused_from`); a
  decision served from cache names its source decision's run record digest;
  generated text keeps its original origin (3.6.6).
- **Unresolved, never a default.** Missing, corrupt, expired, conflicting
  or unverifiable entries are misses. A failed refresh supplies either the
  failure itself or a value still inside its declared freshness, which the
  core's existing freshness check turns into `stale_evidence` when it is
  not (3.6.5). No value kind converts.
- **Core purity.** Keys and closures are computed by the compiler from
  declarations; lookups, stores and clocks are runtime matters.
- **Integrations only.** No HTTP stack, cloud SDK, Node-API or telemetry
  crate enters contract, core or runtime; `make boundaries` is unchanged.
- **No dynamic plugin loading.** A store is a value the host passes to the
  runtime builder (OD-5, R-38).
- **Rustev owns no store.** The host owns the persistent store, its
  durability, encryption and retention; Rustev owns only the entry format
  and the checks it applies.

## 3. Behavior

### 3.1 Policy members and plan identity

`OptimizationPolicy` gains three optional members, each omitted when
absent:

1. `shared_calls`: `max_shared_calls` (per runtime), `max_waiters_per_call`
   and `allocation`, whose only value in this increment is `final_owner`
   (3.2.4).
2. `persistent_cache`: the host's `store_contract` (schema, namespace,
   durability, encryption, atomicity, conflict, expiry, invalidation and
   erasure terms, each a declared string recorded as given), `max_entry_bytes`,
   `max_scope_bytes`, `max_namespace_bytes`, `ttl_ms` and `on_store_failure`
   (`fail_closed` or `continue_without_cache`).
3. `decision_cache`: `tiers` (`memory`, `persistent` or both),
   `max_entry_bytes`, `max_entries` for the memory tier, `ttl_ms`,
   `volatile` (3.6.2), `key_dimensions` (3.5) and `on_refresh_failure`
   (3.6.5).

The compiler refuses an incomplete or unsatisfiable member (3.9) and, for
`decision_cache`, writes the derived key schema and refresh closure into the
plan's execution section. Each member, its derived parts and any change to
them are part of `PlanId`. Document versions follow the #76 coordination
rule: `shared_calls` and `persistent_cache` complete 017 and, like 017's
increment 1 members, are optional members of the existing plan version and of the run record a
plan 2 decision already produces, `rustev.run/1` or spec 020's
`rustev.run/2` (version 1 plus `remote_evidence`); a plan 3 decision
carries them in `rustev.run/3` (version 1 with evidence 2, spec 032 3.10),
and no further run version is introduced; `decision_cache` is a #76 construct, so a plan
that uses it is `rustev.plan/3` (this spec adds to `rustev.plan/3`,
introduced by spec 032), and its run records are `rustev.run/3` (adds to
`rustev.run/3`, introduced by spec 032). Store contents, generations,
hits, joins and handoffs are runtime observations and change no identity.
017 3.1's rule that the policy names every mechanism and bound is
unchanged.

Through these versions, spec 032 is a dependency of the decision-result
cache, and spec 036 is one through acquisition steps as volatile sources
(3.6.2); both are in `depends_on`. The decision-result cache (3.6) is not
implemented before spec 032's versions are (R-3); shared calls, the
persistent tier, per-item isolation and the batching qualification depend
on neither and may be implemented first.

Spec 037 (generation steps, #76 child B5) is referenced only where its
constructs would meet the decision cache: generation steps fall inside the
refresh closure by the general rule (3.6.2), and generated text keeps its
original origin (3.6.6). Neither rule needs 037 to be implemented, so the
decision-result cache does not wait for it; 037 is in `depends_on`
because those rules name its constructs.

### 3.2 Concurrent duplicate suppression

This section replaces 017 3.4's implementation question with a design, and
amends spec 003 3.4.3, 3.4.5 and 3.7.1 for shared attempts only.

#### 3.2.1 Eligibility

1. Two requests may share one external attempt only when their 017
   request-level keys (3.5) are equal, including the dispatch target. The
   key covers what #75 names (artifact, projected input digest, tenant,
   plan) and every other member of 017 3.2.
2. A request joins only an attempt that is in flight: dispatched and not
   yet observed complete. A late request never receives a completed
   attempt's output except through a cache (017 3.4).
3. A request joins only when the shared attempt's dispatched deadline is at
   or after the joiner's own deadline, so no waiter receives less time than
   it would alone; otherwise it runs independently.
4. A joiner holds its own `max_parallel_requests` slot while it waits
   (spec 003 3.4.2) and reserves its own full exposure before joining
   (017 3.5); a request that cannot reserve is refused as an independent
   request would be. It holds no backend permit; the attempt holds one.
5. `max_waiters_per_call` or `max_shared_calls` reached makes the request
   ineligible, handled by the policy's existing `admission`
   (`execute_independent` or `refuse`) with a diagnostic.
6. A request that leads a shared call is dispatched unbatched. A batch
   member is never joinable.

#### 3.2.2 The shared slot

1. The first eligible request creates a slot in the runtime's bounded
   shared-call registry and becomes its first waiter. The slot owns the
   attempt future, the attempt's backend permit, its own cancellation
   signal and the waiter list. It is not a child of any caller's signal.
2. Every waiter's future polls the slot. Polling the slot polls the
   attempt future under mutual exclusion; a waiter that finds the attempt
   being polled registers its waker and returns pending. The waker given to
   the attempt future wakes every waiter that is still registered, so the
   departure of any waiter, including the one that dispatched, never
   strands the attempt.
3. No task is spawned. The attempt is polled only from the caller tasks of
   its waiters, each of which already exists under spec 003 3.4.3. The
   number of slots is bounded by `max_shared_calls` and by the number of
   requests in progress, so 003 3.4.3's bound on waiting futures holds.
4. A panic in the attempt is contained at the attempt as spec 003 3.4.4
   says, whichever waiter's task polled it, and is the outcome of every
   waiter still registered.
5. When the attempt completes, every registered waiter receives the same
   report. Each waiter then applies its own plan's validation, retry and
   fallback rules exactly as an independent attempt would; equivalent
   retries may share again.
6. The slot is removed from the registry when its attempt completes or its
   last waiter leaves. Removal precedes any cache admission, so a late
   request can find only a cache entry.

#### 3.2.3 Leaving, handoff and the last waiter

1. A waiter leaves when its caller cancels, its own deadline passes, or its
   future is dropped. A waiter that leaves while another remains does not
   raise the attempt's signal and does not wait for the attempt: it returns
   at once with its own outcome (`cancelled` or `deadline_exceeded`
   following spec 003 3.3.4 and 3.7.4). Its run record is built and
   delivered as usual; a dropped waiter gets no record (003 3.4.5).
2. If the leaving waiter is the charge owner, ownership passes in the same
   step to the remaining waiter that joined earliest. Its reservation
   becomes the owning reservation, and the leaver's reservation is released
   without a charge. The handoff is recorded on the new owner, and on the
   leaver too unless it was dropped, which has no run record (item 1,
   003 3.4.5).
3. The last waiter to leave raises the slot's signal and polls the attempt
   once more, on its own task, exactly as spec 003 3.7.2 says: a report
   returned by that poll is recorded with its charge and acknowledgement;
   if the poll is still pending, the waiter stops waiting, drops the
   attempt future and records the cancellation as requested with no answer
   observed, remote state `possibly_continuing` and the reservation as
   retained liability (003 3.7.3, spec 013). Either way it records before
   it returns and never waits beyond that one poll. If it leaves by being dropped, the attempt future is dropped
   with it and its ledger settles the reservation as unknown liability,
   counted `abandoned`, as 003 3.4.5 says.
4. The shared attempt's own timeout is the plan's attempt timeout from
   dispatch; at most it ends at the dispatched deadline, which by 3.2.1.3 is
   at or after every waiter's deadline.
5. The call context sent to the adapter (spec 009) names the dispatching
   decision. It is not rewritten on handoff; the records link the shared
   call id.

These rules satisfy spec 003 as written: no spawned task, a cancelled
decision returns without waiting for remote work, and cancellation is
requested only when no waiter remains (003 3.10.2). They amend 003 3.7.1 (a
shared attempt's signal belongs to its slot, raised by the last waiter or
the attempt timeout) and 003 3.4.5 (a dropped owner hands off instead of
settling while another waiter remains).

#### 3.2.4 Charge, reservation and liability

1. Allocation `final_owner`: the waiter that owns the attempt when it ends
   settles the whole observed charge against its reservation. Every other
   waiter records a shared reference and an allocated share of 0; shares
   therefore sum exactly to the charge (017 3.5).
2. An unknown charge is one liability on the final owner's record. Every
   waiter's record names the shared call id and the external attempt id, so
   the liability is reachable from all of them and counted once.
3. A handoff never refunds, rewrites or duplicates a charge. A waiter that
   handed off records zero charge and the successor's waiter id.

#### 3.2.5 Evidence

`OptimizationMechanism` gains `shared_call`; `OptimizationRecord` gains an
optional `shared` member: `shared_call_id`, `attempt_id`, the waiter's own
`waiter_id` (an opaque runtime-generated id, never a decision id),
`role` (`led` or `joined`), `left` (`cancelled`, `deadline` or absent),
`handed_off_to` and `received_from` (waiter ids) and `final_owner`
(boolean). A joiner's request record has no attempt of its own; its
supplied output names the shared attempt as its origin, as a cache hit names
its source attempt.

### 3.3 Tiers and lookup order

A request-level lookup tries, in order: the memory cache, the persistent
cache, an in-flight shared call, then dispatch. A persistent hit is admitted
to the memory cache under its bounds. Each tier is optional and governed by
its own policy member. Offline replay consults no tier (017 3.8).

### 3.4 Runtime store and persistent entries

#### 3.4.1 The seam

`RuntimeStore` is a host-implemented trait in `rustev-runtime`, passed to
the runtime builder. Every method is asynchronous; a call made for a
decision receives its remaining deadline and a cancellation signal and is
polled on that decision's task:

| Method | Meaning |
|---|---|
| `contract()` | The store contract; must equal the plan's `store_contract`, or `prepare` fails with `store_contract_mismatch`. |
| `get(namespace, key_id)` | The stored bytes or none, and the namespace and scope generations read atomically with them. |
| `put(namespace, key_id, bytes, terms)` | Store only if the generations in `terms` still hold, within the byte budgets in `terms`; answers `stored`, `generation_advanced`, `over_budget` or `conflict`. `terms` carries the expiry (3.4.3) as a reclamation hint. |
| `delete(namespace, key_id)` | Remove one entry (corrupt entries, erasure). |
| `invalidate(namespace, selector)` | Atomically advance the selected generation (namespace or scope) and make covered entries unreadable; answers the new generation and the count removed, or a failure. |

`put` answers: `stored`, the entry is written; `generation_advanced`, a
generation moved since the work began, so nothing is written (3.4.3);
`over_budget`, a scope or namespace budget would be exceeded (3.4.4);
`conflict`, another writer stored an entry for the same key first, and
the store keeps whichever entry its contract's `conflict` term names, so
this write may not be the stored one. Every answer other than `stored` is
an admission not made, recorded with the diagnostic of the same name
(`store_conflict` for `conflict`); it is never retried within the
decision and never changes the validated output or judgment. A conflict
is harmless to correctness, because any stored entry is still checked in
full on read (3.4.3).

A store failure, timeout or cancellation is a `StoreFailure` with a
detail. It never fails an inference that already happened. A plan whose
policy enables the persistent tier (3.3) or a persistent decision-cache
tier, prepared on a runtime built without a `RuntimeStore`, fails
`prepare` with `store_missing`; a plan that uses neither needs no store.

#### 3.4.2 Store failure policy

`fail_closed`: a read failure refuses the request under the plan's
`admission` policy; a write failure is recorded and the request keeps its
validated output. `continue_without_cache`: the request proceeds as a miss
and the failure is a diagnostic. Store time counts against the decision
deadline; a store call is not an attempt and has no charge.

#### 3.4.3 Entry document and checks

An entry is the canonical bytes of `rustev.cache-entry/1`: `kind`
(`request` or `decision`), the complete canonical key material and its
digest, the key-schema and namespace versions, the runtime version, the
namespace and scope generations captured before the work began, the
creation evaluation time, `ttl_ms`, the payload, the source run record
digest (the evidence digest) and an integrity digest over everything else.
A `request` payload holds the validated raw output, its target, derivation
and lineage references and source attempt id. A `decision` payload is
3.6.4's.

Persistent expiry uses evaluation time, because the runtime's monotonic
clock does not survive a restart: an entry is expired when the request's
evaluation time is more than `ttl_ms` after the creation evaluation time,
and is a miss (`entry_from_future`) when the evaluation time is before it.
The memory tier keeps 017's injected runtime time.

A read is a hit only if, in order: the schema is known, the integrity
digest matches, the retained key material equals the request's (a mismatch
is a collision, 017 3.2), both generations equal the store's current ones,
the entry is unexpired, and the core validates the output. Each failure is
a miss with a diagnostic, and a corrupt entry is deleted. An old key schema
is a miss, never migrated (017 section 5).

#### 3.4.4 Budgets

`max_entry_bytes` is checked by the runtime before `put`. The scope and
namespace budgets travel in `terms`; the store enforces them and answers
`over_budget`, which is a refused admission and a diagnostic. Eviction
inside the store is the store's, declared in its contract.

### 3.5 Key discipline

1. Rustev derives every key; no runtime, binding or seam API accepts one.
   The request-level key schema is 017's (implementation record), extended
   by the profile member below. The decision-level schema is compiled from
   the plan (3.6.1).
2. Every key holds: `PlanId`; the complete authorized scope (`Scope`, both
   modes, so a tenant is always present and a principal whenever the mode
   has one); the profile identity of the plan, or the literal `none` when
   the plan has no profile (a profile under spec 034, #76 child B4, also changes
   `PlanId`, and is held separately so no profile can collide with none);
   and the key-schema, namespace and runtime versions.
3. `key_dimensions` names upstream-origin dimensions a definition's output
   depends on that are not inputs (for example a catalogue revision). The
   host supplies each as a scope `Handle` on the decision request. A
   request missing any declared dimension is not served from and not
   admitted to the decision cache, under the plan's `admission` policy,
   with the diagnostic `key_dimension_missing`.
4. Every key is checked against retained key material on read (3.4.3); a
   key whose material contains an unknown or noncanonical member is
   ineligible (017 3.2).

### 3.6 Decision-result cache

#### 3.6.1 Key

The decision-level key is the 3.5 members plus the canonical digest of every
snapshot entry the plan reads for its non-volatile inputs (value,
provenance class and as-of time, as supplied), in input declaration order.
Volatile inputs are excluded, so a changed live value can still hit. The
compiler emits the ordered list of key members as the plan's
`decision_key_schema`.

#### 3.6.2 Volatile sources and the refresh closure

`volatile` lists input names. When the plan language gains mid-plan
acquisitions (spec 036, #76 child B1), an acquisition step may also be listed, and
its captured pages are refreshed by dispatching it again. The compiler
refuses an unknown name and a volatile input whose freshness is
`not_required`, because a value that may change between requests needs a
freshness bound.

The refresh closure is the set of steps that read a volatile source
directly or through any other step, computed by the compiler over the
plan's dependency graph and written into the plan as the ordered list of
step ids. Joins, corroboration (spec 032, #76 child B2), generation (spec
037, #76 child B5)
and semantic steps fall inside it by the same rule. Exact steps are
recomputed on every hit wherever they sit, because the core is pure and
recomputing costs no external work; the closure therefore governs which
external work is done again.

#### 3.6.3 Lookup and serving

1. After admission, the runtime derives the key and reads the configured
   tiers before starting the core. A miss runs the decision normally.
2. On a hit the runtime starts the core over the new request's snapshot and
   evaluation time. The core only emits requests and validates what it is
   supplied; key comparison and the choice between the entry and dispatch
   are the runtime's (2.1). For each request the core emits:
   - outside the closure: the runtime supplies the entry's output only if
     the request's 017 key equals the one stored for that step and
     instance; otherwise that request alone is dispatched through the
     normal path, as if it were inside the closure, the diagnostic
     `closure_mismatch` is recorded and its index is listed with the
     mismatched requests, separately from the refreshed ones (3.6.6),
     while the other requests outside the closure are still supplied from
     the entry;
   - inside the closure: the request is dispatched through the normal path
     (which may itself share or use request-level caches).
3. The core validates every supplied output, applies freshness at the new
   evaluation time and finishes the judgment. The served judgment is
   always this recomputation, never stored bytes, so I-1 of 017 holds:
   with the same snapshot, evaluation time and outputs the judgment and
   core evidence equal independent execution.

#### 3.6.4 Admission

A judged decision is admitted only when every request outside the closure
was supplied a validated output from target 0 without a fallback
transition (017's rule for request-level entries). The `decision` payload
holds, per request outside the closure, its step, instance, 017 key,
output, derivation, lineage and source attempt; the stored volatile
snapshot entries for 3.6.5; and the source decision's judgment digest and
run record digest. Requests inside the closure are not stored. Admission
uses the generations captured when the decision started (3.4.3).
A decision that was served from an entry (3.6.3), including one with a
`closure_mismatch` or a refreshed volatile source, is never admitted: the
entry it used stays as it is, and a new entry is written only by a
decision that ran as a miss or under `recompute` (3.6.5).

#### 3.6.5 Refresh failure

A refresh fails when a volatile input has no accepted entry in the new
snapshot, a refreshed acquisition fails, or a refreshed request fails.
`on_refresh_failure` decides:

- `unresolved` (the default): the failure is supplied as it is; the core
  yields `Unresolved` exactly as independent execution would.
- `recompute`: the hit is abandoned and the decision runs again with no
  reuse from the entry, within the same deadline and budget; the abandoned
  attempts stay in the run record with their charges.
- `serve_cached_within_freshness`: for a volatile input only, the stored
  snapshot entry is supplied with its original as-of time, and the core's
  freshness check decides. Inside its declared maximum age the value is
  served and the record carries the notice `refresh_failed`; outside it the
  value is `stale_evidence`, as for any stale input. A refreshed request or
  acquisition failure falls back to `unresolved` under this policy.

No policy supplies a value older than its declared freshness or turns a
failure into a value.

#### 3.6.6 Evidence and origins

The run record gains an optional `decision_cache` member (in
`rustev.run/3`, 3.1): outcome, entry id, source run record digest, age,
generations, the reused request indices, the refreshed request indices
(inside the closure), the mismatched request indices (`closure_mismatch`,
3.6.3), any substituted volatile inputs with their entry digests, the
refresh-failure policy applied and diagnostics (bounded as 017's). The
outcome is exactly one of:

- `hit`: served from an entry (3.6.3) with no request dispatched again,
  because the closure is empty and no request had a `closure_mismatch`;
- `refreshed`: served from an entry with at least one request dispatched
  again, inside the closure or for a `closure_mismatch`; the two index
  lists say which;
- `miss`: no usable entry, or a hit abandoned under `recompute` (3.6.5,
  with the diagnostic `refresh_failed`); the decision ran normally and may
  be admitted (3.6.4);
- `bypassed`: the decision was neither served from nor admitted to the
  cache and ran independently under `admission: execute_independent`
  (`key_dimension_missing`, 3.5, or a store read failure under `fail_closed`, 3.4.2; under
  `continue_without_cache` a read failure is a `miss`);
- `refused`: the same conditions under `admission: refuse`.

`refreshed`, not `hit`, is the normal outcome for a plan with volatile
sources, because their closure is always dispatched again (3.6.3). It does
not imply external work: each refreshed request may still be answered by a
shared call or a request-level cache, and its own 017 record says which,
so the external calls a decision made are read from those records, not
from this outcome.
A served decision (`hit` or `refreshed`) is never admitted (3.6.4).

The runtime's decision result gains `served_from_cache` with the same entry id and source digest.
Reused outputs name their source attempts, so per-field origin computed by
the core from them (including the origin of generated text under spec 037) is the original origin, and a deterministic backstop stays
deterministic. A hit has zero new charge for reused requests.

### 3.7 Per-item isolation

Spec 035 (3.2.2) defines the general descriptor member `isolation`
(`per_item`, or the default `shared`, omitted so existing descriptors and
artifact identities keep their bytes) and applies it to screeners; this
spec applies it to every other capability and batching path. A per-item capability is never a member of a
runtime-planned batch (017 3.3), an adapter-scoped batch (spec 009
3.10.4, spec 012) or a spec 020 remote batch, whatever the batching
configuration; each request is its own call. Shared calls and caches are
unaffected, because they reuse one request's whole result. Spec 035's
screener rule (3.10) and spec 022 3.8, item 4 are instances of the same
member, and any later per-item isolation uses this name.

### 3.8 Adapter-scoped batching qualification

This completes RUSTEV-005 for spec 009 and 012 shared-state batching.

1. **Stage 1, deterministic.** Over loopback with scripted responses:
   batch bounds, member attribution, a missing or malformed member, partial
   member liability, one member cancelled before and after send, member
   deadlines, settlement with remainders and unknown charge, every failure
   class, and per-item isolation. No network.
2. **Stage 2, live.** On synthetic or spec 023 admitted data only, within
   R-29's testing cap and the persistent spend journal: the same cases
   batched and unbatched, comparing answers under an agreement bound and a
   minimum case count declared before the run.
3. **Report.** `rustev.batch-qualification/1` (in `rustev-eval`) records
   the binding identities, the declared bound and minimum, stage 1
   results, per-case paired answers, spend, and a verdict: `qualified` only
   when stage 1 passes and stage 2 meets the declared bound over at least
   the minimum; `not_qualified` when a measured value fails; `unknown`
   otherwise.
4. **Posture.** A `production` binding (spec 022) that enables
   adapter-scoped batching must cite the digest of a `qualified` report for
   the same binding identity apart from the batching switch; otherwise
   construction fails with `batch_unqualified`. Citing a report grants
   nothing: enabling production batching stays the owner's act (R-28).

### 3.9 Refusal and diagnostic codes

Compile refusals are category 12 (`invalid_execution`) with the code as
the detail's first token: `shared_call_bounds`, `persistent_bounds`,
`store_contract_incomplete`, `decision_cache_bounds`,
`volatile_unknown_source`, `volatile_without_freshness`,
`key_dimension_invalid`. Preparation and construction: `store_contract_mismatch`,
`store_missing` (a plan with a persistent tier and no store),
`batch_unqualified`. Runtime diagnostics: `key_dimension_missing`,
`closure_mismatch`, `refresh_failed`, `entry_from_future`,
`entry_corrupt`, `entry_schema_unknown`, `generation_advanced`,
`over_budget`, `store_conflict`, `store_failed`, `shared_full`.

### 3.10 Compatibility

1. Every new member is optional and omitted when absent: policies, plans,
   `PlanId`s, descriptors, artifact identities and run records that do not
   use them keep their versions and bytes, and a plan without
   `decision_cache` never becomes `rustev.plan/3` because of this spec; the existing pre-017 and 017
   goldens stay byte-identical, and new goldens fix one plan per new member.
2. As with 017's `optimization` member, a record that uses a new member is
   rejected by a reader built before this spec; that is intended.
3. `Runtime::invalidate_plan` and `invalidate_scope` keep their signatures
   for the memory tiers. On a runtime with a store they also mark the
   affected persistent namespace or scope unavailable (reads miss, writes
   refuse) until the new asynchronous `Runtime::invalidate` has completed
   for it. `InvalidationResult` gains per-tier results and `complete`; a
   failed tier leaves the selection unavailable until reconciled (017
   3.7).
4. Rustev does not carry invalidation between processes. Store generations
   protect persistent entries everywhere; each process's memory tiers are
   invalidated by the host's call in that process, as the store contract's
   `invalidation` term records.
5. Specs 009 and 012 batching tests stay unchanged and remain special-case
   coverage; 3.8 adds qualification beside them.

## 4. Out of scope

Distributed or cross-process invalidation; a remote store protocol (#76
child A2) and the Node exposure of the store (spec 027); caching
authorizations; cross-scope sharing; approximate keys; serving stored
judgment bytes; migrating entries across key schemas; changing capability
matching or value kinds; production enablement of any mechanism; any
latency, cost or quality claim.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The dispatching waiter cancels while a joiner remains | It returns `cancelled` at once with no charge; ownership passes to the joiner; the attempt completes for the joiner; no task was spawned. |
| The last waiter cancels | It raises the slot signal, polls once, records the answer and charge or liability, returns `cancelled` with its run record. |
| The last waiter's deadline passes | As for cancel, but it returns `deadline_exceeded` (spec 003 3.3.4). |
| The last waiter's future is dropped | No signal is raised and no run record is built; the attempt future is dropped with it and the reservation settles as unknown liability, counted `abandoned` (003 3.4.5). |
| The charge owner's future is dropped while a joiner remains | Handoff; the joiner settles; no unknown liability is recorded for the dropped waiter. |
| A request arrives after the shared attempt completed | It does not join; it hits a cache or dispatches. |
| A joiner's deadline is later than the attempt's dispatched deadline | It runs independently. |
| Two tenants send identical requests and decisions | No join and no hit, at every tier and in the decision cache. |
| Same tenant, different profile | No decision-cache hit. |
| A declared key dimension is missing | Not served from or admitted to the decision cache; diagnostic. |
| A volatile input changes between two identical decisions | Hit; the closure is dispatched again; the judgment reflects the new value. |
| Refresh fails under `serve_cached_within_freshness`, value inside its maximum age | Served with `refresh_failed`; past the maximum age it is `stale_evidence`. |
| A refreshed request or acquisition fails under `serve_cached_within_freshness` | Nothing stored is supplied for it; the failure is supplied as under `unresolved`, and the core yields `Unresolved`. |
| Refresh fails under `recompute` | Full independent run; abandoned attempts and charges stay recorded. |
| A stored request key differs from the new one outside the closure | That request alone is dispatched, the others are supplied from the entry; `closure_mismatch`; the decision is not admitted. |
| A persistent entry is corrupt (integrity digest mismatch) | Miss with `entry_corrupt`; the entry is deleted. |
| A persistent entry is from the future, of an old schema, or older than the store generation | Miss with its diagnostic; the entry is kept (expiry and generations are the store's to reclaim). |
| Invalidation advances the generation while a decision runs | Its result is not stored (`generation_advanced`). |
| Two processes store an entry for the same key at once | One `put` answers `conflict`; that decision records `store_conflict` and keeps its output; the stored entry is the one the contract's `conflict` term names. |
| The store fails under `fail_closed` | Read: refused per `admission`; write: recorded, output kept. |
| A runtime restarts with the same store | Warm hits for unexpired, current-generation entries. |
| A per-item capability with batching enabled | Every request dispatched alone. |
| A production binding enables batching without a qualified report | Refused with `batch_unqualified`. |
| No new member is used | Existing bytes and identities unchanged. |

## Acceptance

- One deterministic test per section 5 row, with a manual clock and a
  manual executor, no wall-clock sleeps, and a spawn counter showing zero
  tasks spawned by shared calls.
- Last-waiter cancellation tests for each leave reason (cancel, deadline,
  drop) and each cancellation answer of spec 003 3.7.3.
- Cross-tenant and cross-profile isolation negative tests at every tier.
- Refresh-on-hit tests with a changed volatile value, and with each
  refresh-failure policy.
- Persistence across a process restart, modelled as a new runtime over the
  same test store, including generation advance between the two.
- Differential tests: shared, persistent-hit and decision-cache-hit
  execution against independent execution for outputs, failures,
  cancellation, deadlines, fallback and core evidence.
- Golden refusals for every 3.9 compile and preparation code.
- Stage 1 of 3.8 for the remote-http and Jev adapters; stage 2 only under
  the work order's live scope, recorded with spend in the implementation
  record.
- Benchmarks report cold and warm p50, p95 and p99 latency and throughput
  for shared calls, the persistent tier and the decision cache, under the
  declared admission and concurrency limits recorded beside them, without
  an acceptance threshold (017 3.8, R-34).
- Existing 009, 012 and 017 tests unchanged; `make code`, declared
  verification and `make gate` pass.
- The decision-result cache is implemented on spec 032's `rustev.plan/3`
  and `rustev.run/3`, not before them (R-3, 3.1).

## Verification

Pending with implementation. The implementing change adds 044 to `make
verify` with repository-local deterministic verification for every
acceptance item. Approval satisfies no acceptance item.

**Owner decision A-49 (2026-10-23): dependencies.** Spec 022 is no longer
a declared dependency of this spec; spec 012 remains one. Only the
production batching posture needs it: 3.8 item 4 and R-2 apply to "a
`production` binding (spec 022)", which spec 022 defines. If this spec is
implemented before spec 022 is complete, 3.8 item 4, R-2's construction
check (`batch_unqualified`) and section 5's row "A production binding
enables batching without a qualified report" move to a later increment of
this spec made after spec 022 is complete, and this spec is not recorded
`complete` until that increment lands. Until then no binding enables
adapter-scoped batching in production (R-28 unchanged). Every other item,
including stage 1 of 3.8 for the Jev adapter, is unchanged.

## Implementation record

Planned increments, each a separately reviewed pull request of about
1,000 lines. Ordinals are build order; increment 1 depends on nothing
else here, and each later one needs increment 2a.

1. Per-item isolation (3.7) on every batching path: runtime-planned
   batching (017 3.3), which today excludes only per-item screeners,
   adapter-scoped batching in `rustev-remote-http` (009 3.10.4) and the
   Jev adapter (012), and the spec 020 remote batch. One test per path;
   section 5 row "A per-item capability with batching enabled". No
   contract member is added: `isolation` is spec 035's.
2. Contract and core members, in two parts. (a) `shared_calls` and
   `persistent_cache` in `OptimizationPolicy`, the `shared_call`
   mechanism and the `shared` member of `OptimizationRecord` (3.2.5),
   `rustev.cache-entry/1` (3.4.3), the compile refusals
   `shared_call_bounds`, `persistent_bounds` and
   `store_contract_incomplete`, and goldens showing that a policy without
   them keeps its bytes and `PlanId`. (b) `decision_cache` in
   `rustev.plan/3`: validation (`decision_cache_bounds`,
   `volatile_unknown_source`, `volatile_without_freshness`,
   `key_dimension_invalid`), the compiled `decision_key_schema` (3.6.1)
   and refresh closure (3.6.2) in the plan's execution section, and the
   `decision_cache` member of `rustev.run/3` (3.6.6). No runtime behavior
   yet.
3. Concurrent duplicate suppression (3.2), in two parts. (a) The bounded
   shared-call registry, the slot with its broadcast waker,
   eligibility (3.2.1), completion for every waiter and slot removal
   before cache admission (3.2.2). (b) Leaving, handoff and the last
   waiter (3.2.3), `final_owner` charges and liability (3.2.4) and the
   records (3.2.5). Tests run on a manual executor with a spawn counter
   and a manual clock; they cover the waiter rows of section 5 and each
   spec 003 3.7.3 cancellation answer.
4. The `RuntimeStore` seam and the persistent tier (3.3, 3.4), the
   request-level key's profile member (3.5) and invalidation across tiers
   (3.10.3, 3.10.4): `store_missing`, `store_contract_mismatch`, every
   entry check and its diagnostic, budgets, both store failure policies
   and a restart modelled as a new runtime over the same test store.
5. The decision-result cache (3.5, 3.6): key dimensions, lookup and
   serving, closure dispatch and `closure_mismatch`, admission, the three
   refresh-failure policies, the run record member and
   `served_from_cache`.
6. Stage 1 of the batching qualification (3.8 items 1 and 3) for the
   remote-http and Jev adapters, and `rustev.batch-qualification/1` in
   `rustev-eval`. Stage 2 (item 2) runs only under the work order's live
   scope; until then a report's verdict is `unknown`.
7. Cold and warm benchmarks (Acceptance), declared verification and the
   Verification section. 044 joins `make verify` here, and stays
   `in-progress`.
8. After spec 022 is complete (A-49): 3.8 item 4, R-2's construction
   check `batch_unqualified` and its section 5 row. This increment is
   externally blocked on spec 022, and this spec is not recorded
   `complete` before it lands.

Decisions within the spec, made for this plan:

- **Key dimensions travel beside the request, not in it.** The host
  supplies 3.5 item 3's handles through a new `Runtime` entry point taking
  the decision request and its dimensions, so `DecisionRequest`, built by
  literal in a dozen crates outside this spec's territory, keeps its
  fields. A plan without `decision_cache` never needs the new entry
  point.
- **Remote-only paths stay in their integrations.** Per-item isolation in
  adapter-scoped and remote batches is enforced where those batches are
  formed (`rustev-remote-http`, `rustev-jev`); the runtime enforces it for
  runtime-planned batches.

**Increment 1 (2026-10-27): per-item isolation on every batching path.**
A backend any of whose capabilities declares `isolation: per_item` is
never batched: `batch_capability` in `crates/rustev-runtime/src/driver.rs`
no longer limits the rule to screeners, and in `rustev-remote-http` the
client's `batch_capability` (the spec 020 remote batch), its direct
`infer_batch` guard and the server's multi-item refusal (009 3.10.4) apply
it to every capability. The client's adapter-scoped coalescing already
keyed per-item capabilities apart. Decisions within the spec: the rule is
per backend, as spec 035 3.15.3 already is for screeners, so a shared
operation on such a backend is not batched either; that is stricter than
3.7 and never weaker. The Jev adapter (012) builds its own descriptor with
no per-item capability, so its batching has nothing to exclude; it gains
no code here, and stage 1 of 3.8 (increment 6) covers it. Tests:
`a_per_item_capability_is_never_batched` in
`crates/rustev-runtime/tests/optimization.rs` and
`integrations/rustev-remote-http/tests/isolation.rs`, which fail without
the change; the spec 035 screening tests are unchanged, and spec 035's
mutation seed `per_item request batched` in
`crates/rustev-eval/mutation/seeds.py` now names the rewritten line, so
its acceptance still detects every seed.

**Increment 2, part a (2026-10-27): the shared-call and persistent
members.** `rustev-contract` adds `shared_calls` (`SharedCallPolicy`, with
allocation `final_owner`) and `persistent_cache` (`PersistentCachePolicy`
and its `StoreContract`) to `OptimizationPolicy`, the `shared_call`
mechanism, and the `shared` member of `OptimizationRecord`
(`SharedRecord`, 3.2.5); each is omitted when absent. `rustev-core`
refuses an incomplete member with the 3.9 code as the detail's first
token: `shared_call_bounds`, `store_contract_incomplete` (naming the blank
term) and `persistent_bounds`. The contract schemas and the generated Node
and WebAssembly declarations are regenerated. Decisions within the spec:
a call admits at least two waiters, a store term that is blank after
trimming is not stated, and the persistent bounds nest
(`max_entry_bytes <= max_scope_bytes <= max_namespace_bytes`) as the
memory tier's do. Until each member's runtime increment lands (3 and 4),
the compiler refuses a complete member as `<member> is not available in
this build`, so no plan can enable a mechanism the runtime would run
independently (017 3.1); the identity tests for these members land with
that runtime. The planned increments change in two places: the
`rustev.cache-entry/1` document moves to increment 4, where the
persistent tier first writes it; and increment 4 adds a
`crates/rustev-cli/` edge (spec 006), because the CLI matches every
`PrepareError` and `store_missing` and `store_contract_mismatch` are new
preparation errors. Tests:
`shared_and_persistent_members_round_trip_and_are_omitted_when_absent`
in `crates/rustev-contract/tests/documents.rs` and
`incomplete_shared_and_persistent_members_are_refused` in
`crates/rustev-core/tests/execution.rs`; the spec 017 byte and identity
tests are unchanged.

**Increment 2, part b (2026-10-28): the decision cache members.** The
policy, plan and run record members of the decision-result cache, with no
runtime behavior.

What part b adds:
- In `crates/rustev-contract/src/optimization.rs`, `decision_cache`
  (`DecisionCachePolicy`, with `DecisionCacheTier` and
  `RefreshFailurePolicy`) in `OptimizationPolicy`, and
  `DecisionCacheRecord` (3.6.6). Each new member is omitted when absent.
- In `crates/rustev-contract/src/plan.rs`, the compiled
  `PlanDecisionCache` (`decision_key_schema` and `refresh_closure`) as an
  optional member of the plan's `DeclaredExecution`. The parser refuses
  it, or the policy member, in a `rustev.plan/2`, and refuses either one
  without the other.
- `RunRecord.decision_cache`, which the parser refuses outside
  `rustev.run/3` and beyond 017's diagnostic bound.
- In `crates/rustev-core/src/execution.rs`, the validation of the member
  against the definition with the 3.9 codes, and the derivation of the
  key schema and the refresh closure from the plan's step order and
  dependencies, written into the plan's execution section.
- The contract schemas and the generated Node and WebAssembly
  declarations are regenerated.

Until increment 5 lands, the compiler refuses a complete, valid member as
`decision_cache is not available in this build`, as part a did for its
members, so no plan can enable a mechanism the runtime ignores (017 3.1).
`served_from_cache` on the runtime's decision result is a runtime type and
lands with increment 5.

Decisions within the spec:
- **A version 1 definition is refused.** Its plan is `rustev.plan/2`, which
  never carries the member (3.1). The refusal is category 12 with the
  detail `decision_cache requires rustev.plan/3`. 3.9 names no code for
  it, because it is a version rule, not an unsatisfiable member.
- **One spelling per tier set.** `tiers` is non-empty, has no repeats and
  is in lookup order, memory before persistent (3.3), so equal policies
  have equal `PlanId`s.
- **Bounds.** `max_entries` is present and positive exactly when the
  memory tier is. A persistent tier needs `persistent_cache`, whose store
  holds its entries. `max_entry_bytes` and `ttl_ms` are positive.
- **Repeats.** A repeated `volatile` entry is a malformed member, not an
  unknown source, so it is also `decision_cache_bounds`.
- **Naming a volatile acquisition.** Spec 036 has landed, so a `volatile`
  entry is an input name, or `step:<id>` for an acquisition or widening
  step (both capture pages). Input names never contain `:`, so the two
  forms cannot collide. Anything else is `volatile_unknown_source`. An
  input whose freshness is `not_required` is
  `volatile_without_freshness`.
- **Key dimension names.** A key dimension follows the input name rule
  (non-empty, no `:` and no whitespace), appears once, and is not an
  input name, since 3.5 item 3 dimensions are not inputs. Anything else is
  `key_dimension_invalid`.
- **Required and default members.** `volatile` and `key_dimensions` are
  required and may be empty. `on_refresh_failure` may be omitted, which
  reads as `unresolved` (3.6.5), and is always written, so both spellings
  give one `PlanId`.
- **The key schema's members.** In order: `plan_id`, `key_schema_version`,
  `cache_namespace_version`, `runtime_version`, `scope` and `profile` (the
  3.5 members, in the request-level key's order); then
  `dimension:<name>` for each key dimension as declared; then
  `input:<name>` for each declared input that is not volatile, in
  declaration order (3.6.1).
- **The refresh closure.** The closure is computed in the plan's
  topological order. A step is inside when it is a volatile acquisition
  step, or when one of its dependencies is a volatile input or a step
  already inside. Exact steps are inside by this rule as 3.6.2 states,
  and the runtime recomputes them on every hit in any case.
- **The run record member.** `entry_id`, `source_run_record` and `age_ms`
  are present only for a decision served from an entry. `generations` is
  present only for an entry of the persistent tier. The request index
  lists are positions in `requests` and are omitted when empty.
  `on_refresh_failure` is present only when a refresh failed.

Tests:
- `decision_cache_members_round_trip_and_are_omitted_when_absent` in
  `crates/rustev-contract/tests/documents.rs`.
- `decision_cache_members_are_validated_then_refused_as_unavailable` and
  `decision_cache_plan_members_are_a_plan_3_pair` in
  `crates/rustev-core/tests/execution.rs`.
- The unit tests `the_decision_key_schema_orders_its_members` and
  `the_refresh_closure_follows_the_step_graph` in
  `crates/rustev-core/src/execution.rs`.
- The spec 017 byte and identity tests, and part a's, are unchanged.

**Increment 3, part a (2026-10-27): the shared slot.**
`crates/rustev-runtime/src/shared.rs` holds the slot of 3.2.2 and 3.2.3
on its own, not yet driven by any request: the bounded registry of
in-flight attempts by request key; entry that joins an attempt in flight
when its dispatched deadline is at or after the joiner's and the waiter
bound has room (3.2.1 items 2, 3 and 5), leads a new slot otherwise, and
reports `Full` at `max_shared_calls`; a slot that owns the attempt
future (wrapped to contain a panic, 3.2.2 item 4) and its own
cancellation signal; polling under mutual exclusion with a waker that
wakes every registered waiter; the same result for every waiter and the
slot's removal from the registry before any waiter takes it (3.2.2 items
5 and 6); leaving with handoff of charge ownership to the earliest
remaining waiter; the last waiter raising the slot's signal and polling
once on its own task, or, when dropped, dropping the attempt future
without raising it. Nothing in it spawns a task or reads a clock. The
planned parts change: part a is this primitive, and part b wires it into
the driver with eligibility by the 017 key, reservations and
`final_owner` settlement (3.2.4), the records (3.2.5), the section 5
waiter rows and the lifting of `shared_calls`' availability refusal.
Tests: the unit tests in `shared.rs`, driven by hand with counting
wakers: one attempt polled from either waiter, a waiter that finds the
attempt being polled, the joining rules, handoff on leave and on drop,
the last waiter's single poll (answered and abandoned), a contained
panic, and a leave after completion.

**Increment 3, part b (2026-10-27): shared calls in the driver.** The
compiler accepts `shared_calls`, and `drive` in
`crates/rustev-runtime/src/driver.rs` shares target 0's attempt through
the slot of part a, one registry per plan namespace in the runtime's
optimizer (`max_shared_calls` bounds each plan's slots; no key spans
plans). The waiting half of `attempt` is now `await_attempt`, raced
against the caller's signal or the slot's own, so a shared attempt
honours the attempt timeout and spec 003 3.3.4 as an independent one
does. The leader's backend permit moves into the slot; a joiner releases
its own before it waits. Each waiter reserves its own exposure: the final
owner settles the observed charge, every other waiter settles none, a
waiter that leaves while another remains settles none and returns at
once (`cancelled` or `deadline_exceeded`), and the last waiter records
the one poll's answer or, with none, the request as possibly continuing
with unknown liability. A decision dropped while it waits hands off when
another waiter remains and otherwise leaves its reservation as the
abandoned attempt's liability. Records: mechanism `shared_call`, outcome
`miss` for the leader and `hit` for a joiner, and the `shared` member;
the final owner's record holds the one attempt record, so the charge and
any liability are counted once, and every other waiter's supplied output
names the shared attempt as its origin. Decisions within the spec: only
target 0's attempt is shared, since the key names target 0; live
re-executions and streamed generation attempts never share; a request
whose attempt in flight ends before its deadline runs alone with the
diagnostic `shared_ineligible`, and one that finds the waiter or slot
bound reached is refused or runs alone under `admission`
(`shared_full`); the shared call id is a digest of the key material and
waiter ids are per-call numbers, so neither carries a decision id. Tests:
`crates/rustev-runtime/tests/shared_calls.rs`, one per waiter row of
section 5 (sharing, handoff on cancel, the last waiter's cancel with each
answer and its deadline, a dropped owner and a dropped last waiter, a
late request, a joiner needing more time, two tenants, a full call), each
checking that the runtime spawned no task; the slot's own tests gain
`abandon`, which decides what a dropped waiter left behind in the same
critical section that removes it, so two waiters dropped at once on two
threads leave exactly one last waiter
(`concurrent_abandons_leave_exactly_one_last_waiter`).

**Increment 4, part a (2026-10-28): the entry document and the store
seam.** Increment 4 lands in two parts: (a) the entry document, the store
seam and the entry checks, driven by no request yet; (b) the persistent
tier in the driver, `store_missing` and `store_contract_mismatch` with
the `crates/rustev-cli/` edge, the request key's profile member,
invalidation across tiers, and the lifting of `persistent_cache`'s
availability refusal.

What part a adds:
- `crates/rustev-contract/src/cache_entry.rs` holds
  `rustev.cache-entry/1` (3.4.3). Its `EntryBody` holds every member but
  the integrity digest, which is tagged with the schema over the body's
  record canonical bytes. The schema and the generated Node and
  WebAssembly declarations are regenerated.
- `crates/rustev-runtime/src/store.rs` holds the `RuntimeStore` seam
  (3.4.1) with its calls, answers and failures, and a `put` answer's
  diagnostic.
- The same module writes a request-level entry and checks a read in the
  order of 3.4.3. Each failed check is a miss with its diagnostic:
  `entry_schema_unknown`, `entry_corrupt` (the only miss that deletes the
  entry), `entry_collision`, `generation_advanced`, then the expiry check
  by evaluation time: `entry_from_future` for an entry created after the
  request's evaluation time, otherwise `entry_expired`.

Decisions within the spec:
- `get` also takes the scope, because the store never interprets an entry
  and still returns the scope's generation on a miss.
- `contract()` is synchronous: it is declared data that `prepare` reads,
  as a backend's descriptor is.
- A store call receives the decision's remaining milliseconds and its
  signal, not an instant of the runtime's clock.
- The key material is lowercase hexadecimal.
- A `request` payload holds the target, the validated output and the
  source attempt id. Its derivation follows from the plan the key names,
  and its lineage is that attempt and the entry's `evidence`.
- `kind` has only `request` until the decision-result cache (increment 5)
  adds `decision`.
- Bytes that are not JSON are `entry_corrupt`. A JSON document that does
  not name `rustev.cache-entry/1`, or names no schema at all, is
  `entry_schema_unknown` and is kept.
- Key material other than the request's is `entry_collision`, whether
  the ids are equal (017 3.2) or the store returned another key's entry.
  An intact entry whose key id is not the one its material names was
  written wrong, and is `entry_corrupt`.
- 3.9 names no code for a key collision or an expiry, so these take the
  same form as its codes: `entry_collision` and `entry_expired`.

Tests:
- The unit tests in `store.rs`: a served entry with its age, each failed
  check in order with its diagnostic and deletion, and the `put` answers.
- `a_cache_entry_is_sealed_over_its_body` in
  `crates/rustev-contract/tests/documents.rs`.

**Increment 4, part b (2026-10-28): the persistent tier.** The compiler
accepts `persistent_cache`. The tier joins the runtime, and increment 4
gains a part c.

What part b adds:
- `RuntimeBuilder::store` installs the host's `RuntimeStore`. `prepare`
  fails with `store_missing` (`PrepareError::StoreMissing`) for a
  persistent tier on a runtime without a store, and with
  `store_contract_mismatch` when the store's contract is not the
  plan's. The CLI and the Node binding report both. Neither installs a
  store, since the Node exposure of the store is out of scope (section
  4).
- In `crates/rustev-runtime/src/driver.rs`, a request that misses the
  memory tier reads the store before it dispatches (3.3), on its own
  task and under the decision's deadline and cancellation.
- An entry that passes every check of 3.4.3, and whose output the core
  validates, is served with the new mechanism `persistent_cache`
  (outcome `hit`, the key id as `entry_id`, its age by evaluation time,
  and its source attempt) and admitted to the memory tier under that
  tier's bounds.
- A miss carries its diagnostic, and a corrupt entry is deleted.
- A read failure under `fail_closed` refuses the request when
  `admission` is `refuse`. Otherwise it is a miss with `store_failed`.
- The generations each read returns are those captured before the
  work. A request whose primary target answered is queued for writing.
- At the end of a judged decision, `write_pending` in
  `crates/rustev-runtime/src/store.rs` writes each queued key once,
  under the generations its read captured. An entry over
  `max_entry_bytes` is not sent. Every answer but `stored`, and every
  failure, is a diagnostic on that request's record. None changes the
  output or the judgment, and none is retried.

Part c holds the request key's profile member (3.5), invalidation
across tiers (3.10.3) and the `Runtime::invalidate` entry point.

Decisions within the spec:
- **The entry's `evidence` is the decision's evidence record digest**
  (3.4.3's "the evidence digest"). The run record cannot carry its own
  digest, and it must carry each write's answer (section 5's
  `store_conflict`). So writes follow the evidence and precede the run
  record, and a cancelled decision, which has no evidence, writes
  nothing.
- **A request whose store read failed captured no generations**, so it
  is never written.
- **Only primary-target outputs are written**, as for the memory tier
  (017 3.1).
- **A request the runtime batches reads and writes no persistent
  entry.** Batch membership is decided before dispatch, and its records
  are the batch's.
- **Live re-execution and the generation stage read and write no
  persistent entry**, as for the memory tier.
- **A request whose plan has no memory tier records the mechanism
  `persistent_cache`** on a miss as well as on a hit. With a memory
  tier, a miss stays `memory_cache`.

Tests: `crates/rustev-runtime/tests/persistent.rs`, over an in-memory
test store with generations and failures on demand:
- the store's presence and contract at `prepare`
- a restart, as a new runtime over the same store, served without
  dispatch with the same proposal
- two tenants
- a corrupt entry, deleted and written again
- an entry from the future, expired, of another schema, and older than
  the store generation, each kept
- an invalidation during the decision
- a conflicting `put`, with the output kept
- both failure policies on read and write
- both budgets
- a persistent hit admitted to the memory tier

`incomplete_shared_and_persistent_members_are_refused` in
`crates/rustev-core/tests/execution.rs` now shows the persistent member
accepted and part of `PlanId`.

**Increment 4, part c (2026-10-28): the profile member and invalidation
across tiers.**

What part c adds:
- Every request key holds the plan's profile identity, or `none`, as its
  own member (3.5.2). `profile_of` in
  `crates/rustev-runtime/src/optimization.rs` reads it from the plan's
  instantiation.
- `InvalidationResult` gains `persistent` (`pending`, `done` with the
  store's generation and the count removed, or `failed`) and `complete`
  (3.10.3).
- `invalidate_plan` and `invalidate_scope` keep their signatures. For a
  plan with a persistent tier on a runtime with a store, they also mark
  the namespace or scope unavailable in this process.
- The new asynchronous `Runtime::invalidate(plan, scope)` invalidates the
  memory tier, then calls the store's `invalidate`. Success clears the
  mark, and a namespace covers every scope in it. A scope's call is not
  complete while its namespace is still marked. A failure leaves the
  selection unavailable until a later call reconciles it.
- While a selection is unavailable, a read is a miss and captures
  nothing, and a queued write is not sent. Both are recorded with the
  diagnostic `persistent tier unavailable until invalidation completes`.

Decisions within the spec:
- The profile identity of a plan whose instantiation applied a chain is
  the JSON array of its profile ids in chain order. The array delimits
  every id, so no chain reads as another and none reads as `none`.
- Adding the member changes every request key's material and id, which
  are runtime observations that no document's identity holds and no
  golden fixes. Entries under the old material are misses, never
  migrated (017 section 5).
- The store call of an invalidation is made for no decision, so it has
  no deadline and its own signal.

Tests:
- `the_profile_is_a_key_member` in `optimization.rs`.
- `invalidation_reaches_every_tier` and
  `a_scope_invalidation_leaves_other_scopes` in
  `crates/rustev-runtime/tests/persistent.rs`.
- The spec 017 invalidation tests, unchanged.

**Increment 5, the plan (2026-10-28): five parts.** The decision-result
cache is too large for one review, so increment 5 lands in five parts.
Each part lifts the compile refusal only for what it runs:

- (a) The decision key and the memory tier, driven by no decision yet.
  The key is derived from the compiled `decision_key_schema` with its
  dimension members (3.5 items 1 to 3, 3.6.1). The tier covers lookup and
  its checks, evaluation-time expiry, bounds, admission under a captured
  generation, and invalidation. The compile refusal stands.
- (b) The memory tier in the runtime. This part adds `Runtime::decide_keyed`,
  serving a hit with closure dispatch and `closure_mismatch` (3.6.3),
  admission of a judged miss (3.6.4), and the run record member and
  `served_from_cache` (3.6.6) with every outcome but a store failure's.
  It also adds the request mechanism of a reused request and
  `on_refresh_failure: unresolved`. The compiler accepts a memory-tier
  decision cache over exact and semantic steps.
- (c) The persistent tier for decision entries: the `decision` kind of
  `rustev.cache-entry/1`, store reads and writes with their checks and
  generations, a store read failure's `bypassed`, `refused` or `miss`
  (3.4.2), and a restart.
- (d) The refresh-failure policies `recompute` and
  `serve_cached_within_freshness` (3.6.5), with the stored volatile
  snapshot entries and `substituted`.
- (e) Plans with acquisition, widening, generation or extraction steps:
  refreshed acquisitions (3.6.2), generated text keeping its origin
  (3.6.6) and the remaining section 5 rows.

**Increment 5, part a (2026-10-28): the decision key and the memory
tier.** `crates/rustev-runtime/src/optimization.rs` gains the module
`decision`, which no decision drives yet. The compiler still refuses
`decision_cache` as not available in this build.

What part a adds:
- `decision_key` derives a decision's key from the plan's compiled
  `decision_key_schema` (3.6.1). The members are the plan, the key-schema,
  namespace and runtime versions, the complete authorized scope, the
  profile identity or `none` (3.5 item 2), each declared dimension's
  handle (3.5 item 3) and each listed input's snapshot entries. A volatile
  input is not in the schema, so a changed live value keeps the key.
- The memory tier holds decision entries in the plan's namespace, under
  the namespace's generation, beside the request entries. An entry holds
  each reused request's 017 key, output and source attempt, the source
  run record's digest, and the source decision's evaluation time.
- A lookup serves an entry only with the request's own key material and
  the current generation, within `ttl_ms` by evaluation time.
- Admission refuses an entry over `max_entry_bytes`, one captured under
  an earlier generation, and one whose key id holds other material. It
  evicts the namespace's oldest decision entries beyond `max_entries`.
- `Runtime::invalidate_plan` and `invalidate_scope` remove the selected
  decision entries with the request entries.

Decisions within the spec:
- **Key material names each member beside its value.** An input member
  holds the count and the digest of each of the input's snapshot entries
  in full as supplied, in snapshot order. That is a superset of value,
  provenance class and as-of time (3.6.1): source, grade and producer
  also reach the judgment under spec 032. A superset lowers hits and
  never safety.
- **Unkeyed decisions.** A declared dimension the host does not supply
  is `key_dimension_missing` (3.5 item 3). A dimension the plan does not
  declare, a schema member this runtime does not know, and a snapshot
  entry without canonical form are `key_ineligible` (3.5 item 4). 3.9
  names no code for the latter, so it takes the same form as its codes.
  Part b handles both under `admission`.
- **Decision entries expire by evaluation time** in the memory tier too
  (decision 6 at approval). An entry created after the request's
  evaluation time is a miss (`entry_from_future`) and is kept. An expired
  entry is a miss (`entry_expired`) and is removed. Other material under
  the same key id is `entry_collision`, and an entry from an earlier
  generation is `generation_advanced`, as for persistent entries.
- **Bounds.** `max_entries` bounds a plan's decision entries, evicted
  first in, first out. `max_entry_bytes` counts the key material and each
  reused request's key material, canonical output and source attempt id.
- **Invalidation.** Removed decision entries count in `removed_entries`
  and `removed_bytes`. A scope's invalidation advances its namespace's
  generation, as 017's does, so other scopes' decision entries are then
  misses too.
- **The entry id** is a digest of the key material and the source run
  record's digest, so two entries for one key differ by their source.
- **The memory entry holds no judgment digest**, because nothing in a
  process reads it. The `decision` payload of part c carries it (3.6.4).

Tests: the unit tests in the `decision` module of `optimization.rs`.
- `the_decision_key_follows_its_schema` covers the profile, scope,
  dimension and input members, a volatile input left out, and each
  unkeyed case.
- `a_decision_entry_expires_by_evaluation_time` covers age, the edge of
  `ttl_ms`, an entry from the future (kept), an expired entry (removed)
  and each miss's diagnostic.
- `admission_is_bounded_and_checked` covers the entry bound, a stale
  generation, a collision on admission and on lookup, and eviction.
- `invalidation_reaches_decision_entries` covers a scope's and a
  namespace's invalidation and a capture from before one.
- The spec 017 tests are unchanged.

**Increment 5, part b (2026-10-28): serving and admitting decisions from
the memory tier.** The compiler accepts a `decision_cache` whose only
tier is `memory` and whose `on_refresh_failure` is `unresolved`, over a
plan with no acquisition, widening, generation or extraction step.
Anything else validates and is then refused as `decision_cache <part> is
not available in this build`, so no plan enables what the runtime would
ignore (017 3.1). Parts c to e lift those refusals.

What part b adds:
- `Runtime::decide_keyed` takes the request and its key dimensions, one
  scope `Handle` per declared dimension; the other entry points supply
  none.
- `DecisionRun` in the `decision` module of
  `crates/rustev-runtime/src/optimization.rs` derives the key, reads the
  tier before the core starts, writes the run record member (3.6.6) and
  decides admission (3.6.4).
- `from_decision` in `crates/rustev-runtime/src/driver.rs` serves a hit
  (3.6.3). A request inside the closure is dispatched. One outside it is
  supplied the entry's output when its 017 key equals the stored one and
  the output passes request validation; otherwise it alone is dispatched
  with `closure_mismatch`. The core recomputes the judgment in every case.
- `Decided.served_from_cache` names the entry and the source run record.
- `OptimizationMechanism` gains `decision_cache`, the mechanism of a
  request supplied from an entry or refused with its decision; the
  schemas and Node and WebAssembly declarations are regenerated.

Decisions within the spec:
- **`hit` and `refreshed` follow what was dispatched again.** 3.6.6
  defines `hit` by an empty closure and `refreshed` by a request
  dispatched again. They disagree for a closure that holds only exact
  steps, or steps the core does not request. Exact steps are recomputed,
  never dispatched, and 3.6.2 says the closure governs external work. So
  a decision that dispatched nothing again is `hit`.
- **A refused decision still judges.** Under `admission: refuse`, each
  request of an unkeyed decision is refused without dispatch, as 017
  refuses one request: mechanism `decision_cache`, outcome `refused`,
  and `backend_unavailable` with the diagnostic. The core still judges
  over them, so a run record exists that says `refused`.
- **An entry is admitted only after its run record is delivered**,
  because the entry names that record's digest, as the generation
  stage's deferred cache writes do. Admission uses the generation
  captured when the decision started. The miss record carries the
  admission diagnostics known before delivery: a request without a
  validated target 0 output, or an entry over `max_entry_bytes`. A
  generation that advances before the insert leaves only a miss for the
  next decision. A served decision is never admitted.
- **The `decision_cache` member counts as a #76 construct** for
  `use_definition_1` in `crates/rustev-core`. 3.1 makes a plan using it
  `rustev.plan/3`, yet a version 2 definition whose only #76 construct
  was the member was refused `use_definition_1`, and as version 1 it is
  refused as requiring `rustev.plan/3`. This closes a gap left by
  increment 2, part b.
- **Live re-execution is `bypassed`** under any `admission`, since it
  observes fresh backend work (spec 026, 3.4), as the request-level
  tiers bypass it.
- **A reused request records its origin.** Its record has no attempt,
  and its optimization record has mechanism `decision_cache`, outcome
  `hit`, the entry id, the age and the source attempt. So
  `supplied_from` names the original attempt, which replay takes as the
  origin, and the request has no new charge.
- **A failed refresh under `unresolved`** is a volatile input the core
  did not accept from the new snapshot, or a refreshed request that
  failed or was not supplied. The failure is supplied as it is. The
  record carries `on_refresh_failure: unresolved` and the diagnostic
  `refresh_failed` naming each one.
- **A reused output the core does not validate** is dispatched as a
  `closure_mismatch`, with a diagnostic saying so.

Tests:
- `crates/rustev-runtime/tests/decision_cache.rs` covers a repeated
  decision served with nothing dispatched and its reused origins and
  charges, a changed volatile input refreshing its closure, a moved
  request key outside the closure, a missing key dimension under both
  admissions, two tenants, expiry by evaluation time and invalidation, an
  invalidation during the decision, a failed request outside the
  closure, and a failed refresh under `unresolved`. Every served judgment
  and its core evidence are compared with independent execution of the
  same request on a fresh runtime (3.6.3 item 3).
- `decision_cache_members_are_validated_and_unrun_parts_refused` in
  `crates/rustev-core/tests/execution.rs`, formerly
  `decision_cache_members_are_validated_then_refused_as_unavailable`,
  shows a memory tier accepted and part of `PlanId`, and the persistent
  tier and the other refresh policies refused.
  `a_decision_cache_over_generation_steps_is_not_available` in
  `crates/rustev-core/tests/generation_compile.rs` covers a step kind.
- `decision_cache_alone_carries_a_version_2_definition` in
  `crates/rustev-core/tests/execution.rs` covers the `use_definition_1`
  fix and the same definition refused without the member.
- The spec 017 tests and increments 2 to 4's tests are unchanged.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #75 leaves the points below open, and each is
decided here within the binding principles:

1. **No task for a shared call.** The attempt lives in a slot driven by
   whichever waiter is polled, with a broadcast waker, and charge ownership
   passes to the earliest remaining joiner. This keeps spec 003 3.4.3
   unrelaxed and lets a cancelling owner return (003 3.7.4), which the
   pre-split branch `impl/017-shared-persistent-wip` did not; that branch
   is reference only.
2. **One attempt, not one logical request, is shared.** Retries and
   fallback stay per waiter and may share again; this keeps every waiter's
   own policy and evidence intact.
3. **Joiners never get less time.** A joiner must fit inside the attempt's
   dispatched deadline (3.2.1.3); otherwise it runs alone.
4. **`final_owner` allocation.** One payer, zero shares for the rest;
   simplest rule that sums exactly and needs no inference after handoff.
5. **Waiter ids, not decision ids,** link shared records, so tenant-only
   scopes do not expose one principal's decision ids to another.
6. **Evaluation-time expiry** for persistent and decision entries, since
   the runtime has no wall clock and its monotonic clock does not survive
   restarts; the caller already controls evaluation time for freshness.
7. **Generations live in the store** and writes are conditional, so 017
   3.7's commit rule holds across processes; cross-process memory
   invalidation stays the host's (3.10.4).
8. **The judgment is always recomputed.** The decision-result cache reuses
   semantic work, not judgment bytes, so 017 section 4's "no caching
   judgments" is narrowed only to allow reuse of a decision's outputs, and
   017 I-1 still holds.
9. **Volatile values need freshness, and stale stays stale.** #75's "serve
   with `StaleEvidence` notice" becomes `serve_cached_within_freshness`,
   where the core's existing freshness rule yields `stale_evidence` past
   the bound; serving a stale value as current would relax a binding
   principle and is not offered.
10. **Non-volatile as-of times are in the key.** Lower hit rates, but no
    reused output ever rests on different evidence metadata.
11. **Admission requires target-0 outputs,** as in increment 1, so a
    degraded fallback result is never replayed as a hit.
12. **Per-item isolation is spec 035's general descriptor member,**
    applied here to every capability and batching path, rather than a
    second, batching-only flag.
13. **Qualification is evidence, not enablement.** A `qualified` report is
    necessary for production batching; enabling stays with the owner
    (R-28). Stage 2 spend is under R-29 and the work order's live scope.
14. **Store exposure elsewhere.** The trait is the Rust seam; its Node form
    follows spec 027 3.6 and a remote form #76 child A2, neither specified
    here.
15. **Shared #76 versions.** Under the #76 coordination rule, the
    decision cache adds to `rustev.plan/3` and `rustev.run/3` (both
    introduced by spec 032) instead of claiming versions of its own; capture and replay
    of such plans follow the rules those specs set for plan 3. The 017
    mechanisms stay in the versions 017 increment 1 used (`rustev.run/1`,
    `rustev.run/2` or, for a plan 3 decision, `rustev.run/3`). No new refusal
    category, unresolved reason or exit code is introduced: compile
    refusals use category 12 with named codes (3.9).
16. **Implementation order.** The decision-result cache waits for the spec
    032 versions it adds to (3.1, R-3); the rest of this spec does not.
    Spec 037 is not waited for.

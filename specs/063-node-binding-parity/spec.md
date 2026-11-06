---
id: "063-node-binding-parity"
title: "Node binding parity: acquisition source, profiles and variants, store seam, capture, repeatability commands (amends 027, 029, 034, 036 and 044)"
status: approved
implementation: pending
created: "2026-11-03"
summary: >
  An amendment of approved spec 027, closing the Node exposure
  that specs 029, 034, 036 and 044 left out of scope (#56, with #62 and
  #73), for release 0.5.0. `@rustev/rustev` gains a host-implemented
  acquisition source for 036's `acquire` and `widen` steps; profile,
  variant and base-profile inputs on `compile`, with `profileCheck` and
  `planDiff`; a host `RuntimeStore` for 044's persistent tier, with a
  stated atomicity contract and invalidation; opt-in capture on `decide`
  that returns a replay bundle, because the evidence a host sink receives
  cannot replay a decision; and 029's repeatability commands with 061's
  host import, in process. Each uses 027's seam rules, the Rust API the
  CLI calls and the shared conformance suite; the WebAssembly package
  gains none. Ratified by the owner on 2026-11-03 (A-78).
amends:
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
  - "034-parameterized-definitions"
  - "036-mid-plan-acquisition"
  - "044-runtime-optimization-increment-2"
extends:
  # The seams, compile inputs, capture, commands, errors, gauges and
  # hand-written declarations of section 3, with their tests.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  # The repeatability renderings and `Host::list` (3.9.3, 3.9.4).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: amending }
  # Cases gain `native`; runtime and capture cases (3.13).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # The CLI calls the moved renderings; its contract is unchanged (3.9.3).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Archive entries for moved source files.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
  - "034-parameterized-definitions"
  - "036-mid-plan-acquisition"
  - "044-runtime-optimization-increment-2"
  - "061-host-scored-repeat-sets"
  - "050-wasm-without-parameterized-definitions"
  - "031-public-api-and-release-family"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A host-implemented acquisition source or store obeys every rule of spec 027 3.6 and section 3.4 here (asynchronous Promise callbacks through a threadsafe function, bounded queue with a typed refusal, remaining deadline and cancellation passed in, a late or never-settling Promise discarded and counted); an acquisition failure reaches the judgment only as spec 036's unresolved or skipped outcome, and a store failure only as a miss or a refusal under the plan's on_store_failure policy, never as a defaulted value."
    anchor: "3-4-host-callbacks-promises-deadlines-and-threads"
  - id: "I-2"
    kind: invariant
    text: "For the same inputs, the binding's compile with a profile or variant, profileCheck, planDiff, repeatability commands and captured replay bundles produce the bytes, PlanId and exit code the CLI or the Rust API produces, under only the comparison each shared conformance case declares."
    anchor: "3-13-conformance"
  - id: "I-3"
    kind: invariant
    text: "No binding API accepts a cache key, store scope or generation from the host, or selects a profile or variant from a principal, tenant or request; Rustev derives every key and the host names the profile and variant explicitly."
    anchor: "2-1-binding-principles"
  - id: "I-4"
    kind: invariant
    text: "Capture is off unless the caller passes it, retains content only by explicit choice, and its bundle and items reach only the caller's decide result; nothing captured is sent to the evidence sink, telemetry, gauges or an error message, and no principal handle, credential or trace header enters a bundle; embedded and external bundles carry unredacted inputs, model outputs and pages, which spec 028's redaction policy does not cover and the host must protect as personal data."
    anchor: "3-8-capture-on-decide"
  - id: "R-1"
    kind: requirement
    text: "The WebAssembly package gains none of these surfaces; its import allowlist, budgets and parameterized-feature exclusion are unchanged."
    anchor: "3-12-webassembly-position"
---

# 063: Node binding parity: acquisition source, profiles and variants, store seam, capture, repeatability commands (amends 027, 029, 034, 036 and 044)

Approved: ratified by the owner on 2026-11-03 (decision A-78); the
implementation is pending. The approved texts of 027, 029, 034, 036 and 044 are not edited; this
spec records what changes in them (3.14). It authorizes no release,
publication, live provider call or spend.

## 1. Purpose

A TypeScript host adopting `@rustev/rustev` 0.3.0 reported gaps (#56,
with #62 and #73 folded in), unchanged on `main`: `Engine.create` never
calls `RuntimeBuilder::acquisition_source`, so a plan with an `acquire` or
`widen` step fails `prepare` with `source_binding` (036 section 2:
"reachable through the library only"); `compile` takes no profile or
variant and `profile check` and `plan diff` are not commands (034 3.15.3:
"The Node and WebAssembly bindings and the service expose these
functions"); there is no store seam, so a persistent tier is refused
`store_missing`, and no invalidation call; and spec 029's commands are CLI
only. The host also shadows each stage of a migration and gates the switch
on offline replay of the shadow decisions, which needs capture (3.8).
Spec 027 3.7 says later surfaces "join the bindings when their owning
specs land, through the same seam mechanism (section 3.6), the same error
mapping (section 3.4) and the same conformance suite (section 3.12)". This
spec is that join.

## 2. Territory

`integrations/rustev-node/` (027): seams, compile inputs, capture,
commands, errors, gauges, hand-written declarations and tests.
`crates/rustev-surface/` (027): the repeatability renderings move here
from `crates/rustev-cli/src/repeat.rs`, and `Host` gains `list` (3.9.4).
`crates/rustev-cli/` (006, internal structure only, as 027 section 2): it
calls the moved renderings; grammar, exit codes, bytes and goldens
unchanged. `tools/rustev-conformance/` (027): cases gain `native`, runtime
and capture cases are added (3.13). `tools/rustev-release/` (031): archive
entries for moved files.

Ratification added the `extends` edges to
`integrations/rustev-node/` and `crates/rustev-surface/` (027, amending),
`tools/rustev-conformance/` (027, additive), `crates/rustev-cli/` (006,
amending), `tools/rustev-release/` (031, additive) and, at completion,
`Makefile` and `scripts/acceptance/` (001, additive), added by the
change that completes this spec.

Unchanged: `rustev-core`, `rustev-contract`, `rustev-runtime` and
`rustev-eval` (every API used exists, including `decide_with_capture` and
`assemble_with_sources`); every document, schema, generated declaration,
`PlanId`, golden and expected byte; the WebAssembly package; 027's error
classes (only codes are added, 3.10); `make boundaries`.

### 2.1 Binding principles

- **No new authority; proposals, never grants.** Pages, entries, bundles,
  diffs and selections are values and evidence; no added type is a
  permission; `principalHandle` stays opaque and never enters a bundle.
- **Derivation classes and lineage.** The core gives acquired values their
  class and lineage (036 3.4.6), re-validates stored entries (044 3.4.3)
  and recomputes them in replay (036 3.15.7); the binding computes none.
- **Unresolved, never a default.** A failed, denied, late or invalid page
  is 036's unresolved outcome; a store failure is a miss or a refusal
  under the plan's policy; a missing capture item is `incomparable`, never
  `reproduced`; an unknown selection is `unknown`, never `qualifies`.
- **Keys and choices are not host inputs (I-3).** 044 3.5.1: "Rustev
  derives every key; no runtime, binding or seam API accepts one." 034
  3.15.3: the host names the profile and variant.
- **No I/O beyond host seams.** No file, network or clock: pages and
  entries come from host objects, captured bytes and command artifacts are
  returned, commands read named inputs, and time is an argument. Sources
  and the store are passed explicitly to `Engine.create` (R-38).

## 3. Behavior

### 3.1 What is bound today and what this spec adds

| Surface | Rust API | Node today | Here | WebAssembly |
|---|---|---|---|---|
| Profile and variant | `compile_profiled_with`, `rustev_surface::profile::plan_command` | none | 3.2 | excluded |
| `profile check`, `plan diff` | `rustev_surface::profile::check`, `plan_diff::diff` | none | 3.3 | excluded |
| Acquisition source | `RuntimeBuilder::acquisition_source`, `compile_with_sources`, `load_checked_with_sources` | none | 3.5 | excluded |
| Store and invalidation | `RuntimeBuilder::store`, `Runtime::invalidate` | refused `store_missing` | 3.6, 3.7 | excluded |
| Capture | `Runtime::decide_with_capture`, `rustev_eval::assemble::assemble_with_sources` | none | 3.8 | excluded |
| Spec 029 and 061 commands | `rustev-eval` repeat, baseline, selection and host-import modules | CLI only | 3.9 | excluded |

### 3.2 Profiles and variants on compile

`Engine.compile` and `compileSync` gain optional fields (027 3.3: "option
bags may gain optional fields by later specs"):

```ts
compile(args: {
  definition?: DocumentInput; plan?: DocumentInput;              // as today
  descriptors?: DocumentInput[]; calibrations?: DocumentInput[]; // as today
  definitions?: DocumentInput[];  // 1 to 64, only with `profile` (034 3.14.1)
  execution?: DocumentInput;      // rustev.execution/1, as `--execution`
  sources?: DocumentInput[];      // rustev.source/1 (3.5)
  profile?: DocumentInput;        // rustev.profile/1, the leaf
  profileBases?: DocumentInput[]; // 1 to 64, for the leaf's `extends` chain
  variant?: string; definitionVersion?: string;  // name; MAJOR.MINOR.PATCH
}): Promise<PlanHandle>;
```

1. **Mapping.** With an instantiation field or a version 2 template (the
   test `rustev_surface::profile::plan_command` applies), the binding
   calls `rustev_core::instantiate::compile_profiled_with(definitions,
   profile, bases, variant, pin, descriptors, calibrations, sources,
   execution)`; otherwise `compile_with_sources`, or `compile` as today.
   The `PlanId` equals `rustev plan compile`'s for the same inputs (I-2).
   Without `execution`, a Node host cannot compile a plan with a
   persistent tier today; this closes that.
2. **Usage rules, as the CLI's.** `definition` with `definitions`;
   `profileBases`, `definitionVersion` or several definitions without
   `profile`; an empty `profileBases`; `plan` with an instantiation field:
   `InvalidDocument` naming the field, before any parse. Profiles parse
   under `DESCRIPTOR_V1`.
3. **Refusals.** A 034 3.9 instantiation refusal is `CompileRefusal` with
   the 034 code in snake case (`variant_required`) and `detail` the object
   `rustev-surface` renders (`stage: "instantiate"`, `code`, `subject`,
   `detail`); a later compile refusal keeps its category code, with
   `stage: "compile"` and `parameters`.
4. **Pure commands.** `command('check' | 'compile')` accepts the same
   fields as input names and routes through `plan_command`, as the CLI.
5. **No mapping from requests.** `decide` names no profile or variant; a
   host compiles one `PlanHandle` per profile and chooses (034 section 4).

### 3.3 Profile check and plan diff

```ts
profileCheck(inputs: Record<string, DocumentInput>, args: {
  definitions: string[]; profile?: string; profileBases?: string[];
  variant?: string; definitionVersion?: string;
}): CommandResult;                                     // pure, synchronous
planDiff(inputs: Record<string, DocumentInput>, args:
  | { leftPlan: string; rightPlan: string; out: string }
  | { definitions: string[]; leftProfile?: string; rightProfile?: string;
      leftVariant?: string; rightVariant?: string; profileBases?: string[];
      definitionVersion?: string; descriptors?: string[]; rules?: string[];
      calibrations?: string[]; execution?: string; out: string }
): Promise<CommandResult>;
```

1. Both also join `command`. Strings name entries of `inputs`, as for
   every 027 command. They call `rustev_surface::profile::check` and
   `plan_diff::diff`, the functions `rustev profile check` and `rustev plan
   diff` call; `out` names the `rustev.plan-diff/1` artifact.
2. 034 3.14.4's usage checks (a) to (d) apply in that order and throw
   `InvalidDocument` naming the check. Refusals are results: `code` 5 with
   the refusal in `out`; `diffed` is code 0.

### 3.4 Host callbacks: Promises, deadlines and threads

The acquisition source and the store are network calls on the host side
(an HTTP client, Redis), so every method of both is an asynchronous host
callback. These rules refine 027 3.6 for them:

1. **Calling.** The runtime's future runs on a Tokio worker of the
   engine's runtime. It enqueues the call on the JavaScript event loop
   through a napi `ThreadsafeFunction` in non-blocking mode and awaits a
   one-shot channel; no Rust thread ever waits on JavaScript. On the event
   loop the binding calls the method; a returned Promise (or any value,
   taken as `Promise.resolve(value)`, or a synchronous throw, taken as a
   rejection) gets one settlement handler that sends the outcome on the
   channel. The binding holds no lock across the call, so a callback may
   call engine methods; a nested `decide` is admitted like any other.
2. **Queue.** A seam's `queueCapacity` (1 to 65,536) bounds the calls the
   runtime is waiting on. A call that would exceed it is not enqueued and
   is refused at once (3.5.6, 3.6.3). If the threadsafe function's own
   queue refuses the call (the event loop is shutting down), the call
   fails the same way.
3. **Deadline.** Each call carries `remainingMs`: for a source, until the
   step's acquisition window closes (036 3.13.3); for a store call made
   for a decision, until the decision's deadline; absent otherwise. The
   runtime races the channel against that time, never trusting the host
   to honor it. When it passes, the runtime stops waiting, the call's
   `AbortSignal` is aborted, and the outcome is the deadline outcome:
   `deadline_exceeded` for a page, a `StoreFailure` for the store.
4. **Cancellation.** The call's `signal` is linked to the decision's
   `CancelSignal` (the decide `AbortSignal`, the decision deadline or
   `shutdown`). An abort is advisory to the host. For a source, the
   runtime polls once for a cancellation answer (spec 003 3.7.3); a host
   answer arriving later is not observed, and the record states
   possibly continuing work (027's R-47 limitation 1 applies). A store
   call cancelled this way is a `StoreFailure`.
5. **Late and never-settling Promises.** Once the runtime stops waiting,
   the call's queue slot is released and the one-shot receiver is
   dropped. A later settlement is discarded and counted in the seam's
   `lateResolutionsDiscarded`; it never reaches a judgment, record, entry
   or capture. A Promise that never settles costs the binding one
   settlement handler and one count in the seam's `abandonedPending`
   gauge until it settles; it holds no Rust thread, runtime permit or
   queue slot, and a host whose Promises never settle sees each call end
   at its deadline. A call with no deadline (store `invalidate` from
   `plan.invalidate`) is bounded by `timeoutMs` (3.6.4).
6. **Shutdown.** `shutdown({ drainMs })` waits for decisions as 027 3.9.2
   states, then raises cancellation; pending callbacks of cancelled
   decisions are abandoned as above, and the resolved value gains
   `abandonedPending`, the count still unsettled.

### 3.5 The acquisition source seam

```ts
interface AcquisitionSource {
  descriptor: DocumentInput;      // rustev.source/1 declaring `operations`
  maxConcurrency: number;         // 1 to 65,536: attempt limit per source
  queueCapacity: number;          // 1 to 65,536; no default
  acquire(call: AcquireCall): Promise<AcquireAnswer>;
}
interface AcquireCall {
  decisionId: string; operation: string;
  request: Uint8Array;   // rustev.acquisition-request/1 canonical bytes
  requestId: string;     // its AcquisitionRequestId
  attemptId: string;     // the idempotency key 036 3.14.2 assigns
  remainingMs: number; principalHandle: Uint8Array; signal: AbortSignal;
  traceparent?: string;
}
interface AcquireAnswer { page: DocumentInput; charge: Charge }  // rustev.acquisition-page/1
// EngineOptions gains: acquisitionSources?: AcquisitionSource[];
```

1. **Installation.** `Engine.create` parses each `descriptor` (bounded
   `rustev.source/1` parser) and installs an adapter with
   `RuntimeBuilder::acquisition_source(adapter, maxConcurrency)`. An
   invalid descriptor or a repeated name is `ConfigRefused`
   (`invalid_source`, today's mapping of `ConfigError::InvalidSource`); a
   capacity out of range is `out_of_range`; all before any thread.
2. **Compilation binds by descriptor.** `compile({ sources })` calls the
   compiler with them; `compile({ plan, sources })` loads through
   `load_checked_with_sources`. Sources are never taken from the installed
   set, so a `PlanId` never depends on what an engine holds. An
   uninstalled `SourceId` is `PlanPrepareRefused` (`source_binding`).
3. **Declared data (027 3.6.1).** `cost_bound(request)` is derived from the
   operation's declared `cost`: `bounded{max_units}` gives that bound,
   `estimated` and `unknown` give `unknown`, as 042's remote client does.
4. **Answers.** The page crosses unchanged (an object is serialized with
   `JSON.stringify`, 027 3.2.1), copied up to 4 MiB (`SNAPSHOT_V1`, the
   page's parse class). The binding validates nothing 036 3.3.4 validates;
   the runtime does, and an invalid page is `invalid_page`, supplied as
   `invalid_backend_output`.
5. **Failures.** A thrown `SeamFailure` of class `transient`,
   `overloaded`, `permanent` or the new `denied` maps to 036's class with
   its `charge`; its `cancel` is kept only when the signal was raised. Any
   other throw or rejection is `permanent`, charge `unknown`, cancellation
   `unconfirmed` if the signal was raised (027 3.6.5). `denied` is never
   retried (036 3.12.4); from another seam it maps as a plain throw.
6. **Queue full.** The attempt ends at once `overloaded`, charge observed
   zero (027 3.6.2), so `retry_on` applies.
7. **Unchanged.** `contextSource` still supplies the initial snapshot (036
   3.16.4). Acquisitions appear in `rustev.run/3` and `rustev.evidence/2`
   at the host sink, which must set `acceptsRun3` or prepare is refused
   `sink_version_unsupported`.

### 3.6 The store seam and invalidation

```ts
interface RuntimeStore {
  contract: { schema: string; namespace: string; durability: string;
    encryption: string; atomicity: string; conflict: string; expiry: string;
    invalidation: string; erasure: string };
  queueCapacity: number;          // 1 to 65,536; no default
  get(call: StoreCall & { namespace: string; scope: string; keyId: string }):
    Promise<{ bytes: Uint8Array | null; generations: Generations }>;
  put(call: StoreCall & { namespace: string; keyId: string; bytes: Uint8Array;
    terms: { scope: string; generations: Generations; maxScopeBytes: number;
             maxNamespaceBytes: number; expiresMs: number } }):
    Promise<'stored' | 'generation_advanced' | 'over_budget' | 'conflict'>;
  delete(call: StoreCall & { namespace: string; keyId: string }): Promise<void>;
  invalidate(call: StoreCall & { namespace: string;
    selector: 'namespace' | { scope: string } }):
    Promise<{ generation: number; removed: number }>;
}
interface StoreCall { remainingMs?: number; signal: AbortSignal; traceparent?: string }
interface Generations { namespace: number; scope: number }
// EngineOptions gains: store?: RuntimeStore;
// PlanHandle gains:
invalidate(options?: { principalHandle?: Uint8Array; timeoutMs?: number }):
  Promise<InvalidationResult | null>;
interface InvalidationResult {
  generation: number; removedEntries: number; removedBytes: number;
  persistent: null | 'pending' | { generation: number; removed: number };
  complete: boolean;
}
```

1. **Installation.** `contract` is checked as the closed `StoreContract`
   (nine non-empty strings; otherwise `ConfigRefused`,
   `invalid_store_contract`) and an adapter is installed with
   `RuntimeBuilder::store`. `prepare` keeps `store_missing` and
   `store_contract_mismatch` (`PlanPrepareRefused`).
2. **Mapping.** Each method is the `RuntimeStore` method of its name (044
   3.4.1), called under 3.4. `namespace`, `scope` and `keyId` are
   Rustev-derived opaque strings the host stores and never computes (I-3);
   `expiresMs` is 044 3.4.3's reclamation hint, an evaluation time.
3. **Bounds and failures.** Generations, counts and budgets are safe
   integers (0 to 2^53 - 1); `get` bytes are copied up to 4 MiB
   (`RECORD_V1`, the entry's parse class). An answer out of bounds, an
   unknown `put` answer, a throw, a full queue (`host_seam_queue_full`), a
   deadline, a cancellation or a late settlement is a `StoreFailure` under
   the plan's `on_store_failure` (044 3.4.2), never a hit.
4. **Invalidation.** `plan.invalidate()` calls `Runtime::invalidate(plan,
   principalHandle)`, memory tiers then store. The store call has no
   decision deadline, so `timeoutMs` (1 to 600,000, default 30,000) bounds
   it; at the timeout its signal is aborted and the result has
   `persistent: 'pending'`. It resolves `null` for a plan without an
   optimization namespace and `complete: false` (not an exception) when
   the store failed or timed out, the selection staying unavailable to the
   persistent tier until a later call completes it (044 3.10.3). It is per
   process (044 3.10.4).

### 3.7 Store atomicity contract

Rustev serves an entry only if its integrity digest, key material and
captured generations all match and both current generations equal the
entry's (044 3.4.3), so correctness rests on the generations, and the
store's other duties protect budgets and the honesty of its answers. The
host's `contract.atomicity` term names what it implements; Rustev records
it and cannot verify it. A conforming store guarantees:

- **G1. Monotonic generations.** Each namespace generation and each
  (namespace, scope) generation starts at 0 when absent and never
  decreases. A store that can lose generation state (eviction, flush,
  failover, restart without persistence) must lose or delete every entry
  of that namespace with it; otherwise a later advance can make an
  invalidated entry readable again.
- **G2. Invalidation is the advance.** `invalidate` atomically increments
  the selected generation and answers the new value; removing entries is
  optional (`removed` may be 0), since a stale entry already fails the
  generation check. Once it answers, every later `get` from any process
  sharing the store sees the new value (linearizable).
- **G3. Snapshot reads.** `get` returns the entry and both generations as
  of one instant.
- **G4. Conditional writes.** `put` atomically checks that both current
  generations equal `terms.generations`, applies the `conflict` term
  (put-if-absent for first-writer-wins), checks `maxScopeBytes` and
  `maxNamespaceBytes` against its byte counters, then writes the entry
  with its scope and updates both counters, answering `stored`,
  `generation_advanced`, `conflict` or `over_budget`.
- **G5. Read-your-writes.** A `put` answered `stored` is visible to every
  later `get`; `delete` removes the entry and decrements its counters
  atomically, and deleting an absent entry succeeds.

Redis-implementable contract, keys under one hash tag per namespace so
every script touches one cluster slot (`{rx:<ns>}:g`, `{rx:<ns>}:g:<scope>`,
`{rx:<ns>}:e:<keyId>` a hash of `bytes` and `scope`, `{rx:<ns>}:b`,
`{rx:<ns>}:b:<scope>`):

| Method | What must be atomic | Redis |
|---|---|---|
| `get` | read entry and both generations together (G3) | one read-only Lua script (`HGET` of the entry, `GET` of both generations); on the primary, never a lagging replica (G2) |
| `put` | compare both generations, conflict test, budget test, write, counter updates (G4) | one Lua script (`EVALSHA`); or `WATCH` both generation keys and the entry, then `MULTI`/`EXEC`, retrying on abort |
| `delete` | remove the entry and decrement its scope and namespace counters (G5) | one Lua script |
| `invalidate` | increment the selected generation (G1, G2) | `INCR` (atomic alone); optional entry removal may run afterwards; use `WAIT` for replicas when `durability` claims it |

Generation keys carry no TTL and must not be evictable (for example
`maxmemory-policy volatile-*` with TTLs only on entries, or a separate
non-evicting instance holding both). Entries may carry a TTL at least the
plan's `ttl_ms`; reclaiming early is only a miss. `expiresMs` is an
evaluation time and is not converted to a wall-clock TTL by Rustev.

Note (non-normative): a host backed by a replicated Redis or Valkey must
route every store call to the primary endpoint, never a reader endpoint,
and must use an eviction policy that cannot evict keys without a TTL (for
example `volatile-lru`) or otherwise exclude generation keys from
eviction; a reader endpoint breaks G2 and an evicted generation key
breaks G1.

### 3.8 Capture on decide

**Finding: the sink's evidence is not enough to replay.** A host sink
receives the run record (`rustev.run/1`, `/2` or `/3`) and nothing else.
Offline replay (spec 004 3.3, 036 3.15.4) needs the retained plan and its
dependencies, the snapshot, and every value supplied to the core in supply
order with its retained bytes (004 3.2). The run record lacks the model
outputs: an attempt records `end: Output` without the output
(`rustev_contract::run::AttemptEnd`). It lacks the pages: "Page contents
are never embedded in evidence or run records" (036 3.14.4), and an
`AcquisitionRecord` holds the request id, page digest and result only. It
lacks the supply sequence that historical reproduction follows (036
3.15.4), and spec 004 5.3.4 states "No capture data is sent to the
evidence sink by default". A host that logged its own backend responses
and pages still lacks the target indexes and supply order. The runtime
already captures plan 3 decisions with acquisitions, semantic and
generation steps (`Runtime::decide_with_capture`; reproduced without
source calls in `crates/rustev-eval/tests/acquisition_replay.rs` and
`generation_replay.rs`), so the binding exposes it:

```ts
// DecideOptions gains:
capture?: {
  scope: Scope;            // as `replay` takes it: principal or tenant_only, all fields
  maxBytes: number;        // 1 to 16,777,216 (spec 004 5.3.3)
  createdAtMs: number;     // host wall time; the binding reads no clock
  retain?: 'digest_only' | 'embedded' | 'external';   // default 'digest_only'
  lifetimeMs?: number;     // default 604,800,000 (7 days); at most 2,592,000,000
  hostCapMs?: number;      // a tighter host cap
};
// DecideResult (and SinkFailure) gain:
capture?: {
  status: 'complete' | 'limit_exceeded' | 'cancelled' | 'abandoned';
  bundle?: Uint8Array;                // rustev.replay/1 or /2 canonical bytes
  items?: Record<string, Uint8Array>; // 'external': reference -> bytes
  error?: { code: string; detail: unknown };  // assembly refused
};
```

1. **Mapping.** With `capture`, `decide` calls `decide_with_capture` and
   assembles the bundle with `assemble_with_sources` from what the plan
   handle retained at compile (plan, descriptors, calibrations, source
   descriptors), the decision's snapshot (given or fetched from
   `contextSource`), the run record and the capture. A `rustev.plan/3`
   decision yields `rustev.replay/2` with its acquisition pages.
2. **What is captured.** The plan and dependency documents; the snapshot;
   the run record; every supplied model output (semantic, generation,
   extraction and rank steps) and every acquisition page, each with its
   supply sequence number, `RequestId` or `AcquisitionRequestId`, target
   and producing attempt; and runtime-reason documents for supplied
   failures. Request projections are not embedded; replay recomputes them.
3. **Retention.** `digest_only` (default) keeps digests only, so the
   bundle is not replayable (`incomparable`: 004's `missing` for an
   item, 036's `content-not-retained` for an acquisition page);
   `embedded` keeps bytes inside the bundle; `external` returns each
   item's bytes in `items` under a reference equal to its tagged digest,
   which the host stores and later supplies to replay. Lifetime defaults
   to 7 days and never exceeds 30 (004 3.1.2); expiry and erasure are the
   host's, and embedded copies outlive erasure unless the host removes
   every copy (004 3.1.8).
4. **Privacy (I-4).** The scope is required and never downgraded (004
   3.1.3). No principal handle, credential, `traceparent`, `tracestate` or
   telemetry key enters a bundle. The bundle and items go only to the
   caller; nothing captured reaches the sink, telemetry, gauges or an
   error `detail`, and the binding keeps no copy after returning.
   An `embedded` or `external` bundle carries the decision's inputs, model
   outputs and acquisition pages unredacted: spec 028's redaction policy
   applies to telemetry only and does not apply to bundles or items. A
   host must treat such a bundle and its items as personal data, under
   its own encryption at rest, retention and erasure. The 7-day default
   lifetime and the 30-day cap of 3.8.3 apply unchanged.
5. **Outcomes.** Invalid capture options are `CaptureConfigRefused` before
   admission (027 3.4.1). Exceeding `maxBytes` discards the payload and
   reports `limit_exceeded`; the decision, record and delivery are
   unaffected. An assembly refusal (for example over the 16 MiB case
   bound) leaves the decision unchanged and sets `error`. A rejected
   decision throws as today and captures nothing. On `SinkFailure` the
   error carries `capture` beside the record.
6. **Replay.** `engine.replay({ 'b.json': bundle, ...items under
   'items/' }, { bundle: 'b.json', nowMs, scope, store: 'items' })`, the
   existing 027 command `rustev replay` renders, reproduces the decision
   with zero backend and source calls and reports `reproduced`,
   `diverged` or `incomparable` with its reason. Candidate comparison over
   captured shadow bundles is `engine.evaluate` (004 3.4). The CLI's
   `rustev run --capture-bytes`, which still refuses plan 3 decisions
   (`crates/rustev-cli/src/run.rs`), is not changed here.

### 3.9 Repeatability commands

1. **Commands.** These names join `command` and get dedicated async
   methods returning a `CommandResult`, calling the function the CLI
   command calls, moved into `rustev-surface` (3.9.3):

| Binding | CLI | Arguments beyond the CLI flags in camel case |
|---|---|---|
| `eval` with `repeatConfig` | `eval --repeat-config` (029 3.13.1) | `bundles` a string or list of input prefixes; `ledgerNew` a boolean; `ledgerOut` an artifact |
| `evalCandidates` | `eval candidates` (029 3.13.5) | `candidates` an input prefix |
| `baselineRecord` | `baseline record` (029 3.13.2, 061 3.7.1) | `eval` an input prefix; `nowMs` required |
| `evalReproducibility` | `eval reproducibility` (029 3.13.3, 061 3.7.2) | `baselineEval`, `run` input prefixes |
| `evalDiff` | `eval diff` (029 3.13.4) | as above; `out` an artifact prefix |
| `gateSelect` | `gate select` (029 3.13.6, 061 3.7.3) | `baselineEval`, `candidates` input prefixes |
| `evalRepeatImport` | `eval repeat import` (061 3.2) | `repeatConfig`, `config` input names; `runs` a list of 1 to 64 input prefixes in run order; `out` an artifact prefix |

2. **Outcomes are values.** `code` is the exit code (0, 4, 11, 12, 13) and
   `out` the CLI's stdout bytes; `not_reproducible`, `none_qualified` and
   `unknown` are results, never exceptions. Usage rules, checked before
   any input is read, throw `InvalidDocument` naming the rule. `eval`
   without `repeatConfig` and `gate` keep their bytes. 061's provenance
   separation (its I-1) holds unchanged: mixing host and Rustev sets is
   `code` 4, `mixed-provenance`.
3. **One implementation.** The renderings, with 061's import, move from
   `crates/rustev-cli/src/repeat.rs` into `rustev-surface` behind `Host`,
   and the CLI calls them; its goldens and the `repeat-*` expected bytes
   stay byte-identical without re-blessing.
4. **Directories are input prefixes.** A directory argument names the
   inputs under `<prefix>/`. `Host` gains `list(dir, max)`: the immediate
   entry names in byte order, at most `max` (029 3.13.5's 64 candidates;
   006 3.3's caps); the filesystem host reads the directory, the memory
   host derives names from inputs. Outputs are artifacts `<out>/<file>`,
   all or none. `nowMs` is an argument, and the `live` source stays
   refused (`live-source-unavailable`) as in the CLI.

### 3.10 Errors

027 3.4.1's classes are unchanged. New codes: a 034 3.9 code in snake
case for `CompileRefusal` (3.2.3), and `invalid_store_contract` for
`ConfigRefused` (3.6.1); `SeamFailure` gains the class `denied` (3.5.5).
Capture options use `CaptureConfigRefused` with `zero_limit`,
`limit_too_large`, `invalid_scope` or `out_of_range` (a lifetime or cap).
Everything else uses existing codes.

### 3.11 Gauges, shutdown and declarations

`gauges().seams` gains `acquisition:<source name>` per source and `store`,
each with 027 3.9.3's queue depth, refusals and late discards plus
`abandonedPending` (3.4.5). `shutdown` is as 3.4.6. Additions are declared
by hand in `index.d.ts`; no generated declaration changes.

### 3.12 WebAssembly position

The WebAssembly package gains nothing (R-1). The acquisition source, the
store, invalidation and capture are excluded: it has no runtime or
decision execution (027 3.10.1) and its graph may not contain
`rustev-runtime` (027 3.14.3); its offline `replay` already reads
bundles. Profiles, variants, `profileCheck` and `planDiff` are excluded:
spec 050 keeps `parameterized` out of the module, and its typed refusal
stands. The repeatability commands are excluded by the owner's decision.

### 3.13 Conformance

1. **Existing cases gain `native`.** The 23 `parameterized.*` cases (`plan
   check`, `compile`, `show`, `diff`, `profile check`) and the 11
   `repeat-*` cases (029's six commands) change surfaces from `cli` to
   `cli`, `native`, with the same expected bytes and only 027 3.12.3's
   CLI-presentation exclusions. 061's import goldens gain `native` when
   they join the suite.
2. **New runtime cases** (`rust-api`, `native`; bytes under
   `ManualClock`), each mirroring a test:

| Case | Mirrors |
|---|---|
| `runtime.acquire-page`, `runtime.acquire-retry-denied` | `rustev-runtime/tests/acquisition.rs::retries_only_named_classes_denied_never_retried_and_panics_contained` |
| `runtime.acquire-window` | `acquisition.rs::window_expiry_cancels_and_queued_pages_do_not_dispatch` |
| `runtime.acquire-widen` | `acquisition.rs::widening_switches_sources_keeps_protected_fields_and_dedupes` |
| `runtime.store-restart-hit` (two engines, one scripted store) | `rustev-runtime/tests/persistent.rs::a_restarted_runtime_hits_the_same_store` |
| `runtime.store-failure-policy` | `persistent.rs::store_failures_follow_the_failure_policy` |
| `runtime.store-invalidation` | `persistent.rs::invalidation_reaches_every_tier` |
| `runtime.capture-replay` (embedded and external) | `rustev-eval/tests/acquisition_replay.rs::five_acquisitions_two_paged_reproduce_without_source_calls` |
| `runtime.capture-generation` | `rustev-eval/tests/generation_replay.rs` |

   The case format gains scripted `sources`, `store` and `capture` beside
   `backends`; expected outputs are the run record, the bundle bytes and
   the replay output (`reproduced`, zero source and backend calls). Both
   runners produce the same bytes.
3. **Host-seam cases** (native, `node --test`): full source and store
   queues, late and never-settling Promises, a synchronous throw, a
   non-Promise return, `denied`, an abort, and shutdown with pending
   callbacks, as 027 3.12.4 requires of every host seam.

### 3.14 What changes in specs 027, 029, 034, 036 and 044

1. **027:** 3.3's compile inputs, decide options and `Engine.create`
   options gain 3.2, 3.5, 3.6 and 3.8's fields (3.3's listed `capture?`
   is specified by 3.8); 3.7's table gains the source and store seams, and
   its "mid-plan acquisition through the context source (B1, which
   replaces the before-admission call ...)" reads: through a separate
   acquisition source seam, the snapshot call unchanged; 3.6 gains 3.4;
   section 2 and 3.1 gain 3.3 and 3.9's commands; 3.9 gains 3.11; 3.4.1
   gains 3.10's codes.
2. **029, 034, 036 and 044, section 4 each:** the Node exposure listed as
   out of scope is this spec's; WebAssembly stays out (3.12).
3. **061** (ratified, A-77): its 3.10 sentence "The Node and WebAssembly
   bindings gain nothing in this spec" stands for 061; this spec adds
   `evalRepeatImport` to Node (3.9.1).

### 3.15 Relation to spec 011

011 increment 6 (Node `local-embed`: 011 3.17.1, 3.17.5; 027 3.13) is
implemented separately; `LocalEmbedBackend.open` and `features()` are not
touched, and no Cargo feature is added, so everything here ships in the
prebuilt packages. Both edit `EngineOptions` and `index.d.ts`; the later
change rebases. Captured decisions on a `local-embed` backend are
replayable like any other.

## 4. Out of scope

Any WebAssembly exposure (3.12); a remote store protocol; cross-process
invalidation; an acquisition hook on `contextSource`; projection- or
request-dependent host cost bounds beyond 3.5.3; host batch backends;
choosing a profile per request; streaming capture or capture delivered to
the sink; changing the CLI's `run --capture-bytes`; any change to a
document, identity, golden, `rustev-core`, `rustev-runtime` or
`rustev-eval`; release, publication or spend.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A plan with an `acquire` step, compiled without `sources` | `CompileRefusal` (`source_binding`) naming the step. |
| The plan binds a `SourceId` no installed source has | `PlanPrepareRefused` (`source_binding`). |
| Two acquisition sources share a name | `ConfigRefused` (`invalid_source`); no thread started. |
| A source's queue is full | `overloaded`, charge observed zero; `retry_on` applies. |
| A source throws `SeamFailure` `denied` | Not retried; `missing_evidence` in the judgment, no exception. |
| A source's Promise never settles | The page is `deadline_exceeded` at the window's close; `abandonedPending` counts it; no thread or slot held. |
| A source throws synchronously, or returns a non-Promise page | Treated as a rejection, or as a resolved Promise. |
| A page with an unknown field, or over 4 MiB | `invalid_page`; unresolved `invalid_backend_output`. |
| A page resolves after the window closes | Discarded and counted; never in a judgment or capture. |
| A persistent tier and no `store` | `PlanPrepareRefused` (`store_missing`). |
| A store contract without `erasure` | `ConfigRefused` (`invalid_store_contract`). |
| `get` throws under `continue_without_cache` / `fail_closed` | A miss with `store_failed` / refused under `admission`, as in Rust. |
| `get` returns generation 2^53, or `put` an unknown answer | `StoreFailure`; never a hit. |
| `invalidate` whose Promise never settles | At `timeoutMs`: `persistent: 'pending'`, `complete: false`; no persistent read or write for that selection. |
| Non-atomic host: `get` reads the entry and generations separately | Detectable: spurious misses with `generation_advanced` diagnostics, never a stale hit, given G1, G2 and G4. |
| Non-atomic host: `put` checks generations, then writes | Detectable for generations: the entry keeps its old generations and is never served. Budgets may be exceeded silently: not detectable by Rustev. |
| Non-atomic host: put-if-absent races under first-writer-wins | Both writers answered `stored`; harmless (entries checked in full); the `conflict` term misreported: not detectable. |
| Host resets a generation (evicted key, flush, failover lost an `INCR`) while entries remain | A later advance can make an invalidated entry pass every check: not detectable by Rustev. G1 forbids it. |
| Host deletes entries on `invalidate` without advancing the generation | An in-flight `put` captured before still passes the generation check and stores a stale entry: not detectable. G2 forbids it. |
| Host serves `get` from a lagging replica | Stale hits after an invalidation: not detectable. G2 forbids it. |
| `decide` without `capture` | No bundle; result unchanged. |
| `capture` with `maxBytes` 0, above 16 MiB, or a lifetime above 30 days | `CaptureConfigRefused` before admission. |
| A capture exceeding `maxBytes` | `status: 'limit_exceeded'`, no payload; decision and delivery unchanged. |
| `retain: 'digest_only'` bundle replayed | `incomparable{missing}` (`content-not-retained` for an acquisition page), never `reproduced`. |
| An `external` item withheld at replay | `incomparable` naming the item; zero backend or source calls. |
| `embedded` capture under a telemetry redaction policy that drops a snapshot field | The field's canary is present in the bundle (unredacted, 3.8.4) and absent from telemetry. |
| A canary in the snapshot and in the principal handle | Snapshot canary only in the bundle; principal canary nowhere; neither in sink, telemetry or gauges. |
| `compile` with `profileBases` and no `profile` | `InvalidDocument` before any parse. |
| A template with variants compiled without `variant` | `CompileRefusal` `variant_required`, `detail.code` `variant-required`. |
| One template, profile and variant through the CLI and `compile` | Equal `PlanId`. |
| `gateSelect` with one verdict unknown | `code` 12, `unknown`; no exception. |
| `gateSelect` with a host baseline and Rustev-evaluated candidates | `code` 4, `mixed-provenance` (061 I-1); nothing written. |

## Acceptance

- The 34 `parameterized.*` and `repeat-*` cases pass on `native` with
  unchanged expected bytes; CLI goldens are byte-identical after 3.9.3.
- The nine runtime cases of 3.13.2 pass on `rust-api` and `native` with
  identical run record, bundle and replay bytes.
- `node --test integrations/rustev-node/test/*.test.mjs` covers every
  binding row of section 5, 3.13.3's host-seam cases, the gauges,
  invalidation, and a reference Redis-shaped store test double that
  implements G1 to G5 and its non-atomic variants, with no network access
  or wall-clock sleep.
- `make bindings` passes with no generated declaration change; the
  hand-written declarations type-check; the WebAssembly exports and import
  allowlist are unchanged.
- `make code`, `make conformance`, `spec-spine verify` for 027, 029, 034,
  036, 044 and 063, and `make gate` pass.

## Verification

Pending with implementation. At completion a `verify:cli` block runs
`cargo test -p rustev-conformance --locked`, `cargo test -p rustev-surface
--locked --all-features` and `cargo test -p rustev-cli --locked`; 063
joins `make verify` and the `runtime-evidence` group. Binding behavior is
tested by `make bindings` and the CI `bindings` job, as for 049.

## Planned increments

All six target release 0.5.0, each one main-based pull request of about
1,000 changed lines, and each builds and passes its checks on `main`
alone. Increments 1 (capture), 3 (acquisition source) and 4 (store)
depend on no other increment of this spec, so each can ship in a 0.5.x
release on its own if the others run long; the downstream cascade and
content cache wait only on 3 and 4.

Capture comes first because it is the only capture path for a host's
shadow-then-switch: the CLI fallback cannot capture the partner's
decisions. Its phase C plan is a plan 3 decision, which `rustev run
--capture-bytes` refuses (`crates/rustev-cli/src/run.rs`), and its model
backends are JavaScript callbacks the CLI cannot run.

1. **Capture on decide** (3.8): capture options, assembly, `items`, the
   two capture cases, privacy tests. Its acquisition coverage joins with
   whichever of 1 and 3 lands second.
2. **Profiles and variants** (3.2, 3.3): compile inputs, `profileCheck`,
   `planDiff`, codes, declarations, tests; `native` on 23 cases.
3. **Acquisition source** (3.4, 3.5): the callback machinery of 3.4, the
   seam, `sources` on compile and load, `denied`, gauges, scripted
   `sources` in both runners, four `runtime.acquire-*` cases, host-seam
   tests.
4. **Store and invalidation** (3.4, 3.6, 3.7): the seam (3.4's machinery
   if increment 3 has not landed, shared afterwards), `plan.invalidate`,
   scripted `store`, three `runtime.store-*` cases, the Redis-shaped
   double, host-seam tests.
5. **Repeatability move** (3.9.3, 3.9.4): `repeat.rs`, with 061's import,
   into `rustev-surface` as a rename with file I/O replaced by `Host`, so
   review sees the changed lines; `Host::list`; archive entries. No
   binding change.
6. **Repeatability commands** (3.9.1, 3.9.2): seven commands including
   `evalRepeatImport`, declarations, tests, `native` on 11 cases. Needs 5.

The last increment to land adds `verify:cli` and `VERIFIED_SPECS`.

## Implementation record

**Increment 1, 2026-11-04 (3.8, capture on decide).** `decide` takes
`capture` and returns `capture` (`integrations/rustev-node/src/capture.rs`,
`index.d.ts`, `index.js`): options are checked before the context fetch
and admission (`CaptureConfigRefused`: `zero_limit`, `limit_too_large`,
`invalid_scope`, `out_of_range`), the decision runs through
`Runtime::decide_with_capture`, and the bundle is assembled with
`assemble_with_sources` from the plan handle's retained plan, descriptors
and calibrations. `external` returns each item under its tagged digest. A
captured `SinkFailure` carries `capture` beside `record`; the native error
message holds no captured byte. `test/capture.test.mjs` covers the capture
rows of section 5 and `SinkFailure`. The conformance cases
`runtime.capture-replay` (embedded and external, semantic steps) and
`runtime.capture-generation` run on `rust-api` and `native` with
identical bytes. Acquisition pages join with increment 3: until then no
binding plan has a source, and source descriptors are passed empty. The
digest-only reason in 3.8.3 and section 5 now names 004's `missing` for an
item, as replay has always reported, and 036's `content-not-retained` only
for an acquisition page; no behavior changed.

**Increment 2, 2026-11-05 (3.2, 3.3, profiles and variants).** `compile`
and `compileSync` take `definitions`, `execution`, `profile`,
`profileBases`, `variant` and `definitionVersion`
(`integrations/rustev-node/src/profile.rs`, `index.d.ts`, `index.js`):
3.2.2's usage rules throw `InvalidDocument` naming the field before any
document is serialized or parsed; a version 2 template or an
instantiation field compiles through `compile_profiled_with`, anything
else through `compile_with_sources`, so `execution` now reaches a plan
with a persistent tier. An instantiation refusal is `CompileRefusal` with
the 034 code in snake case and the surface's `stage`, `code`, `subject`
and `detail`; a later compile refusal keeps its category code with
`stage: "compile"` and `parameters` (3.10). `command('check' | 'compile')`
routes through `plan_command`; `profileCheck` and `planDiff` call
`profile::check` and `plan_diff::diff` and join `command`, with 034
3.14.4's checks (a) to (d) in order. The declarations are written by hand
(3.11). `test/profile.test.mjs` covers each usage rule, both refusal
kinds with their detail objects, `PlanId` and plan bytes equal to `rustev
plan compile`'s, `execution` reaching a persistent tier (refused
`store_missing` at prepare without a store), base chains and version pins,
and both commands' happy and refusal paths. The 23 `parameterized.*`
cases run on `cli` and `native` with unchanged expected bytes (3.13.1);
only `manifest.json` was re-blessed. `sources` (increment 3a) reach both
compile paths, profiled or not.

**Increment 3a, 2026-11-06 (3.5.2, sources on compile and load).**
`compile` and `compileSync` take `sources` (`integrations/rustev-node/src/source.rs`,
`index.d.ts`, `index.js`): a definition compiles through
`compile_with_sources` and a `plan` loads through
`load_checked_with_sources`, each source parsed by the bounded
`rustev.source/1` parser and never taken from the engine's installed set.
A graded definition without its sources is `CompileRefusal`
(`source_binding`); a graded plan loaded without them is `CompileRefusal`
(`load`), as any load mismatch. Fact-only sources need no runtime adapter,
so such a plan prepares on an engine with no installed source.
`test/source.test.mjs` covers a synthetic graded-input definition. The
acquisition seam, `denied`, gauges and the `runtime.acquire-*` cases are
the rest of increment 3.

**Increment 3, 2026-11-06 (3.4, 3.5, acquisition source).**
`EngineOptions.acquisitionSources` installs each host source with
`RuntimeBuilder::acquisition_source` (`integrations/rustev-node/src/source.rs`):
a descriptor that does not parse, or a repeated name, is `ConfigRefused`
(`invalid_source`) and a bound out of range `out_of_range`, before any
thread. Each attempt is an asynchronous host callback
(`src/callback.rs`): enqueued through a non-blocking threadsafe function,
awaited on a one-shot channel, its queue slot held by the waiting side and
released when the runtime stops waiting; an unsettled Promise is counted in
`abandonedPending` until it settles and its late settlement in
`lateResolutionsDiscarded`. The cost bound comes from the operation's
declared `cost`; a page is copied up to 4 MiB and validated by the core; a
`SeamFailure` keeps its class, including `denied`, and its `cancel` only
after an abort; anything else is `permanent` with an unknown charge.
`gauges().seams` gains `acquisition:<name>` with `abandonedPending`, and
`shutdown` resolves with `abandonedPending`. Capture assembly receives the
plan handle's source descriptors. `test/source.test.mjs` covers the source
rows of section 5 and 3.13.3's host-seam cases (full queue, never-settling
and late Promises, a synchronous throw, a non-Promise page, `denied`, the
abort at the window's close, shutdown with a pending callback). The cases
`runtime.acquire-page`, `runtime.acquire-retry-denied`,
`runtime.acquire-window` and `runtime.acquire-widen` run on `rust-api` and
`native` with identical bytes from scripted `sources` in both runners. Two
differences from the mirrored tests: the adapter-panic row is left out (a
host cannot panic the runtime), and in `runtime.acquire-window` the source
never answers, so both runners record `not_observed`, since a host's
cancellation answer crosses threads and cannot be ready when the runtime
polls once (the reason `runtime.cancel-stop` excludes `native`).

## Decisions at approval

Decided in this text, with reasons: a separate `acquisitionSources` seam,
not a hook on `contextSource` (the Rust traits are separate; 036 3.16.4);
explicit `sources` on compile (identity never depends on installed
state); cost bounds from the descriptor (as 042's client); safe-integer
generations, not `bigint` (as every count the binding returns);
content-addressed external capture references (the binding needs no
synchronous host callback during assembly); no Cargo feature (no new
dependency).

Ratified by the owner on 2026-11-03 (A-78), with these answers to the
open questions:

1. **Repeatability path.** Native only: in-process commands, with the
   repeat code moved into `rustev-surface` (increments 5, 6).
2. **Release target.** All six increments target 0.5.0, independent, so
   that 1 (capture), 3 (acquisition) and 4 (store) can each ship in a
   0.5.x alone.
3. **WebAssembly repeatability commands.** None.
4. **Store seam shape.** Mirrors `RuntimeStore` with generations, with the
   atomicity contract of 3.7.
5. **061's import.** `evalRepeatImport` in increment 6 (061 ratified as
   A-77).
6. **Capture on decide.** In scope: the sink's evidence cannot replay a
   decision (3.8), so capture is increment 1 and can ship alone.
   Asynchronous host callbacks for the source and store (3.4) are a
   condition of increments 3 and 4.

The owner also accepted on 2026-11-03 (A-78) the asynchronous seams of
3.4, the store atomicity contract and Redis table of 3.7, and capture on
decide, with two changes now in the text: capture is increment 1 and can
ship in a 0.5.x alone, because the CLI fallback cannot capture the
partner's decisions (a plan 3 decision, which `rustev run
--capture-bytes` refuses, with JavaScript model backends the CLI cannot
run); and embedded and external bundles carry unredacted content outside
spec 028's redaction policy, which hosts protect as personal data (3.8.4),
with the 7-day default and 30-day cap kept. The non-normative note on
replicated Redis or Valkey (primary endpoint only; generation keys not
evictable) is part of 3.7.

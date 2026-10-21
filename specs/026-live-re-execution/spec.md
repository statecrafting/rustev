---
id: "026-live-re-execution"
title: "Live re-execution with fresh observations"
status: approved
implementation: pending
created: "2026-09-26"
summary: >
  A separately invoked live re-execution path that creates a new decision from
  a newly identified snapshot, obtains fresh host authorization for every
  dispatch, applies new deadlines and budgets, and records new backend,
  cancellation, cost, retention, and delivery observations. It may compare
  the new proposal with a retained historical proposal, but never reuses a
  historical decision identity, supplies retained backend outputs as fresh,
  rewrites a replay bundle, or lets offline replay contact a backend.
amends:
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list (spec 021, 3.5).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "013-remote-state-on-failed-attempts"
  - "017-general-runtime-optimization"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Offline reproduction and candidate comparison remain unable to dispatch a backend; only the explicit live re-execution entry point may request fresh external work."
    anchor: "3-1-separate-live-entry-point"
  - id: "I-2"
    kind: invariant
    text: "Every live re-execution has a new decision id, a newly constructed identified snapshot, a fresh evaluation time, fresh admission and budgets, and new runtime observations; it never mutates or relabels a historical record."
    anchor: "3-2-new-decision-and-snapshot"
  - id: "I-3"
    kind: invariant
    text: "Before each external dispatch, the host independently authorizes the exact scope, backend target, artifact, operation, maximum exposure, and remaining time; missing, expired, mismatched, or refused authorization prevents dispatch."
    anchor: "3-3-host-authorization-at-dispatch"
  - id: "I-4"
    kind: invariant
    text: "Every attempted external effect, cancellation answer, observed or unknown charge, delivery result, retained reference, and comparison limitation is attributable to the new decision and cannot be concealed by the historical source."
    anchor: "3-6-evidence-cost-and-external-effects"
---

# 026: Live re-execution with fresh observations

Approved (A-17, 2026-09-26) as its own reviewable change before any code
(R-16); implementation is pending. Approval does not authorize a provider
call, select a provider, approve spend, change retention, or make a quality or
production claim.

## 1. Purpose

Define fresh execution of a previously recorded decision shape without
confusing it with spec 004 offline historical reproduction. Live
re-execution answers a new question at a new evaluation time from a newly
constructed snapshot and newly observed backend work. A caller may compare
the resulting proposal with a historical proposal, but neither result becomes
authority and the old record remains immutable.

The feature is explicit and opt-in. Loading, reproducing, evaluating, or
comparing a replay bundle never enters the live path and never receives a
backend, authorizer, network transport, or live budget by implication.

## 2. Territory and compatibility

This spec adds neutral live-run and comparison documents under
`rustev-contract`, a separately named entry point under `rustev-runtime`, and
comparison support under `rustev-eval`. It does not add I/O to core or eval,
change plan compilation, change the normal `decide` path, authorize an action,
or give replay a runtime dependency.

Existing `rustev.replay/1`, replay outcomes, candidate comparison, run records,
and `decide` APIs retain their bytes and behavior. New live documents use new
schema names. A live run may use an existing compiled plan or a separately
identified current plan, but plan selection is explicit and both historical
and live `PlanId` values remain visible.

## 3. Behavior

### 3.1 Separate live entry point

The only entry point is explicitly named `reexecute_live`. It requires a
compiled plan, a new decision id, a new snapshot, evaluation time, runtime
bounds, host authorization seam, cancellation signal, and an optional
historical comparison reference. None is inferred from a replay bundle.

The offline replay and candidate-comparison APIs continue to accept only
retained documents and synchronous byte resolution. They have no path to a
backend registry or `reexecute_live`. A CLI or integration added later must
make live mode a distinct command or route and must not overload `replay`.

### 3.2 New decision and snapshot

Every invocation uses a decision id unequal to the source decision and not
already active. The caller constructs a current snapshot through the normal
validated snapshot contract. The runtime computes its `SnapshotId`; it never
copies the old snapshot id onto changed content or fetches current context
from a historical reference.

Evaluation time is supplied anew. Current plan, descriptor, calibration,
artifact, context revision, principal scope, and retention identities are
validated as current inputs. Historical values may be displayed as comparison
metadata, but cannot fill missing current inputs or backend outputs. Missing,
stale, conflicting, or invalid current evidence remains unresolved or refused
under the plan contract.

The invocation produces an ordinary new judgment and run record through spec
003. Historical bundles, captures, run records, receipts, and judgments are
read-only. No live outcome overwrites, appends to, or relabels them.

### 3.3 Host authorization at dispatch

Rustev does not define or mint an authorization token. The host supplies an
authorizer seam and opaque, non-secret subject and scope handles. Immediately
before every external attempt, the runtime presents the exact decision,
scope, backend, artifact, operation, canonical request identity, maximum cost
exposure, remaining time, and whether the target is primary, retry, or
fallback.

The host returns only an in-process allow or refuse answer plus an opaque
decision reference for evidence. The answer is scoped to that attempt, cannot
be exported as permission, and authorizes no downstream action. Missing,
expired, mismatched, errored, or refused answers prevent dispatch and become a
typed `authorization_refused` runtime reason. Rustev never downgrades scope,
switches targets, retries, or falls back to avoid a refusal.

Authorization is checked again for every retry and fallback because target,
artifact, remaining time, or exposure may differ. Admission, plan capability,
budget reservation, and authorization are all required; none substitutes for
another. A host may refuse all external work while still permitting exact
steps and a resulting unresolved proposal.

### 3.4 Fresh dispatch, budgets, and time

Each live re-execution receives new admission, end-to-end deadline,
concurrency, retry, fallback, and cost-budget state under spec 003. Historical
reservations, charges, timeouts, attempts, cache observations, or provider
responses are never credited to the new run.

No attempt is sent before its full worst-case exposure is reserved and the
host authorizer allows that exact dispatch. An unknown cost remains unknown
liability. A historical zero charge does not imply a free new call. General
optimization under approved spec 017 remains independently
disabled unless the live plan enables it and every isolation and evidence
rule is met.

Current backend output is validated and supplied through the same core path as
normal runtime execution. Retained output may be used only by spec 004 offline
comparison, never as the result of a live attempt.

### 3.5 Cancellation and possibly continuing work

Cancellation follows specs 003 and 013. Before dispatch it causes no external
attempt. After dispatch the runtime requests cancellation and records the
backend's actual stopped, possibly-continuing, or unconfirmed answer. A caller
deadline or dropped future never proves remote work stopped or charge is zero.

The new decision may end cancelled or unresolved while external work may
continue. The runtime retains enough attempt identity for later cost and
remote-state reconciliation. It never changes the historical decision's
termination or cancellation observations.

### 3.6 Evidence, cost, and external effects

The new run record remains the evidence of execution. A separate bounded
`rustev.live-reexecution/1` document binds:

- new and historical decision ids, or explicit absence of a historical case;
- new and historical plan and snapshot identities;
- authorization decision references and refusals for every proposed attempt;
- every admission, reservation, dispatch, retry, fallback, cancellation,
  remote-state, charge, sink-delivery, and reconciliation observation;
- current capture and retention posture, including explicit unavailable,
  expired, erased, or inaccessible historical items; and
- comparison definition, outcome, exclusions, and limitations.

The document is bounded by `REPLAY_V1`, contains no credential or secret
authorization material, and is not itself permission or proof that the host's
authorization decision was correct. Effects are recorded even when the new
decision produces no proposal or evidence delivery fails. Unknown acceptance,
cost, remote state, or durability stays unknown.

### 3.7 Retention and erasure

Live re-execution does not extend source-bundle or source-item expiry. A
historical link is a digest plus identities, not embedded historical content
unless the host makes a separate explicit retention choice allowed by spec
004. The new run's capture and retention choices are independent and default
to the existing capture-off and digest-only posture.

The host continues to own access checks, storage, expiry, and erasure. If a
source item expires or is erased during a run, comparison becomes unavailable
or incomparable with the observed reason; live execution does not restore the
item from a copied digest. New retained data follows its own bounded lifetime
and erasure process.

### 3.8 Divergence and comparison

Comparison is optional and occurs only after the live run finishes or reaches
a recorded terminal outcome. The evaluator first verifies the historical case
under spec 004. It then compares an explicitly named proposal payload or
outcome category chosen by a versioned task adapter. Full judgment bytes are
not the agreement metric because decision, plan, snapshot, and time may
legitimately differ.

The report separates at least:

- snapshot, plan, artifact, served-identity, and evaluation-time changes;
- accepted proposal change, unresolved-category change, and unchanged result;
- unavailable or incomparable historical evidence;
- new backend, policy, calibration, and runtime observations;
- observed, estimated, and unknown cost; and
- cancellation, remote-state, retention, and delivery differences.

Changed output is `live-diverged`, not failed historical reproduction.
Unchanged output is agreement only for the named adapter and compared fields;
it is not determinism, calibration, quality, correctness, or authorization.

## 4. Out of scope

Automatic or scheduled re-execution; background monitoring; mutation of
historical records; silent provider contact; a replay fallback to live work;
provider or transport selection; authorization service implementation;
credential storage; action execution; persistent storage; legal retention
advice; real-user data; paid qualification; production enablement; quality,
calibration, determinism, or cost claims.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Offline replay encounters a missing retained output | Incomparable; zero backend calls. |
| The caller reuses the historical decision id | Refused before admission or authorization. |
| Current snapshot bytes differ but carry the old id | Refused as an identity mismatch. |
| Authorization is missing or expires before dispatch | No dispatch; `authorization_refused` is recorded. |
| Primary is allowed but fallback is refused | Primary evidence remains; fallback is not dispatched. |
| Cancellation after send is unconfirmed | New run records possibly continuing work and honest liability. |
| Source retention expires during comparison | Comparison becomes unavailable or incomparable; source is not restored. |
| Proposals match under different plans | Named-field agreement only; no determinism or quality claim. |
| Evidence delivery fails after a charged attempt | Failure, charge, and remote state remain in the returned live result. |

## Acceptance

- Compile-time tests prove replay and eval have no backend or runtime path and
  that only `reexecute_live` accepts the host authorizer.
- One deterministic test covers each section 5 row without network access or
  wall-clock sleeps.
- Scripted backends and authorizers cover allow, refusal, expiry, retry,
  fallback, cancellation, unknown charge, possibly continuing work, and sink
  failure with exact attempt attribution.
- Mutation tests prove a live result cannot alter the source bundle, capture,
  run record, judgment, or receipt.
- Comparison tests separate identity changes, proposal changes, unavailable
  history, cost, cancellation, retention, and delivery observations.
- Bounds reject oversized handles, authorization references, effect lists,
  comparison records, and retained content before unbounded allocation.
- Existing spec 004 offline replay tests retain a backend-call count of zero.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval of this spec authorizes only the
contract. No acceptance item is satisfied by approval or by existing offline
replay evidence.

## Implementation record

Delivered in two increments, each its own commit; nothing here claims an
acceptance item before the increment that satisfies it. Plan:

1. Contract and runtime: the `rustev.live-reexecution/1` document and the
   authorization vocabulary (3.3, 3.6), and `reexecute_live` with the host
   authorization seam (3.1 to 3.5).
2. Evaluation: comparison of a finished live run with its verified
   historical case (3.7, 3.8), section 5 rows 1, 7 and 8, the separation
   and immutability tests, mutation seeds and verification.

**Increment 1 (contract and runtime).**
`crates/rustev-contract/src/live.rs` defines `rustev.live-reexecution/1`
(`LiveReexecution`, `REPLAY_V1` bounds, record digest only, not
identified): the new decision, plan and computed snapshot identities, the
evaluation time, the opaque subject and scope, the historical case as a
`HistoricalLink` (decision, plan and snapshot identities, evaluation time
and the bundle's record digest, never content) or explicit `none`, the
admission, the new run record's digest, termination and cost summary, one
`LiveEffect` per proposed external attempt (role, target, backend,
artifact, `RequestId`, the digest of what was presented, exposure,
remaining time, the host's `AuthorizationReference` or the typed refusal,
and the dispatch observation: end, cancellation, remote state and charge),
the attempts whose charge is unknown (`unreconciled`), the delivery
observation, the capture and historical retention posture, and the
comparison section. `check` bounds every list (at most 4096 effects, 64
notes or fields) and every free text (256 bytes), and refuses an effect or
liability not attributable to the new decision, or a new decision id
equal to the historical one. `AuthorizationReference` is 1 to 256 bytes,
refused on construction and parse. `AuthorizationRequest` is what the
runtime presents; its tagged digest
(`rustev.live-authorization/1`) binds an answer to exactly one attempt.
Refusals are `missing`, `expired`, `mismatched`, `errored` and `refused`,
each recorded as `authorization_refused`.

`crates/rustev-runtime/src/live.rs` adds `Runtime::reexecute_live(plan,
LiveRequest, &dyn LiveAuthorizer, cancel)`, the only function that takes an
authorizer. Before admission and before any authorization it refuses a
decision id equal to the linked historical one, a snapshot whose computed
`SnapshotId` differs from the identity stated for it, an invalid capture
configuration, and a plan whose worst-case attempts exceed the document
bound; every admission refusal of `decide` is returned as such. Then it
runs an ordinary new decision through spec 003 (fresh admission, deadline,
ledger, retries and fallback). In the attempt driver, after the attempt's
full worst-case reservation and the deadline and cancellation re-check,
the host is asked about that exact attempt (decision, subject, scope, step,
instance, operation, target, role, backend, artifact, `RequestId` for that
target, attempt id, exposure, remaining time); a panic is contained as
`errored`. An allow is accepted only if it names the presented digest and
is still within its validity by the runtime clock. A refusal releases the
reservation, sends nothing, and ends the request with a stop transition
`authorization_refused: <refusal>`; nothing retries, falls back or
downgrades to avoid it, and retries and fallbacks are each authorized
again. The live document is returned with the new run record, the
delivery result (also when delivery failed) and the optional capture.

**Engineering choices of increment 1**, made within this spec and open to
the owner's review:

- E-26-1: `authorization_refused` is typed in the live document; the core
  is supplied `backend_unavailable` with detail
  `authorization_refused: <refusal>`, the form spec 003 3.6.4 uses for
  failure classes. Spec 002's closed set of four runtime reasons, every
  policy's static coverage, `rustev.run/1` and the core are unchanged (this
  spec does not extend `rustev-core`).
- E-26-2: the authorizer seam is synchronous and lives in `rustev-runtime`,
  not in the core seams; its answer (`DispatchAnswer`) is neither
  serializable nor clonable, and only its reference is kept. Validity is
  a duration from the question, judged by the runtime's clock.
- E-26-3: on the live path the memory cache is not consulted and general
  batching is not formed (a diagnostic says so when the plan enables
  either): each attempt is fresh work, authorized on its own (3.3, 3.4).
- E-26-4: the live document is not added to the emitted JSON Schemas of
  spec 027, 3.2.2: that would regenerate the committed TypeScript
  declarations under `integrations/rustev-wasm/` and
  `integrations/rustev-node/`, which this spec does not extend.

Tests: `crates/rustev-contract/tests/live.rs` and
`crates/rustev-runtime/tests/live.rs` (rows 2 to 6 and 9 of section 5;
allow, refusal, mismatch, error, panic, expiry, retry, fallback,
cancellation, unknown charge, possibly continuing work and sink failure
with exact attempt attribution; a host refusing all external work still
gets a judgment from exact steps; cache and batch bypass; capture). The
compile-fail examples on `reexecute_live` show that `decide` and
`decide_with_capture` take no authorizer.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Is a library-only live entry point sufficient, or is a separately governed
   CLI or serving surface required by a demonstrated consumer?
2. Which host authorization contract can provide attempt-scoped answers and
   durable references without making Rustev an authority service?
3. Which task adapters and retained fields are allowed for live divergence
   reports, and what privacy basis permits retaining the comparison?
4. Which providers, if any, may later be qualified for live use, under what
   spend cap and separate approval?

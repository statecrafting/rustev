---
id: "010-integrations"
title: "Optional ecosystem integrations and serving boundary"
status: approved
implementation: pending
created: "2026-09-26"
summary: >
  A shared boundary contract for three independently optional integration
  crates: `rustev-aicortex` adapts attributed context and observation
  proposals, `rustev-rahi` adapts evidence delivery and optional operational
  hosting, and `rustev-serve` exposes bounded registered-plan execution over
  an application-operated service boundary. Every integration preserves
  explicit identity, provenance, failure and unresolved effects; none grants
  authority, owns domain judgment, or makes Rustev depend on an ecosystem
  service. Concrete external API bindings and readiness claims require later
  evidence before implementation.
establishes:
  - { kind: directory, path: "integrations/rustev-aicortex/", planned: true }
  - { kind: directory, path: "integrations/rustev-rahi/", planned: true }
  - { kind: directory, path: "integrations/rustev-serve/", planned: true }
extends:
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No integration output is authorization: Aicortex context remains attributed input, Rahi receipts establish only their declared acceptance or durability, and a serving response is only a Rustev judgment, unresolved outcome, or failure."
    anchor: "3-1-common-boundary"
  - id: "I-2"
    kind: invariant
    text: "Each integration is independently optional and removable; contract, core, runtime, evaluation, backends and packages build without all three, and no crate depends on an integration crate."
    anchor: "3-2-dependencies-and-partial-adoption"
  - id: "I-3"
    kind: invariant
    text: "Every external object, request, response, receipt and failure used by an integration retains its producer identity or an explicit unknown; an adapter never substitutes a requested identity for an observed identity."
    anchor: "3-3-identity-provenance-and-failure"
  - id: "I-4"
    kind: invariant
    text: "Aicortex, Rahi, HTTP and async dependencies remain under integrations; rustev-contract and rustev-core gain no async runtime, HTTP or ecosystem dependency."
    anchor: "3-2-dependencies-and-partial-adoption"
  - id: "R-1"
    kind: requirement
    text: "Before an integration is implemented, an owner-reviewed binding record identifies the exact external contract, immutable dependency versions, compatibility range, failure mapping, security and privacy posture, resource limits, and retained qualification evidence."
    anchor: "3-4-binding-records-before-implementation"
---

# 010: Optional ecosystem integrations and serving boundary

Approved (A-15, 2026-09-26) as its own reviewable change before any code
(R-16); implementation is pending. Approval does not claim that Aicortex,
Rahi, any HTTP service, deployment, or external contract is ready. It does not
approve a release, a live provider call, production traffic, real-user data,
secrets access, or publication. Each binding record remains a separate
owner-review act under R-34.

## 1. Purpose

Reserve one coherent integration boundary for three optional adapters named in
the architecture: attributed knowledge through Aicortex, operational evidence
and hosting through Rahi, and an application-operated Rustev service. The
shared contract prevents an adapter from erasing identity, provenance,
failure, or the line between a proposal and authority.

The three future crates have one owning spec because this spec establishes all
three directories and defines their complete common boundary. A later spec may
amend this contract or add behavior inside an owned directory, but it does not
become a second owner. If the external contracts cannot satisfy this boundary,
the affected crate remains absent.

## 2. Territory and lifecycle

When implemented after the required binding-record reviews, this spec owns:

- `integrations/rustev-aicortex/`, an adapter between an authorized Aicortex
  read surface and Rustev context plus a separate observation-proposal sink;
- `integrations/rustev-rahi/`, an evidence sink and optional operational
  hosting adapter over a selected Rahi contract; and
- `integrations/rustev-serve/`, a bounded HTTP service around host-registered
  Rustev plans and a host-constructed runtime.

It extends the workspace, lockfile, verification target and dependency-family
boundary check only as needed to add those crates. It does not amend the
approved meaning of a Rustev value, judgment, plan, run record, evidence sink,
remote backend exchange, retry, cancellation, budget, or authorization
boundary.

No crate or placeholder directory is created by the specification unit. Concrete
external APIs, protocol versions, dependency selections, authentication
mechanisms, storage guarantees and deployment topology remain pending the
binding records in section 3.4.

## 3. Behavior

### 3.1 Common boundary

1. Rustev produces proposals and unresolved outcomes, never grants. An
   integration defines no `Permitted`, `Grant`, capability token, executor
   credential, or equivalent authorization type. It does not translate a
   confidence, label, receipt, HTTP success, or knowledge-store acceptance
   into permission.
2. The host owns authentication, principal resolution, authorized reads,
   authority policy, action execution and deployment policy. An integration
   may carry a host-supplied opaque tenant or principal handle for isolation
   and audit, but cannot construct it from semantic output.
3. External text and structured values are data. They cannot change the
   registered plan, selection policy, thresholds, backend binding, retry,
   fallback, budget, retention policy, or executable operation.
4. Every Rustev value produced from external context retains the source
   provenance and receives the derivation class required by specs 001 and 002.
   Missing, stale, conflicting, malformed, unauthorized or unidentifiable
   external data becomes a typed refusal or unresolved outcome, never a
   default.
5. An integration is replaceable at its seam. Removing or swapping it changes
   no core decision semantics except through explicit input, backend, sink or
   plan identities already recorded by the approved contracts.

### 3.2 Dependencies and partial adoption

1. Only the crate that binds an ecosystem contract depends on that ecosystem's
   packages. `rustev-aicortex` may depend on Aicortex, `rustev-rahi` may depend
   on Rahi, and `rustev-serve` may depend on an HTTP and async stack. None may
   depend on statecraft-cli at runtime.
2. All three crates live under `integrations/`. No workspace crate depends on
   any of them. They may depend inward on the smallest Rustev crates required
   by their seam.
3. `rustev-contract` and `rustev-core` gain no HTTP, async-runtime, Aicortex,
   Rahi or statecraft dependency, direct or transitive. This spec adds no
   ecosystem name or vendor-specific field to their neutral documents.
4. Each integration has default features empty. Ecosystem bindings and server
   transports are explicit features only when more than one binding exists;
   a feature does not silently enable network access, artifact acquisition or
   a second integration.
5. Acceptance builds each future crate alone with its minimum features, builds
   contract and core with no integration, and proves the existing workspace
   without the three new crates remains valid. No test requires a live external
   service unless separately authorized qualification supplies one.

### 3.3 Identity, provenance and failure

1. Every adapter descriptor binds its crate version, external protocol and
   schema version, selected dependency versions, configuration digest, and
   behavioral limits. Mutable aliases are descriptive only and never identity.
2. An external object retains the identity supplied by its producer. If the
   producer cannot expose a stable revision or served identity, the adapter
   records `unknown` and applies the binding record's refusal or restricted-use
   rule. It never copies a requested id into an observed field.
3. Conversion is fallible and bounded before typed construction. Unknown
   fields, duplicate keys, excessive bytes, nesting, collections or values,
   invalid Unicode where prohibited, and non-finite numbers are rejected with
   a closed adapter failure. Input bounds do not claim injection resistance.
4. Failures state their effect: nothing accepted, acceptance unknown, accepted
   under the external system's named durability terms, or remote work possibly
   continuing. Timeouts and connection loss never assert rollback, absence of
   charge, or absence of a write.
5. Each operation receives one end-to-end deadline and cancellation signal.
   Internal phase limits fit within the remaining deadline. Cancellation is
   acknowledged as stopped only when the adapter can establish that no request
   left the process or the external system confirms the stop.
6. Retries, fallback and redelivery are host or runtime policy. An integration
   does not retry invisibly, switch an endpoint, broaden a query, change a
   storage class, or select a different plan.
7. Logs and retained evidence exclude secrets and apply the host's data
   minimization and retention policy. Digests and opaque references are the
   default; retaining content is an explicit host choice with its own authority
   and erasure obligations.

### 3.4 Binding records before implementation

Each crate requires a separate owner-reviewed record before implementation.
The record is evidence for deciding whether implementation may begin, not a
readiness or release claim. It contains:

1. exact upstream repositories, immutable revisions or released package
   versions, licenses, notices, supported platforms and supply-chain checks;
2. exact request, response, object and receipt schemas with version and
   compatibility rules, byte examples and negative fixtures;
3. authentication and credential ownership, authorized scopes, tenant and
   principal handling, TLS posture, endpoint identity and redirect policy;
4. connection, queue, concurrency, byte, item, deadline, cancellation and cost
   bounds, including behavior after uncertainty;
5. a total mapping from external failures and partial success into the closed
   Rustev failure, unresolved, delivery or transport effects that apply;
6. privacy, logging, retention, correction, revocation and erasure behavior,
   including which obligations remain with independent consumers;
7. offline contract tests and separately authorized live qualification, if any,
   with immutable evidence identities and limitations; and
8. a pass or refuse result for every required row. Missing evidence is a
   refusal, not permission to infer compatibility.

The current architecture says Aicortex and Rahi were not assessed beyond
their summaries. That is insufficient for either binding record. This spec
therefore selects no dependency or external version.

### 3.5 Aicortex adapter

1. Reads begin with a host-authorized query and return only attributed values
   the external contract says the caller may read. Each mapped field carries
   source, actor or producer, stable claim identity, revision, validity or
   as-of time, trust or provenance class, and correction or revocation state,
   or the adapter refuses the field as unidentifiable.
2. The adapter maps a bounded, explicit projection into Rustev context. It
   does not perform open-ended retrieval, add undeclared fields, reinterpret a
   claim as verified, or make an Aicortex confidence a Rustev calibrated
   probability.
3. Corrections, revocations and erasures become identified invalidations when
   the selected external contract supports them. Gaps, lag and disconnects are
   visible. No invalidation capability is invented from polling.
4. The outbound direction accepts only an attributed observation proposal
   whose lineage identifies the decision, plan, snapshot and observed outcome.
   A Rustev judgment alone is not an observation about reality. External
   acceptance does not make the proposal true or authorized.
5. Read and proposal-write capabilities are configured independently. A host
   can build and use the context adapter without enabling writes.

### 3.6 Rahi adapter

1. Rahi is optional operational infrastructure. The adapter may implement an
   evidence sink and may supply host-selected storage or process plumbing; it
   does not compile plans, choose backends, interpret values, select a domain
   outcome, or own a Rustev judgment.
2. Evidence delivery uses the decision id as its deduplication key. A receipt
   records the external record identity, accepted digest, acceptance time if
   supplied, and the exact declared durability state. It proves only what that
   state says, never durable persistence by implication.
3. Duplicate, conflict, rejection, overload, timeout, unavailable and
   acceptance-unknown results are distinct. A digest mismatch on an existing
   decision id is a conflict and never an accepted duplicate.
4. Rahi-owned authentication, tenancy, quotas, storage retention and audit
   remain external enforcement. Rustev records the selected sink policy and
   receipt but does not claim those controls are configured or effective.
5. Hosting Rustev beside Rahi does not make Rahi a runtime dependency of core
   or contract and does not make the integration mandatory for evidence.

### 3.7 Serving adapter

1. The service runs only host-registered, identified plans and host-registered
   backend bindings. A request cannot upload a plan, operator, selection
   policy, fallback, executable code, credential, endpoint, or resource limit.
2. The host authenticates the caller and resolves the opaque authorized scope
   before Rustev dispatch. The service rejects an absent or invalid scope and
   never derives one from request content or a judgment.
3. Request bytes are bounded before parsing. The service then applies the
   approved contract's typed bounds, admission, deadline, budget and
   cancellation rules. Transport success is separate from decision status:
   judged, unresolved, refused, cancelled, overloaded and evidence delivery
   failure remain distinguishable.
4. A response identifies the service build, protocol, plan, definition,
   execution policy, decision and evidence disposition. Backend and served
   model identity remain as explicit as the underlying record, including
   `unknown`; the service does not strengthen them.
5. Network disconnect after dispatch is an uncertain outcome. It does not
   prove cancellation, no provider work, no charge or no evidence write.
   Idempotent resubmission and record lookup require the same host-scoped
   decision id and cannot silently start a second decision.
6. This spec does not define `rustev.run/2`, runtime-planned remote batching,
   connection reuse, per-phase timeout semantics, or a principal wire option.
   Those proposed refinements belong to spec 020 and must amend approved specs
   where they change approved behavior.
7. The service returns proposals and unresolved outcomes only. An HTTP 2xx,
   a resolved judgment, or a high score is never an authorization or evidence
   that an application effect occurred.

## 4. Out of scope

Implementing any crate; selecting or changing an external API; provider or
deployment qualification; production serving; authentication or identity;
application authority; action execution; arbitrary plan upload; a hosted
Rustev product; dynamic loading; distributed execution; new runtime
optimization; a new remote protocol version; publishing or release.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A crate outside `integrations/` depends on Aicortex, Rahi or an HTTP stack | The boundary check fails naming the path. |
| Contract or core gains an async executor through an integration helper | The boundary check fails; the dependency is refused. |
| Aicortex returns a claim without a stable revision where the plan requires freshness | The field is refused or unresolved, never treated as current. |
| A judgment is sent as an independently true Aicortex observation | Refused before dispatch. |
| Rahi times out after request bytes were sent | Delivery is acceptance-unknown; no durability receipt is invented. |
| Rahi reports the same decision id with a different digest | Conflict, never duplicate success. |
| A serving request names an unregistered plan or supplies a new fallback | Refused without backend dispatch. |
| The client disconnects after serving dispatch | The record preserves possible continuation, charge and delivery effects; no false cancellation acknowledgement. |
| A serving response contains a resolved judgment | It remains a proposal, not an authorization. |
| Any external binding lacks the section 3.4 record | The affected crate remains unimplemented. |

## Acceptance

Before this spec may become implementation-complete:

- all three section 3.4 records have owner-reviewed pass results, or the
  corresponding crate is removed from the approved implementation scope by a
  separately reviewed spec change;
- one test per section 5 row passes with offline fakes and negative controls;
- each integration builds and tests independently with minimum features and no
  other integration present;
- contract, core and the standalone embedded workspace build without all three
  crates, and the dependency boundary check passes;
- schema and conversion tests cover every identity, bound, unknown and failure
  effect in the selected external contract, with no live service required;
- any separately authorized live qualification records exact external
  identities, conditions and limitations and is not relabeled as readiness;
- `make code`, the declared verification for this spec, and `make gate` pass.

## Verification

Pending with implementation. The eventual implementation change
must replace this paragraph with executable, repository-local verification
covering the accepted subset and every negative case. Approval alone creates
no crate or runnable acceptance target.

## Questions settled by R-34

The owner accepted the recommendations below when approving this spec. Each
binding record remains a separate owner-review act.

1. Which immutable Aicortex and Rahi contracts, if any, pass the binding
   record, and should any integration remain deferred after review?
2. Which authentication and protocol surface may `rustev-serve` bind after
   spec 020 resolves the proposed remote refinements?
3. Which live qualification, data and deployment environments, if any, are
   authorized separately from implementation?

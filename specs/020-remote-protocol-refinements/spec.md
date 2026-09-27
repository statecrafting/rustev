---
id: "020-remote-protocol-refinements"
title: "Remote protocol and evidence refinements"
status: approved
implementation: pending
created: "2026-09-27"
summary: >
  A separately reviewable refinement of the approved runtime and remote
  contracts: `rustev.run/2` references bounded remote exchange evidence,
  the runtime may dispatch an explicitly compatible general batch, transport
  phases receive identified ceilings inside one total attempt budget,
  connection reuse is bounded and isolation-keyed, and a host-operated
  endpoint may receive an opt-in opaque principal reference. All features
  default off or preserve version 1 behavior. Approval authorizes
  implementation only: no migration, qualification, deployment, live call,
  spend, or publication is authorized or claimed.
amends:
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: amending }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
  - "017-general-runtime-optimization"
references:
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Every transport phase ceiling consumes the one remaining attempt budget; no phase, retry, pooled connection or batch grants more time, cost, authority or work than the identified plan permits."
    anchor: "3-4-phase-specific-time-ceilings"
  - id: "I-2"
    kind: invariant
    text: "A pooled connection is reusable only within one complete transport isolation key and never carries credentials or principal context across a key boundary."
    anchor: "3-5-bounded-connection-reuse"
  - id: "I-3"
    kind: invariant
    text: "A principal reference is optional host-supplied routing context for a host-operated endpoint, never a credential, grant, policy input, provider field or authorization."
    anchor: "3-6-host-endpoint-principal-reference"
  - id: "I-4"
    kind: invariant
    text: "A run record references remote evidence without embedding secrets or unbounded exchange bodies, and unavailable referenced evidence is explicit rather than reconstructed."
    anchor: "3-2-rustev-run-2"
---

# 020: Remote protocol and evidence refinements

Approved (A-19, 2026-09-26) as its own reviewable change before any code
(R-16); implementation is pending. Approval does not claim that a remote
provider, transport, batch path, pool, principal mapping, migration, or
consumer has been qualified.

## 1. Purpose

Close the deferred remote-boundary gaps recorded by spec 009 and the backlog:

1. make retained remote exchange evidence discoverable from the run record;
2. let the runtime, rather than an adapter alone, plan compatible remote
   batches under the general optimization contract of spec 017;
3. divide one attempt budget into observable transport-phase ceilings;
4. permit bounded connection reuse without weakening isolation; and
5. expose the already permitted host-endpoint principal option as an explicit,
   privacy-bounded wire field.

These refinements preserve Rustev's authority boundary. They alter transport
and evidence, not value kinds, judgment rules, retries, fallback, or action
authority.

## 2. Territory, versions, and compatibility

`rustev.run/2` is an additive contract document. `rustev.run/1` remains valid
and remains the default emitted schema until a host selects version 2 at sink
construction. A version 2 record has a deterministic version 1 projection as
defined in 3.2. A version 1 reader is never given version 2 bytes.

The neutral remote protocol gains `rustev.remote/2` only for the wire members
defined here. Version 1 negotiation and messages remain valid. An adapter
binding selects exactly one negotiated version and records it in artifact
identity. There is no opportunistic per-call upgrade, downgrade, or fallback.
Foreign-wire adapters may implement equivalent behavior through a versioned
mapping without claiming that the provider speaks `rustev.remote/2`.

All new execution features are disabled by default. Existing version 1 run
records, unbatched dispatch, one undivided transport timeout, fresh connection
behavior, and omitted principal bytes remain byte-identical. Enabling or
changing a behavior-affecting policy changes the adapter binding identity and
therefore every affected `PlanId`.

Spec 017 is approved but pending. This spec defines the remote seam needed by
its general batching contract, but does not independently enable general
batching. The adapter-scoped batching already governed by specs 009 and 012 is
unchanged.

## 3. Behavior

### 3.1 Identified configuration

The adapter binding document adds, with explicit `disabled` values:

- accepted run-record schema at the evidence sink;
- remote protocol or foreign mapping version;
- batch-dispatch posture and declared hard bounds;
- transport phase policy;
- connection-pool policy and isolation-key schema version; and
- principal wire posture and principal-reference mapping version.

Every setting that can alter an output, failure, dispatch, privacy route, or
deadline is artifact identity. Operational addresses, credentials, current
pool contents, queue timing, and connection age remain runtime configuration
or observations and are never embedded as secrets in an identity document.
Construction refuses an incomplete combination. Negotiated terms cannot
silently enable a feature the binding disabled or raise a declared bound.

### 3.2 `rustev.run/2`

Version 2 contains all version 1 members with the same meanings and adds a
bounded `remote_evidence` list. There is at most one entry for each remote
attempt recorded by the run. Entries are ordered by attempt order and contain:

- attempt id and its decision step and logical-request references;
- exchange-record digest, exchange schema, retention class, and locator
  reference, or an explicit unavailable reason;
- requested identity, served identity and disclosure posture;
- adapter binding, protocol or mapping version, and endpoint class;
- remote error code, final remote state, and charge provenance;
- batch id and member index when dispatched as a batch member; and
- a redacted principal-reference digest only when that option was sent.

The locator is an opaque host reference, not a URL with credentials. The list,
each string, and each encoded document have declared hard limits. Raw request
and response bodies, credentials, secret addresses, unredacted principal
references, provider reasoning, and arbitrary headers are never embedded.
Exchange records retain their separate spec 009 limits and retention policy.

The version 1 projection removes `remote_evidence`, changes only the schema
tag and otherwise emits the exact canonical version 1 representation. The
projection is permitted only when every version 1 required member is present;
failure is an evidence-sink error, not a partial record. A sink selects one
accepted schema when constructed. Runtime delivery never retries a rejected
version 2 record as version 1.

Offline replay uses retained raw outputs as spec 004 requires. It performs no
network request and does not fetch an exchange record to regenerate a value.
A missing, expired, corrupt, or unauthorized exchange reference is reported as
remote evidence unavailable; it does not make an otherwise complete historical
replay invent data or call the backend. Evaluation may verify available
exchange digests and reports availability separately from judgment parity.

### 3.3 Runtime-planned remote batches

The existing `DecisionBackend::infer` behavior remains unchanged. A separate
optional batch-capable seam accepts a nonempty, bounded list of complete
logical attempt calls and returns exactly one attributable report per member.
A backend exposes it only when its descriptor and negotiated remote terms
declare compatible shared-state batching.

The runtime may call the seam only under an approved and enabled spec 017
batch policy. Every member has its own attempt id, call context, authorization
scope, reservation, remaining budget, cancellation signal, output validation,
failure, retry and fallback decision, remote state, charge share, and run
record. The runtime passes complete membership at dispatch. The adapter never
adds, drops, reorders, retries, substitutes, or merges logical members.

Batch identity records the policy, backend artifact, ordered attempt ids and a
digest of canonical member identities. The dispatch deadline is no later than
the earliest remaining member deadline. A member cancelled or expired before
the first request byte is removed and recorded unsent. After send, cancellation
and possibly-continuing liability remain per member. A missing, duplicate,
extra, or mismatched response fails only the attributable members when that can
be established; an un-attributable envelope failure is recorded for every
dispatched member.

Provider charge allocation follows spec 017 and sums exactly to the observed
batch charge. Unknown charge remains liability. Adapter-scoped batching in
specs 009 and 012 remains a distinct special case and is not evidence that this
runtime-planned path is qualified.

### 3.4 Phase-specific time ceilings

An enabled transport policy supplies positive ceilings for resolution,
connect, secure handshake, request write, first response byte, response body,
and cancellation acknowledgement. Each ceiling is a maximum within
`CallContext::remaining_ms`, never a separately replenished budget. Before a
phase starts, its effective allowance is the lesser of its configured ceiling
and the one remaining attempt budget. A phase that consumes less leaves the
remainder available; no later phase recovers time already consumed.

The response-body ceiling measures the complete bounded body and is not reset
by chunks. Redirects remain prohibited. Connection reuse may skip resolution,
connect, and handshake, but does not transfer their unused ceilings to extend
the total budget. The runtime deadline and raised cancellation signal remain
authoritative.

An expired pre-send phase maps to `transport_unsent` with observed zero charge
when spec 009 permits that claim. An expired phase after request bytes maps to
`transport_interrupted`, with remote state and charge handled by spec 013.
Cancellation acknowledgement expiry follows spec 009. The exchange record
states the phase, configured ceiling, effective allowance, elapsed observation,
bytes-sent state, and resulting existing error code. The adapter adds no retry.

### 3.5 Bounded connection reuse

Reuse is opt-in. The policy hard-bounds total connections, connections per
origin, concurrent streams, idle connections, idle lifetime, absolute
connection lifetime, and uses per connection. Exhaustion follows the declared
queue-or-refuse behavior within the member's existing deadline and concurrency
limits. Pool state is runtime state, never core state or replay input.

A connection can be reused only under an exact isolation key containing:

- endpoint class and resolved authority;
- protocol, proxy route, TLS verification and pinning posture;
- credential identity digest, never credential bytes;
- privacy and provider-routing options;
- principal wire posture and principal-reference scope; and
- pool-key schema and adapter binding identities.

Unknown or noncanonical key material disables reuse. A connection never moves
between keys, tenants, principals, credentials, proxies, TLS policies, or
endpoint classes. HTTP multiplexing is allowed only within one exact key and
preserves per-member cancellation and evidence.

A stale reused connection that fails before request bytes is
`transport_unsent`; failure after bytes is `transport_interrupted`. The adapter
does not transparently reconnect or replay. A plan-declared runtime retry uses
a new attempt id and ordinary admission. The exchange record reports fresh or
reused, a nonsecret connection instance id, age bucket, use count, multiplex
posture, isolation-key digest, and eviction reason.

### 3.6 Host-endpoint principal reference

The wire option is `omitted` by default. It can be enabled only for an endpoint
classified `host_operated`; configuration for a third-party endpoint is
refused. The host supplies a bounded opaque principal reference and mapping
version. It may derive that reference from `CallContext`'s principal handle,
but Rustev does not interpret, authenticate, authorize, or expand it.

The reference is routing and isolation context only. It is not a credential,
bearer token, capability, tenant grant, policy fact, input value, action
permission, or proof that the endpoint enforced isolation. The endpoint cannot
return a changed principal. The reference has a strict byte limit, canonical
encoding, declared scope and host-managed rotation. It is sent only in the
identified request field, omitted from logs and run records, and represented
in evidence by a domain-separated digest and mapping version.

Changing posture, mapping, or scope changes adapter binding identity. Rotating
the opaque value does not change `PlanId`, but changes the pool isolation key
and evidence digest. A missing reference when enabled refuses the attempt
before send. The option never reaches a foreign provider through redirects,
proxy-added application fields, fallback, or provider-specific extras.

### 3.7 Migration and failure discipline

Version negotiation occurs at adapter construction. A version mismatch is a
capability failure before a plan is compiled or an `identity_mismatch` if a
negotiated endpoint drifts. Run-sink version mismatch is an evidence delivery
failure and never changes the judgment. Unknown fields, unsupported options,
limit violations, and batch membership conflicts fail closed under the
existing error taxonomy.

The Jev adapter must version any foreign-wire mapping changed by these
features. Its existing batching remains off by default as spec 012 requires.
No earlier stage-3 qualification is reused as proof for a changed mapping,
principal option, pool policy, phase policy, or runtime-planned batch path.
A future qualification uses a newly authorized, bounded protocol and dataset
and reports exact identities; this spec authorizes no live call or spend.

## 4. Out of scope

Implementation; approval of spec 017; automatic migration; transparent retry;
new value kinds; changing judgment or fallback semantics; sending raw input or
principal data beyond the selected endpoint; third-party principal forwarding;
distributed pools; cross-process connection sharing; publication; deployment;
provider qualification; production enablement; and rerunning any closed paid
qualification.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A version 2 sink rejects a run record | Evidence delivery fails; no silent version 1 retry and no judgment change. |
| A retained exchange record is absent during replay | Replay remains offline; remote evidence is reported unavailable. |
| General batching is disabled or spec 017 is not approved | The runtime uses the existing single-call seam. |
| One batch response omits a member | That member fails explicitly; no sibling value fills it. |
| A response-body chunk arrives just before its ceiling repeatedly | The complete-body ceiling still expires; chunks do not reset it. |
| A pooled connection has a different credential or principal scope | It is not reused. |
| A stale pooled connection fails after request bytes | No transparent retry; interrupted and possibly-continuing evidence is retained. |
| Principal forwarding is enabled for a third-party endpoint | Adapter construction refuses the configuration. |
| Principal forwarding is enabled but the call has no reference | The attempt is refused before any request byte is sent. |
| A provider returns a principal or authorization claim | It is ignored as authority and cannot enter core input or judgment. |

## Acceptance

- Canonical fixtures prove version 1 bytes remain unchanged, version 2 round
  trips within every bound, and version 2 projects deterministically to version
  1 when projection is valid.
- Offline replay tests cover available, missing, corrupt, expired and forbidden
  exchange references without a transport or store call.
- Differential tests compare eligible runtime-planned batches with independent
  calls across values, failures, cancellation, deadlines, fallback, charge,
  remote state and evidence.
- A deterministic fake transport covers every phase boundary, exact exhaustion,
  slow response chunks, cancellation races and the error mappings in 3.4.
- Pool tests cover every key member, limit and eviction rule, stale connections,
  multiplex cancellation, credential rotation, principal rotation, and strict
  cross-scope negative controls.
- Principal tests prove default omission, host-only construction, byte bounds,
  redaction, no authority conversion, no redirect or provider forwarding, and
  refusal before send when required context is absent.
- Jev tests distinguish its existing adapter batch from general runtime
  batching and use new qualification evidence for every enabled refinement.
- `make code`, each declared spec verification, and `make gate` pass.

## Verification

Pending with implementation. Approval authorizes only this contract.
The implementing change must add repository-local deterministic verification
for every acceptance item before implementation can be called complete.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Should the owner approve these refinements as one compatibility release or
   split run evidence, batch dispatch, transport policy, pooling, and principal
   forwarding into separately implemented amendments?
2. Which sink migration window, if any, should support both run schemas?
3. Which measured transport distributions justify initial phase ceilings and
   pool bounds without turning observations into universal defaults?
4. Is the host-endpoint principal reference needed by a concrete consumer, and
   what privacy review must precede its enablement?
5. Which newly authorized qualification can support Jev adoption without
   rerunning the closed stage-3 experiment or spending by implication?

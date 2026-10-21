---
id: "020-remote-protocol-refinements"
title: "Remote protocol and evidence refinements"
status: approved
implementation: complete
created: "2026-09-26"
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
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: amending }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list (spec 021, 3.5).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # The document declarations generated from the contract's schemas (spec
  # 027, 3.2.3), regenerated when an exchange or run member is added.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
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
(R-16). All six planned increments are delivered (see the implementation
record); the owner ruled the two acceptance items the record lists as not
verifiable as written not applicable as built (R-47), and `implementation`
is `complete`. Approval does not claim that a remote
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

## Implementation record

Delivered in increments, each its own change; nothing here claims an
acceptance item before the increment that satisfies it. Plan:

1. Phase-specific time ceilings (3.4) and the binding's phase member (3.1).
2. The host-endpoint principal reference and `rustev.remote/2` (3.6, 3.7).
3. Bounded connection reuse (3.5).
4. `rustev.run/2` and offline remote-evidence availability (3.2).
5. Runtime-planned remote batches (3.3).
6. Jev mapping versions, the remaining acceptance items, verification.

**Increment 1 (phase ceilings).** `integrations/rustev-remote-http/src/phases.rs`:
`PhasePolicy` (a positive ceiling for resolution, connect, secure
handshake, request write, first response byte, response body and
cancellation acknowledgement; zero is refused at construction as
`SetupError::Config`) and `TransportPhases`, `disabled` by default.
`PhaseClock` holds the running phase; entering a phase fixes its effective
allowance as the lesser of its ceiling and the budget left at that moment,
and phases only move forward, so unused time returns to the budget, a
skipped phase transfers nothing, and a chunk never restarts the response
body. `phased` watches the running phase's deadline and ends the exchange
with `TransportError::PhaseExpired`. `HttpTransport::with_phases` enters
the phases on the real connection: resolution, connect, the TLS handshake
(absent on plaintext loopback), request write, first response byte (from
the flush that follows the connection taking the request body, reported by
`TrackedIo`), and response body (from the first response byte to the
complete bounded body). An expiry before request bytes left is
`transport_unsent` with `observed{0}`; after, `transport_interrupted` with
charge `unknown` and `possibly_continuing` (spec 013). The cancellation
acknowledgement ceiling cuts the budget of every cancel exchange. The
adapter adds no retry. `rustev.remote-exchange/1` gains an optional
`phase_expiry` (phase, configured ceiling, effective allowance, elapsed
time, whether the ceiling or the budget bounded it, bytes sent, code),
omitted when absent, so every existing record keeps its bytes.
`AdapterBinding` gains `transport_phases`, not written when disabled: a
binding with every refinement disabled keeps its spec 009 bytes and
artifact (pinned by `a_binding_with_every_refinement_disabled_keeps_its_spec_009_bytes`),
and an enabled policy changes the artifact and so every affected
`PlanId`. The Jev adapter compiles the new module and maps the new
transport error, but keeps the policy disabled. Tests:
`integrations/rustev-remote-http/tests/phases.rs` runs a deterministic fake
transport on Tokio's paused clock (every phase boundary and its code,
exact exhaustion, no carry-over, budget-bounded allowances, slow response
chunks, a cancellation raised first or at the same instant) and loopback
exchanges (refusal of a zero ceiling, unchanged binding and records when
disabled, a silent remote expiring the first-byte phase, a dribbled body
expiring the body ceiling, a never-answered cancel bounded by the
acknowledgement ceiling). The schemas and the generated declarations are
regenerated; `src/phases.rs` joins the reviewed archive list.

**Increment 2 (principal reference and `rustev.remote/2`).**
`rustev-contract`: `PROTOCOL_V2` and `protocol_name`; the documents keep
their schema strings and carry `version: 2`. `InferRequest` gains an
optional `principal` (`WirePrincipal`: the reference as lowercase
hexadecimal of at most 128 bytes, a mapping version and a scope of 1 to 64
printable ASCII bytes), omitted when absent so version 1 bytes are
unchanged; `members_fit_version` refuses it in a version 1 request or when
not canonical. `WirePrincipal::evidence` is the domain-separated digest
(`rustev.principal-reference/1`, over mapping version, scope and
reference) with the mapping version; the exchange record gains an optional
`principal` holding exactly that. `WirePrincipal`'s `Debug` never prints
the reference. `integrations/rustev-remote-http/src/principal.rs`:
`EndpointOperator` (third party by default), `PrincipalSource` (the host
supplies the reference per attempt; `HandleAsReference` sends the opaque
principal handle), `PrincipalPosture` (omitted by default) and its binding
view `PrincipalWire`. The client negotiates exactly
`ClientConfig::protocol_major` (1 by default): a server that answers
another version, or none, fails construction, and nothing is upgraded or
downgraded per call. Construction refuses a principal posture for a
third-party endpoint, over version 1, or with a bad label. Per attempt, a
missing or oversized reference refuses the attempt before any byte
(`malformed_request`, `observed{0}`, `finished`). The reference is sent
only in the request's `principal` field; batched members share an exchange
only with the same reference. The record holds its digest, and retained
request bytes hold the digest in its place. The binding gains
`principal_wire` (posture, mapping version and scope), not written when
omitted; rotating the reference keeps the binding and changes the digest.
The server serves `ServerConfig::versions` (`[1]` by default), refuses a
principal in a version 1 request or a non-canonical one (400), includes
the principal in the idempotency content (another principal under the same
attempt id is `attempt_id_conflict`), and hands the reference to the hosted
backend as its opaque principal handle. Redirects stay unfollowed, so the
reference reaches no other endpoint. Tests:
`integrations/rustev-remote-http/tests/principal.rs` (default omission,
host-only construction and exact negotiation, the reference reaching only
the host-operated endpoint with evidence and retained bytes redacted,
rotation, refusal before send when missing or beyond 128 bytes, the
conflict, a version 1 request refused, a returned principal member failing
the answer closed and an authorization claim kept as a provider extra
without effect, a redirect never followed), unit tests in the module and in
the contract.

**Increment 3 (bounded connection reuse).**
`integrations/rustev-remote-http/src/pool.rs`: `PoolPolicy` (total
connections, per origin, streams per connection, idle connections, idle
lifetime, absolute lifetime, uses per connection, and `refuse` or
`queue{max_wait_ms}` on exhaustion; every bound positive, per-origin and
idle bounds within the total), `ConnectionPool`, a host-owned object
clients may share, and `KeyMaterial`, the isolation key: endpoint class,
scheme and authority, wire protocol and HTTP version, proxy route
(`direct`), TLS posture (`plaintext`, `webpki-roots`, or the host's
`ClientConfig::tls_label` for its own configuration), the credential
identity digest (`rustev.credential-identity/1`, never the credential),
privacy options, principal posture with mapping version and scope, the
principal reference's evidence digest per exchange, the key schema
(`rustev.pool-key/1`) and the adapter binding. The key is the tagged
digest of its canonical material; an unknown member (a custom TLS
configuration without a label, no binding, another schema) makes a key
that never reuses. An idle connection is taken only under the exact key;
when the pool is full, an idle connection of another key is closed (never
moved) to make room. Checkout sweeps closed, idle-expired and
lifetime-expired connections; release evicts on failure or an unfinished
response, closure, the use bound or the lifetime, and the oldest idle
connection gives way past the idle bound. Exhaustion refuses at once or
waits within the declared wait and the attempt budget, then is
`transport_unsent` with `observed{0}`. `HttpTransport::post_observed`
runs an exchange on a pooled connection whose stream reports to hooks
swapped per exchange (`wire::IoHooks`), so the request-byte tracker and the
phase clock are always the current exchange's; a reused connection skips
resolution, connect and handshake without carrying their ceilings. A stale
connection is never replaced mid-exchange: one that fails before request
bytes is `transport_unsent`, after them `transport_interrupted`, and the
adapter adds no reconnect or replay. Cancel exchanges always use a fresh
connection. The exchange record gains an optional `connection`: fresh or
reused, the nonsecret instance id, an age bucket, the use count, the
multiplex posture (`none`), the isolation-key digest and the eviction
reason. The binding gains `connection_pool` (policy and key schema), not
written when disabled. Tests: the pool's unit tests (every key member,
unknown material, every bound and eviction rule on a paused clock, refuse
and queue within the wait and the budget) and
`integrations/rustev-remote-http/tests/pool.rs` over loopback behind a
counting TCP front (no reuse when disabled; one connection for three
exchanges; the use bound; credential rotation and two clients with one
binding sharing; privacy options and endpoint class as key members;
principal rotation; a closed idle connection evicted at checkout; a stale
connection failing after request bytes, interrupted with no reconnect;
exhaustion refused before any byte).

**Increment 4 (`rustev.run/2`).** `rustev-contract`: `schema::RUN_V2`;
`RunRecord` gains `remote_evidence` (exactly in version 2) and
`remote_evidence_truncated` (version 2 only, omitted when false), both
absent in every other version, so version 1 and 3 bytes are unchanged
(every existing golden passes untouched). `RemoteEvidence`: attempt id,
step and instance, `ExchangeReference` (`retained` with the exchange
record's digest, schema, retention class and opaque locator, or
`unavailable` with `no_exchange`, `pending`, `not_delivered` or
`no_locator`), requested and served identity with disclosure, binding,
protocol or mapping version, endpoint class, remote code, final remote
state, charge provenance (`reported`, `attributed`, `nothing_sent`,
`unknown`), optional batch membership and optional principal digest.
Bounds: at most 256 entries, each string at most 256 bytes, at most 8
instance ids, at most 8 KiB of canonical bytes per entry (`bounded`
cuts deterministically, further for strings that escape to several
bytes); `check_version_members` refuses a member in the wrong version or
beyond a bound, and the run record's parser applies it, so a version 1 or 3
record carrying either member, or a version 2 record without the list or
beyond its bounds, is refused when read. The `rustev.run/1` and
`rustev.run/3` schemas therefore keep their bytes, and the `rustev.run/2`
schema requires the list (spec 027, 3.2.2: a schema describes its parser).
`project_v1` removes the list, changes only the schema tag
and is refused, never partial, for anything but a valid version 2 record.
`rustev-core`: `EvidenceSink::run_schema` (the one schema a sink selects,
`rustev.run/1` by default) and `DecisionBackend::take_remote_evidence`
(`None` by default). `rustev-runtime`: a plan 2 decision's record is
`rustev.run/2` exactly when the sink selected it; each attempt's entry is
taken once from the backend that ran it, in attempt order, with the step,
instance and final remote state the record holds; entries leave from the
end, and the record says so, until the record fits `RECORD_V1`. A rejected
version 2 record is an evidence delivery failure like any other; nothing
retries it as version 1. `rustev-remote-http`: `ClientConfig::run_evidence`
(off by default; the binding gains `run_record_schema`, not written when
off) keeps a pending entry per attempt, completes it from the delivered
exchange record before any member is answered, and records `no_exchange`
for an attempt refused before any exchange or stopped before any of its
request bytes left (a member withdrawn from a coalescing exchange is in no
exchange record, so its pending entry would otherwise stay pending);
`ExchangeSink::deliver_located`
lets the host name a locator (the default names none), and
`Exchanges::deliver` returns the record digest and the delivery result.
`rustev-eval`: a version 2 record enters a bundle as its version 1
projection, so `rustev.replay/1` bundles are unchanged and replay
reproduces from retained raw outputs without reading any exchange record;
`remote_evidence::availability` checks each reference against a
host-supplied synchronous `ExchangeStore` (`available` only for the bounded
record whose digest and canonical bytes match; otherwise `missing`,
`expired`, `forbidden`, `corrupt`, `inaccessible`, or the record's own
`not_referenced` reason), reported apart from judgment parity. Tests:
`crates/rustev-contract/tests/run_v2.rs` (version 1 bytes, round trips at
every bound, members per version, exact projection),
`crates/rustev-runtime/tests/run_v2.rs` (a version 1 sink gets version 1
and nothing is taken; one entry per remote attempt in order with the
projection equal to the version 1 record; a rejected version 2 record
delivered once, never as version 1, with the same judgment),
`crates/rustev-eval/tests/remote_evidence.rs` (offline replay of a version
2 record as its projection, byte-identical bundle, no backend call;
available, missing, corrupt, swapped, expired, forbidden, inaccessible and
not referenced) and `integrations/rustev-remote-http/tests/run_evidence.rs`
(support routing over loopback: entries checked against the host's store,
no body, credential or principal reference embedded, `no_locator`,
`not_delivered`, the principal digest, `no_exchange` for a refused
attempt) and `integrations/rustev-remote-http/tests/batching.rs` (a member
withdrawn before sending has `no_exchange`). The new eval source file joins the reviewed archive list.

**Increment 5 (runtime-planned remote batches).** The batch-capable seam
is spec 017's (`DecisionBackend::batch_capability` and `infer_batch`);
`DecisionBackend::infer` is unchanged. `RemoteClient` offers it only when
`ClientConfig::batch_dispatch` (2 to 32 members, off by default; the
binding gains `batch_dispatch_max_members`, not written when off) and the
negotiated terms declare shared-state batching; the effective bound is the
lesser of the two, so terms never raise it. Equal compatibility means the
same binding and the same shared state, which is what one request may
carry. `infer_batch` refuses a batch beyond the bound, or whose members
differ in principal or decision, before any byte; checks every member as
an independent attempt would (a member already cancelled or out of budget
is removed and reported unsent); sends the rest in one exchange, in the
runtime's order (never sorted, added to, merged or retried), bounded by
the earliest member budget; and answers each member from its own answer,
so a missing, duplicate or mismatched answer fails only that member and an
answer naming an unasked attempt, which no member owns, fails every member
(spec 009's envelope rule). The batch report's charge is the sum of the
members' (the runtime allocates it, spec 017), its cancellation is
`unconfirmed` if any member's is, and its remote state is possibly
continuing if any member's is. The exchange record's attribution is the
new `runtime_planned`, and remote evidence names the runtime's batch id and
member index. Tests: `integrations/rustev-remote-http/tests/runtime_batch.rs`
compares the support-routing decision batched (one exchange of three
members in member-index order, one batch id) with the same decision
dispatched independently (three exchanges): values and judgment, a member
failure, a deadline and a cancellation after send (every member possibly
continuing with charge unknown, in both), and the cost; a retrying step
runs alone, a plan without a batch policy never batches, and an adapter
without batch dispatch offers no seam; a missing member answer fails that
member only.

**Increment 6 (Jev, drift, verification).** A negotiated endpoint that
answers another protocol version is now `identity_mismatch` (3.7), not
`malformed_response`. `integrations/rustev-jev/tests/refinements.rs`: the
Jev adapter offers no general batch seam (with or without its own
batching), its default batch dispatch refuses without contacting the
gateway, its own adapter-scoped batch is still one request attributed as
shared state, its exchange records carry no spec 020 member, and it keeps
no remote evidence. No spec 020 refinement is enabled on the Jev adapter:
its transport keeps phases and pooling disabled, its binding
(`rustev.jev-binding/1`) and foreign-wire mapping are unchanged, so no
mapping version changes and no earlier qualification is reused for a
changed mapping.

Acceptance, item by item:

- Canonical fixtures: `crates/rustev-contract/tests/run_v2.rs`, the
  exchange-record and binding byte pins, and every existing golden
  unchanged.
- Offline replay: `crates/rustev-eval/tests/remote_evidence.rs`.
- Differential batches: `integrations/rustev-remote-http/tests/runtime_batch.rs`
  (values, failures, cancellation, deadlines, charge, remote state,
  evidence). Fallback is covered by eligibility: a step that can retry or
  fall back never batches (spec 017, 3.3); the test uses a retrying step,
  which takes the same rule.
- Phases: `integrations/rustev-remote-http/tests/phases.rs`.
- Pools: the pool's unit tests and `integrations/rustev-remote-http/tests/pool.rs`.
- Principal: `integrations/rustev-remote-http/tests/principal.rs` and
  `run_evidence.rs`.
- Jev: `integrations/rustev-jev/tests/refinements.rs`.

Not verifiable as written, for the owner to rule on:

1. Multiplex cancellation (pool acceptance). The HTTP binding speaks
   HTTP/1.1 only; a pool policy must declare one stream per connection,
   so nothing is multiplexed and there is no multiplexed cancellation to
   test. Covering it needs an HTTP/2 binding, a new dependency that spec
   021's audit would have to admit.
2. "New qualification evidence for every enabled refinement" (Jev). No
   refinement is enabled on the Jev adapter, so there is none to produce;
   enabling one would need a new mapping version and a newly authorized
   qualification, which this spec does not authorize (3.7).

**Ruling, 2026-10-21 (R-47).** The owner ruled both items not applicable
as built: the binding is HTTP/1.1 only, so there is no multiplexed
cancellation, and no refinement is enabled on the Jev adapter, so there is
no new qualification evidence. `implementation` is `complete`, and 020
joins `make verify`. An HTTP/2 binding or an enabled refinement would make
the item applicable again, under its own authorization.

Engineering choices made by the implementing agent, not by the owner:

- 3.1 asks for explicit `disabled` values in the binding. They are explicit
  in the configuration types, which have no implicit state, but are not
  written into the binding document, so the artifact and `PlanId` of every
  existing adapter stay byte-identical as section 2 requires.
- The request-write phase ends at the first completed flush after the
  connection took the request body; the response-body phase includes the
  response head, from the first response byte to the complete body.
- The cancellation acknowledgement ceiling bounds the whole cancel
  exchange, whose own transport phases also apply.
- `rustev.remote/2` keeps the HTTP paths `/rustev/remote/1/...` and the
  document schema strings; the `version` member distinguishes it. A
  version 1 server refuses a version 2 describe with 400, which
  construction reports as a negotiation failure (`malformed_request`); no
  plan can be compiled against the adapter.
- The HTTP binding speaks HTTP/1.1 only, so a pool policy must declare
  one stream per connection and nothing is multiplexed; the multiplex
  cancellation item of the acceptance therefore has nothing to cover until
  a binding that multiplexes exists.
- The authority in the isolation key is the configured scheme, host and
  port, not the resolved address: two names for one address never share a
  connection.
- A replay bundle holds a version 2 record's version 1 projection, so
  `rustev.replay/1` admits exactly what it admitted; the remote evidence
  stays with the delivered record. Replay itself never consults an
  exchange store; the acceptance item's "without a transport or store
  call" is read as that, and availability is a separate, explicit check
  through the host's store.
- A plan 3 decision's record stays `rustev.run/3` without remote evidence
  even for a sink that selected version 2: this spec defines the list for
  version 2 only.
- Spec 017's batch report carries one cancellation answer and one remote
  state for the batch. A member whose work may continue therefore makes
  every member of its batch possibly continuing with charge unknown: per
  member liability is never understated, but can be overstated until the
  host reconciles it. Finer per-member remote state needs a change to the
  spec 017 seam, which this spec does not make.
- A host-operated `rustev.remote/1` server hands a version 2 request's
  reference to the hosted backend as `CallContext::principal_handle`; spec
  009's engineering choice of an empty handle still holds for every
  version 1 request.

## Verification

Declared for `spec-spine verify 020`; 020 is in `make verify` (R-47).

```verify:cli
# 3.2, 3.4 to 3.6: documents, bounds, projection and byte pins.
cargo test -p rustev-contract --locked
# 3.2: the sink seam and the backend's remote evidence.
cargo test -p rustev-core --locked
# 3.2, 3.3: run/2 delivery and spec 017's batch seam.
cargo test -p rustev-runtime --locked
# 3.2: offline replay and remote evidence availability.
cargo test -p rustev-eval --locked
# 3.1, 3.3 to 3.7: phases, pools, principal, run evidence, batches.
cargo test -p rustev-remote-http --locked
# 3.3, 3.7: the Jev adapter's batch is not the runtime's; nothing enabled.
cargo test -p rustev-jev --locked
# Boundaries: no HTTP stack or async runtime below integrations/.
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-contract -p rustev-core -p rustev-runtime -p rustev-eval -p rustev-remote-http -p rustev-jev --all-targets --locked -- -D warnings
```

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

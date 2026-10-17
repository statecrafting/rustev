---
id: "042-decision-service-and-remote-seams"
title: "Decision service sidecar and remote context and sink protocols (amends 010)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved spec 010 (R-16), for #76 child A2 (#57). Makes
  the `rustev-serve` service API concrete as `rustev.serve/1`: plan
  registration by content identity from plain or signed plan bundles,
  decisions, cancellation, lookup, server-sent event streaming, offline
  replay, evaluation and explanation, health, readiness and OpenMetrics,
  served over HTTP/1.1 and HTTP/2 with JSON and over gRPC from one handler
  layer, on TCP and Unix domain socket listeners. Defines two new
  transport-neutral callback protocols with an HTTP binding,
  `rustev.remote-context/1` (describe, fetch, acquire, cancel and
  invalidation by push or poll) and `rustev.remote-sink/1` (describe and
  deliver with receipts), behind bounded queues with deadlines,
  cancellation and typed refusals when full. Adds no plan construct: every
  `PlanId`, run record and judgment byte is unchanged. Approved (A-39)
  under the 2026-10-16 delivery work order; implementation is pending, and
  `rustev-serve` implementation still waits for the owner's approval of its
  spec 010 section 3.4 binding record (R-34).
amends:
  - "010-integrations"
extends:
  # Pure document types of the service API, plan bundles and the two
  # callback protocols, beside the rustev.remote/1 documents.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: additive }
  # A read-only, bounded progress observer for streamed step events.
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  # HTTP binding (client and host-side server) of the two callback protocols.
  - { spec: "009-remote-adapter-protocol", unit: { kind: directory, path: "integrations/rustev-remote-http/" }, nature: additive }
  # gRPC and Sigstore dependency families (integration-only).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "010-integrations"
  - "017-general-runtime-optimization"
  - "020-remote-protocol-refinements"
  - "027-node-and-wasm-bindings"
  - "028-opentelemetry-evidence-sink"
  - "031-public-api-and-release-family"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No service or callback output is authorization: a response, receipt, registration, verified signature or HTTP or gRPC success is only a judgment, unresolved outcome, refusal, integrity fact or evidence disposition, and the principal handle passes through as opaque bytes."
    anchor: "2-1-binding-principles-of-76"
  - id: "I-2"
    kind: invariant
    text: "For every conformance case, the HTTP/JSON and gRPC transports return byte-identical documents, and their judgments and delivered run records equal the CLI's and the bindings' bytes for the same case."
    anchor: "3-17-shared-conformance"
  - id: "I-3"
    kind: invariant
    text: "Every callback seam call goes through a bounded queue that refuses with a typed failure when full, carries its remaining deadline and cancellation, and discards and counts a late response; nothing buffers without bound."
    anchor: "3-12-bounded-callback-seams"
  - id: "I-4"
    kind: invariant
    text: "Shutdown loses no evidence silently: every admitted decision ends delivered, failed with the caller told, or dropped or unflushed and counted with its decision id in the shutdown report."
    anchor: "3-14-graceful-shutdown"
  - id: "I-5"
    kind: invariant
    text: "This spec adds no plan construct: every plan and definition golden keeps its bytes and PlanId, and run record and judgment bytes are unchanged."
    anchor: "3-19-compatibility"
  - id: "R-1"
    kind: requirement
    text: "rustev-serve is not implemented until the owner approves its spec 010 section 3.4 binding record, which covers at least the rows of section 2.2."
    anchor: "2-2-relation-to-spec-010-and-the-binding-record-gate"
  - id: "R-2"
    kind: requirement
    text: "A plan bundle is registered only when its content recompiles to the PlanId in its path and compile report, and its signature satisfies the configured mode; a wrong key, tampered content or an unsigned bundle under a mode that forbids it is refused with its code."
    anchor: "3-3-plan-registration-and-signed-bundles"
  - id: "R-3"
    kind: requirement
    text: "Every refusal before a decision runs is a typed rustev.serve-refusal/1 with a code from section 3.5; Retry-After is sent only when the runtime states a retry time."
    anchor: "3-5-admission-refusals-and-limits"
  - id: "R-4"
    kind: requirement
    text: "Request header and body byte limits are enforced before parsing; the document's bounded parser applies after."
    anchor: "3-5-admission-refusals-and-limits"
---

# 042: Decision service sidecar and remote context and sink protocols (amends 010)

Approved by the owner (A-39, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. A-33 to A-38 are reserved for specs 036 to 041, which are
proposed in parallel and merge first by number order. This spec implements
#76 child A2 (#57). Spec 010's approved text is not edited; this spec
records what it makes concrete and what it adds, as 014 does for 004.
Spec 010's binding-record gate stands unchanged:
`rustev-serve` may be implemented only after the owner separately approves
its section 3.4 binding record (R-34), and nothing here approves one.
Approval authorizes no release, publication, image push, deployment, live
provider call or spend, production traffic or real-user data.

## 1. Purpose

Some hosts cannot load a native module, and some want several processes to
share one warm runtime (plan registry, caches, connection pools). Spec 010
reserved `integrations/rustev-serve/` as a bounded service around
host-registered plans and a host-constructed runtime, and fixed its
boundary. This spec makes the service API concrete and gives the service two
ways to call back into the host that today exist only in process: a remote
context source and a remote evidence sink.

## 2. Territory

When delivered:

- `integrations/rustev-serve/` (owner spec 010, amending): the `rustev-serve`
  library and binary, the handler layer, the HTTP and gRPC transports, the
  generated `.proto` under `proto/`, configuration, registration, shutdown,
  metrics, and the image definition under `image/`.
- `crates/rustev-contract/` (spec 002, additive): pure serde types, with
  `deny_unknown_fields`, exact schema strings and declared parse bounds, for
  every document this spec names; the operation table of `rustev.serve/1`
  (3.9); and their JSON Schemas through spec 027's emitter. No existing
  document, bound or canonical byte changes.
- `crates/rustev-runtime/` (spec 003, additive): an optional progress
  observer (3.7.3). With none attached, behavior and bytes are unchanged.
- `integrations/rustev-remote-http/` (spec 009, additive): the HTTP binding
  of `rustev.remote-context/1` and `rustev.remote-sink/1`: a client that
  implements `ContextSource` and `EvidenceSink`, and a host-side server that
  hosts any `ContextSource` or `EvidenceSink`, over the HTTP stack the crate
  already uses. `rustev.remote/1` and `/2` are unchanged.
- `tools/rustev-boundaries/`, `Cargo.toml`, `Cargo.lock`, `Makefile`
  (spec 001, additive): the dependency families of 3.18, pinned workspace
  entries, and `042` in `make verify` at completion.
- `tools/rustev-conformance/` (planned by spec 027): the service transport
  cases of 3.17, added when that directory exists, under spec 027 3.12.1,
  which already names the A2 transports.

`integrations/rustev-serve/` and `tools/rustev-conformance/` are planned,
not yet written, so this spec declares no `extends` edge on them; the
`amends` edge on spec 010 carries the change, and the change that first
writes either directory adds the edge.

### 2.1 Binding principles of #76

- **Proposals, never grants.** The service returns judgments, unresolved
  outcomes, refusals and evidence dispositions (spec 010 3.7.7). A
  registration, a verified signature, a receipt or a 2xx status is an
  integrity or delivery fact, never authorization. No document here defines
  a `Permitted` or grant type.
- **Derivation classes and lineage.** Judgments and run records cross as
  the canonical bytes the runtime produced, with their derivation classes
  and lineage; the service never re-renders them. Context fetched remotely
  keeps each entry's provenance class, source and `as_of_ms`.
- **Unresolved, never a default.** A failed fetch is `context_unavailable`
  and no decision runs; a failed acquisition is the unresolved outcome spec
  036 defines; a missing field stays missing. No kind converts.
- **Pure core.** Contract gains only pure types; core is untouched; the
  runtime gains an optional synchronous observer, no I/O in core and no
  async runtime in contract or core.
- **New dependency classes only under `integrations/`.** 3.18.
- **No dynamic plugin loading (D-06, R-38).** Plans are data registered by
  content identity; backends, sources and sinks are those compiled into the
  binary or reached through declared protocols; configuration names them.
  Nothing is loaded as code.
- **No identity, authority or knowledge store.** The service authenticates
  only its caller at the transport, passes the principal handle through
  opaquely, keeps decisions only for a bounded idempotency window, and
  stores no knowledge.

### 2.2 Relation to spec 010 and the binding-record gate

Spec 010 stays in force; this spec makes 3.7 concrete and adds no exception
to 3.1 to 3.3. In particular: a decision request cannot upload a plan,
operator, policy, fallback, code, credential, endpoint or limit (010 3.7.1);
plan registration is a host act on an administrative route (3.3), not part
of a decision request. The `rustev.run/2` and connection-reuse refinements
spec 010 3.7.6 defers to spec 020 are used by this spec only as spec 020
defines them.

The `rustev-serve` binding record under spec 010 3.4 must, in addition to
that section's eight items, fix: the HTTP/1.1 and HTTP/2 server crates; the
gRPC crates and protobuf generator; the TLS crate, mTLS posture and accepted
cipher suites; the Unix socket permissions model; the Ed25519 verification
crate; the Sigstore verification crate, the Sigstore bundle media type and
the trusted-root handling of 3.3.5; the base image by digest; and the
platforms. The documents in `rustev-contract` and the HTTP binding in
`rustev-remote-http` are not spec 010 crates, add no dependency family and
may be implemented before that record; `rustev-serve` may not.

## 3. Behavior

### 3.1 Service surfaces and listeners

1. **Routes.** `rustev.serve/1` has these operations, each with one HTTP
   route and one gRPC method (3.9):

   | Route | Group | Operation |
   |---|---|---|
   | `GET /v1/service` | decide | Describe the service (`rustev.serve-service/1`) |
   | `PUT /v1/plans/{planId}` | admin | Register a plan bundle (3.3) |
   | `GET /v1/plans/{planId}` | decide | Show the registration record |
   | `POST /v1/plans/{planId}/decisions` | decide | Run one decision (3.4); `?stream=1` streams (3.7) |
   | `GET /v1/decisions/{decisionId}` | decide | Look up a retained decision (3.6) |
   | `POST /v1/decisions/{decisionId}/cancel` | decide | Cancel (3.6) |
   | `POST /v1/replay` | decide | Offline replay (3.8) |
   | `POST /v1/evaluate` | decide | Evaluation (3.8) |
   | `POST /v1/explain` | decide | Explanation (3.8) |
   | `POST /v1/context/invalidations` | admin | Pushed invalidations (3.10.6) |
   | `GET /healthz`, `GET /readyz` | ops | Liveness, readiness (3.13) |
   | `GET /metrics` | ops | OpenMetrics (3.13) |

2. **Listeners.** Configuration declares one or more listeners, each TCP or
   Unix domain socket, each serving a declared non-empty set of route
   groups. A route outside a listener's groups answers `endpoint_unavailable`
   (404). Every TCP listener, loopback included, requires TLS with client
   certificate authentication (3.2.1), because a loopback address can be
   shared by co-located processes or containers (for example, containers in
   one network namespace); a Unix
   socket is created with mode `0600`, or `0660` when configuration states
   it (group access for a sidecar sharing a group); any other mode is
   refused at startup, and an existing path is refused, not replaced.
3. **HTTP versions.** HTTP/1.1 and HTTP/2 (ALPN over TLS, prior knowledge
   in cleartext on Unix sockets only) serve the same routes. Bodies
   are `application/json` holding the documents named here; another media
   type is `unsupported_media_type` (415).
4. **Endpoints that join later.** `POST /v1/explain` and the `field` events
   of 3.7 are part of `rustev.serve/1` but are served only once the spec
   implementing #76 D2 (#71), respectively B5 (#63), is implemented; until
   then the route answers `endpoint_unavailable` and `rustev.serve-service/1`
   lists it as absent. The `acquire` operation of 3.10 likewise waits for
   spec 036 (B1, #59).

### 3.2 Caller authentication and the principal handle

1. **Transport only.** The service authenticates its caller (the host
   process) at the transport: mTLS against configured client CA roots and an
   optional exact subject allowlist, or the Unix socket's filesystem
   permissions. A static bearer token is accepted only when configuration
   sets `auth: development_bearer`, and only on Unix socket listeners, in
   addition to the socket's permissions; configuring it on a TCP listener,
   loopback included, is refused at startup. A
   failed authentication is `unauthenticated` (401) before any body byte is
   read. Rustev holds no users, sessions or roles.
2. **Principal handle.** Every decide-group operation that runs or reads a
   decision requires header `rustev-principal`: base64url without padding,
   decoding to 1 to 1,024 bytes. It is the host's opaque authorized scope
   (spec 003 `principal_handle`), never interpreted, never derived from
   content or a judgment (010 3.7.2). Absent or undecodable is
   `principal_invalid` (400).
3. **Trace context.** Optional header `traceparent`, validated exactly as
   spec 027 3.8 (`invalid_trace_context`, 400). It is passed to the sink and
   context callbacks of that decision and to spec 028's sink when
   configured; it enters no identity, run record or judgment.

### 3.3 Plan registration and signed bundles

1. **Bundle.** `rustev.plan-bundle/1` holds: the definition document; the
   execution policy document or `none`; calibration documents; a tenant
   profile or `none` (accepted only once #76 B4 (#62) is implemented,
   otherwise `profile_unsupported`); `bindings`, a map from each backend id
   the definition uses to a backend binding name declared in configuration
   (3.15); and `compile_report`, the compile output `rustev-surface` renders
   for these inputs, including `plan_id`. Bindings name configured backends
   only; a bundle cannot carry an endpoint, credential or code.
2. **Registration.** `PUT /v1/plans/{planId}` takes
   `rustev.serve-register/1` (`bundle`, and `signature` or `none`). The
   service compiles the bundle with the existing compiler against the
   descriptors of the named bindings, obtained at startup (spec 009 3.2.1).
   It refuses `compile_refused` (422, carrying the compile refusal bytes),
   `plan_id_mismatch` (422) when the computed `PlanId`, the report's
   `plan_id` and the path differ, `binding_unknown` (422), and
   `registry_full` (409) beyond `max_plans` (1 to 4,096). Nothing is loaded
   as code.
3. **Registration record.** `rustev.plan-registration/1`: `plan_id`,
   execution policy id or `none`, profile id or `none`, bundle digest
   (`tagged_digest` of the bundle's canonical bytes), the signer (scheme,
   key id or certificate identity and issuer) or `unsigned`, the signing
   mode in force, the service build and configuration digest, and
   `registered_at_ms` (wall time, an observation). The `RegistrationId` is
   the tagged digest of the record without `registered_at_ms`.
4. **Idempotent by content.** A repeated registration with the same
   `RegistrationId` returns the existing record (200). The same `PlanId`
   with a different `RegistrationId` (another signer, mode or bundle bytes)
   is `registration_conflict` (409). Registrations are immutable and live
   until the process ends; configuration may preload bundles from files at
   startup through the same path.
5. **Signatures.** `rustev.plan-bundle-signature/1` signs the byte string
   `rustev.plan-bundle-signature/1\n` followed by the bundle digest string.
   Schemes:
   - `ed25519`: `key_id` and signature, verified against configured public
     keys by key id;
   - `sigstore`: a Sigstore bundle (media type pinned by the binding
     record), verified offline against a host-supplied trusted-root file,
     an exact certificate identity and an exact OIDC issuer from
     configuration, with its transparency-log inclusion proof required.
     The service performs no network fetch for verification.
6. **Modes.** Configuration selects exactly one: `require_signed`;
   `allow_unsigned` (development; reported in `rustev.serve-service/1` and
   `/readyz`); or `deny_unsigned_for_profiles: [profile ids]`, which admits
   unsigned bundles except those carrying a listed profile. Refusals:
   `bundle_tampered` (the signed digest is not the bundle's digest),
   `signature_invalid` (verification fails), `signature_untrusted` (unknown
   key id, or identity or issuer differs), `unsigned_refused`; all 422.
7. **Signer in evidence.** Every decision response (3.4) and every
   `rustev.remote-sink/1` delivery (3.11) carries the `RegistrationId`,
   signer and key id or identity of the plan it ran. Run record bytes are
   unchanged: the join is the decision id and `PlanId`.

### 3.4 Running decisions

1. **Request.** `rustev.serve-decide/1`: `decision_id` (spec 003 3.9.2
   bounds), `evaluation_time`, and exactly one of `snapshot` (a snapshot
   document in a version the runtime accepts, `rustev.snapshot/1` today and
   the #76 track's `rustev.snapshot/2` once its spec is implemented,
   parsed by its bounded parser) or
   `context: {source}`, naming a configured remote context source (3.10).
   Header `rustev-deadline-ms` (positive integer) can only tighten the plan's
   deadline, as `DecisionRequest.deadline_ms`; over gRPC the call's own
   deadline tightens it further.
2. **Context.** With `context`, the service calls the source's `fetch` once
   before admission with the plan's declared input fields, as spec 027 3.7
   does; a failure is `context_unavailable` (502) and no decision runs.
   Once spec 036 is implemented, a plan's declared acquisitions go through
   the same source's `acquire` during the decision.
3. **Response.** For a decision that ran, `rustev.serve-decision/1`:
   `status` (`judged`, `cancelled` or `evidence_not_delivered`),
   `decision_id`, `plan_id`, `registration_id`, signer, execution policy id
   or `none`, the service build, protocol and configuration digest, the
   `rustev.judgment/1` document exactly as the runtime produced it (absent
   when cancelled), the run record digest, the evidence disposition
   (receipt, ticket, `dropped` with count, or `not_delivered` with the
   error and whether the sink may have received it), and `replayed` (3.6).
   Backend and served identities stay as the run record has them, including
   `unknown` (010 3.7.4).
4. **Status codes.** `judged` and `cancelled` are HTTP 200 and gRPC `OK`.
   `evidence_not_delivered` is HTTP 500, or gRPC `INTERNAL` with the
   document in binary metadata `rustev-decision-bin`, carrying the full
   decision document and the run record bytes: work and spending happened,
   and no judgment is returned as a success (spec 003 3.9.4). The status
   code alone is not the outcome: 200 covers both `judged` and
   `cancelled`, and this 500 means a decision ran, unlike a refusal's 5xx
   (3.5), which means none did. A client reads `status`, or
   `decision_ran` in a refusal.
5. **Disconnect.** A client disconnect or a dropped gRPC call does not
   cancel; only the cancel operation or the deadline does. The decision
   finishes, its evidence follows the sink policy, and its response is
   retained for lookup (3.6). The service never reports a disconnect as a
   stop (010 3.7.5).

### 3.5 Admission, refusals and limits

1. **Refusal document.** When no decision ran, the body is
   `rustev.serve-refusal/1`: `code`, `detail` (at most 256 bytes),
   `decision_ran: false`, and `retry_after_ms` only when the runtime states
   one. Codes and statuses:

   | Code | HTTP | gRPC | When |
   |---|---|---|---|
   | `overloaded` | 429 | `RESOURCE_EXHAUSTED` | Runtime admission queue full (spec 003 3.4.1) |
   | `idempotency_window_full` | 429 | `RESOURCE_EXHAUSTED` | 3.6.2 |
   | `queue_deadline` | 503 | `UNAVAILABLE` | Deadline passed while queued |
   | `shutting_down` | 503 | `UNAVAILABLE` | 3.14 |
   | `not_ready` | 503 | `UNAVAILABLE` | 3.13 |
   | `context_unavailable` | 502 | `UNAVAILABLE` | 3.4.2, with the remote code in `detail` |
   | `cancelled_before_admission` | 409 | `CANCELLED` | Cancelled while queued |
   | `decision_in_progress`, `decision_id_conflict`, `registration_conflict`, `registry_full` | 409 | `ALREADY_EXISTS` or `FAILED_PRECONDITION` | 3.6, 3.3 |
   | `invalid_request`, `invalid_document`, `principal_invalid`, `invalid_trace_context`, `invalid_deadline` | 400 | `INVALID_ARGUMENT` | Bounds or parse |
   | `unauthenticated` | 401 | `UNAUTHENTICATED` | 3.2.1 |
   | `plan_not_registered`, `decision_not_found`, `endpoint_unavailable` | 404 | `NOT_FOUND` | |
   | `body_too_large`, `headers_too_large` | 413, 431 | `RESOURCE_EXHAUSTED` | 3.5.3 |
   | `unsupported_media_type` | 415 | `INVALID_ARGUMENT` | 3.1.3 |
   | `compile_refused`, `plan_id_mismatch`, `binding_unknown`, `profile_unsupported`, `bundle_tampered`, `signature_invalid`, `signature_untrusted`, `unsigned_refused` | 422 | `FAILED_PRECONDITION` | 3.3 |

   The binding record fixes the gRPC code of each refusal in the one row that
   lists two.
2. **Retry-After.** Sent only with `retry_after_ms`. The runtime states no
   retry time today, so no refusal carries one; a later spec that gives the
   runtime a basis adds it. A guessed value is never sent.
3. **Limits before parsing.** Per listener: maximum header bytes (4 KiB to
   64 KiB, so one maximum-length `rustev-principal` header and the other
   request headers always fit) and, per route, maximum body bytes (registration up to 16 MiB,
   others up to 4 MiB, each configurable downward), enforced on the
   declared length and while reading, before any parse; then the document's
   bounded parser. Connection count, HTTP/2 concurrent streams and
   per-connection request rate are bounded by configuration with stated
   ranges; exceeding them refuses the connection or stream.
4. **Runtime bounds.** `max_in_flight`, `max_queued` and the per-backend
   concurrency of spec 003 3.4 are configuration. The service adds no
   queue of its own in front of runtime admission.

### 3.6 Idempotency, lookup and cancellation

1. **Window.** The service retains, per host scope (principal handle
   digest, decision id), the request digest and the response document of
   every decision that ran, for `idempotency_window_ms` (1,000 to
   86,400,000), bounded by `max_retained` (1 to 1,000,000) and
   `max_retained_bytes` (1 MiB to 16 GiB).
2. **Resubmission.** Inside the window, the same scope and decision id with
   the same request digest returns the retained response with
   `replayed: true` and starts nothing; with a different digest it is
   `decision_id_conflict`; while running it is `decision_in_progress`. When
   the window cannot accept a new entry without evicting an unexpired one,
   the new decision is refused `idempotency_window_full`; an unexpired entry
   is never evicted. After expiry, decision-id uniqueness is the host's
   (spec 003 3.9.6). Retention is memory; a restart forgets it, and
   `rustev.serve-service/1` states so.
3. **Lookup.** `GET /v1/decisions/{decisionId}` with the same principal
   returns the retained response, or the running state, or
   `decision_not_found`. A backpressure ticket's outcome (receipt or
   failure) appears here once the delivery worker settles it.
4. **Cancel.** `POST /v1/decisions/{decisionId}/cancel` raises the
   decision's `CancelSignal` and returns `rustev.serve-cancel/1`: `queued`
   (it will end `cancelled_before_admission`), `requested` (admitted;
   attempts answer per spec 003 3.7), `already_finished`, or
   `decision_not_found`. With `?wait_ms=N` (0 to 60,000) it waits up to N
   ms for the decision to end and then includes each attempt's `CancelAck`
   and `RemoteEnd` from the run record (`not_requested`, `stopped`,
   `unconfirmed`, `no_answer`; `finished`, `stopped`,
   `possibly_continuing`). It never reports `stopped` the runtime did not
   record.

### 3.7 Streaming

1. `?stream=1` on HTTP returns `text/event-stream`; over gRPC the method is
   server-streaming. Events are `rustev.serve-event/1` documents with a
   sequence number and a kind:
   - `admitted`;
   - `step`: step id, instance ids, outcome (`supplied`, `unresolved` with
     its reason, `skipped`) and derivation class, with no value bytes;
   - `field`: a streamed generated field as the spec implementing B5
     defines it (3.1.4);
   - `final`: exactly the `rustev.serve-decision/1` the non-streaming call
     returns, or the refusal.
2. Each stream has a bounded event queue (16 to 65,536). When full, `step`
   and `field` events are dropped and counted, never blocking the decision;
   `final` is never dropped and carries `events_dropped`.
3. **Progress observer.** `rustev-runtime` gains an optional observer
   registered per decision: a synchronous, non-blocking callback invoked
   after a step's result is recorded, receiving only the data of 3.7.1
   `step`. It cannot change, delay or fail the decision; a panic inside it
   is contained and counted. Without an observer, the runtime's behavior,
   record and judgment bytes are unchanged, and with one they are too.

### 3.8 Replay, evaluation and explanation

1. **Replay.** `POST /v1/replay` takes `rustev.serve-replay/1`: a replay
   bundle and its inputs as documents or as references. Output is the bytes
   `rustev-surface` renders for `rustev replay` (spec 027). Replay makes zero
   remote calls (spec 004 3.3.5).
2. **Evaluation.** `POST /v1/evaluate` takes `rustev.serve-evaluate/1`: the
   inputs of `rustev eval` and, when spec 029 is implemented, its repeat
   inputs, with each file path replaced by an opaque reference. References
   are resolved by spec 004's `Resolver` (the host byte provider), which the
   service implements over configured read-only directory roots, one per
   reference prefix; a reference is never a URL, never escapes its root,
   and an unresolvable one is reported as spec 004 reports `inaccessible`.
   Output is the bytes `rustev-surface` renders.
3. **Explanation.** `POST /v1/explain` takes the evidence and query of
   the spec implementing D2 (proposed as spec 040), with evidence as
   documents or resolver references, and returns that spec's explanation
   record canonical bytes unchanged and its Markdown rendering, computed
   by the same library function through `rustev-surface` (3.1.4). The
   caller's redaction policy is the one configuration names for that
   listener; a reference resolves only within the caller's principal
   scope, and evidence of another scope is `decision_not_found`, never
   explained (spec 010 3.7.2).
4. These operations run on a bounded blocking pool (1 to 64 threads, queue
   1 to 4,096; full is `overloaded`) and honor `rustev-deadline-ms`.

### 3.9 The gRPC transport

1. **One schema source.** `rustev-contract` holds the operation table of
   `rustev.serve/1`: per operation, its route, group, request and response
   schema names, and streaming. The HTTP router and
   `integrations/rustev-serve/proto/rustev/serve/v1/serve.proto` are both
   generated from it by a deterministic generator; the committed `.proto` is
   drift-checked like spec 027's type declarations.
2. **Messages carry canonical bytes.** Each request and response message has
   a `bytes document` field holding the same canonical JSON document the
   HTTP body holds; headers become metadata of the same names; a refusal
   uses the gRPC status of 3.5.1 with the refusal document in binary
   metadata `rustev-refusal-bin`, and `evidence_not_delivered` uses
   `INTERNAL` with the decision document in `rustev-decision-bin` (3.4.4).
   No document is re-encoded as protobuf
   fields, so evidence bytes and digests cannot differ by transport.
3. **One handler layer.** Both transports decode into the same typed
   operation and call the same handler; transport code maps only framing,
   metadata and status.

### 3.10 The remote context protocol

`rustev.remote-context/1` lets the service (or any Rust host, through
`rustev-remote-http`) use a context source in another process. It is
transport-neutral; its HTTP binding is `POST /rustev/remote-context/1/<op>`
with the documents below as JSON bodies, TLS except on loopback or Unix
sockets, and host-supplied client credentials.

1. **Describe** (`rustev.remote-context-describe/1`), at construction, never
   per decision. Request: supported versions. Response: the chosen version
   and, per source id: fields (name, declared type, provenance class of
   spec 002, freshness as `observation` or `revision` time with the
   staleness bound the source guarantees or `unknown`, and an evidence
   grade once #76 B2 (#60) is implemented, otherwise absent); the
   source's `rustev.source/1` descriptor of spec 036 with its `SourceId`,
   verbatim (absent until spec 036 is implemented); and terms: cancellation (`none`, `best_effort`, `confirmed`), idempotency
   window, maximum request and response bytes, invalidation (`none`,
   `push`, or `poll{interval_ms, max_lag_ms}`), and the endpoint class,
   which must be `host_operated`. A third-party class is refused at
   construction. An unknown version is a capability failure; the source is
   then unavailable and the service not ready.
2. **Fetch** (`rustev.remote-context-fetch/1`). Request: version, source id,
   describe digest, attempt id `<decision id>/context/<n>` (components
   escaped as spec 003 3.9.2), budget in milliseconds less a declared
   transit margin, trace id, `traceparent` if any, the principal handle
   (the endpoint is host-operated and authorizes the read; spec 009 3.1.3
   and spec 020 3.6 apply), and the field names. Response: a
   snapshot document (a version the runtime accepts, as 3.4.1) parsed by its bounded parser, or
   `failure{code, detail}`. Fields the caller may not read are absent, and
   absent stays missing. A field not requested, a duplicate entry not
   representable as spec 002 conflict, or an unknown field is
   `malformed_response`.
3. **Acquire** (`rustev.remote-context-acquire/1`). The client is an
   ordinary installed `AcquisitionSource` of spec 036 (3.12) whose
   descriptor is the one describe returned. Request: version, `SourceId`,
   operation, the `rustev.acquisition-request/1` canonical bytes and its
   `AcquisitionRequestId` verbatim, the attempt id spec 036 assigns,
   budget, trace id, `traceparent` and principal handle as for fetch.
   Response: a `rustev.acquisition-page/1` document's canonical bytes
   verbatim, or `failure{code, detail}`, and the charge as spec 009 3.8
   reports it. Codes map to spec 036's failure classes: `unauthorized` and
   a new code `denied` (the source refused the lookup for this principal)
   map to `denied`, never retried; the others as spec 009 3.6 maps them.
   The client validates nothing spec 036 3.3.4 validates; the runtime
   does. Page validity, retries, capture and evidence are spec 036's.
   Spec 036 is not yet on `main` (it is proposed as PR #91); `acquire` is
   implemented only once it is, and if its approved documents differ from
   what is named here, `acquire` is aligned by amendment first.
4. **Cancel** (`rustev.remote-context-cancel/1`): attempt ids; answer per id
   `stopped`, `unconfirmed` or `unknown_attempt`. `best_effort` never yields
   `stopped`.
5. **Semantics shared with `rustev.remote/1`.** One budget per call (spec
   009 3.4) divided by spec 020 3.4's phase ceilings when enabled; bounded
   connection reuse under spec 020 3.5's isolation key when enabled; at
   most one submission per attempt id, no invisible retry, no redirect; an
   endpoint receiving an attempt id again inside its window returns the
   recorded result for identical content and `malformed_request`
   (`attempt_id_conflict`) otherwise; the closed error taxonomy of spec 009
   3.6 (without `identity_mismatch`, which is replaced by
   `describe_mismatch` when a response names another describe digest). Each
   exchange yields a `rustev.remote-context-exchange/1` record (attempt id,
   request and response digests, code, phases, retention digest-only by
   default) to the host's exchange sink; a sink failure is counted and
   never changes the result.
6. **Invalidation.** `rustev.remote-context-invalidation/1` items carry:
   source id, a sequence number strictly increasing per source, kind (`correction`,
   `revocation`, `erasure`), the field names, and either the affected
   principal handle or `all_scopes`. Under `push`, the host posts batches to
   `POST /v1/context/invalidations` (admin group); under `poll`, the client
   calls `rustev.remote-context-invalidations/1` with the last sequence
   number. The service applies each item, before acknowledging it, as
   spec 017 invalidation: `invalidate_scope` for a named principal, or
   `invalidate_plan`, for every registered plan reading a named field. A
   sequence gap, a poll failure, or lag beyond `max_lag_ms` invalidates
   every plan reading that source's fields, marks the source
   `invalidation_degraded` in readiness and metrics, and is counted. With
   `none`, no invalidation is claimed, and none is inferred from polling
   (spec 010 3.5.3).

### 3.11 The remote sink protocol

`rustev.remote-sink/1` delivers evidence to a sink in another process. HTTP
binding: `POST /rustev/remote-sink/1/<op>`, transport rules as 3.10.

1. **Describe** (`rustev.remote-sink-describe/1`): chosen version, accepted
   run-record schemas (`rustev.run/1`, and `rustev.run/2` of spec 020 and
   the #76 track's `rustev.run/3` once their specs are implemented), the durability states the sink names, the deduplication
   window, and maximum record bytes. The client selects one schema at
   construction (spec 020 3.2); it never retries a rejected record in
   another schema.
2. **Deliver** (`rustev.remote-sink-deliver/1`): version, decision id, the
   record schema, the record's canonical bytes and its digest, the
   `RegistrationId` and signer (3.3.7), the service build, and
   `traceparent` if any. Answer: `accepted{durability, external_id,
   accepted_digest}`, `duplicate{accepted_digest}` (same decision id and
   digest inside the window), `conflict` (same decision id, another digest),
   or `rejected{code}`. An `accepted_digest` differing from the sent digest
   is `malformed_response`.
3. **Receipts and failures.** `accepted` yields receipt
   `remote-sink:<durability>:<digest>`, and `duplicate`
   `remote-sink:duplicate:<digest>`; each proves only what its state names
   (spec 003 3.9.6, spec 010 3.6.2). `conflict` and `rejected` are errors
   that did reach the sink; `transport_unsent` is an error that did not; a
   timeout or an interruption after request bytes is an error with
   `may_have_received: true`. No durability is invented.
4. **Policy unchanged.** The client implements `EvidenceSink`; the
   runtime's `fail_decision`, `backpressure` and `drop_counted` policies
   apply as spec 003 3.9.4 states, and spec 028's `TeeSink` may combine it
   with the OpenTelemetry sink.

### 3.12 Bounded callback seams

The rules of spec 027 3.6 bind every remote context and sink client:

1. Each client has a declared in-flight limit and queue capacity (1 to
   65,536; no default). When full, the call is not enqueued: a fetch fails
   `context_unavailable` with `remote_seam_queue_full`, an acquisition gets
   spec 036's unavailable outcome with the same reason, and a delivery
   fails `remote_seam_queue_full` with `may_have_received: false`, handled
   by the sink policy.
2. Every call carries its remaining deadline and a cancellation signal
   linked to the decision's (or, for delivery, the sink policy's timeout).
3. A response arriving after the caller stopped waiting is discarded,
   counted in `late_responses_discarded`, and recorded in the exchange
   record; it never reaches a snapshot, judgment or receipt.
4. Queue depth, refusals and late discards are exposed in metrics (3.13).

### 3.13 Health, readiness and metrics

1. `/healthz` is 200 while the process serves requests.
2. `/readyz` is 200 only when: every preloaded bundle is registered; every
   configured backend, context source and sink completed describe; no sink
   or source is in a degraded state the configuration marks as
   readiness-affecting (`invalidation_degraded` always is); and the service
   is not shutting down. Otherwise 503 with a `rustev.serve-readiness/1`
   body naming each failing item and the signing mode.
3. `/metrics` is OpenMetrics text: the runtime's `Gauges`, `SinkStats`,
   admission refusals by code, idempotency window occupancy, stream drops,
   callback queue depth, refusals and late discards, invalidation lag and
   gaps, and registrations. Metrics are operator views, not evidence; they
   reset with the process.

### 3.14 Graceful shutdown

On the first `SIGTERM` or `SIGINT` (a second one of either stops
immediately):

1. readiness turns 503 and admission stops (`shutting_down`);
2. in-flight decisions run for at most `drain_ms` (0 to 600,000);
3. the rest are cancelled through their signals and end with spec 003's
   honest cancellation evidence, delivered under the sink policy;
4. the delivery worker flushes its queue for at most `flush_ms` (0 to
   600,000);
5. the service writes `rustev.serve-shutdown/1` to standard error: counts
   of drained, cancelled, delivered, failed, dropped (`drop_counted`) and
   unflushed records, with the decision id of every failed and unflushed
   record (bounded list plus a total). It exits 0 when nothing is
   unflushed and no failure was hidden from a caller, and 3 otherwise.

An immediate stop skips 2 and 4 and reports every pending record as
unflushed. No record leaves the accounting.

### 3.15 Configuration and service identity

`rustev.serve-config/1` (identity-bearing, parsed under declared bounds,
refused whole when any member is out of range or unknown) holds: listeners
and their groups, TLS and authentication; limits; runtime configuration;
backend bindings by name, each a backend compiled into the binary (by
feature, for example the reference rules backend) or a `rustev.remote/1`
or `/2` endpoint, with its expected `DescriptorId`; context sources and
sinks by protocol endpoint, or spec 028's sink when built with that
feature; the sink policy; the signing mode and keys; preloaded bundle files;
the idempotency window; drain and flush bounds; and resolver roots. Secrets
are references to files, never inline. The configuration digest excludes
secret bytes and addresses and is reported in every response; backend
binding identities remain as specs 009 and 020 define them.

### 3.16 Container image and binary

1. The crate builds the `rustev-serve` binary. `image/` holds the image
   definition: a distroless base pinned by digest, the binary only, user and
   group 65532 (non-root), no shell, a read-only root filesystem
   compatible layout, entrypoint `rustev-serve --config
   /etc/rustev/serve.json`, platforms `linux/amd64` and `linux/arm64`.
2. Building, reproducibility, SBOM, signature, provenance, vulnerability
   scan and publication are spec 031's (3.1 layer 7, 3.9); this spec adds no
   release workflow. The image is a family member only when this spec is
   implemented at the candidate commit.

### 3.17 Shared conformance

1. Under spec 027 3.12, the suite gains the service as two surfaces,
   `serve-http` and `serve-grpc`, each over TCP and over a Unix socket. The
   Rust runner starts the service in process on loopback with scripted
   backends, a remote context source and a remote sink hosted by
   `rustev-remote-http`'s server side, and runs every golden case: the
   response's judgment bytes and the run record bytes the sink receives
   must equal the case's expected bytes under spec 027 3.12.3's comparison,
   and every case asserts `plan_id`.
2. A transport-differential check compares, per case, the HTTP and gRPC
   documents byte for byte (I-2).
3. Protocol conformance for `rustev.remote-context/1` and
   `rustev.remote-sink/1` runs against any endpoint (the Rust host server,
   and a host's own endpoint by URL), with positive and negative cases:
   timeouts at each phase, cancellation (`stopped`, `unconfirmed`, late
   answer), oversized bodies both ways, idempotency windows (repeat,
   conflict, expiry), unknown and unsupported versions, invalidation gaps,
   and duplicate and conflicting deliveries.
4. `make conformance` runs these with no Node toolchain and no network
   beyond loopback.

### 3.18 Dependency boundaries

`tools/rustev-boundaries` gains integration-only families: gRPC (`tonic`,
`tonic-build`, `prost`, `prost-build`, `prost-types`) and Sigstore
(`sigstore`, `sigstore-protobuf-specs`). HTTP and TLS already are. The
transitive purity check of `rustev-contract` and `rustev-core` refuses
them. Exact crates and versions are fixed by the binding record (2.2);
the generator of 3.9.1 runs at development time and its output is
committed, so no build script needs network access.

### 3.19 Compatibility

1. No plan construct is added; every plan and definition golden keeps its
   bytes and `PlanId`. `rustev.run/1`, `rustev.judgment/1`,
   `rustev.snapshot/1`, `rustev.remote/1` and `/2` keep their bytes.
2. New documents, all versioned by name: `rustev.serve-config/1`,
   `rustev.serve-service/1`, `rustev.serve-register/1`,
   `rustev.plan-bundle/1`, `rustev.plan-bundle-signature/1`,
   `rustev.plan-registration/1`, `rustev.serve-decide/1`,
   `rustev.serve-decision/1`, `rustev.serve-cancel/1`,
   `rustev.serve-event/1`, `rustev.serve-refusal/1`,
   `rustev.serve-replay/1`, `rustev.serve-evaluate/1`,
   `rustev.serve-readiness/1`, `rustev.serve-shutdown/1`,
   `rustev.remote-context-describe/1`, `rustev.remote-context-fetch/1`,
   `rustev.remote-context-acquire/1`, `rustev.remote-context-cancel/1`,
   `rustev.remote-context-invalidations/1`,
   `rustev.remote-context-invalidation/1`, `rustev.remote-context-exchange/1`,
   `rustev.remote-sink-describe/1` and `rustev.remote-sink-deliver/1`.
   `rustev.plan-bundle/1`, `rustev.plan-registration/1` and
   `rustev.serve-config/1` are identity-bearing; the others are record
   documents. Spec 004's `rustev.request/1` (logical request identity) is
   unchanged and is not the decision request.
3. `rustev.serve/1`, `rustev.remote-context/1` and `rustev.remote-sink/1`
   follow spec 031 3.6's naming and support window; the service API schema
   is diffed by spec 031 3.8's check.
4. Without `rustev-serve`, every other crate builds and behaves as today
   (spec 010 I-2); the callback clients are opt-in.

## 4. Out of scope

Implementing `rustev-serve` before its binding record is approved; any
binding record approval; user identity, sessions or authorization; plan
deletion or hot replacement; arbitrary code, operator or endpoint upload;
dynamic loading; distributed execution or cross-instance coordination of
idempotency windows; durable storage of decisions; a gRPC binding of the
callback protocols; a Node implementation of callback endpoints; the
`acquire` member before spec 036 is implemented, explanation before D2,
streamed fields before B5, profiles before B4, grades before B2; spec 028's
own behavior; publishing, pushing or tagging the image or crate; release
workflows; deployment; live provider calls or spend; real-user data.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Decision request body carries a definition, fallback or endpoint | `invalid_document`; nothing compiled or dispatched. |
| Decision for an unregistered plan id | `plan_not_registered` (404). |
| Bundle whose content compiles to another `PlanId` than its path | `plan_id_mismatch`. |
| Bundle signed by a key not configured | `signature_untrusted`. |
| Bundle bytes changed after signing | `bundle_tampered`. |
| Unsigned bundle under `require_signed` | `unsigned_refused`. |
| Unsigned bundle with a listed profile under `deny_unsigned_for_profiles` | `unsigned_refused`; without a listed profile, registered as `unsigned`. |
| Same `PlanId` registered with another signer | `registration_conflict`. |
| Registration on a listener without the admin group | `endpoint_unavailable`. |
| Missing `rustev-principal` | `principal_invalid`; nothing admitted. |
| Development bearer, or a listener without TLS, configured on any TCP listener, loopback included | Startup refused. |
| Unix socket mode other than `0600` or `0660` configured | Startup refused. |
| Body above the route limit with a correct declared length | `body_too_large` before any parse. |
| Runtime admission queue full | 429 `overloaded`, no `Retry-After`. |
| Deadline passes while queued | 503 `queue_deadline`. |
| Same decision id and digest resubmitted inside the window | Retained response, `replayed: true`, no second decision. |
| Same decision id, different body | `decision_id_conflict`. |
| Window full of unexpired entries | `idempotency_window_full`; nothing evicted. |
| Client disconnects after admission | Decision completes; evidence follows policy; lookup returns it. |
| Cancel with an unconfirmed backend | `unconfirmed` and `possibly_continuing`, never `stopped`. |
| Sink rejects under `fail_decision` | 500 `evidence_not_delivered` with the run record. |
| Remote sink times out after sending | Error with `may_have_received: true`; no receipt. |
| Remote sink answers `conflict` | Error; never duplicate success. |
| Remote sink returns another accepted digest | `malformed_response`. |
| Remote context queue full | `context_unavailable` (`remote_seam_queue_full`); no decision runs. |
| Remote context answers after the deadline | Discarded and counted; no decision used it. |
| Remote context returns a field not requested | `malformed_response`; `context_unavailable`. |
| Context endpoint declares a third-party class | Construction refused; not ready. |
| Context describe offers only an unknown version | Capability failure; not ready. |
| Invalidation sequence gap | Every affected plan invalidated, `invalidation_degraded`, counted. |
| Stream consumer stalls | `step` events dropped and counted; `final` delivered with the count. |
| HTTP and gRPC give different bytes for a case | Transport-differential check fails. |
| Explain before D2 is implemented | `endpoint_unavailable`. |
| Shutdown with records still queued after `flush_ms` | Listed as unflushed in the shutdown report; exit 3. |
| A crate outside `integrations/` depends on `tonic` or `sigstore` | `make boundaries` fails. |

## Acceptance

- The spec 010 3.4 binding record for `rustev-serve` has the owner's
  approval before any `rustev-serve` code lands; until then the service
  items below are reported as externally blocked, never as passed.
- Shared conformance (3.17.1): identical judgments and run records for every
  golden through `serve-http` and `serve-grpc`, over TCP and a Unix socket,
  equal to the CLI's and the native binding's expected bytes, each asserting
  `plan_id`.
- Transport differential: identical documents over HTTP/JSON and gRPC for
  every case, including refusals and streamed `final` events.
- Callback protocol conformance with positive and negative cases for
  timeouts, cancellation, oversized bodies, idempotency windows, unknown
  versions, invalidation gaps and delivery conflicts (3.17.3).
- Shutdown under load: with scripted slow, failing and hanging sinks under
  each sink policy, accounted records equal admitted decisions, and every
  loss is counted with its decision id.
- Signed bundles: valid Ed25519, valid Sigstore against a committed test
  trusted root, wrong key, wrong identity, tampered content, and unsigned
  under each of the three modes.
- Admission: each refusal code of 3.5.1 has a test asserting status, gRPC
  code and document bytes, and that no `Retry-After` is sent.
- Bounded queues: queue full, late response and cancellation propagation for
  context fetch and sink delivery, with memory bounded by capacity.
- Compatibility: every plan, definition, CLI and runtime golden is
  byte-identical, with and without a progress observer attached.
- `.proto` and JSON Schema drift checks pass.
- The image builds for `linux/amd64` and `linux/arm64` as non-root under
  spec 031's candidate workflow; until that workflow exists this item is
  reported as externally blocked.
- Every row of section 5 has a deterministic test without network access
  beyond loopback.
- `make code`, `make conformance`, declared verification and `make gate`
  pass.

## Verification

Pending with implementation. Approval satisfies no acceptance item and
approves no binding record.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #57 leaves the points below open, and each is
decided within this spec, as written:

1. **Decision request document.** #57 names the body `rustev.request/1`;
   that name is spec 004's logical request identity, and reusing it would
   change an existing version's meaning (spec 031 3.6.1). The body is
   `rustev.serve-decide/1`.
2. **Registration is an admin route.** Registration over the API stays
   within spec 010 3.7.1 because it is served only on listeners that carry
   the admin group, and a decision request still cannot carry a plan.
3. **gRPC carries canonical JSON bytes.** Protobuf messages are generated
   from the same operation table as the HTTP routes, and wrap the canonical
   document bytes instead of re-encoding them, so evidence digests are
   transport-independent.
4. **Signer in evidence without a new run record.** The signer travels in
   the decision response and the sink delivery, joined by decision id;
   `rustev.run/1` and `/2` keep their bytes.
5. **Sigstore verification is offline** against a host-supplied trusted
   root, with exact identity and issuer; no network fetch.
6. **Disconnect never cancels**, on either transport, matching spec 027
   3.5.3; the cancel operation and the deadline do.
7. **`evidence_not_delivered` is HTTP 500** with the full decision document,
   so a client that ignores bodies cannot mistake it for success.
8. **Idempotency window never evicts unexpired entries**; a full window
   refuses new decisions, so no resubmission can start a second decision
   inside the window (spec 010 3.7.5).
9. **Callback protocols have an HTTP binding only.** gRPC is the service
   API's second transport; a gRPC binding of the callbacks needs a later
   amendment if a host asks for one.
10. **Invalidation granularity.** Field and principal invalidations map to
    spec 017's existing plan and scope selectors; gaps invalidate
    conservatively.
11. **Retry-After.** None today, because the runtime states no retry time.
12. **Spec 036 is not yet on `main`.** `acquire` carries spec 036's
    proposed documents (`rustev.source/1`, `rustev.acquisition-request/1`,
    `rustev.acquisition-page/1`, PR #91) verbatim and is aligned by
    amendment, if needed, before it is implemented. Spec 036 is not in
    `depends_on` because an edge to a spec absent from `main` is unresolved;
    by number order spec 036 merges first, and this spec gains the edge
    when it is rebased onto it.
13. **No loopback exemption.** Loopback is not a single-process trust
    boundary, so every TCP listener needs TLS and client certificates, and
    the development bearer is allowed on Unix sockets only. Conformance
    runs over loopback with test certificates.
14. **Explanation endpoint.** Spec 040's requirement on A2 (same library
    function, canonical bytes unchanged, host redaction, no cross-scope
    resolution) is adopted in 3.8.3.

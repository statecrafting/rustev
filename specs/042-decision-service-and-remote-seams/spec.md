---
id: "042-decision-service-and-remote-seams"
title: "Decision service sidecar and remote context and sink protocols (amends 010)"
status: approved
implementation: in-progress
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
  spec 010 section 3.4 binding record (R-34 in the founding decisions).
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
  # The service crate, established by spec 010 (section 2: the change that
  # first writes it under this spec adds the edge).
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: additive }
  # gRPC and Sigstore dependency families (integration-only).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-wasm/" }, nature: additive }
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # The CLI harvest names the service's conformance surfaces on `run` cases (3.17.1).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/tests/common/" }, nature: additive }
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
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "032-evidence-quality"
  - "034-parameterized-definitions"
  - "036-mid-plan-acquisition"
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
is pending. A-34 to A-38 ratify specs 037 to 041, which merged
first by number order. This spec implements
#76 child A2 (#57). Spec 010's approved text is not edited; this spec
records what it makes concrete and what it adds, as 014 does for 004.
Spec 010's binding-record gate stands unchanged:
`rustev-serve` may be implemented only after the owner separately approves
its section 3.4 binding record (R-34 in the founding decisions), and
nothing here approves one.
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
   refused at startup, and an existing path is refused, not replaced: after
   an unclean stop the startup error names the path, and the operator
   removes the stale socket file before restarting.
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
   loopback included, is refused at startup. Each listener using it is
   reported in `rustev.serve-service/1` and `/readyz`, as `allow_unsigned`
   is (3.3.6). A failed authentication is `unauthenticated` (401) before
   any body byte is read. Rustev holds no users, sessions or roles.
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
   profile or `none` (accepted only once spec 034, #76 B4, is implemented,
   otherwise `profile_unsupported`); `bindings`, a map from each backend id
   the definition uses to a backend binding name declared in configuration
   (3.15); and `compile_report`, the compile output `rustev-surface` renders
   for these inputs, including `plan_id`. Bindings name configured backends
   only; a bundle cannot carry an endpoint, credential or code.
   Two optional members carry the rest of spec 034's instantiation inputs
   (3.3.8): `variant` and `profile_bases`.
2. **Registration.** `PUT /v1/plans/{planId}` takes
   `rustev.serve-register/1` (`bundle`, and `signature` or `none`). The
   service compiles the bundle with the existing compiler against the
   descriptors of the named bindings, obtained at startup (spec 009 3.2.1).
   It refuses `compile_refused` (422, with `compile_refusal`, 3.5.1),
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
8. **Variant and base profiles.** Spec 034 makes a variant and a profile's
   `extends` chain inputs of instantiation: "A definition that declares
   variants is instantiated for exactly one of them; a definition that
   declares none is instantiated with variant `none`" (034 3.4.2), and
   "The host names one profile (the leaf) and supplies 0 to 64 further
   profiles available to `extends`. The chain is built from the leaf by
   following `extends` to the profile with exactly that name and version"
   (034 3.7.1). The bundle carries both:
   - `variant`: `{ "named": <name> }`, absent for the variant `none`,
     the name matching `[a-z][a-z0-9_]{0,63}` (034 3.1.3). It is the variant the bundle is
     instantiated for, as `plan compile --variant` names it (034 3.14.1),
     with or without a profile.
   - `profile_bases`: a list of 1 to 64 `rustev.profile/1` documents
     (spec 034 3.7.1 admits 0 to 64; none is spelled by omission), each
     checked by its own bounded parser (`DESCRIPTOR_V1`) and carried as its
     original bytes, available to the leaf profile's `extends` chain as
     `plan compile --profile-base` supplies them. A non-empty list without a
     profile is `invalid_document`, as `--profile-base` without
     `--profile` is a usage error (034 3.14.1).
   Both are omitted when unused, so a bundle without them keeps its
   canonical bytes, bundle digest and `RegistrationId`. Absence is the
   only spelling of unused: a bundle that states `"variant": "none"` or an
   empty `profile_bases` is `invalid_document`, so one set of inputs has
   exactly one bundle digest. The bundle still carries one definition: no version
   range selects among definitions and no `--definition-version` pin
   applies, so a profile whose range does not admit that definition's
   version is refused `definition-unsatisfied` (034 3.6.3). The service
   instantiates with exactly these inputs (034 3.8.1) and compiles the
   result as 3.3.2 states. Every instantiation refusal of 034 3.9
   (`variant-required`, `variant-unknown`, `profile-missing`,
   `profile-duplicate`, `profile-cycle`, `profile-chain-too-deep`, the
   parameter codes and the others) is `compile_refused` (422) with
   `compile_refusal` the refusal document `rustev-surface` renders for
   it, bounded as 3.5.1 states; no new refusal code is added. Digest
   effects: the bundle digest covers `variant` and every base profile's
   bytes when present, so two bundles that differ only in a base profile
   the chain never reaches have the same `PlanId` (034 3.10.2: the other
   profiles supplied "are not recorded and do not affect it") but
   different bundle digests and `RegistrationId`s, and the second is
   `registration_conflict`, as 3.3.4 states for any `PlanId` registered
   again with other bundle bytes, whoever signed it. The registration record's profile id
   is the leaf's `ProfileId`; `deny_unsigned_for_profiles` (3.3.6) refuses
   an unsigned bundle when any profile of its resolved chain, leaf or
   base, is listed.

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
   error and whether the sink may have received it), `replayed` (3.6), and,
   only when `status` is `evidence_not_delivered`, `run_record`: the run
   record bytes exactly as the runtime produced them, because the sink may
   not hold them. The judgment and the run record are each bounded by
   spec 002's `RECORD_V1` (4 MiB), so a `rustev.serve-decision/1` response
   body is at most 9 MiB.
   Backend and served identities stay as the run record has them, including
   `unknown` (010 3.7.4).
4. **Status codes.** `judged` and `cancelled` are HTTP 200 and gRPC `OK`.
   `evidence_not_delivered` is HTTP 500 with the full decision document and
   its `run_record` (3.4.3) as the body, or gRPC `INTERNAL` with metadata
   `rustev-decision-id` naming the decision; a gRPC client reads the same
   document by lookup (3.6.3), because status metadata is bounded far below
   a run record. The document is retained for the idempotency window
   (3.6.1); after it expires the lookup is `decision_not_found`. The window
   is at least 60 seconds from the decision's end, so a gRPC client looks
   up within that time; a resubmission does not extend it (3.6.2), so one
   replayed near the window's end leaves only the remainder for the
   lookup, after which the run record is only where the sink policy left
   it. Work
   and spending happened, and no judgment is returned as a success (spec
   003 3.9.4). The status
   code alone is not the outcome: 200 covers both `judged` and
   `cancelled`, and this 500 means a decision ran, while a refusal's 502
   or 503 (3.5) means none did, so the status alone does not tell them
   apart. A client reads `status`, or
   `decision_ran` in a refusal.
5. **Disconnect.** A client disconnect or a dropped gRPC call does not
   cancel; only the cancel operation or the deadline does. The decision
   finishes, its evidence follows the sink policy, and its response is
   retained for lookup (3.6). The service never reports a disconnect as a
   stop (010 3.7.5).

### 3.5 Admission, refusals and limits

1. **Refusal document.** When no decision ran, the body is
   `rustev.serve-refusal/1`: `code`, `detail` (at most 256 bytes),
   `decision_ran: false`, `retry_after_ms` only when the runtime states
   one, and, only with `compile_refused`, `compile_refusal`: the compiler's
   refusal document exactly as it produced it when it is at most 2 KiB,
   otherwise only its digest and length (the same compiler reproduces it
   from the bundle). A refusal document is therefore at most 4 KiB on
   either transport, under 6 KiB once base64-encoded as gRPC binary
   metadata, so it fits the common 8 KiB HTTP/2 header-list default of
   clients as well as the service (binary metadata, 3.9.2; header limits,
   3.5.3). `body_too_large` and
   `headers_too_large` use
   `RESOURCE_EXHAUSTED`, the code gRPC itself gives an oversized message;
   an intermediary that maps it to 429 loses only the HTTP status, never
   the `code`. Codes and statuses:

   | Code | HTTP | gRPC | When |
   |---|---|---|---|
   | `overloaded` | 429 | `RESOURCE_EXHAUSTED` | Runtime admission queue full (spec 003 3.4.1), or the replay, evaluation and explanation pool queue full (3.8.4) |
   | `idempotency_window_full` | 429 | `RESOURCE_EXHAUSTED` | 3.6.2 |
   | `queue_deadline` | 503 | `UNAVAILABLE` | Deadline passed while queued |
   | `shutting_down` | 503 | `UNAVAILABLE` | 3.14 |
   | `not_ready` | 503 | `UNAVAILABLE` | 3.13 |
   | `context_unavailable` | 502 | `UNAVAILABLE` | 3.4.2, with the remote code in `detail` |
   | `cancelled_before_admission` | 409 | `ABORTED` | Cancelled while queued |
   | `decision_id_conflict`, `registration_conflict` | 409 | `ALREADY_EXISTS` | 3.6, 3.3 |
   | `decision_in_progress` | 409 | `ABORTED` | 3.6 |
   | `registry_full` | 409 | `FAILED_PRECONDITION` | 3.3 |
   | `invalid_request`, `invalid_document`, `principal_invalid`, `invalid_trace_context`, `invalid_deadline` | 400 | `INVALID_ARGUMENT` | Bounds or parse |
   | `unauthenticated` | 401 | `UNAUTHENTICATED` | 3.2.1 |
   | `plan_not_registered`, `decision_not_found`, `endpoint_unavailable` | 404 | `NOT_FOUND` | |
   | `body_too_large` | 413 | `RESOURCE_EXHAUSTED` | 3.5.3 |
   | `headers_too_large` | 431 | `RESOURCE_EXHAUSTED` | 3.5.3 |
   | `unsupported_media_type` | 415 | `INVALID_ARGUMENT` | 3.1.3 |
   | `compile_refused`, `plan_id_mismatch`, `binding_unknown`, `profile_unsupported`, `bundle_tampered`, `signature_invalid`, `signature_untrusted`, `unsigned_refused` | 422 | `FAILED_PRECONDITION` | 3.3 |
   | `internal_error` | 500 | `INTERNAL` | A runtime invariant failure with no run-record termination (spec 037 3.12.6, a generation invariant); no decision record and no `detail` beyond the code |

   Each code has exactly one HTTP status and one gRPC code, while several
   codes may share either, as the three 503 codes and `context_unavailable`
   (502) share gRPC `UNAVAILABLE` but not their HTTP status; the refusal
   document's `code` is what tells them apart, over either transport. A conflicting
   resubmission is `ALREADY_EXISTS` and never idempotent success, a decision
   still running is `ABORTED` (retry after it ends), a decision cancelled
   while queued is `ABORTED` too (its code tells the two apart; `CANCELLED`
   is not used, because gRPC bridges map it to the non-standard HTTP 499),
   and a full registry is
   `FAILED_PRECONDITION` (an operator must act).
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
4. **Runtime bounds.** `max_in_flight`, `max_queued`,
   `max_parallel_requests` (the requests in progress within one decision,
   spec 003 3.4.2) and the per-backend concurrency of spec 003 3.4 are
   configuration. `max_parallel_requests` is optional, from 1 to 65,536;
   absent, it is 1, the service's behavior before the member existed. A
   configuration without it keeps its configuration digest; one that
   states it, 1 included, is a different configuration. The service adds
   no queue of its own in front of runtime admission.

### 3.6 Idempotency, lookup and cancellation

1. **Window.** The service retains, per host scope (principal handle
   digest, decision id), the request digest and the response document of
   every decision that ran, for `idempotency_window_ms` (60,000 to
   86,400,000; the floor leaves a gRPC client at least a minute to look
   up an `evidence_not_delivered` document, 3.4.4), bounded by
   `max_retained` (1 to 1,000,000) and
   `max_retained_bytes` (16 MiB to 16 GiB), counting each entry's request
   digest and response document bytes. Admission reserves the largest
   response a decision can produce (9 MiB, 3.4.3) and releases the excess
   once the response is known, so a decision that ran is always retained
   for its window. Each in-flight decision therefore holds 9 MiB: the
   16 MiB floor admits one at a time, and a host sizes the bound as at
   least 9 MiB times its admission concurrency plus the retained responses
   it expects; a full bound refuses `idempotency_window_full`. The request
   digest is the
   `tagged_digest` of the path's `PlanId` and the `rustev.serve-decide/1`
   document's canonical bytes, every member included; headers (deadline,
   trace context) are not part of it, so a retry resends the same document
   to the same plan.
2. **Resubmission.** Inside the window, the same scope and decision id with
   the same request digest returns the retained response with
   `replayed: true` and starts nothing; `?stream=1` is not part of the
   digest, so a resubmission with it gets a stream holding only the `final`
   event with the retained response, and one without it gets the retained
   document as the body. A resubmission replays the original status: over
   gRPC, a unary `evidence_not_delivered` decision replays `INTERNAL` with
   `rustev-decision-id`, its document read by lookup within the same
   window (3.4.4), and a streaming one sends the `final` event with the
   retained response (`replayed: true`) and then ends with `INTERNAL`, as
   3.7.1 states for a first run. With a different digest it is
   `decision_id_conflict`; while running it is `decision_in_progress`. When
   a new entry, with its reservation, would exceed `max_retained` or
   `max_retained_bytes` unless an unexpired one were evicted, the new
   decision is refused `idempotency_window_full`; an unexpired entry
   is never evicted. After expiry, decision-id uniqueness is the host's
   (spec 003 3.9.6). Retention is memory; a restart forgets it, and
   `rustev.serve-service/1` states so.
3. **Lookup.** `GET /v1/decisions/{decisionId}` with the same principal
   returns the retained response, or, while the decision has not ended,
   `rustev.serve-decision-state/1` (decision id, `plan_id`, and `queued`
   or `running`; HTTP 200, gRPC `OK`), or `decision_not_found`. A
   backpressure ticket's outcome (receipt or
   failure) appears here once the delivery worker settles it.
4. **Cancel.** `POST /v1/decisions/{decisionId}/cancel` raises the
   decision's `CancelSignal` and returns `rustev.serve-cancel/1`: `queued`
   (it will end `cancelled_before_admission`), `requested` (admitted;
   attempts answer per spec 003 3.7) or `already_finished`. A decision
   unknown in the caller's scope is the refusal `decision_not_found` (404,
   `rustev.serve-refusal/1`), as for lookup. With `?wait_ms=N` (0 to
   60,000) it waits up to N ms for the decision to end and then includes
   each attempt's `CancelAck` (`not_requested`, `stopped`, `unconfirmed`,
   `no_answer`) and `RemoteEnd` (`finished`, `stopped`,
   `possibly_continuing`), copied from the run record; neither field ever
   reports a `stopped` the runtime did not record.

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
   A stream's HTTP status is 200 once its headers are sent, whatever the
   outcome, so a streaming client reads the `final` event (its `status`, or
   the refusal), and only a client that does may ask for `?stream=1`. Over
   gRPC, a `final` event with `evidence_not_delivered` ends the stream with
   `INTERNAL`.
2. Each stream has a bounded event queue (16 to 65,536). When full, `step`
   and `field` events are dropped and counted, never blocking the decision;
   `final` is never dropped and carries `events_dropped`: the service
   handler, not the observer, writes it after the decision ends, into a
   slot each stream reserves for it outside the event queue.
3. **Progress observer.** `rustev-runtime` gains an optional observer
   registered per decision: a synchronous callback invoked after a step's
   result is recorded, receiving only the data of 3.7.1 `step`. It must not
   block: it performs no I/O, takes no lock a decision can wait on, and only
   offers the event to the stream's bounded queue (3.7.2) without waiting.
   The service's observer is the only one it installs and does exactly that.
   It cannot change, delay or fail the decision; a panic inside it
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
   reference prefix; a reference is never a URL and never escapes its root:
   its path after the prefix is relative, its components are non-empty and
   never `.` or `..`, and the opened file's canonical path must lie under
   the root's canonical path (a symlink out of the root fails). An
   escaping or unresolvable reference is reported as spec 004 reports
   `inaccessible`.
   Output is the bytes `rustev-surface` renders.
3. **Explanation.** `POST /v1/explain` takes `rustev.serve-explain/1`
   and answers `rustev.serve-explanation/1`. They are the envelope spec
   040 3.15.3 leaves to this spec ("its transport, authentication, bounds
   and envelope"), meeting its requirements: the route "takes this spec's
   evidence and query, with evidence as documents or host-resolved
   references; it returns the explanation's record canonical bytes
   unchanged and the Markdown rendering of 3.14, computed by the same
   library function; it applies the redaction policy the host configures
   for the caller (3.13); and it never explains evidence outside the
   caller's authorized scope (spec 010, 3.7.2)."
   - **Request** (`rustev.serve-explain/1`): `evidence`, `query` and
     `options`, in the shape spec 040 3.15.2 gives the bindings
     (`engine.explain(evidence, query, { nowMs?, scope?, resolve?,
     redaction? })`, "where `evidence` is `{ record, plan? }` or
     `{ bundle }` as bytes and `query` is the query of 3.3 or
     `{ diff: { other: evidence } }`"). Each evidence member (`record`,
     `plan`, `bundle`) is a document embedded as its own bytes or a
     resolver reference, as 3.8.2 resolves references, and is parsed under
     the limits spec 040 3.1.2 states. `query` is spec 040 3.3's query, or
     `{ diff: { other } }` with `other` an evidence set of the same shape.
     `options` holds `scope` and `now_ms` exactly when a side is in bundle
     form, following spec 040 3.16.2 ("When either side is a bundle ...
     `--now-ms` is required and the scope flags follow spec 006, 3.1.4;
     when neither is, all of them are refused"); given when no side is a
     bundle, or missing when one is, the request is `invalid_request`.
     The request carries no redaction policy and no store resolver.
   - **Scope.** `options.scope` must equal the scope the host resolves for
     the caller's principal, as for replay (3.8.1); otherwise the answer
     is `decision_not_found` and nothing is resolved or explained. A
     bundle's external items resolve only through the host's resolver for
     that scope, and a reference only within the caller's principal
     scope.
   - **Redaction.** The policy is the `redaction_policy` the configuration
     names for the listener the request arrived on (3.15); a listener
     with none explains unredacted, as the library does without a policy.
   - **Answer** (`rustev.serve-explanation/1`): `explanation`, the
     `rustev.explanation/1` record canonical bytes the library returns,
     carried unchanged as an embedded document (3.1.4), and `markdown`, its
     spec 040 3.14 rendering, both from one call of
     `rustev_eval::explain::{explain, explain_diff, render_markdown}`
     through `rustev-surface`. A spec 040 refusal (3.17) is
     `invalid_document` carrying the refusal code string in `detail`.
   - **gRPC.** As every operation (3.9.2): the request and the answer are
     the `bytes document` of the generated messages.
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
   `INTERNAL` with `rustev-decision-id`, the document read by lookup (3.4.4).
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
   grade once spec 032, #76 B2, is implemented, otherwise absent); the
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
   These are spec 036's approved documents (A-33), carried verbatim;
   `acquire` is implemented once spec 036 is.
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
   `POST /v1/context/invalidations` (admin group) as
   `rustev.serve-invalidations/1` (source id and items in sequence order),
   answered by `rustev.serve-invalidations-ack/1` (the highest sequence
   number applied); under `poll`, the client
   calls `rustev.remote-context-invalidations/1` with the last sequence
   number. The first poll after startup carries no sequence number: the
   source answers with its current sequence number and no items, which
   sets the baseline and is never a gap, because every cache the service
   holds is memory and starts empty, so nothing it holds can be stale.
   Until that baseline poll succeeds, the source is `invalidation_degraded`
   as after a gap. The service applies each item, before acknowledging it, as
   spec 017 invalidation: `invalidate_scope` for a named principal, or
   `invalidate_plan`, for every registered plan reading a named field. A
   sequence gap, a poll failure, or lag beyond `max_lag_ms` invalidates
   every plan reading that source's fields, marks the source
   `invalidation_degraded` in readiness and metrics, and is counted. The
   service treats the three kinds alike, because each means only that
   cached values may be wrong: a `revocation` (the source withdraws a value
   or a principal's access to it) needs no other action here, since the
   next fetch asks the source, which applies its own access decision. The
   state clears once the cause is gone: after a gap or a poll failure, when
   every affected plan is invalidated and a fresh baseline poll (as at
   startup) or a push batch contiguous with the
   last applied item succeeds; after lag, when it falls
   back within `max_lag_ms`. An
   `erasure` item is applied as cache invalidation only: the service holds
   no store to erase, a response retained under 3.6 for that scope stays
   until its window expires (at most `idempotency_window_ms`, up to 24
   hours), so a host under a prompt-erasure obligation sets the window no
   longer than that obligation allows. The window is reported in
   `rustev.serve-service/1`, and each `erasure` item applied while a
   response for its scope is retained is counted in metrics; the host's
   erasure obligation stays the host's. With
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
   `may_have_received: true`; `malformed_response` (an unparsable answer, or
   an `accepted_digest` differing from the sent digest) is an error with
   `may_have_received: true` and yields no receipt. Each error is handled
   by the sink policy (3.11.4). No durability is invented.
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
and their groups, TLS and authentication; limits; runtime configuration
(`max_in_flight`, `max_queued`, the optional `max_parallel_requests` and
`per_backend_concurrency`, 3.5.4);
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
   `rustev.serve-invalidations/1`, `rustev.serve-invalidations-ack/1`,
   `rustev.serve-decision-state/1`,
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
| Bundle whose definition declares variants, with `variant: none` | `compile_refused`, instantiation code `variant-required`. |
| Bundle whose profile `extends` a profile not in `profile_bases` | `compile_refused`, instantiation code `profile-missing`. |
| Bundle with `profile_bases` and no profile | `invalid_document`. |
| Bundle stating `"variant": "none"` or an empty `profile_bases` | `invalid_document`. |
| Unsigned bundle whose listed profile is a base of its chain under `deny_unsigned_for_profiles` | `unsigned_refused`. |
| Same `PlanId` registered with another signer | `registration_conflict`. |
| Same `PlanId` registered with other bundle bytes, for example a base profile its chain never reaches (3.3.4, 3.3.8) | `registration_conflict`. |
| Registration on a listener without the admin group | `endpoint_unavailable`. |
| Missing `rustev-principal` | `principal_invalid`; nothing admitted. |
| `rustev.serve-decide/1` with both or neither of `snapshot` and `context` | `invalid_document`; nothing admitted. |
| Evaluation reference with a `..` component, or a symlink resolving outside its root | Reported `inaccessible`; no byte outside the root is read. |
| Cancel or lookup of a decision id unknown in the caller's scope | 404 `decision_not_found` refusal; nothing cancelled. |
| Development bearer, or a listener without TLS, configured on any TCP listener, loopback included | Startup refused. |
| Unix socket mode other than `0600` or `0660` configured | Startup refused. |
| Body above the route limit with a correct declared length | `body_too_large` before any parse. |
| Runtime admission queue full, or the 3.8.4 pool queue full | 429 `overloaded`, no `Retry-After`. |
| Deadline passes while queued | 503 `queue_deadline`. |
| Same decision id and digest resubmitted inside the window | Retained response, `replayed: true`, no second decision. |
| Same decision id, different body | `decision_id_conflict`. |
| Window full of unexpired entries | `idempotency_window_full`; nothing evicted. |
| Client disconnects after admission | Decision completes; evidence follows policy; lookup returns it. |
| Cancel with an unconfirmed backend | `unconfirmed` and `possibly_continuing`, never `stopped`. |
| Sink rejects under `fail_decision` | 500 `evidence_not_delivered` with the decision document's `run_record` bytes. |
| Remote sink times out after sending | Error with `may_have_received: true`; no receipt. |
| Remote sink answers `conflict` | Error; never duplicate success. |
| Remote sink returns another accepted digest | `malformed_response`. |
| Remote context queue full | `context_unavailable` (`remote_seam_queue_full`); no decision runs. |
| Remote context answers after the deadline | Discarded and counted; no decision used it. |
| Remote context returns a field not requested | `malformed_response`; `context_unavailable`. |
| Context endpoint declares a third-party class | Construction refused; not ready. |
| Context describe offers only an unknown version | Capability failure; not ready. |
| Invalidation sequence gap | Every affected plan invalidated, `invalidation_degraded`, counted. |
| `erasure` invalidation for a scope with a retained decision response | Cached context invalidated; the retained response is still returned to a resubmission or lookup until its window expires. |
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

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it.

**Increment 1, remote half (contract documents).** Spec 2.2 allows the
documents in `rustev-contract` before the `rustev-serve` binding record.
`crates/rustev-contract/src/remote_context.rs` defines the
`rustev.remote-context/1` documents of 3.10: describe (request and
response: per source, fields with type, provenance class, freshness and
staleness bound, and spec 032's evidence grade; terms with cancellation,
idempotency window, byte maxima, invalidation and the `host_operated`
endpoint class), fetch, cancel, invalidation and the poll-mode
invalidations exchange, and the exchange record.
`crates/rustev-contract/src/remote_sink.rs` defines the
`rustev.remote-sink/1` describe and deliver documents of 3.11. Each is a
strict serde type with its exact schema string and the bounded parser,
and its JSON Schema is emitted through spec 027's emitter under
`crates/rustev-contract/schema/`. Held back until spec 036 is implemented,
as 3.10.1 and 3.10.3 state: the acquire documents with the `denied` code,
and the source's `rustev.source/1` descriptor in describe; extension
points mark both. No existing document, bound or canonical byte changes.
Tests: `tests/042_remote_documents.rs` (round trips, unknown fields, wrong
schema strings, bounds at the limit and one past). The declaration
generator (`tools/rustev-conformance/js/gen-types.mjs`) now types a member
schema that constrains nothing as `unknown`, not an open object, so a
snapshot entry's `value` (here and in `rustev.snapshot/1`) admits any JSON
value in TypeScript as the parser does.

**Increment 1, serve half (contract documents).**
`crates/rustev-contract/src/serve.rs` defines `rustev.serve/1` with its
operation table (3.9) and every `rustev.serve-*/1` document of 3.1 to 3.9
and 3.13 to 3.15; `crates/rustev-contract/src/plan_bundle.rs` defines
`rustev.plan-bundle/1`, `rustev.plan-bundle-signature/1` and
`rustev.plan-registration/1` (3.3). Embedded documents (plan bundles and
signatures, judgments, run records and compile refusals) are checked by
their own bounded parsers and carried as their original bytes, so a
service envelope never re-encodes evidence; this uses `serde_json`'s
`raw_value` feature, which adds no dependency. Held back until specs 034
and 036 are implemented, with extension points: parameters, variants,
tenant profiles and profile-dependent signing (034), and mid-plan
acquisition inputs and tiered-widening outcomes (036). Tests:
`tests/042_serve_documents.rs`.

Engineering choices made by the implementing agent where the spec states a
bound without a value (3.5.3, 3.14), not by the owner: per listener at most
65 536 connections; per HTTP/2 connection 1 to 1 024 concurrent streams;
per connection 1 to 10 000 requests per second; the shutdown report lists
at most 10 000 failed and 10 000 unflushed decision ids, in order, with
the total and `truncated`. Other unstated sizes follow the surrounding
documents: 4 MiB envelopes (10 MiB for events, 8 MiB for the shutdown
report), 256-byte names and error text, 4 096-byte paths, endpoints and
attempt ids, and 1 024-entry configuration lists.

**Runtime progress observer (with `rustev-serve` increment 1).**
`crates/rustev-runtime/src/progress.rs` adds `ProgressObserver` and
`Runtime::decide_observed` (3.7.3, "**Progress observer.** `rustev-runtime` gains an
optional observer registered per decision"): a synchronous callback after each
step's result is recorded, carrying the step id, instance ids, outcome and
the derivation class the compiled step determines, never a value. A panic
inside it is contained and counted (`observer_panics`). `Runtime::decide`
installs none, and every existing runtime test, golden and conformance
case is byte-identical; `tests/progress.rs` checks events against the
judgment for an exact and a model-backed step.

**Boundary families (with `rustev-serve` increment 2).**
`tools/rustev-boundaries/` adds the integration-only gRPC family (`tonic`,
`tonic-build`, `tonic-prost`, `tonic-prost-build`, `prost`, `prost-build`,
`prost-types`) and the Sigstore family (`sigstore`, `sigstore-verify`,
`sigstore-protobuf-specs`) of 3.18; the transitive purity check of
`rustev-contract` and `rustev-core` refuses both, with tests.

**`rustev-serve` increments 1 and 2 (2026-10-22, recorded in spec 010).**
The service over HTTP/1.1 and HTTP/2 (#190, #191) and the gRPC transport
of 3.9 (#187) are recorded in spec 010's implementation record, with
`integrations/rustev-serve/tests/COVERAGE.md` mapping section 5. The gRPC
transport is complete: the `.proto` and its Rust code are generated from
the operation table and drift-checked (`tests/grpc_generation.rs`), and
`grpc_differential` compares HTTP and gRPC documents for success, each
refusal class, streaming and `evidence_not_delivered`. Not yet delivered:
the HTTP binding of the two callback protocols in `rustev-remote-http`
(3.10 to 3.12), and the shared conformance surfaces (3.17).

**Increment 3: the image definition (2026-10-29).**
`integrations/rustev-serve/image/Dockerfile` defines the image of 3.16.1:
a builder stage on the Linux gnu toolchain image spec 031's `artifacts.sh`
uses, and a runtime stage on spec 010's recorded base,
`gcr.io/distroless/cc-debian12:nonroot` pinned by its index digest, holding
only the binary, as user and group 65532, with entrypoint
`/usr/local/bin/rustev-serve --config /etc/rustev/serve.json`; nothing in
the runtime stage writes to the filesystem, so it runs with a read-only
root. `Dockerfile.dockerignore` sends sources only.
`integrations/rustev-serve/tests/image.rs` checks the definition: both
bases pinned by digest, the runtime base equal to the binding record's
index, the builder equal to `artifacts.sh`'s, and the runtime stage
exactly the copy, the user and the entrypoint. `image/check.sh <image>
<platform>` checks a built image: platform, user, entrypoint, no shell,
and that the binary starts on a read-only root filesystem. Building it for
`linux/amd64` and `linux/arm64` in the candidate workflow, and the rest of
3.16.2, are spec 031's (3.1 layer 7); until that workflow builds the image,
the image acceptance item stays externally blocked. In
`tools/rustev-release/release-set.json` the image stays `absent`, with its
reason updated. This change adds the `extends` edge on
`integrations/rustev-serve/` that section 2 calls for.

**Increment 4 part 1: the callback host server and the seam queue
(2026-10-29).** `integrations/rustev-remote-http/src/callback_server.rs`
is the host-side HTTP server of both callback protocols: it hosts any
`ContextSource` behind `POST /rustev/remote-context/1/{describe,fetch,
cancel,invalidations}` and any `EvidenceSink` behind
`POST /rustev/remote-sink/1/{describe,deliver}`, plain HTTP on loopback
only, TLS otherwise, or a Unix socket. A fetch runs an attempt id at most
once inside the idempotency window (identical content gets the recorded
answer, other content `malformed_request` with `attempt_id_conflict`), a
fetch naming another describe digest is `describe_mismatch`, and a fetch
whose budget is spent never starts. Cancel answers `stopped` only under
`confirmed` cancellation, by dropping the hosted fetch; `best_effort` and
`none` answer `unconfirmed`, and an unknown id `unknown_attempt` (3.10.4).
Under `poll` invalidation the server keeps a bounded log with one sequence
across its sources; the first poll gets the current sequence and no items,
and items dropped from the log show to a poller as a gap (3.10.6). A
delivery is `accepted` once, then `duplicate` for the same digest and
`conflict` for another inside the deduplication window; a `rejected`
delivery is not remembered (3.11.2). Every table is bounded, and a full
one with nothing expired answers `503`.
`integrations/rustev-remote-http/src/seam_queue.rs` is the bounded queue of
3.12: an in-flight limit and a queue capacity, each 1 to 65,536 with no
default, refusal at once with `remote_seam_queue_full` when full, a queued
call waiting no longer than its deadline, and counters for depth, refusals
and late responses. Tests: `tests/callback_server.rs` and the module's unit
tests. The clients of both protocols, which use the queue, follow in part 2.
No existing module of the crate changes; `tools/rustev-release/
archive-policy.json` lists the two new files.

**Increment 4 part 2: the remote context client (2026-10-29).**
`integrations/rustev-remote-http/src/context_client.rs` is the
`rustev.remote-context/1` client of 3.10: `RemoteContextClient` implements
`ContextSource`. It describes the endpoint once at construction, and an
unknown version, an undescribed source or an endpoint class other than
`host_operated` (which does not parse) is a capability failure there. Each
fetch goes through its `SeamQueue` (3.12.1): a full queue fails at once
with `remote_seam_queue_full` and is counted. A fetch carries the remaining
budget less the declared transit margin, the describe digest, the call's
principal handle as the spec 020 principal reference with the configured
mapping version and scope, and the attempt id
`<escaped decision id>/context/<n>`, numbered per decision; it is sent at
most once, with no retry. An answer naming a field not requested, or a
local-only code, is `malformed_response`. When the caller stops waiting,
the client asks the endpoint to cancel (unless the source declares `none`)
and an answer that still arrives is discarded and counted in
`late_responses_discarded` (3.12.3). Each exchange yields a digest-only
`rustev.remote-context-exchange/1` record to an optional host exchange
sink, whose failures are counted and change no result (3.10.5). `cancel`
rejects a `stopped` answer from a source without confirmed cancellation,
and `invalidations` polls (baseline with no sequence) and refuses a
sequence that does not increase. Tests: `tests/context_client.rs`
against the host server of part 1. In
`tools/rustev-release/archive-policy.json` the crate lists the file and its
unpacked bound becomes 393,216 bytes (384 KiB), as `rustev-runtime`'s did
in #330: the package unpacks to 332.6 KiB against the earlier 320 KiB.

**Increment 4 part 3: remote context sources in the service (2026-10-29).**
`integrations/rustev-serve/src/contexts.rs` builds one
`RemoteContextClient` per entry of `context_sources` when the standalone
binary starts, the entry's key being the source id the endpoint describes,
with the entry's in-flight limit and queue capacity (3.12.1). A source
whose describe fails is installed as unavailable: each fetch from it fails
`context_unavailable`, and it is reported not described, so readiness
reflects it when the endpoint is readiness-affecting (3.10.1). Queue depth
(queued calls, beyond those in flight), refusals and late discards are
reported to the health table every second (3.12.4). The bearer credential
is read as for sinks (`sinks::transport`). The crate compiles the shared
sources `callback_server.rs`, `context_client.rs`, `seam_queue.rs` and
`taxonomy.rs` of `rustev-remote-http` by path, as it does the others.
Engineering choices made by the implementing agent where 3.10.2 names a
value the configuration document has no member for, not by the owner: the
transit margin is 25 ms; the principal reference is the caller's principal
handle unchanged, with mapping version `rustev-serve/principal-handle/1`
and scope `rustev-serve`; the describe budget is 10 seconds; a source is
described once at startup and not again. Test: the module's unit test
(a described source fetches through the client; an undescribed one is
unavailable).

**Increment 4 part 3, pushed invalidations (2026-10-29).**
`integrations/rustev-serve/src/invalidations.rs` serves
`POST /v1/context/invalidations` (admin group) for sources whose describe
declared `push` (3.10.6); the context client records each source's fields
and terms after describe (`Service::describe_context_invalidation`). Each
item is applied before the acknowledgement, for every registered plan
reading a named field: `invalidate_scope` for a named principal,
`invalidate_plan` for `all_scopes`; the three kinds alike. Items at or
below the last applied sequence are skipped. The first batch after startup
and any batch not contiguous with the last applied item first invalidate
every plan reading the source's fields; a gap is counted, marks the
source `invalidation_degraded` in readiness and metrics, and the next
contiguous batch clears it. An erasure applied while a response for its
scope is retained is counted (`rustev_context_erasures_while_retained_total`).
The acknowledgement names the highest sequence applied. A source that is
not configured is `context_unavailable`, one that does not declare `push`
`invalid_request`. `rustev.serve-service/1` now lists the operation. A
`poll` source is polled every `interval_ms` by `src/contexts.rs`: it is
`invalidation_degraded` until its baseline poll (no sequence) succeeds;
items apply as pushed ones do, and an answer whose sequence passes its
last item is a gap; a failed poll invalidates every plan reading the
source, counts one gap per outage and needs a fresh baseline; lag beyond
`max_lag_ms` invalidates and degrades likewise, and the lag is reported.
A polled principal reference is a principal handle only under this
service's mapping version; any other applies to every scope. Tests:
`pushed_invalidations_ack_in_order_and_report_gaps_and_erasures` in
`tests/handlers.rs` and
`contexts::tests::a_polled_source_sets_its_baseline_applies_items_and_degrades_on_failure`;
`tests/COVERAGE.md` updated.

**Increment 4 part 3, follow-up: one invalidation pass per poll tick
(2026-10-30).** In `integrations/rustev-serve/src/contexts.rs` a poll tick
now makes at most one `invalidation_lost` call. Before, a failed poll on
the tick where lag first passed `max_lag_ms` invalidated the source's plans
twice; the gap was counted once either way. Test:
`contexts::tests::a_tick_invalidates_at_most_once_even_when_failure_and_lag_coincide`.

**Per-decision parallel requests (2026-10-30).** As ratified in 3.5.4 and
3.15: `rustev.serve-config/1`'s `runtime` gains the optional
`max_parallel_requests` (`RuntimeConfig` in
`crates/rustev-contract/src/serve.rs`, 1 to 65,536, never null, omitted
from the canonical form when absent so such a configuration keeps its
digest), and `integrations/rustev-serve/src/service.rs` hands it to the
runtime, 1 when absent. The configuration's JSON Schema and the TypeScript
document types are regenerated. Test:
`parallel_requests_are_optional_bounded_and_absent_keeps_the_digest` in
`crates/rustev-contract/tests/042_serve_documents.rs`.

**Increment 6 part 1: explanation documents (2026-10-30).**
`crates/rustev-contract/src/serve.rs` defines `rustev.serve-explain/1`
(`ServeExplain`: `evidence`, `query`, `options`) and
`rustev.serve-explanation/1` (`ServeExplanation`: `explanation`, the
embedded `rustev.explanation/1` record, and `markdown`) under 3.8.3.
Evidence is `{ record, plan? }` or `{ bundle }`, each member a
`DocumentInput` (embedded bytes or a resolver reference) of
`rustev.run`, `rustev.plan` and the replay bundle. The query is spec 040
3.3's query or `{ diff: { other } }`. `options` holds the optional
`scope` and `now_ms`. The parser admits either member alone, and the
handler refuses a combination that does not match the bundle sides as
`invalid_request`, as 3.8.3 states. `ServeExplain::has_bundle` names
whether either side is a bundle. The answer parses under
`EXPLANATION_LIMITS` (33 MiB, strings up to 16 MiB): an explanation is
bounded as a replay bundle is, and its Markdown by the same byte count.
The operation table's `explain` entry now names the two schemas, and its
answer is a document. The JSON Schemas, the TypeScript document types and
the gRPC stub's operation comments are regenerated. Until the handler
lands, the service still answers `explain` as unavailable. Tests: the
round-trip contract and
`explain_requests_take_either_evidence_form_and_name_their_bundle_sides`
in `crates/rustev-contract/tests/042_serve_documents.rs`.

**Increment 5 part 1: shared conformance through the service (2026-10-29).**
`integrations/rustev-serve/tests/conformance/runner.rs` runs every `run`
golden of the suite's manifest (the four `actionable.run-*` cases and
`lodging.run`) through `serve-http` and `serve-grpc`, each over a
mutual-TLS TCP listener and a Unix socket (3.17.1). The plan is registered
as an unsigned bundle, the case's snapshot is served by a
`rustev.remote-context/1` source and the run record is delivered to a
`rustev.remote-sink/1` sink, both hosted by `rustev-remote-http`'s callback
server and reached through the service's `RemoteContextClient` and
`RemoteSink`. The judgment equals the stdout golden's, `plan_id` is
asserted, and the delivered record equals the record golden under its
normalizations. Per listener the HTTP and gRPC decision documents are
compared byte for byte, `run_record_digest` and `evidence` included
(3.17.2). `make conformance` runs the test (3.17.4). The remote sink now
delivers every run record schema the sink described rather than
`rustev.run/1` only, and the service's sinks accept a schema only when
every configured sink described it: an actionable plan's record is
`rustev.run/3`. A test seam, not API: `Service::with_clock` and
`sinks::standalone_with_clock` hand a caller's `Clock` to the runtime and
exist only under `cfg(test)`, so the runner is a unit-test module of the
crate (`src/lib.rs` includes it by path) rather than an integration test.
A non-default cargo feature would need the crate to dev-depend on itself
to reach its integration tests, an edge 001 3.4.4's check refuses. The
runner uses a manual clock. The service measures its own budget accounting
for a decision (the context fetch's elapsed time and timeout, and the
remaining deadline handed to the runtime) on that same clock rather than
on a separate real-time source, so under the manual clock a fetch takes
0 ms and the record's `deadline_ms` is the CLI's. The default clock
(`TokioClock`) keeps the earlier behavior.

**Increment 5 part 2: the serve surfaces and the runtime cases through the
service (2026-10-30).** The suite's manifest names the service as the
surfaces `serve-http` and `serve-grpc` (3.17.1): every `run` case the CLI
harvest writes (`crates/rustev-cli/tests/common/harvest.rs`) and every
runtime case (`tools/rustev-conformance/tests/runtime.rs`) carries both, and
`cases/manifest.json` and the case files are rebuilt with them; this change
adds the `extends` edge on the harvest. The runner
in `integrations/rustev-serve/tests/conformance/runner.rs` selects cases by
surface and runs each through the surfaces it names, over the mutual-TLS
TCP listener and the Unix socket. A runtime case binds the suite's scripted
backends (`rustev_conformance::runtime::Scripted`, now public, with
`result_line`) under their backend ids, the case's other descriptors inert,
so the service compiles the plan against the descriptors the case compiled
it against; the plan is registered with its declared execution policy and
calibrations. The service runs with the case's `max_in_flight`,
`max_queued` and `max_parallel_requests`, per-backend concurrency and
`fail_decision` sink timeout, on the manual clock. The case's steps drive
it: a start sends the decision request and waits until the runtime has the
decision in hand (admitted, queued or rejected), since the service fetches
the context first; a cancel is the service's cancel operation. Each
decision's answer maps to the case's result (the refusals `overloaded`,
`queue_deadline` and `cancelled_before_admission` are the rejections
`overloaded`, `queue_deadline` and `cancelled` the runtime case records,
spec 003's `Rejection` kinds), the delivered record is compared unnormalized, every decision
asserts `plan_id`, and per listener the HTTP and gRPC answers are
byte-identical. `runtime.cancel-stop` runs here too: the service's backends
are Rust seams polled in process, as on `rust-api`. Tests:
`every_runtime_case_through_both_transports_and_listeners`,
`every_run_golden_through_both_transports_and_listeners` and
`every_serve_case_is_a_run_or_a_runtime_case`.

**Increment 5 part 3: callback protocol conformance (2026-10-30).** 3.17.3
reads: "Protocol conformance for `rustev.remote-context/1` and
`rustev.remote-sink/1` runs against any endpoint (the Rust host server,
and a host's own endpoint by URL), with positive and negative cases:
timeouts at each phase, cancellation (`stopped`, `unconfirmed`, late
answer), oversized bodies both ways, idempotency windows (repeat,
conflict, expiry), unknown and unsupported versions, invalidation gaps,
and duplicate and conflicting deliveries." The owner decided (2026-10-30)
how it splits, without an amendment: by who is tested. URL mode runs every
case that checks the endpoint's own behavior (bounds, idempotency windows,
versions, invalidation gaps, duplicate and conflicting deliveries);
client-side cases (timeouts at each phase, late answers, oversized
responses, cancellation outcomes) run against a scripted test endpoint
inside the crate. `integrations/rustev-serve/tests/conformance/callback.rs`,
a submodule of the conformance runner (so `make conformance` runs it,
3.17.4, and both clients, the context client and the service's
`RemoteSink`, are in reach), holds both. The endpoint suite covers describe
and its unknown versions (refused) and a fetch or delivery naming version 2
(a capability failure), an unknown document version, a request above the
declared request bound (refused with a 4xx), every answer read within the
declared response bound and holding only the fields asked for, a foreign
describe digest (`describe_mismatch`), repeat (same bytes, dispatched once
where countable), conflict (`attempt_id_conflict`) and expiry of the
idempotency window, cancel of an unknown attempt (`unknown_attempt`) and of
a finished one (never `stopped`), the baseline poll and contiguous items
and a visible gap when items leave the endpoint's log, a delivery accepted
once with its digest and a described durability, then `duplicate`, then
`conflict` for another digest, a record above the record bound refused,
and the deduplication window's expiry. It runs against the Rust host
server twice (with a driver that publishes invalidations and counts
dispatches, and without, as a host endpoint by URL runs), and by URL with
`RUSTEV_CALLBACK_ENDPOINT` (and `RUSTEV_CALLBACK_BEARER_FILE`) in an
ignored test, whose preconditions the module states: windows of at most
5,000 ms and a record bound below the larger fixture record. Without a
driver the invalidation case checks the structure of the polls the
endpoint answers. The client suite, against a raw HTTP/1.1 scripted
endpoint, covers a wait for an in-flight slot (`transport_unsent`, nothing
sent), the first response byte under a phase ceiling and a stalled body
under the budget (`remote_deadline`), a budget spent by the transit margin
(nothing sent), a late answer discarded, counted and recorded with the
attempt cancelled at the endpoint (and no cancel under `none`), `stopped`
accepted only under confirmed cancellation, oversized bodies both ways
(`malformed_response`, and a request above the source's bound never sent),
an unknown describe version or a third-party class refused at
construction, and for the sink client a timeout after sending
(`may_have_received: true`, no receipt), a full queue
(`may_have_received: false`), a stalled or oversized answer, and an unknown
describe version. `RemoteSink::send` becomes crate-visible for these tests.

**Increment 5 part 2, follow-up: the runtime cases run on one thread
(2026-10-30).** `every_runtime_case_through_both_transports_and_listeners`
failed now and then under load: `runtime.retry` recorded
`explicit_deadline` as dispatched or ended at 10 ms rather than 0. On a
worker pool the driver could read the runtime's gauges in the middle of
the decision's poll, after one attempt answered and released its permit
and before the next was dispatched, and so advance the manual clock past
the retry delay early. The test now runs on a current-thread runtime, as
the suite's native runner (`tools/rustev-conformance/src/runtime.rs`)
drives the same cases, so the driver reads the gauges and the clock's
sleepers only between the decision's polls. The byte comparison is
unchanged.

**Increment 6 part 2: the explanation handler (2026-10-30).** `POST
/v1/explain` and gRPC `Explain` are served (3.8.3), and the service's
`available_endpoints` now lists `explain`. The request needs the
`rustev-principal` header as every scoped operation does. The handler
(`integrations/rustev-serve/src/offline.rs`) runs on the bounded blocking
pool and honors `rustev-deadline-ms` as replay and evaluation do (3.8.4).
The caller must have the scope the host resolves for its principal; with a
bundle side, `options.scope` must equal it, or the answer is
`decision_not_found` before anything is resolved; `scope` and `now_ms`
given without a bundle side, or missing with one, are `invalid_request`.
Each evidence member is its embedded bytes or a reference read under the
configured resolver roots as 3.8.2 reads one (an unresolvable reference
is `decision_not_found`, as for replay), handed to `rustev-surface`'s
explain as named byte inputs, with the configured roots as the resolver of
a bundle's external items and no resolver otherwise. The redaction policy
is the one the request's listener names, and none when it names none. The
answer carries the library's explanation bytes unchanged and its Markdown;
a spec 040 refusal is `invalid_document` with the refusal code (for
example `plan-mismatch`) in `detail`. The contract parsed `evidence` and a
`diff` query as untagged enums, which buffer their input, so an embedded
document (a raw value keeping its bytes) never parsed; both now parse by
member name, with the same accepted shapes and the same JSON Schema.
Tests: `every_explain_golden_through_both_transports` runs every explain
case of the shared suite that hands the library no resolver of its own
(the request carries none) through HTTP/2 JSON and gRPC: explanation and
Markdown bytes equal the goldens, the two redaction cases take their
policy from the listener, refusals map as above, and the transports answer
the same bytes; `explain_checks_scope_and_resolves_references_under_roots`
covers the scope mismatch, a referenced bundle equal to the embedded one,
a parent component and an absent file; and
`explain_evidence_embeds_documents_on_either_side_and_keeps_their_bytes`
in `crates/rustev-contract/tests/042_serve_documents.rs`. Section 5's
"Explain before D2 is implemented" row no longer applies: spec 040 is
implemented.

**Increment 7 part 1: the acquire documents (2026-10-30).** Spec 036 is
implemented, so the members 3.10.1 and 3.10.3 held back in increment 1
(remote half) are defined in `crates/rustev-contract/src/remote_context.rs`.
`rustev.remote-context-acquire/1` request: version, `source` (the
`SourceId`), `operation`, `request` (the `rustev.acquisition-request/1`
document as its canonical bytes), `request_id` (its
`AcquisitionRequestId`), the attempt id spec 036 assigns
(`<decision id>/<step id>/<tier>,<page>/<n>`, `r<n>` on a rerun, each
component escaped as for fetch), budget, trace id, `traceparent` and the
principal handle. Its parser refuses request bytes that are not canonical,
a `request_id` that is not their identity, and a `source` or `operation`
other than the request's. Response: `page` (the page document's bytes
exactly as sent) or `failure{code, detail}`, and `charge` (the run record's
`Charge`: observed, estimated or unknown). The failure codes are the
context codes and `denied`, and the exchange record's `code` admits
`denied` (`AcquireCode`). The response parser checks only that a page is a
JSON object: parsing it and spec 036 3.3.4's validation are the runtime's,
so an invalid page reaches the runtime and is classified there. The
response envelope is bounded at 4 MiB, as the fetch response is. A
described source may carry `descriptor`: `id` (its `SourceId`) and
`document` (its `rustev.source/1` bytes verbatim); describe refuses bytes
that are not canonical and an `id` that is not their identity. The
service's `step` event gains the `skipped` outcome of 3.7.1 (spec 036 3.8).
Each addition is optional or a new value, so no existing document's bytes
change. Tests in `tests/042_remote_documents.rs`:
`acquire_documents_round_trip_and_close_their_envelopes`,
`acquire_pages_keep_their_bytes_and_are_not_validated_here`,
`acquire_requests_match_their_embedded_request_and_attempt_form` and
`hosted_descriptors_are_identified_by_exactly_their_bytes`; the `skipped`
outcome in `tests/042_serve_documents.rs`.

**Increment 7 part 2: acquire over the callback protocol (2026-10-30).**
`integrations/rustev-remote-http/src/context_client.rs`: when describe
carries a source's descriptor, `RemoteContextClient::acquisition_source`
yields a `RemoteAcquisitionSource`, an ordinary `AcquisitionSource` whose
descriptor is exactly the described one (3.10.3, spec 036 3.12). An
attempt goes through the client's seam queue, so fetches and acquisitions
share its in-flight limit; a full queue is the `overloaded` class with
reason `remote_seam_queue_full` and nothing charged (3.12.1). It sends the
runtime's request bytes, request id and attempt id unchanged, with the
remaining budget less the transit margin, and hands the runtime the page
bytes and the reported charge as received. `denied` and `unauthorized` are
the `denied` class; the other codes take spec 009 3.6's classes, a remote
`cancelled` the caller did not ask for being transient. On the runtime's
cancel signal, an attempt whose bytes never left is `stopped` with nothing
charged; otherwise the endpoint is asked to cancel (unless its terms say
`none`) and only its `stopped` under confirmed cancellation is reported as
`stopped`, the rest `unconfirmed`. An answer after the caller stopped
waiting is discarded and recorded late, as for fetch. `cost_bound` is the
operation's declared `bounded` cost and otherwise `unknown`, because an
estimated or unknown cost model states no units (engineering choice).
`integrations/rustev-remote-http/src/callback_server.rs`:
`CallbackServer::host_acquisition` serves a hosted `AcquisitionSource` for
a hosted context source (one `SourceId` per server), describe carries its
descriptor, and `POST /rustev/remote-context/1/acquire` runs each attempt
id at most once inside the idempotency window under the fetch rules
(repeat answered from the record, other content `attempt_id_conflict`);
an unknown version, source or operation is `capability`. A hosted failure
is sent as `denied`, as the code its detail names when that code has the
failure's class, or as the class's code (`remote_error`, `overloaded`,
`capability`). Tests: `integrations/rustev-remote-http/tests/acquire.rs`.

**Increment 7 part 3: acquisitions in the service (2026-10-30).** 3.4.2
states: "Once spec 036 is implemented, a plan's declared acquisitions go
through the same source's `acquire` during the decision." The owner's
reading (no amendment): every configured context source whose
describe carries its `rustev.source/1` descriptor is installed as an
acquisition source service-wide, for every plan, and startup is refused when
two configured sources describe the same `SourceId`. A plan binds a source by
name and `SourceId`, and the runtime installs one source per name, so two
configured sources describing the same source name are refused at startup
too. `integrations/rustev-serve/src/contexts.rs` installs each described
client's `RemoteAcquisitionSource` (3.10.3) with the endpoint's
`max_in_flight` as its attempt bound; `Host` gains `acquisitions` (per
configured context source, an acquisition source and its bound), so an
embedding host supplies its own the same way. A registration compiles
against the descriptors of every installed source (3.3.2); a plan binds only
the sources its definition names, so the others change no `PlanId`. A plan
naming a source that serves no acquisitions is `compile_refused`. The
runtime's progress observer gains `skipped` (a defaulted method with the
step's compiled derivation, additive to `rustev-runtime`'s API), and the
service streams it as the `skipped` outcome of 3.7.1. Tests:
`a_guard_skipped_step_is_observed_as_skipped` in
`crates/rustev-runtime/tests/acquisition.rs`, and
`integrations/rustev-serve/tests/conformance/acquire.rs` (an acquiring plan
through HTTP/2 JSON and gRPC over TCP and Unix, byte-identical per listener,
the hosted source's `acquire` called once and the delivered record carrying
the acquisition; a guard-skipped acquisition streamed as `skipped` over both
transports; duplicate `SourceId` refused at startup; a source without a
descriptor installs none).

**Increment 8: spec 034 bundles and signing (2026-10-30).** Spec 034 is
implemented, so the members increment 1 held back for it are defined
(3.3.1, 3.3.3, 3.3.6, 3.3.8). `crates/rustev-contract/src/plan_bundle.rs`:
the bundle carries `profile` (the leaf `rustev.profile/1`, its bytes kept),
`variant` (`{"named": <name>}`) and `profile_bases` (1 to 64 profiles, only
with a profile), each omitted when unused; the registration record carries
`profile_id`, the leaf's `ProfileId`, omitted for none; `signing_mode`
admits `deny_unsigned_for_profiles` with 1 to 1 024 `ProfileId`s
(engineering bound: an empty list would be `allow_unsigned` by another
name). Absence is the only spelling of no profile. 3.3.1 states "a tenant
profile or `none`", 3.3.3 states "profile id or `none`", and 3.3.8 states
"Both are omitted when unused, so a bundle without them keeps its canonical
bytes, bundle digest and `RegistrationId`. Absence is the only spelling of
unused." The owner's reading (no amendment): `none` is spelled by omission
for `profile` and `profile_id` too, so `"profile": "none"` is
`invalid_document`, and existing bundles and `RegistrationId`s are
unchanged. The bundle's definition is a definition
document or a spec 034 template (a `rustev.definition/2` document declaring
`parameters` or `variants`), carried as its bytes; anything else is
refused as before. A compile refusal document may also be the
instantiation refusal `rustev-surface` renders (stage `instantiate` and the
034 3.9 code) or a compile refusal of an instantiated template (stage
`compile` and its parameters). `integrations/rustev-serve/src/service.rs`
compiles a concrete definition with no profile or variant as before and
otherwise instantiates with exactly the bundle's inputs and no version pin
(`compile_profiled_with`). Under `deny_unsigned_for_profiles` an unsigned
bundle whose leaf is listed is `unsigned_refused` before compilation, and
one whose resolved chain (the plan's instantiation evidence) holds a listed
profile after it. `profile_unsupported` is no longer produced. Tests:
`bundles_carry_profiles_variants_and_bases_and_omit_them_when_unused` in
`crates/rustev-contract/tests/042_serve_documents.rs`, and
`integrations/rustev-serve/tests/profiles.rs`: the section 5 rows
(`variant-required`, `profile-missing`, bases without a profile, `"variant":
"none"` and an empty `profile_bases`, a listed base under
`deny_unsigned_for_profiles`, an unreached base profile's
`registration_conflict`), the leaf `ProfileId` in the record, and a
listed leaf.

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
12. **`acquire` follows spec 036.** `acquire` carries spec 036's approved
    documents (`rustev.source/1`, `rustev.acquisition-request/1`,
    `rustev.acquisition-page/1`, A-33) verbatim, and spec 036 is in
    `depends_on`; `acquire` is implemented once spec 036 is. Every other
    approved spec this one cites is in `depends_on`; specs 037 and 040 are
    not yet on `main`, are cited only for features served once they are
    implemented, and join `depends_on` when this spec is rebased onto them.
13. **No loopback exemption.** Loopback is not a single-process trust
    boundary, so every TCP listener needs TLS and client certificates, and
    the development bearer is allowed on Unix sockets only. Conformance
    runs over loopback with test certificates.
14. **Explanation endpoint.** Spec 040's requirement on A2 (same library
    function, canonical bytes unchanged, host redaction, no cross-scope
    resolution) is adopted in 3.8.3.

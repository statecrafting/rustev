---
id: "009-remote-adapter-protocol"
title: "Remote adapter protocol"
status: draft
implementation: pending
created: "2026-09-24"
summary: >
  Increment 3 (design roadmap 17.1, ordinal 009): what every decision backend
  reached across a network must guarantee, and a versioned, transport-neutral
  protocol (`rustev.remote/1`) for backends served by another process. Covers
  capability description and negotiation at setup, attempt identity and
  idempotency, one total time budget per attempt, cancellation that never
  claims more than the remote side confirmed, validation of responses into
  `RawOutput` without inventing mass, a closed error taxonomy mapped onto spec
  003's failure classes, explicit served-model identity (unknown is a value,
  never an omission), cost reporting, retained exchange records for audit and
  replay, and privacy options passed through and recorded. Protocol messages
  live in `rustev-contract`; every network implementation lives under
  `integrations/`. Amends no approved behavior. Draft: claims no code.
extends:
  # The protocol documents are a new, additive module of the contract crate
  # (spec 001, 3.4.3: remote protocol messages live in rustev-contract).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: additive }
  # Adds the integrations/ member glob and the HTTP stack as a workspace
  # dependency usable only under integrations/ (spec 001, 3.4.2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds integration manifests to the code targets and 009 to `make verify`.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A remote adapter never grants authority: nothing a remote party returns is authorization, changes a plan, policy, threshold, retry or fallback, or is supplied to the core as anything but a raw output or a classified failure."
    anchor: "3-1-authority-and-provenance"
  - id: "I-2"
    kind: invariant
    text: "Served-model identity is always explicit: an exchange record states a served identity or `unknown`, and never copies the requested model into the served field."
    anchor: "3-7-model-and-served-identity"
  - id: "I-3"
    kind: invariant
    text: "Retries and fallback happen only as the plan's execution policy declares them: an adapter never resubmits an attempt, never retries internally and never substitutes another model, provider or backend."
    anchor: "3-3-attempt-identity-and-idempotency"
  - id: "I-4"
    kind: invariant
    text: "An adapter never manufactures mass: it does not renormalize, fill a missing option, convert a label or a score into a distribution, or pass a provider-reported confidence as a probability."
    anchor: "3-5-response-validation-into-rawoutput"
  - id: "I-5"
    kind: invariant
    text: "One attempt has one total time budget, the remaining time at dispatch; no connect, write, read or remote timeout extends beyond it."
    anchor: "3-4-one-total-budget-and-cancellation"
  - id: "I-6"
    kind: invariant
    text: "An adapter acknowledges `stopped` only when it can establish that no request byte left the process or the remote side confirmed the stop; every other ended-early attempt is unconfirmed and its charge is liability."
    anchor: "3-4-one-total-budget-and-cancellation"
  - id: "I-7"
    kind: invariant
    text: "`rustev-contract` and `rustev-core` carry no vendor, model or provider name and no HTTP dependency; protocol messages are vendor-neutral."
    anchor: "2-territory"
  - id: "R-1"
    kind: requirement
    text: "Capabilities are negotiated once, at setup, before any plan is compiled against the adapter; a descriptor that changes afterwards fails attempts as `identity_mismatch` and is never adopted silently."
    anchor: "3-2-capability-description-and-negotiation"
  - id: "R-2"
    kind: requirement
    text: "Every remote failure is one code of the closed taxonomy of 3.6, mapped to exactly one spec 003 failure class."
    anchor: "3-6-error-taxonomy"
  - id: "R-3"
    kind: requirement
    text: "Every exchange produces a bounded exchange record keyed by attempt id, retained under the host's policy with digest-only content by default."
    anchor: "3-9-exchange-records-and-replay"
---

# 009: Remote adapter protocol

Draft: a proposal, not a claim about code. The ordinal is the one design
roadmap 17.1 reserves for this subject. Rationale: design sections 8, 9, 15
and 17 (not contract); founding decision D-06 (traits plus a versioned
protocol for remote adapters); owner decisions R-05, R-08, R-10, R-16, R-19,
R-20; and R-26 (Rustev as the decision engine of the travel-memory product,
with a remote backend), all in `docs/decisions/00-founding-decisions.md`.
The first consumer is spec 012 (the Jev integration), which conforms to Part
A of this spec while speaking its provider's own wire format.

This spec **amends no approved spec**. Everything here is expressible through
the seams and records spec 003 already approved (`DecisionBackend`,
`AttemptCall`, `AttemptReport`, `AdapterFailure`, `CancelAck`, `Charge`,
`RemoteState`). Where that vocabulary is too narrow, section 5 names the gap
and says what an amendment would change; the one the owner chose to pursue
is a separate amendment of spec 003, proposed as draft spec 013 in its own change.

## 1. Purpose

Let a plan bind a backend that runs in another process or with another
provider, with the same guarantees an in-process backend gives: declared
capabilities, no invented values, honest cost and cancellation, identified
producers and replayable evidence. A network makes three things uncertain
that are certain in process: whether the remote side is still working,
what it charged, and which model actually answered. This spec makes each
of them explicit instead of assumed.

## 2. Territory

Nothing is claimed while this spec is a draft. When it is made concrete it
claims:

- `crates/rustev-contract/src/remote.rs` (an additive module of spec 002's
  crate, 3.10): the `rustev.remote/1` documents and their parse limits.
  Serde types only; no transport, no async runtime, no vendor name.
- `integrations/rustev-remote-http/`: the HTTP binding (3.11), both a client
  that implements `DecisionBackend` over the protocol and a server that
  hosts any `DecisionBackend` behind it. It is the only crate this spec adds
  that may depend on an HTTP stack or Tokio (spec 001, 3.4.2).
- Shared adapter machinery used by foreign-wire adapters (spec 012) lives in
  the same integration crate or a sibling under `integrations/`; no crate
  outside `integrations/` depends on it (spec 001, 3.4.4).

`rustev-core` gains nothing: the seams of spec 003 3.2.6 are sufficient.

## 3. Behavior

Part A (3.1 to 3.9) binds every **remote adapter**: any `DecisionBackend`
whose `infer` sends bytes over a network, whatever the wire format. Part B
(3.10, 3.11) is the `rustev.remote/1` wire protocol, one conforming way to
reach a backend in another process.

### 3.1 Authority and provenance

1. A remote adapter is a decision backend and nothing more. Its outputs are
   `model-derived` (spec 001, 3.3.1) whether or not a model produced them.
   Nothing a remote party returns (metadata, headers, provider extras,
   confidence, reasoning text if any) is authorization, becomes an input
   fact, alters a threshold or policy parameter (R-14), or changes retries,
   fallback or budgets.
2. The adapter supplies the runtime exactly one of: a `RawOutput`, or a
   classified failure (3.6). Everything else it learns goes to the exchange
   record (3.9), which no policy reads.
3. The principal handle of `CallContext` is opaque and is not sent to a
   remote party unless the host's adapter configuration explicitly enables
   it for an endpoint the host operates. It is never sent to a third-party
   provider.

### 3.2 Capability description and negotiation

1. **At setup, never during inference.** The host constructs the adapter
   explicitly (as spec 005 3.11 does for the rules backend); construction
   negotiates the protocol version and obtains the remote capability
   description. Plans are compiled against the descriptor the adapter then
   returns from `descriptor()`. The runtime's descriptor check (spec 003,
   3.6.5) applies unchanged.
2. **Descriptor.** A remote adapter's `descriptor()` is an ordinary
   `rustev.backend/1` document: operations with output kind and
   `max_options`, input limit in canonical projection bytes with
   `on_excess: refuse`, and determinism. A remote backend declares
   `unspecified` unless it can prove more (design section 14). It never
   declares an operation it would satisfy by composing others (a `rank`
   built from per-candidate scores, for example); composition belongs in
   the plan.
3. **Artifact identity.** The adapter's `ArtifactId` is the contract digest
   (spec 002, 3.3.2) of an adapter binding document that holds every
   output-affecting input: protocol version, endpoint class (not a secret
   URL or key), the remote backend's own artifact identity when it reports
   one, the requested model identifier and any version pin, the question
   mapping version and any mapping tables, privacy options that can change
   routing (3.8), and request limits. Spec 004 3.4.3 requires this: hidden
   provider context would make request identity unsound. Credentials,
   network addresses, concurrency and cost rates are configuration, not
   identity.
4. **Remote terms.** Beside the descriptor, negotiation yields terms the
   runtime does not read but the adapter enforces and records: cost model
   (3.8), cancellation support (`none`, `best_effort`, `confirmed`),
   served-identity disclosure (`pinned`, `reported`, `unknown`), shared
   state batching (`none` or `max_items`), maximum request and response
   bytes, and supported privacy options.
5. **Drift.** If a later exchange reveals a different remote artifact,
   descriptor or pinned served identity than the one negotiated, the
   attempt fails `identity_mismatch` (3.6) and its output is never
   supplied. The adapter does not renegotiate itself; the host rebuilds
   it, which changes the descriptor identity and so the `PlanId` of any
   plan compiled against it.

### 3.3 Attempt identity and idempotency

1. The attempt id of spec 003 3.9.2 is the idempotency key. A remote
   adapter makes at most one submission per attempt id. It never resubmits,
   retries on its own, follows a redirect to another provider, or falls
   back to another model; retries and fallback are the runtime's, only as
   the execution policy declares them (R-10), and a runtime retry is a new
   attempt with a new id.
2. A protocol endpoint (3.10) that receives an attempt id again within its
   declared idempotency window returns the recorded result, or
   `in_progress`, for identical content, and `malformed_request` with
   `attempt_id_conflict` for different content. It never runs the attempt
   twice.
3. A foreign provider without idempotency keys gets no attempt id by
   default: decision ids are caller-supplied and may carry information the
   host does not want to disclose. The exchange record joins the provider's
   own generation id to the attempt id locally.

### 3.4 One total budget and cancellation

1. **One budget.** An attempt's budget is `CallContext::remaining_ms` at
   dispatch. Every transport phase (resolution, connect, handshake, write,
   wait, read) draws from that one budget; the adapter sets no independent
   phase timeout that could outlive it. The remote side is told the budget
   minus a declared transit margin, and a conforming endpoint does not start
   work after it expires. The runtime's own deadline and attempt timeout
   still apply and raise the attempt's cancellation signal (spec 003, 3.7).
2. **Timeouts belong to the runtime.** An adapter does not report a timeout
   class of its own. When the budget runs out the runtime has raised the
   signal; the adapter answers as for cancellation (3 below). A remote
   party that reports it gave up is `remote_deadline` (3.6).
3. **Cancellation.** When the signal is raised the adapter stops waiting,
   sends a cancel message if the remote terms include one, and reports
   `cancelled` with an acknowledgement:
   - `stopped` only if no request byte left the process, or the remote side
     confirmed the stop and its final charge within the budget;
   - `unconfirmed` otherwise, with charge `unknown` unless a final charge was
     reported, so the runtime keeps the reservation as liability
     (spec 003, 3.5.4).
   `best_effort` cancellation never yields `stopped`.
4. **Possibly continuing.** Spec 003 records `possibly_continuing` for an
   attempt whose cancellation was not confirmed. An adapter therefore never
   converts "I stopped waiting" into "it stopped". A response that arrives
   after the runtime stopped waiting is never supplied (spec 003, 3.3.4);
   the adapter records it in the exchange record and, when it carries a
   final charge, offers it to the host for reconciliation of the shared
   ledger (spec 003, 3.5.5).

### 3.5 Response validation into RawOutput

1. **Bounded parse.** A response is read up to the negotiated maximum
   response bytes and parsed under declared depth, string, collection and
   number bounds before any typed construction (as spec 002 3.2 does for
   documents). An oversized or unparseable body is `malformed_response`.
2. **Echo checks.** The response must name the attempt ids it answers (or,
   for a foreign wire, the question keys the adapter sent), and each at most
   once. A missing, extra or duplicated answer is `malformed_response` for
   the affected attempts.
3. **Mapping, not judging.** A well-formed answer is converted to the
   `RawOutput` variant the descriptor declares for that operation, keys
   taken from the answer and floats as received. The adapter does not
   renormalize, fill a missing option, drop an extra key, clip values, or
   convert between kinds. Whether the mapped output is valid (exact keys,
   finite values, sum within tolerance) is the core's judgment through the
   pre-supply check (spec 003, 3.2.4), which reports `invalid_output`; the
   adapter does not pre-empt it.
4. **Lossless reshaping only.** A reshaping is allowed only when it loses
   and adds nothing, and is part of the mapping version in the artifact
   identity. Example: a probability `p` for a two-outcome proposition
   becomes the distribution `{false: 1 - p, true: p}`; the sum is `1`
   within binary64 rounding, which the plan's tolerance covers.
5. **Provider extras.** A provider-reported argmax, confidence, expected
   score or explanation is retained in the exchange record as
   provider-reported, and is never supplied, never treated as a calibrated
   probability, and never used to repair an output.

### 3.6 Error taxonomy

Closed. Each code maps to exactly one spec 003 failure class, and the
adapter's failure detail begins `remote:<code>` so the run record carries
the code within its existing 256-byte detail (spec 003, 3.9.3).

| Code | When | `AdapterFailure` | Charge reported |
|---|---|---|---|
| `transport_unsent` | resolution, connect or handshake failed; no request byte left the process | `transient` | `observed{0}` |
| `transport_interrupted` | the connection failed after request bytes were sent and before a complete response | `transient` | `unknown` |
| `rate_limited` | the remote side refused for rate (HTTP 429 or equivalent); any retry-after is recorded, never slept on | `overloaded` | as reported, else `observed{0}` |
| `overloaded` | the remote side refused for load (HTTP 503, 529 or equivalent) | `overloaded` | as reported, else `observed{0}` |
| `unauthorized` | credentials refused (401, 403) | `permanent` | as reported, else `observed{0}` |
| `capability` | protocol version, operation, option count, model or input size not supported | `permanent` | as reported, else `observed{0}` |
| `malformed_request` | the remote side rejected the request as invalid (400, 422), including `attempt_id_conflict` | `permanent` | as reported, else `observed{0}` |
| `malformed_response` | 3.5.1 or 3.5.2 failed | `permanent` | as reported, else `unknown` |
| `identity_mismatch` | 3.2.5, or a pinned served identity differs (3.7) | `permanent` | as reported, else `unknown` |
| `remote_deadline` | the remote side reports it stopped for its budget | `transient` | as reported, else `unknown` |
| `remote_error` | any other remote failure (5xx) | `transient` | as reported, else `unknown` |
| `cancelled` | 3.4.3 | `cancelled` | 3.4.3 |

Whether a class is retried or falls back is the execution policy's
decision (spec 003, 3.6). A 4xx status the table does not name is
`malformed_request`; a status the adapter cannot classify is `remote_error`.
`observed{0}` for a refusal is permitted only when the remote side
documents that refused requests are not charged; otherwise `unknown`.

### 3.7 Model and served identity

Every exchange record carries:

- `requested_model`: exactly what the adapter asked for;
- `served`: `{identity, disclosure}` where `disclosure` is `pinned` (the
  remote side guarantees the identity), `reported` (the remote side said so;
  unverified), or the explicit value `unknown`. `unknown` is never replaced
  by the requested model or by a routing slug;
- `route`: the remote side's routing metadata as reported (provider chain,
  resolved provider, canonical slug), as opaque strings;
- `generation_id`: the remote side's identifier for the computation, or
  `none`.

When the negotiated disclosure is `pinned` and the served identity differs,
the attempt fails `identity_mismatch`. When it is `reported` or `unknown`,
a change of served identity cannot be detected per call. That limitation
is stated wherever results are reported (spec 004 reports, qualification
records), and calibration bound to such an artifact (spec 002, 3.6) is
bound to the adapter binding, not to an unverified model version.

### 3.8 Cost and privacy

1. **Cost.** The adapter's `cost_model` is `bounded` only when the remote
   side enforces a per-call maximum it declared; otherwise `estimated`
   (a price table and a size estimate) or `unknown`. Under a `hard` cost
   policy spec 003 3.5.3 therefore refuses plans that reach a non-bounded
   remote adapter; that is intended. A charge is `observed` only from a cost
   the remote side reports for that exchange, converted to the deployment's
   units by a declared rate and rounded up (spec 003, 3.5.1); `estimated`
   from reported usage and the declared price table; `unknown` otherwise.
   The reported amount and currency are kept verbatim in the exchange
   record. Units are not money.
2. **Privacy options.** Options the remote terms support (for example: no
   provider-side retention, a provider allowlist) are configured by the
   host, sent on every request, and recorded in the exchange record as
   *requested*. An adapter never claims they were honored; it cannot
   observe that. Options that can change routing, and therefore output, are
   in the artifact identity (3.2.3).
3. **What leaves the process.** Only the projection (or the parts a
   foreign mapping sends; spec 012) and the question material. The host is
   responsible for projecting only fields it may disclose to that remote
   party; a plan's `project` list is the disclosure list, and it is visible
   in the plan.

### 3.9 Exchange records and replay

1. **Record.** Each exchange yields a `rustev.remote-exchange/1` record,
   keyed by the attempt ids it served: protocol and adapter binding
   identity, request digest, response digest, the outcome per attempt
   (output, code), 3.7's identity fields, usage and cost as reported, the
   charge supplied to the runtime, privacy options requested, provider
   extras (3.5.5), and, for a batched exchange, every member attempt id and
   how the charge was attributed (3.10.4). Free text is cut to 256 bytes;
   the record has a fixed maximum size.
2. **Delivery.** The adapter hands records to a host-supplied exchange sink
   injected at construction. The run record is unchanged (spec 004, 5.8):
   the join is the attempt id, which the run record already carries. A sink
   failure never changes the attempt's result; it is counted, as spec 003
   3.9.7 counts its own.
3. **Retention.** Host-owned, under R-19: digest-only by default. Request
   and response bytes are retained only under explicit retention and never
   outlive the replay bundle they support.
4. **Replay.** Spec 004 reproduces from the retained `RawOutput`, not from
   the wire; replay makes zero remote calls (spec 004, 3.3.5), and live
   re-execution stays deferred (R-20). When response bytes are retained,
   re-running the adapter's mapping over them offline must reproduce the
   supplied `RawOutput` byte for byte; this checks the mapping, not the
   model.

### 3.10 The rustev.remote/1 documents

Serde types in `rustev-contract`, parsed under declared limits, with
`deny_unknown_fields` and exact schema strings:

1. `rustev.remote-describe/1` request: the protocol major versions the
   client supports. Response: the chosen version and, per hosted backend,
   its `rustev.backend/1` descriptor and `DescriptorId`, and the remote terms
   of 3.2.4, including the idempotency window.
2. `rustev.remote-infer/1` request: protocol version, backend id,
   `DescriptorId`, artifact, budget in milliseconds, trace id, privacy
   options, and one or more items `{attempt_id, projection}` with the
   projection as its exact canonical bytes. Response: per item either
   `output` (a `RawOutput`, the `rustev.backend-output/1` vocabulary) or
   `failure{code, detail}`; the identity fields of 3.7; the charge; and
   provider extras.
3. `rustev.remote-cancel/1`: attempt ids; the answer per id is `stopped`
   with a final charge, `unconfirmed`, or `unknown_attempt`.
4. **Shared-state batching.** An infer request may carry several items only
   when the terms declare `shared_state{max_items}`, and only for attempts
   of the same decision whose projections share byte-identical `values` and
   `instance` members (the state), differing only in `operation`, `task`,
   `question`, `options` and `candidates`. This is the batching increment
   R-08 deferred, scoped to one decision and one remote adapter (accepted
   as such by R-28), and it satisfies spec 003 3.10 for that scope. It is
   off by default, and an adapter enables it only after the batched path is
   qualified for that adapter:
   - equivalence: items are never merged or deduplicated; each keeps its own
     attempt id, projection and output (spec 003, 3.10.1);
   - waiters: each item keeps its own cancellation; cancelling one sends a
     cancel for that item only if the terms support per-item cancel, and the
     exchange is abandoned only when no member still waits (spec 003, 3.10.2);
   - cost: the exchange charge is attributed to the members that receive the
     response in attempt-id order, with integer shares summing exactly to
     the rounded-up total; a member that stopped waiting reports `unknown`,
     so its reservation stays liability until the host reconciles it from
     the exchange record (spec 003, 3.10.2). Liability can therefore exceed
     the true unattributed remainder until reconciled; it is never below
     it (accepted by R-28);
   - isolation: one decision only, so one ledger and one authorized scope;
   - evidence: the exchange record lists every member (spec 003, 3.10.4).
   The adapter may hold a dispatched attempt for a declared coalescing
   window, at most a declared number of milliseconds and never past the
   budget, to gather siblings; the window is configuration, recorded in the
   exchange record. No cache, cross-decision sharing or duplicate
   suppression is introduced; those stay deferred.

### 3.11 HTTP binding

`integrations/rustev-remote-http/`: `POST` of the three documents to
`/rustev/remote/1/describe`, `/infer` and `/cancel`, JSON bodies as the
documents' canonical bytes, bearer credentials supplied by the host, TLS
required except for loopback. HTTP status codes map to 3.6. The server side
hosts any `DecisionBackend`, forwards the attempt id and budget, and maps
the backend's `AttemptReport` to the response one to one. The client side
implements `DecisionBackend` and Part A. Neither side exposes any other
provider's wire format; in particular Rustev offers no Jev-compatible
endpoint (R-05, design section 19).

## 4. Out of scope

Live re-execution (R-20); caches, duplicate suppression and cross-decision
batching (spec 003, 3.10); streaming responses; discovery or dynamic loading
of remote backends (D-06); authentication schemes beyond host-supplied
bearer credentials; any specific provider (spec 012 for Jev); a
Rustev-hosted service or deployment (spec 010 and the owner's release
authority).

## 5. Limits of the approved vocabulary

Recorded so they are not hidden. This spec amends nothing; the gap in 1 is
proposed as a separately reviewable amendment (R-16, R-28).

1. **Remote state on failure.** Spec 003 derives `possibly_continuing` from
   cancellation only. A `transport_interrupted` failure, where request bytes
   were sent and no response came, is recorded as `finished` although remote
   work may continue. The unknown charge keeps the reservation as liability,
   so cost stays honest; the remote-state field does not. Draft spec 013
   proposes the correction as an `amends` change to spec 003 (R-28). Until
   it is approved and delivered, this limitation holds.
2. **Structured remote evidence in the run record.** The code travels in
   the failure detail and everything else in the exchange record (3.9). A
   typed member of `rustev.run/1` would need a new run schema through an
   `amends` change to specs 003 and 004. R-28 keeps the separate sink now
   and defers `rustev.run/2` to a later change.
3. **Runtime-planned batching.** 3.10.4 batches inside the adapter with a
   coalescing window. A runtime that hands the adapter every sibling at once
   (no window) needs a batch-aware seam, which is an `amends` change to
   spec 003 3.2.6.

## 6. Observable negative cases

| Case | Expected |
|---|---|
| The remote side returns a distribution missing one declared option | The adapter supplies it unchanged; the core's pre-supply check records `invalid_output`. |
| A response carries only an argmax label for a `distribution` binding | `malformed_response`; no distribution is constructed. |
| A response names a served model while disclosure is `unknown` | Recorded as `reported` only if the terms say the field is a served identity; never promoted to `pinned`. |
| The served identity differs from a pinned one | `identity_mismatch`, output not supplied. |
| The signal is raised after the request was written, with `best_effort` cancel | Ack `unconfirmed`, charge `unknown`, run record `possibly_continuing`. |
| The signal is raised before any byte was sent | Ack `stopped`, charge `observed{0}`. |
| The same attempt id arrives twice at a protocol endpoint with different content | `malformed_request` (`attempt_id_conflict`); nothing runs twice. |
| An HTTP 429 with `Retry-After: 30` | `overloaded` with the value recorded; the adapter does not wait. |
| A transport timeout configured longer than the remaining budget | Refused at construction. |
| A crate outside `integrations/` depends on the HTTP binding | The boundary check fails (spec 001, 3.5). |

## Acceptance (planned)

- The protocol documents round-trip through their canonical bytes, refuse
  unknown fields and schemas, and enforce their parse limits.
- A loopback conformance suite runs the HTTP client against the HTTP server
  hosting spec 005's rules backend and scripted backends (spec 003, 3.12),
  covering every row of 3.6 and section 6, batching with a cancelled member,
  late responses, and reconciliation offers. All fixtures are labeled
  synthetic; no third-party endpoint is contacted (R-17, R-23).
- Both reference plans run through the runtime over the loopback binding and
  produce the same judgments as with the in-process rules backend; replay
  from captured bundles reproduces them with zero remote calls.
- The boundary check passes with the new crate; `rustev-contract` and
  `rustev-core` gain no HTTP or async dependency.

## Verification

Planned; not run until this spec is made concrete, approved and added to
`make verify`.

```verify:cli
# 3.10: protocol documents, limits and canonical bytes.
cargo test -p rustev-contract --locked remote
# 3.1 to 3.11: conformance over loopback against rules and scripted backends.
cargo test -p rustev-remote-http --locked
# 2, I-7: dependency rules; no HTTP stack or vendor name below integrations/.
cargo run -p rustev-boundaries --locked --quiet
sh -c 'if grep -rniE "jev|typesafe|vercel|openai|anthropic" crates/rustev-contract/src crates/rustev-core/src; then exit 1; fi'
cargo clippy -p rustev-remote-http --all-targets --locked -- -D warnings
```

## Resolved questions

Decided by the owner on 2026-09-24 (R-28):

1. Adapter-scoped batching (3.10.4) is the R-08 optimization step for
   remote adapters, off by default until qualified.
2. The batched cost attribution (shares to members that received the
   response, liability for the rest until reconciled) is accepted.
3. The remote-state gap (5.1) goes to a separate amendment of spec 003,
   draft spec 013.
4. Exchange records go to a separate host sink now (3.9); a `rustev.run/2`
   member is a later change (5.2).

## Open questions

1. The default coalescing window and `max_items` for batching, to be set
   from qualification measurements.

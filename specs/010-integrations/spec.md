---
id: "010-integrations"
title: "Optional ecosystem integrations and serving boundary"
status: approved
implementation: in-progress
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
  - { kind: directory, path: "integrations/rustev-aicortex/" }
  - { kind: directory, path: "integrations/rustev-rahi/" }
  - { kind: directory, path: "integrations/rustev-serve/" }
extends:
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  # Each implemented crate is named in the release set's exclusions; none is
  # in the release family.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
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

Run by `make verify` (010 is in `VERIFIED_SPECS`). The implemented subset
is `rustev-rahi` and `rustev-aicortex` (see the implementation record);
`rustev-serve` is not implemented, because its record refuses. No step
needs a network beyond loopback or a live service: each Rahi store runs in
the test process. Cargo fetches the pinned Aicortex revision like any
locked dependency.

```verify:cli
# 3.6 and the rustev-rahi record: every failure row against a scripted
# store, and accepted, duplicate, conflict, erased and refused deliveries
# against a single-voter Rahi 0.7.1 store in a temporary directory.
cargo test -p rustev-rahi --locked
cargo clippy -p rustev-rahi --all-targets --locked -- -D warnings
# I-2, I-4: the integration builds alone, and no crate outside
# integrations/ reaches Rahi or Aicortex.
cargo build -p rustev-rahi --locked
# 3.5 and the rustev-aicortex record: the bounded projection, refusals and
# proposal staging against a single-voter Rahi 0.6.0 store with Aicortex's
# schema in a temporary directory.
cargo test -p rustev-aicortex --locked
cargo clippy -p rustev-aicortex --all-targets --locked -- -D warnings
cargo build -p rustev-aicortex --locked
sh -c 'for p in rustev-contract rustev-core rustev-runtime rustev-eval rustev-surface rustev-cli; do t=$(cargo tree -p "$p" -e normal,build --locked --prefix none) || exit 1; if printf "%s\n" "$t" | grep -E "^(rahi|aicortex)"; then exit 1; fi; done'
cargo run -p rustev-boundaries --locked --quiet
# 3.1.1: no authorization type in the integration.
sh -c 'if grep -rnE "(struct|enum|trait|type)[[:space:]]+(Permitted|Grant)([^A-Za-z0-9_]|$)" integrations/rustev-rahi integrations/rustev-aicortex; then exit 1; fi'
```

## Implementation record

Implementation follows the binding records below: a crate is implemented
only when its record passes every row (3.4.8).

- Increment 1, `integrations/rustev-rahi/`: `RahiEvidenceSink` over
  `rahi-store =0.7.1` receipts, the `rustev` migration set (one table of
  record canonical bytes beside each receipt) and `stage_erasure`. Delivery
  outcomes are a closed `Delivery` (accepted, duplicate, conflict,
  rejected, unavailable, acceptance unknown) with the effect of each
  stated; through the `EvidenceSink` seam an unknown acceptance gives no
  answer, so the runtime's delivery timeout reports it as possibly
  received. Receipts are `rahi:<durability>:<external_id>:<digest>` and
  `rahi:duplicate:<external_id>:<digest>`, with durability
  `raft-log-async` or `raft-log-synced` as the host's store configuration
  applies, so the external record identity (3.6.2) reaches the run record
  through the seam. A `Changed` classification whose head holds no
  accepted digest (Rahi reports the empty string) gives a conflict with no
  stored digest (`stored: None`), never `sha256:`. No hosting
  adapter. Not in the release family (`tools/rustev-release/release-set.json`
  excludes it). Section 5 rows covered: the Rahi timeout and digest
  conflict rows, and the boundary rows through `rustev-boundaries`.
- `tools/rustev-boundaries/`, under spec 001 3.5 as amended by R-45
  (approved by the owner on 2026-10-21): `cargo metadata` resolves features
  once for the whole workspace, so Rahi's graph switches on optional edges
  of shared crates (`crypto-common` to `rand_core` to `getrandom`) that no
  pure crate's own build takes, and the transitive rules of spec 001
  3.4.1 and spec 027 3.14.3 reported paths that do not exist in those
  builds. A transitive violation now stands only when the crate's own
  resolution (`cargo tree -p <crate> -e normal,build`, every target; the
  WebAssembly target for `rustev-wasm`) also holds a forbidden package; a
  failure to read that graph keeps the violation. Every other rule is
  unchanged, and the new rule has a unit test with its negative control.
- Increment 2, `integrations/rustev-aicortex/`: `AicortexContext` (a
  bounded read of one claim-history page per bound subject under an
  explicit transaction bound, mapped through a declared projection of 1 to
  64 field bindings into attributed snapshot entries, with refusals and
  missing fields named) and `stage_observation` (an observation proposal
  with its Rustev lineage staged into the host's `TxnBuilder`, never
  committed). It pins Aicortex at the record's revision and Rahi 0.6.0,
  which that revision pins, so one host cell cannot link it and
  `rustev-rahi` (record item 1's limitation). Not in the release family
  (`tools/rustev-release/release-set.json` excludes it). Implementing it
  first showed that its Git dependency failed the release tool's manifest
  test, which then judged every package in the workspace resolution; the
  owner chose to scope that rule to the release closure (spec 021 3.4,
  R-49), and the record's item 1 passes since that change.
- `Makefile` (additive): `010` joins `VERIFIED_SPECS`.

**`rustev-serve` increment 1 (2026-10-22, under the approved record,
A-47).** `integrations/rustev-serve/` (`publish = false`) implements 3.7
as spec 042 makes it concrete, over HTTP/1.1 and HTTP/2: TLS 1.3 with the
record's three `ring` suites, client certificates required on TCP
listeners, Unix sockets created `0600` (or `0660`) and never over an
existing path (`src/transport.rs`); the transport-neutral handler layer
(`src/service.rs`) with host-registered plans and backends only, plan
registration verifying Ed25519 bundle signatures (`ring`) and Sigstore
bundles offline against the host-supplied trusted root
(`sigstore-verify`, `src/signatures.rs`), decisions, admission and the
refusal table of 042 3.5.1, idempotency, lookup of queued, running and
finished decisions, cancellation, server-sent progress streams fed by the
runtime's optional observer (`src/stream.rs`), health, readiness and
metrics (`src/health.rs`), graceful shutdown accounting, and the binary.
Standalone sinks reuse `rustev-remote-http`'s sources by path, as
`rustev-jev` does (spec 001 3.4.4). In `tools/rustev-release/release-set.json`
the crate moves from `absent` (planned members, each with a `kind`) to
`excluded` (existing workspace packages outside the family, which carry a
name and a reason only, like every other `excluded` entry); its image stays
`absent`. Tests: `tests/handlers.rs` and
`tests/transport.rs` over loopback, with committed test certificates and
Sigstore fixtures under `tests/fixtures/`. Not in this increment: the gRPC
transport (042 3.9), the image (3.16), the shared conformance cases
(3.17) and anything that depends on specs 034 or 036; 010 stays in
progress until they land. Open against spec 042 (needs an amendment, not decided here): a decision
task that panics after it started has no honest `final` event, because a
refusal states `decision_ran: false` and there is no decision document;
such a stream ends without `final`, and the non-streaming call answers an
empty HTTP 500.

## Questions settled by R-34

The owner accepted the recommendations below when approving this spec. Each
binding record remains a separate owner-review act.

1. Which immutable Aicortex and Rahi contracts, if any, pass the binding
   record, and should any integration remain deferred after review?
2. Which authentication and protocol surface may `rustev-serve` bind after
   spec 020 resolves the proposed remote refinements?
3. Which live qualification, data and deployment environments, if any, are
   authorized separately from implementation?

## Binding records (section 3.4, R-44)

The three records below are the section 3.4 records, with the rows of spec
042 2.2 added to the `rustev-serve` record. The owner approved the binding
records on 2026-10-21 in the coordinating session ("bindings approved so
unblock 010") before their text existed. The agent then wrote them from the
cited sources, and the owner confirmed the written text on 2026-10-21 in
review of the change that adds it (R-44, A-45). Each record is evidence for deciding whether
implementation may begin. It is not a readiness, compatibility or release
claim, and it approves no live call, deployment, spend, real-user data or
publication.

Sources were read as data on 2026-10-21 and never built or run from their
checkouts: `https://github.com/statecrafting/aicortex` at
`6415b765e5267188b4ecf302a93869352e28cccc` (`main`) and
`https://github.com/statecrafting/rahi` at
`0f338f96b029cbe7b2439f288329787050962641` (`main`, the commit annotated tag
`v0.7.1`, object `3f3859cf58d1c880258afe5a52400e7a37ad44b9`, peels to), plus
the crates.io API and the Sigstore, rustls and distroless publications the
rows name. A path in a row is relative to the repository the record binds.
Each row ends in **pass**, **refuse** or **unverified**; refuse and
unverified both block implementation (3.4.8).

### `rustev-aicortex`

Binding: in process, through Aicortex's host-library mode (Aicortex spec
053). Aicortex has no network read or write surface at this revision. The
cell mounts no product route (`apps/aicortex/src/cell.rs:55-59`), its HTTP and
MCP specs 020 and 021 are approved with `implementation: pending`, and a
claims surface over either is "a later extension" (spec 050, lines 300-301;
spec 052, lines 264-265). Spec 050 D-4 (lines 341-348) selects library
linking for the first consumer. The adapter therefore runs inside a Rahi cell
that links Aicortex, reads through the host's `rahi_store::StoreHandle` and
stages writes into the host's `rahi_store::TxnBuilder`. The host owns the
only commit (Aicortex spec 053 I-3).

| Item | Record | Result |
|---|---|---|
| 1. Upstream | Git dependency on `aicortex-types`, `aicortex-claims` and `aicortex-store` (each `version = "0.1.0"`, `publish = false`) at the immutable revision above. Nothing is on crates.io (`aicortex` and `aicortex-types` return 404 on 2026-10-21) and Aicortex spec 055 (publication) is pending (lines 33-34). Aicortex spec 053 (lines 289-291) names "one exact merged Git revision" as the downstream handoff, so a revision pin is the sanctioned way to consume it. License Apache-2.0 (`LICENSE`, `Cargo.toml:16`); no NOTICE file. Its graph pins `rahi-store`, `rahi-types` and `rahi-ledger` at `=0.6.0` (`Cargo.toml:26-34`; crates.io checksums `59f167d26959c4d7d5807fea2ea82328dcb693c50135f5e0ac58a0161c3db6a4`, `d08efc5b7a805d4610d7bfa90e7a585b6f3a70eaeebdab14a8655aad96ec7d3c`, `952f5bfbcf713c8b9f4febad833b531000868ef7153daeb1a3f6cab5c98ca8a7`) and `hiqlite-patched =0.15.0-patched.3` (`9d3586f7db4e971836ffc5982086bcfa1de0c0293492a194efd7d4ac21ff5ebf`). MSRV 1.96 (`Cargo.toml:15`), toolchain 1.96.0 (`rust-toolchain.toml:5`); Rustev builds it with its own pinned 1.98.1. Platforms: none stated; Aicortex CI runs on `ubuntu-latest`. Supply chain: its `deny.toml` (advisories `unmaintained = "all"`, crates.io sources only) under `cargo-deny-action` in CI; in Rustev, `Cargo.lock` fixes the git revision and every registry checksum. Limitation: Aicortex pins Rahi 0.6.0 and `rustev-rahi` pins 0.7.1, so one host cell cannot link both adapters until Aicortex adopts 0.7.1. Each crate builds and works alone (I-2). Rustev's own supply-chain check: the release tool's manifest test (`tools/rustev-release/tests/manifests.rs`, implementing spec 021 3.4) first rejected every package in the workspace resolution that is not from crates.io, so a workspace member with this dependency failed `make code` (found by building the adapter, 2026-10-21). The owner scoped that rule to the release closure (spec 021 3.4, R-49); `rustev-aicortex` is excluded from the release family, so its Git dependency is outside the closure, and every release member still resolves from crates.io only. New lockfile edges in the workspace: Aicortex enables `uuid`'s `serde` feature, so `uuid` 1.26.1 gains a dependency on `serde_core`, serde's own core crate (which `serde` 1.0.220 and later depend on; the lockfile already holds it at 1.0.229 from crates.io, checksum `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48`), and no new package entry. | pass |
| 2. Schemas | Rust types at the revision, not a wire format: `Claim` (`crates/aicortex-types/src/claim.rs:292-315`, `deny_unknown_fields`, `CLAIM_SCHEMA_VERSION = 1` at line 32), `ClaimRecord` with `TxStamp { seq, recorded_at }` and `ValidTime` (`crates/aicortex-claims/src/history.rs:20-92`), `ProjectionCandidate`, `SlotState` and `CurrentView` (`crates/aicortex-claims/src/projection.rs:28-114`), `project` (line 416), `AsOf`, `TxBound` and `ValidBound` (`crates/aicortex-claims/src/asof.rs`), `ClaimProposal` (`crates/aicortex-types/src/claim_evidence.rs:299-316`), `SourceRef` and `Provenance` (`crates/aicortex-types/src/provenance.rs:79-241`). Compatibility: registered predicate versions are immutable (Aicortex spec 050 I-3) and every record carries its schema version (spec 011 B-11); any other revision needs a new record. Byte examples: `crates/aicortex-types/testdata/claims/*.json` (12 files). Negative fixtures: the `refuse-*` files under `crates/aicortex-gate/testdata/claims/`. The adapter's own boundary types (field bindings, the observation proposal) are Rust values validated at construction. | pass |
| 3. Authentication | None inside Aicortex in library mode. The tenant boundary is `Scope { owner: Sub, kind }` (`crates/aicortex-types/src/scope.rs:176-182`), filtered on every statement (`crates/aicortex-store/src/claim_repo.rs:161-162`). The host supplies a resolver from Rustev's opaque principal handle to a `Scope`, and it may refuse. The adapter never derives a scope from content or a judgment (3.1.2). No TLS, endpoint or redirect exists at this seam; the store's Raft transport belongs to the host cell. | pass |
| 4. Bounds | A read is one page of `ClaimRepo::history` per bound subject (`claim_repo.rs:421-433`; at most `MAX_CLAIM_PAGE_ROWS = 500`, refused above) under an explicit transaction bound. The adapter declares 1 to 64 field bindings and refuses a subject whose history fills the page, so a partial history is never read as complete. The whole fetch runs inside the call's remaining deadline. Dropping it after a deadline or cancellation has no effect, because a read writes nothing. A proposal stages one statement set per call and the adapter never commits, so cost, queueing and concurrency belong to the host's transaction. | pass |
| 5. Failure mapping | Read: a store error (any `rahi_types::Error` variant), a page that fills the bound, a projection error (`projection.rs:116-127`) or a deadline ends the fetch with an error, so no decision runs on a partial snapshot. Per field: `Value` maps to one entry, `Conflicted` to one entry per distinct value (a Rustev conflict), and `Retracted` and `Absent` to a missing field. A value of a kind other than the bound one, `hostile_content` or `origin_erased` is refused and the field stays missing, with its reason. Write: invalid input is refused before staging, so nothing is staged. Staging fails only with `Error::Validation` from `ClaimProposalRepo::stage_append` (`crates/aicortex-store/src/claim_proposal_repo.rs:40`). Acceptance is the host's commit, and admission is Aicortex's gate after that; neither is the adapter's. | pass |
| 6. Privacy | The read skips erased claim rows (`claim_repo.rs:480`); retractions project as `Retracted` and become missing fields. Aicortex gives a consumer no correction, revocation or erasure notification (the library has no push or feed, and `GET /events` belongs to its pending spec 020), so the adapter declares invalidation `none` and infers none from polling (3.5.3). The adapter logs and retains nothing. Entry `source` strings carry claim ids and revisions, never content. Erasing what a decision recorded stays the host's obligation. | pass |
| 7. Tests | Offline: unit tests of mapping, bounds, refusals and proposal construction, and integration tests against a single-voter Rahi 0.6.0 store in a temporary directory on loopback, as Aicortex's own tests run (`crates/aicortex-store/tests/common/mod.rs`). No live qualification is authorized or claimed. | pass |
| 8. Proposal lineage (3.5.4) | `ClaimProposal` and `Provenance` have no typed decision, plan, snapshot or outcome member. The adapter writes lineage into the open members: `SourceRef { system: "rustev", external_id: <decision id>, locator: "rustev:plan/<PlanId>/snapshot/<SnapshotId>" }` (`provenance.rs:79-90`; `SourceSystem` grammar at lines 43-56), stance `observed`, and the host-attributed observer as proposer. The proposal and claim ids derive from the decision id and the observation key, so a resubmission is the same proposal. A judgment offered as an observation is refused before staging. | pass |

Result: **pass** on every row, so implementation may begin. Item 1 first
read pass, was amended to refuse when the adapter, built against this
record, failed the workspace supply-chain check named in that row, and
passes again since R-49 scoped that check to the release closure.

### `rustev-rahi`

Binding: an in-process evidence sink over Rahi's store receipts (Rahi spec
045, approved and complete), inside a Rahi cell the host composes. Rahi has
no remote evidence-ingest surface. Its chassis routes are probes, metrics,
binding, auth, session and operator surfaces
(`crates/rahi-edge/src/probes.rs:103-116`,
`crates/rahi-edge/src/router.rs:380-383`), and no CLI verb ingests a client
record (`crates/rahi-cli/src/verbs.rs`). The decision ledger is not
selected: its record's closed `Outcome` is `allow`, `deny` or `degrade`
(`crates/rahi-ledger/src/record.rs:197-208`), the kernel's adjudication
vocabulary, so a Rustev proposal written there would read as an
authorization outcome (3.1.1). No hosting adapter is selected; 3.6.1 makes
one optional. Hosting Rustev in a cell is the host's own composition through
Rahi's `Cell` trait (`crates/rahi-cli/src/cell.rs:26-107`), and this crate adds
nothing to it.

| Item | Record | Result |
|---|---|---|
| 1. Upstream | crates.io `rahi-store =0.7.1` (checksum `5ffe41f10c2c64e60c807d1a6c555bf1e72d5d650c6d24dc6c4ddf666b353403`) and `rahi-types =0.7.1` (`3ed2f5ea421d3b2f6a9bf486a1af859f54b7b161d797aedab60782643b66f5bb`), the release of tag `v0.7.1` above, with `hiqlite-patched =0.15.0-patched.3` (`9d3586f7db4e971836ffc5982086bcfa1de0c0293492a194efd7d4ac21ff5ebf`). License Apache-2.0 (`LICENSE`, `Cargo.toml:14`); no NOTICE file. MSRV 1.96 (`Cargo.toml:13`). Platforms: images for `amd64` and `arm64` (`.github/workflows/image.yml:37-42`). The supported topology is N=1; N=3 has never run (`README.md:40`, `CHANGELOG.md:29-30`). Supply chain: `deny.toml` (yanked denied, crates.io sources only) under `cargo-deny` in CI (`.github/workflows/ci.yml:22-26`); in Rustev, `Cargo.lock` checksums. The resolved `serde_json` features are `default`, `std` and `raw_value` only, so Rustev's canonical numbers are unaffected. | pass |
| 2. Schemas | Rust API: `ReceiptKey { tenant, namespace, key }` (`crates/rahi-store/src/receipt.rs:62-70`; namespace at most 256 bytes and key at most 1 KiB, lines 32-34), `ContentDigest` (lines 176-205, 64 lowercase hex), `Classification` (lines 256-283: `FirstSeen`, `Redelivered`, `Changed`, `CollisionRedelivered`, `Erased`), `Receipts::classify` (line 423, a consistent read) and `Receipts::stage_first` (line 495), the caller-owned `TxnBuilder` and `StoreHandle::txn` (`crates/rahi-store/src/store.rs:592`, one Raft entry, all or nothing), `MigrationSet` and `receipt_set` (`crates/rahi-store/src/migration_set.rs:186-231`, `receipt.rs:757`). The adapter adds one named migration set, `rustev`, version 1: one table keyed by the receipt's key digest, holding the decision id, the schema, the Rustev record digest, the record canonical bytes and the adapter's observation time, and requiring `rahi.receipts` at version 1 or later. Byte examples: the stored bytes are `rustev.run/1` or `/3` record canonical bytes, as in the existing Rustev goldens. Negative fixtures: the adapter's own tests, since Rahi ships none for receipts. Compatibility: exact `=0.7.1`. Before 1.0 a Rahi patch release may not change the consumer contract (`CHANGELOG.md:10-11`), but the receipt API is not in that contract's list (lines 3-7), so any other Rahi version needs a new record. | pass |
| 3. Authentication | None at this seam: the host's cell owns identity (rauthy), the kernel and the store handle. The receipt `tenant` is a host-supplied opaque string, never interpreted (Rahi spec 045 B-1); the adapter takes it at construction and never derives it from content. No TLS, endpoint or redirect exists in process; the Raft transport and its secrets are the host's `StoreConfig`. | pass |
| 4. Bounds | Record bytes are at most a configured `max_record_bytes` from 1 to 1,048,576, Rahi's per-parameter ceiling (`crates/rahi-store/src/blob.rs:48`); a larger record is refused before any write. One delivery is one consistent read and at most one transaction, plus one consistent read after a refused transaction; nothing is retried. The deadline is the runtime's delivery timeout (spec 003 3.9.4), and queueing and concurrency are the runtime's sink policy. Nothing is charged. | pass |
| 5. Failure mapping | `FirstSeen`, then a committed transaction: `accepted`, receipt `rahi:<durability>:<external_id>:<digest>`, where `external_id` is Rahi's receipt key digest and the accepted revision (`<key digest>#1`). `Redelivered` with the same digest: `duplicate`, receipt `rahi:duplicate:<external_id>:<digest>`. `Changed` or `CollisionRedelivered` (the same decision id with another digest): `conflict`, never a duplicate. `Erased`: `rejected` (`erased`). A lost compare-and-swap (`Error::Conflict`, `receipt.rs:489`) or a constraint failure the store reports as `Error::Validation` (`receipt.rs:356-360`, Rahi spec 045 D-16) rolled the batch back, so the adapter reclassifies with one read and answers `duplicate` or `conflict`. A failed read before the transaction is `unavailable`, with nothing accepted, and so is a failed read after a refused transaction, since the store rolled that transaction back. Any other transaction error (`Upstream`, `Io`, `Stale`, `Integrity`, `Config`, `Unauthorized`, `Denied`, `NotFound`; `crates/rahi-types/src/error.rs:13-34`) is `acceptance_unknown`. The `EvidenceSink` seam reports uncertainty only by timing out (spec 003 3.9.4), so for `acceptance_unknown` the sink gives no answer and the runtime's delivery timeout reports that the sink may have received the record. That loses evidence: the store's own error (for example `Io`) does not reach the run record, which says only that no answer came within the delivery timeout, and the delivery holds its slot (a worker slot under a queued sink policy) for the full timeout. Both follow from spec 003 3.9.4's seam, not from this adapter; carrying the error would need a seam that reports uncertainty with a detail, a change to spec 003. Every other failure is an error with nothing accepted. | pass |
| 6. Privacy | The adapter stores the record bytes the host's sink policy sends and nothing else, and logs nothing. Retention and erasure are Rahi's receipt rules (`retain_until`, tombstones and erasure; Rahi spec 045 B-20 to B-22), applied by the host. Rahi's erasure reaches only its receipt tables, so the adapter supplies one staging call that erases a decision's receipt and deletes its stored record in the host's transaction. An erased decision id is refused, never rewritten. A backup taken before an erasure still holds the rows, and backups are the host's. Durability is named exactly: a single voter syncs the Raft log asynchronously (`LogSync::ImmediateAsync`) and a cluster syncs it before acknowledging (`Immediate`), `crates/rahi-store/src/store.rs:656-668`. The host declares which from its `StoreConfig`, and the receipt states `raft-log-async` or `raft-log-synced`, nothing stronger. | pass |
| 7. Tests | Offline: the mapping against a scripted receipt store (every row of item 5 and both negative cases of 3.6.3), and integration tests against a single-voter Rahi 0.7.1 store in a temporary directory on loopback. No live qualification is authorized or claimed. | pass |

Result: **pass** on every row, so implementation may begin. The optional
hosting adapter is not selected and is not implemented.

### `rustev-serve`

Binding: the `rustev.serve/1` service API and the two callback protocols of
spec 042, over HTTP/1.1, HTTP/2 and gRPC. Unlike the two records above, the
contract here is Rustev's own; the external parts are the server, gRPC, TLS
and signature crates and the base image.

| Item | Record | Result |
|---|---|---|
| 1. Upstream | HTTP: `hyper` 1.x with `server`, `http1` and `http2` (the lockfile has 1.11.1, checksum `27b501faa50e7a26c3d3560ca625132f4078a17771f4810baf70475ae48cbe43`, MIT), `hyper-util` 0.1.21 with `server-auto` for ALPN, and `h2` 0.4. gRPC: `tonic` 0.14.6 (`ac2a5518c70fa84342385732db33fb3f44bc4cc748936eb5833d2df34d6445ef`, MIT, MSRV 1.88), `tonic-prost` 0.14.6 (`50849f68853be452acf590cde0b146665b8d507b3b8af17261df47e02c209ea0`), `prost` 0.14.4 (`528ac67416ff8646872a3c02cad9cc4ee5dc9f9540c9b10771855c95cb2e5ae1`, Apache-2.0), and the generator `tonic-prost-build` 0.14.6 (`654e5643eff75d7f8c99197ce1440ed19a3474eada74c12bbac488b2cafdae27`), run at development time with its output committed (042 3.18). TLS: `rustls` 0.23.46 with the `ring` provider and `tokio-rustls` 0.26, already workspace dependencies. Ed25519: `ring` 0.17.14 (`ring::signature::ED25519`), already locked. Platforms: the binary on macOS and Linux for development, the image on `linux/amd64` and `linux/arm64` (042 3.16). | pass |
| 2. Schemas | Amended 2026-10-22: spec 042's documents are in `rustev-contract` as strict serde types with exact schema strings, `deny_unknown_fields` and declared parse bounds: `crates/rustev-contract/src/serve.rs` (`rustev.serve/1` with its operation table, and every `rustev.serve-*/1` document), `src/plan_bundle.rs` (`rustev.plan-bundle/1`, `rustev.plan-bundle-signature/1`, `rustev.plan-registration/1`), `src/remote_context.rs` and `src/remote_sink.rs` (`rustev.remote-context-*/1`, `rustev.remote-sink-*/1`). Their JSON Schemas are emitted under `crates/rustev-contract/schema/` through spec 027's emitter, with TypeScript declarations in both bindings. Byte examples and negative fixtures (unknown fields, wrong schema strings, each bound at its limit and one past) are `crates/rustev-contract/tests/042_serve_documents.rs` and `tests/042_remote_documents.rs`. Compatibility follows 042 3.19. Members that depend on specs 034 and 036 are held back with extension points (042 implementation record). Bounds the spec states without a value were chosen and recorded in 042's implementation record. | pass |
| 3. Authentication | Transport only (042 3.2). TLS 1.3 only, with the three `rustls` `ring` suites `TLS13_AES_256_GCM_SHA384`, `TLS13_AES_128_GCM_SHA256` and `TLS13_CHACHA20_POLY1305_SHA256` (`rustls::crypto::ring::cipher_suite`). Client certificates are required on every TCP listener, verified by `rustls::server::WebPkiClientVerifier` against configured roots, plus an optional exact subject allowlist. Unix sockets are created `0600`, or `0660` when configured, and an existing path is refused (042 3.1.2), using `tokio::net::UnixListener` and `std::os::unix::fs::PermissionsExt`. The development bearer is accepted on Unix sockets only. The principal handle passes through opaquely. No redirect is followed or issued. | pass |
| 4. Bounds | Spec 042 states every connection, stream, byte, queue, deadline, retention and drain bound with its range (3.5.3, 3.5.4, 3.6.1, 3.7.2, 3.8.4, 3.12, 3.14). | pass |
| 5. Failure mapping | Spec 042 3.5.1 maps every refusal to one HTTP status and one gRPC code; 3.4.3 to 3.4.5 cover decision statuses and disconnects, and 3.11.3 the sink receipts and errors. | pass |
| 6. Privacy | Spec 042: memory-only retention within a bounded window (3.6.1), erasure applied as cache invalidation with retained responses kept until their window ends and reported (3.10.6), metrics that are not evidence (3.13.3), and secrets by file reference, excluded from the configuration digest (3.15). | pass |
| 7. Tests | Spec 042 3.17 defines the conformance and transport-differential suites, which do not exist yet. No live qualification is authorized. | pass |
| 042 2.2: server crates | As item 1. | pass |
| 042 2.2: gRPC crates and generator | As item 1. | pass |
| 042 2.2: TLS crate, mTLS, cipher suites | As items 1 and 3. | pass |
| 042 2.2: Unix socket permissions | As item 3. | pass |
| 042 2.2: Ed25519 crate | `ring` 0.17.14, as item 1. | pass |
| 042 2.2: Sigstore crate, bundle media type, trusted root | Media type `application/vnd.dev.sigstore.bundle.v0.3+json` (`Bundle.media_type` in `protos/sigstore_bundle.proto` of `sigstore/protobuf-specs`). Amended 2026-10-22: the crate is `sigstore-verify` 0.15.0 (Apache-2.0, `8d81887abe3161d7f0d04f224236e14390f782ce6f3b5e2717819ef2d7b452cb`, MSRV 1.86, `sigstore/sigstore-rust`) with `default-features = false, features = ["aws-lc-rs"]`. Its `verify(artifact, bundle, policy, trusted_root)` checks a v0.3 bundle against a `TrustedRoot` the host supplies (`TrustedRoot::from_json`), offline: with the `tuf`, `rustls` and `native-tls` features off, the resolved tree has no HTTP client, no `reqwest`, no `tokio` and no TUF fetcher (`sigstore-trust-root` 0.15.0 makes TUF and its TLS optional; checked with `cargo tree` on 2026-10-22). Crypto is `aws-lc-rs` 1.18.2 (`faac5829c2b74c28f830747e7818ccfb684261b5f48a1118b1e2a13d36dfab13`, ISC AND (Apache-2.0 OR ISC)) over `aws-lc-sys` 0.46.0 (`f1622d8446a2d4b2ce0c7eefc73dd43a99779028d5ee5c2dd8073a658ba8a2bc`), the provider `sigstore-crypto` uses by default; without it no provider is installed and verification panics, and a `ring`-backed provider would be security-sensitive code of Rustev's own. `aws-lc-sys` builds C with a C compiler and CMake on the build host and the image's builder stage. The crate is used only inside `rustev-serve` (042 3.18). | pass |
| 042 2.2: base image | `gcr.io/distroless/cc-debian12:nonroot`, index `sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f` (amd64 `sha256:777e96cf322c46bc32aca926c263624c4dc8d7cf37e2fa65ba2c7e697318ebbb`, arm64 `sha256:9ae6393649d0c55122c66fe6d4803b0d85b2d7b60b2b09b690ce80c086f40a83`), resolved from the registry on 2026-10-21. `cc` rather than `static`, because the binary links glibc. User and group 65532. | pass |
| 042 2.2: platforms | `linux/amd64` and `linux/arm64`. | pass |

Result (amended 2026-10-22): every row **pass**. The 2026-10-21 record
refused item 2 (spec 042's documents did not exist) and left the Sigstore
row unverified; both are amended above with the evidence that resolves
them. The owner reviewed and approved this amended text on 2026-10-22
(A-47), so `rustev-serve` may be implemented under 3.4. The approval is
for implementation only; it approves no live call, deployment, image
publication, spend or real-user data.

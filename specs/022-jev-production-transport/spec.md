---
id: "022-jev-production-transport"
title: "Production transport and served identity for the Jev adapter (amends 012)"
status: approved
implementation: in-progress
created: "2026-09-27"
summary: >
  The separately reviewable spec 012 amendment that backlog item RUSTEV-010
  and spec 012 R-3 require before production use: a Jev binding is
  production-eligible only when its transport pins a model version and every
  answered exchange proves the served identity against that pin. Defines the
  qualification of TypeSafe's direct API with a pinned version, the evidence
  under which Gateway pinning could be accepted instead, and the transport,
  TLS, timeout, connection, privacy and cost-reconciliation facts a
  production binding records. Development evidence with `served: unknown`
  is never relabeled as production evidence. Approved (A-21) with the
  additions of tracking issue #76 child C3 (section 3.8). Approval authorizes
  implementation of the no-network stage; live stages stay inside R-29's
  testing cap, and no deployment is authorized.
amends:
  - "012-jev-integration"
extends:
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
  - "020-remote-protocol-refinements"
  - "023-real-data-quality-qualification"
references:
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A binding is production-eligible only when its transport pins a model version and the pin is verified per answered exchange; a binding whose served identity can be `unknown` is refused for production at construction."
    anchor: "3-1-production-eligibility"
  - id: "I-2"
    kind: invariant
    text: "An answered exchange whose observed served identity differs from the pin, or cannot be observed on a transport that promised it, is `identity_mismatch` and never supplies an output."
    anchor: "3-2-direct-transport-with-a-pin"
  - id: "I-3"
    kind: invariant
    text: "Evaluation, calibration and qualification evidence produced under one binding identity is never reported as evidence for another; a change of transport, pin or mapping requires requalification."
    anchor: "3-6-evidence-and-requalification"
  - id: "R-1"
    kind: requirement
    text: "Gateway pinning is accepted only after a recorded fact shows the Gateway both honors a requested version and reports the served version per response; documentation alone is not sufficient."
    anchor: "3-3-gateway-pinning-as-an-alternative"
  - id: "R-2"
    kind: requirement
    text: "A production binding records its transport facts (endpoint, API version, TLS policy, timeouts inside the one attempt budget, connection behavior, privacy terms and cost source) and a reconciliation procedure between the spend journal and the provider's billing record."
    anchor: "3-4-transport-facts-of-a-production-binding"
---

# 022: Production transport and served identity for the Jev adapter (amends 012)

Approved by the owner (A-21, 2026-10-16, under the 2026-10-16 delivery work
order). The no-network stage that approval authorizes is implemented (see
the implementation record); the live stages are externally blocked. It
is the "separately approved spec 012 amendment" that backlog item RUSTEV-010
names, and it makes spec 012 R-3 and owner decision R-28 item 2 concrete. Spec
012's approved text is not edited; this spec records the change (R-16).

Some facts do not exist yet: whether TypeSafe's direct API is available to
the owner with a pinned version, its current request and response shapes,
its data-retention terms, and whether the Vercel AI Gateway can pin a
version. They are confirmed against the provider's documentation and
recorded as facts in the decisions record at implementation time (3.7 stage
2); until then the mapping stays unverified. No stage of section 3.7 ran as
part of approving this spec. Live stages run only on synthetic data or data
admitted under spec 023, inside R-29's USD 5 testing cap, with the persistent
spend journal refusing before any call that would exceed it.

## 1. Purpose

Spec 012 delivers a working adapter whose served model is `unknown` on the
Gateway (C-09, C-13). That is acceptable for development and qualification
only (R-28). A production decision needs to know which model answered, so
that a silent version change behind the transport cannot move judgments
without a change of identity. This amendment defines when a Jev binding may
be used in production and what evidence a production binding carries.

## 2. Territory

When approved and delivered: `integrations/rustev-jev/` (spec 012's crate)
gains a binding schema version with the production fields of 3.4, the
eligibility check of 3.1, the direct-transport mapping of 3.2 as verified
by 3.7, and the reconciliation record of 3.5. No crate outside
`integrations/` changes, and spec 012's development bindings keep their
bytes and identities.

## 3. Behavior

### 3.1 Production eligibility

1. A binding declares its use: `development` (spec 012 as delivered) or
   `production`. The use is in the artifact identity.
2. A `production` binding is refused at construction unless its transport
   is `direct` with a version pin, or `gateway` with a pin under 3.3's
   accepted evidence. `gateway` without accepted pinning and
   `gateway_typesafe_base` are refused for production.
3. A `production` binding is refused at construction when any required fact
   of 3.4 is missing. Unknown is not a default (Statecraft rule: a key no
   layer supplies is unknown, and unknown is not success).
4. Production use still requires the owner's separate acts: a privacy
   decision for the data sent (spec 012 3.1.3), the production spend cap
   (R-29), and deployment approval. Eligibility of a binding is none of
   those.

### 3.2 Direct transport with a pin

1. The binding names the direct endpoint, the API version and the model
   version pin (for example `jev-1.13.0`, as TypeSafe documents; unverified
   here until 3.7 stage 2).
2. Each answered response's reported model version is compared with the
   pin. Equal: `served` is `reported` with that value. Different, absent, or
   in a field whose shape the mapping does not recognize:
   `identity_mismatch` (spec 009 3.7), and no output is supplied.
3. Request and response shapes are fixed by a new mapping version derived
   from recorded direct-API exchanges (3.7 stage 2), never assumed from the
   Gateway's shapes. Every spec 012 3.4 rule (lossless reshaping, keys as
   received, provider confidence kept only as provider-reported) carries
   over unchanged.
4. The adapter still sends each attempt at most once and never switches
   transport on its own (spec 012 I-4). A Gateway fallback for a production
   plan is a separate adapter instance named in the plan's execution policy,
   and it is refused if that instance is not itself production-eligible.

### 3.3 Gateway pinning as an alternative

Gateway pinning is accepted only when a decisions-record fact, obtained from
live calls under an owner authorization, shows all of:

1. a request field that selects an exact model version, honored by the
   Gateway;
2. a response field that reports the served version, per response;
3. a request for a version the provider does not serve is refused rather
   than rerouted; and
4. the provider allowlist `only: ["typesafe-ai"]` still holds.

Vendor documentation without such a recorded observation is not sufficient.
Until the fact exists, the Gateway remains a development transport.

### 3.4 Transport facts of a production binding

A production binding records, and its artifact identity covers:

| Fact | Content |
|---|---|
| Endpoint | Scheme, host, path and API version; no redirects followed. |
| TLS | Minimum protocol version, root store source, and whether the host may pin a certificate; plaintext refused. |
| Timeouts | Connect and response ceilings, each inside the one remaining attempt budget (spec 009 3.4); phase ceilings follow spec 020 when that spec is delivered. |
| Connections | Whether connections are reused, and the isolation key that bounds reuse (spec 020 3.5 when delivered; otherwise one connection per attempt). |
| Retries | None inside the adapter; retries and fallback stay plan policy (R-10). |
| Privacy | The provider's retention and training terms for the direct API, as a dated fact, and the request options that invoke them. |
| Cost | The price table source and date, the unit rate, and whether a per-response charge is reported. |
| Credential | The credential's custody class: the host registers a credential callback at construction, and the key value is obtained only at dispatch through that callback (3.8.1). The value is never stored in the binding and never enters identity or evidence; the binding records only the custody class. |

### 3.5 Cost reconciliation

1. Production spend is held by spec 012's persistent `SpendJournal` under
   the production cap (R-29), with the hard stop before dispatch.
2. A reconciliation record compares, per calendar month, the journal's
   observed, estimated and liability totals with the provider's billing
   record for the same credential. The difference is reported with its
   sign; it never silently rewrites journal entries.
3. Liability for attempts that ended `possibly_continuing` (spec 013) is
   settled only from the provider's record, never assumed refunded.

### 3.6 Evidence and requalification

1. The spec 012 stage 3 record, made on the Gateway with `served: unknown`
   and without zero data retention, is development evidence. It is not
   evidence about a direct-API binding, and no report says otherwise.
2. A production binding needs its own qualification (3.7 stage 3), naming
   its binding identity. A change of transport, pin, mapping version or
   option description table produces a new identity and needs
   requalification before production use.
3. Calibration artifacts are bound to the artifact identity they were
   fitted on (spec 002 3.6); one fitted under a development binding does not
   apply to a production binding.

### 3.7 Qualification stages

1. **Mechanics (no network).** Loopback fixtures of the direct transport:
   pin match, pin mismatch, missing version field, each spec 009 error code,
   refusal of every ineligible production binding at construction, and
   reconciliation arithmetic.
2. **Smoke (owner-authorized live calls only).** A bounded number of direct
   calls on synthetic fixtures, recording request and response shapes and
   the reported version field as fixtures.
3. **Qualification (owner-authorized).** Spec 012 3.8 stage 3's metrics on
   the production binding, on a dataset admitted under spec 023 or the
   synthetic-provenance set with that limit stated.

### 3.8 Additions from the tracking issue (C3)

1. **Transport `direct` in `rustev.jev-binding/2`.** The binding names the
   endpoint, the pinned model version and the request and response shapes
   of 3.2. The API key reaches the adapter only through a host credential
   callback invoked at runtime; the library never reads a credential from a
   file or the environment, and the key is in no identity or evidence.
   Every request and response shape the mapping accepts has a recorded
   fixture.
2. **Served identity tolerance.** A mismatch between the reported version
   and the pin is `identity_mismatch` (3.2.2) unless the binding declares an
   explicit tolerance policy naming the accepted versions. A tolerance
   policy is part of the binding identity, and every accepted non-pin
   version is recorded as `served` on the attempt. Nothing is accepted
   silently.
3. **Cost.** A charge is `observed` when the response reports it, otherwise
   `estimated` from the bound price table (3.4); when neither is possible the
   charge is `unknown`, never zero.
4. **Screening use.** When the screening seam of tracking issue #76 child B7
   is delivered, the adapter may declare a `screen` capability with
   `isolation: per_item`, because batching several items into one screening
   call has been observed to change another item's verdict. The runtime then
   never batches items for it.
5. **Privacy posture.** Zero-retention request flags, where the provider
   supports them, are part of the binding identity and recorded on every
   attempt. A binding that cannot set them records that fact; it does not
   omit it.
6. **Spend journal.** Spec 012's persistent `SpendJournal` applies unchanged
   to the direct transport.
7. **Requalification dataset.** Requalification (3.7 stage 3) uses a dataset
   admitted under spec 023 when one exists; until then it reuses the
   existing synthetic-provenance set with that limitation stated in the
   record. Gateway evidence is never relabeled as direct-transport evidence
   (I-3).

## 4. Out of scope

Deployment, production traffic and real user data (separate owner acts).
Any change to value kinds, plan semantics or authority. A second provider.
A Jev wire-compatible surface (R-05). Qualifying adapter-scoped batching
(backlog RUSTEV-005, a spec 009 and 012 qualification task).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A `production` binding on the Gateway without accepted pinning | Refused at construction; no request sent. |
| A `production` binding with no recorded privacy terms | Refused at construction. |
| A pinned direct response reports another version | `identity_mismatch`; no output supplied. |
| A pinned direct response has no version field | `identity_mismatch`; never `served: unknown` on a production binding. |
| A production plan falls back to a development Gateway instance | Refused when the plan is prepared. |
| A report cites the Gateway stage 3 record as production evidence | Rejected by review; the binding identities differ (I-3). |
| The billing record exceeds the journal's observed total | Reported as a signed difference; journal entries unchanged. |

## Acceptance

- Stage 1 of 3.7 passes over loopback with zero external calls.
- Every negative case of section 5 has a failing test before the fix that
  makes it pass.
- Recorded fixtures cover success, version mismatch, a declared tolerance,
  each of HTTP 400, 401, 403, 429 and 5xx, and a response without cost.
- The served identity is recorded on every attempt, and the mismatch path is
  tested.
- When a screening capability is declared, per-item isolation holds under
  every batching setting.
- The adapter reads no environment variable or file for a credential.
- Stages 2 and 3 run only under the owner's spend authority and are
  recorded, with spend, in this spec's implementation record.
- `make code`, declared verification, and `make gate` pass.

## Verification

Declared for `spec-spine verify 022`; 022 is in `make verify`, in CI's
`integrations-core` acceptance group beside spec 012, whose crate it shares.
Approval authorizes the contract and the no-network implementation; no
acceptance item is satisfied by approval or by the spec 012 Gateway
evidence. The two live tests are ignored and never run here.

```verify:cli
# 3.1 to 3.7 stage 1, 3.8 and section 5: eligibility, facts, pin and
# tolerance, error codes, cost, reconciliation, evidence, plan preparation,
# over loopback only (the tests assert no external host is reached).
cargo test -p rustev-jev --test production --locked
# Spec 012's development bindings keep their bytes and identities.
cargo test -p rustev-jev --locked
# 3.8.1: the adapter reads no environment variable for a credential.
sh -c 'if grep -rnE "std::env|env::var|env!\(|option_env!" integrations/rustev-jev/src; then exit 1; fi'
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-jev --all-targets --locked -- -D warnings
```

## Implementation record

**Increment 1 (stage 1, no network).** `integrations/rustev-jev/src/production.rs`
adds the production binding (schema 2) whose artifact identity covers every
fact of 3.4: endpoint, TLS, spec 020's seven phase ceilings (validated
against the one remaining attempt budget), the connection fact (reuse off,
one connection per attempt; a binding declaring reuse is refused until an
isolation key under spec 020 3.5 is supported), retries none, dated privacy
and cost facts, and the credential custody class. The credential value is
obtained only at dispatch through the host's callback (3.8.1) and never
enters identity or evidence; the adapter reads no environment variable or
file. Also: the eligibility check of 3.1, the served-version pin and
declared tolerance of 3.2, gateway pinning of 3.3, the monthly
reconciliation record of 3.5 (signed difference, never rewriting the
journal; `possibly_continuing` liability settled only from a billing line
through `SpendJournal::reconcile_billing`), the evidence checks of 3.6 and
the qualification gate of 3.7. Spec 012's development bindings keep their
bytes and identities (`tests/fixtures/development-binding-v1.golden.json`).
Stage 1 runs over loopback with the explicitly synthetic direct mapping
(`rustev-jev-direct-synthetic/1`), never asserted to be the provider's API
shape. Tests: `tests/production.rs`.

Not done, and blocked on the owner: stages 2 and 3 of 3.7 (live calls under
the owner's spend authority), and therefore the recorded fixtures of
acceptance item 3 and a verified direct mapping. Until then
`JevBackend::new_production` refuses construction (`Unresolved`), and the two
live tests are ignored with that reason.

**Closure (2026-10-23).** Declared verification added (section
Verification); 022 joins `VERIFIED_SPECS` and the `integrations-core`
acceptance group. `implementation` stays `in-progress`: the no-network
stage is delivered, but the Acceptance section also requires fixtures
recorded from the direct API and stages 2 and 3 recorded with their
spend, and those are externally blocked (below). Acceptance, item by
item:

- Stage 1 over loopback with zero external calls: `tests/production.rs`
  (`assert_loopback_only`, `direct_fixture_matrix_records_each_served_identity`).
- Section 5 negative cases: Gateway without accepted pinning and missing
  privacy terms (`missing_stale_conflicting_and_invalid_facts_refuse_construction`,
  `gateway_documentation_or_claimed_observations_cannot_enable_unverified_mapping`);
  other and absent versions (`pinned_mismatched_missing_malformed_and_tolerated_versions`);
  development fallback (`production_preparation_refuses_a_development_gateway_fallback`);
  Gateway evidence cited for another identity
  (`development_evidence_and_changed_bindings_require_requalification`);
  signed billing difference (`billing_differences_are_signed_and_do_not_rewrite_the_journal`).
- Fixtures for success, mismatch, declared tolerance, HTTP 400, 401, 403,
  429, 500 and 503, and a response without cost: covered over the
  synthetic mapping and spec 012's recorded Gateway exchanges
  (`http_failures_and_no_cost_are_local_fixtures`,
  `tests/fixtures/synthetic-022-direct.json`). Fixtures recorded from the
  direct API are externally blocked (below).
- Served identity on every attempt and the mismatch path: the pin and
  matrix tests above.
- Screening (3.8.4): not applicable as built. Spec 035's seam is not
  delivered, the core exposes no `screen` operation, and the adapter
  declares no screening capability; the item applies when it does.
- No credential from environment or file: the host callback is the only
  path (`callback_failure_is_unsent_and_journal_cap_precedes_callback`),
  and the declared verification refuses any environment read in the
  crate's sources.

Externally blocked, never reported as passed: stage 2 (live smoke on the
direct API, which yields the recorded direct-API fixtures and a verified
mapping) and stage 3 (qualification of a production binding). Both need
an owner-provided direct-API credential and spend under R-29's USD 5
testing cap; neither ran, and no money was spent. Until they run,
`JevBackend::new_production` refuses construction, so no binding is
production-eligible. Delivering them needs a new mapping version and its
recorded fixtures, which change the binding identity (3.6.2). 022 becomes
`complete` only when both are recorded here with their spend.

## Questions settled at approval

The owner settled the draft's open questions in the 2026-10-16 delivery work
order and tracking issue #76 (OD-1, child C3):

1. **Transport choice.** Qualify the direct API with a pinned version.
   Gateway pinning stays an alternative only under 3.3's evidence bar.
2. **Access.** The credential is supplied by the host at runtime through a
   callback (3.8.1). Live stages need an owner-provided credential; without
   one they wait, and the no-network stage proceeds.
3. **Privacy.** Live testing uses synthetic data or data admitted under spec
   023 only. Production data classes remain a separate owner decision (spec
   012 3.1.3).
4. **Requalification dataset.** As 3.8.7.
5. **Testing spend.** Charged to R-29's USD 5 testing cap, shared by all
   providers, until the owner raises it in writing.

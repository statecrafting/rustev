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
4. **Running stage 3 (amended 2026-10-29, A-68).** Stage 3's calls are made
   through `JevBackend::new_qualification`, which builds an adapter from a
   schema-2 production binding and is never a production construction:
   1. It refuses, before any credential lookup or dispatch, unless the
      binding's facts pass the checks of 3.4 for the day, the transport is
      `direct`, the binding's `mapping` and its decision binding's mapping
      version are the direct mapping recorded under stage 2
      (`rustev-jev-mapping/2`), the endpoint is the facts' HTTPS endpoint,
      and the spend journal is the testing journal (R-29).
   2. Every exchange record names the production binding's artifact
      identity and protocol, and its requested privacy carries the binding's
      zero-retention options, or the recorded fact that none is available,
      plus `qualification: stage 3`. So the evidence is evidence for that
      binding (3.6.2) and is never mistaken for production traffic.
   3. The credential is obtained only at dispatch through the host's
      callback (3.8.1). Prices come from the binding's facts. The served
      identity is checked against the pin and any declared tolerance, as in
      production.
   4. It grants no production eligibility. `JevBackend::new_production`
      still refuses until a stage 3 record is accepted. Accepting one, and
      the construction it then permits, is 3.7.5.
5. **Accepting stage 3 and constructing a production binding (amended
   2026-10-29, A-69).**
   1. A stage 3 record is accepted only as a `Qualification` document (its
      binding's artifact identity, the dated calls, the owner's
      authorization, the dataset with its admission or stated limitation,
      and the report and spend journal references with their totals) that
      passes `Qualification::check`. It must name the binding's exact
      identity (3.6.2), and its calls fact must not be stale. The owner's
      authorization names the decision row that accepted it. Recording a
      record is not accepting it, and the adapter accepts nothing on the
      host's word: the check runs at construction.
   2. `ProductionBinding::production_eligible` takes the accepted
      `Qualification`. It holds only when 3.4's facts pass for the day, the
      use is `production`, the transport is `direct` with a pin, the binding's
      `mapping` and its decision binding's mapping version are the direct
      mapping recorded under stage 2, and the qualification passes 1. A
      Gateway binding stays ineligible here: 3.3's evidence path is not
      delivered.
   3. `JevBackend::new_production` takes the binding, the host's
      credential callback, the accepted `Qualification`, a spend journal
      and the day. It constructs only when 2 holds and the journal is the
      production journal (R-29's production cap). Otherwise it refuses
      before any credential lookup. It then behaves as 3.7.4's construction
      does, at the binding's endpoint, with the binding's prices and
      privacy record, and with `qualification` recording the accepted
      record's digest instead of `stage 3`. It keeps no request or response
      bytes (spec 009 R-19), since production content is not synthetic.
   4. A binding that enables adapter-scoped batching is constructed only as
      spec 044 3.8 item 4 allows. Until spec 044's increment 8 delivers that
      check, `new_production` refuses any binding whose batching is not
      `off`.
   5. `check_production_plan` takes the accepted qualifications and calls 2
      for each binding a plan reaches.
   6. Eligibility is not deployment. 3.1.4's acts (a privacy decision for
      the data sent, the production spend cap, and deployment approval)
      stay the owner's, and none of them is implied by an accepted record.
      Facts that go stale (their `valid_through` passes) make the binding
      ineligible until they are re-read and requalified where 3.6.2
      requires it.

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
| `new_qualification` with a production journal, a Gateway transport, a mapping other than the direct mapping, a non-HTTPS endpoint, or facts that fail 3.4 | Refused at construction; no credential lookup, no request sent. |
| A stage 3 exchange's record | Names the production binding's identity and `qualification: stage 3`. |
| `new_production` with no qualification, one for another identity, one without the owner's authorization, report or spend references, stale facts or calls, a testing journal, a Gateway binding, a mapping other than the direct mapping, or batching on | Refused at construction; no credential lookup, no request sent. |
| A production plan reaching a binding without an accepted qualification | Refused when the plan is prepared. |

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
  recorded, with spend, in this spec's implementation record. Stage 3 runs
  only through 3.7.4's qualification construction, and a production binding
  is constructed only through 3.7.5.
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
# 3.2.3 and 3.7 stage 2: the direct mapping against the recorded exchanges.
cargo test -p rustev-jev --test direct_recorded --locked
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
  direct API: stage 2 (below).
- Served identity on every attempt and the mismatch path: the pin and
  matrix tests above.
- Screening (3.8.4): not applicable as built. Spec 035's seam is not
  delivered, the core exposes no `screen` operation, and the adapter
  declares no screening capability; the item applies when it does.
- No credential from environment or file: the host callback is the only
  path (`callback_failure_is_unsent_and_journal_cap_precedes_callback`),
  and the declared verification refuses any environment read in the
  crate's sources.

**Stage 2, the direct smoke (2026-10-29).** Authorized by the owner on
2026-10-29, with the owner's `TYPESAFE_API_KEY` read through spec 012's
smoke harness (`--transport direct --pin jev-1.13.0`, A-66) and every
attempt reserved against the shared testing journal (R-29). Two runs of
the harness's four synthetic cases (two propositions, one `classify`, one
`rubric`), at `https://api.typesafe.ai/v1/systemone`:

1. Under spec 012's mapping (`rustev-jev-mapping/1`), `classify` and
   `rubric` were answered in the Gateway's shapes, and each response
   reported `model: jev-1.13.0`, the pin. Both propositions were HTTP 400
   (`api_usage_error`, "Invalid request."): the direct API has no
   `boolean` question type. Every charge was `unknown`, because the
   response reports usage as `input_tokens` and `output_tokens`, which the
   Gateway price table did not name.
2. Under the direct mapping below, all four were answered; the
   propositions as `{"type": "noul", "noul": p}` (0.96 for the flight
   confirmation, 0.02 for the hotel), and every charge was `estimated`
   from reported usage.

Spend: the journal went from 1,421,364 observed, 344,520 estimated and
13,001,784 liability units (nano-USD) to 1,421,364, 411,552 and
13,079,694. The first run's `unknown` charges stay liability at their
full reservations (77,910 units); the second run's were 67,032 estimated.
About USD 0.00015 in all.

The direct mapping (3.2.3), from these exchanges:
`DIRECT_MAPPING_VERSION`, `rustev-jev-mapping/2 (direct shapes of 022
stage 2)`, is the only mapping version a direct binding accepts and the one
`JevBinding::direct` sets; a Gateway binding still accepts only
`rustev-jev-mapping/1` (`each_transport_accepts_only_its_mapping_version`).
It changes the direct binding identity (3.6.2). A proposition is asked as
`noul` with spec 012's `{"false": d, "true": d}` criteria, and its `noul`
field `p` becomes `{"false": 1 - p, "true": p}`; a `boolean` answer, or a
`noul` answer without a numeric `noul`, is `malformed_response`.
`classify` and `rubric` keep spec 012's shapes. The price table also names
`input_tokens` and `output_tokens` at the same rates, so a direct charge is
estimated from reported usage. The response reports no cost, so a direct
charge is never `observed`. Recorded fixtures:
`tests/fixtures/recorded-direct-{boolean-flight,boolean-hotel,choice-hotel,score-flight}-200.json`
and `recorded-direct-boolean-type-400.json` (the refused `boolean`
request), replayed by `tests/direct_recorded.rs`: each recorded request is
the body the mapping sends, the refused request differs from it only in
its type, and each recorded answer replays on loopback to the live output,
an estimated charge and served `jev-1.13.0`. The hand-written direct
fixtures (`synthetic-022-direct.json`,
`synthetic-direct-{pinned,other}-version-200.json`) now answer
propositions as `noul`; their cost members stay synthetic, for the
reconciliation mechanics. The recorded exchanges keep no request headers
and were checked for the key before they were written.

**The stage 3 construction (2026-10-29, A-68).** 3.7.4 is amended in, so
stage 3 has a construction to run through: before it, `new_production`
refused until stage 3 was recorded, and `new_mechanics` reaches only
loopback. The facts the first stage 3 binding records were read on
2026-10-29 from TypeSafe's public pages, and the owner accepted them
(A-68). That binding pins `jev-1.14.0`, by the owner's choice, so a stage
2 smoke at that pin comes first.

Implemented: `JevBackend::new_qualification` refuses everything 3.7.4.1
names before any credential lookup
(`qualification_refuses_everything_3_7_4_forbids_before_any_credential`),
and its record names the production binding's identity and protocol,
`qualification: stage 3` and the zero-retention fact
(`a_qualification_record_names_the_production_binding_and_stage_3`). It
shares its construction with `new_mechanics`, and it keeps request and
response bytes, since its content is synthetic. Two defects surfaced only
live, and each now has a test. Passing the binding's full endpoint as the
client's base doubled the path (HTTP 404), so the client now takes its
`origin` (`a_qualification_adapter_takes_the_endpoint_origin`). The
synthetic mechanics `privacy` member was sent under the direct mapping
(HTTP 400), so it is now sent only under the synthetic mapping
(`only_the_synthetic_mapping_adds_a_privacy_member`). The `live_direct_smoke`
placeholder test, which said no direct mapping was verified, is removed.

**Stage 2 at `jev-1.14.0` (2026-10-29).** The harness's four synthetic
cases at the new pin were all answered in the direct mapping's shapes, each
reporting `model: jev-1.14.0`. Their fixtures,
`tests/fixtures/recorded-direct-114-*-200.json`, replay in
`tests/direct_recorded.rs` beside the `jev-1.13.0` recordings, which stay as
that pin's evidence. Spend: 67,032 units estimated.

**Stage 3, qualification (2026-10-29).** This ran on the production binding
`sha256:12897cdf3796818833af333769e913d1a1f65a267532ea4de2ec3b144fd40bd4`:
schema 2, direct transport, pin `jev-1.14.0`, `rustev-jev-mapping/2`, and
A-68's facts, constructed through `new_qualification` and run with this
change's code.

- Set: spec 012's stage 3 synthetic-provenance set, with that limitation
  stated (no spec 023 dataset is admitted, 3.8.7). It has the same dataset
  ids, read from that run's retained canonical documents and snapshots, and
  each file was checked against its recorded checksum. 50 cases: 23
  relevance, 23 message_kind and 4 preference_scope.
- Calls: one request per question. All 50 exchanges were HTTP 200, and every
  served identity was reported `jev-1.14.0`. There were 47 proposals and 3
  escalations. Charges were estimated from reported usage.
- Spec 004 reports, with spec 005's rules baseline over the same snapshots
  as its own report (as in spec 012's record). Error among accepted, mean
  log loss and Brier score, Jev against rules:

  | Split, task | n | Jev | Rules |
  |---|---|---|---|
  | final_test relevance | 11 | 0.00, 0.031, 0.010 | 0.27, 0.75, 0.46 |
  | final_test message_kind | 11 | 0.30 (10 accepted), unknown, 0.50 | 0.375 (8 accepted), 1.29, 0.58 |
  | calibration relevance | 12 | 0.00 (11 accepted), 0.101, 0.048 | 0.42, 1.00, 0.65 |
  | calibration message_kind | 12 | 0.30 (10 accepted), 0.56, 0.40 | 0.67 (9 accepted), 2.12, 0.94 |
  | calibration preference_scope | 4 | 0.33 (3 accepted), unknown, 0.85 | 1.00 (1 accepted), 2.24, 1.10 |

  `unknown` log loss means a true class was given probability 0 (infinite
  loss), which spec 004 reports as unknown rather than as a number.
  final_test has no preference_scope case. Provider confidence is a
  provider statistic only. The content is of synthetic provenance, so none
  of this is evidence about real user data (R-04, R-28).
- Two earlier runs failed on the defects above: 50 exchanges at HTTP 404,
  then 50 at HTTP 400. One one-case check passed before the run recorded
  here. The failed runs' charges are `unknown` and stay liability at their
  full reservations (4,995,312 units).
- Spend for stage 3, its check and the failed runs: estimated 1,950,900
  units and liability 4,995,312 units. The shared testing journal now
  stands at 1,421,364 observed, 3,465,372 estimated (including spec 044's
  batching run) and 18,075,006 liability units: USD 0.023 of R-29's
  USD 5.
- The raw records (the binding document, bundles, exchanges, reports and
  the evidence index with checksums) stay outside this repository, in the
  host application's retained evidence store beside spec 012's.

Stage 3 is recorded. Still open, so 022 stays `in-progress`: 3.7.5's
implementation, which accepts a stage 3 record and lets `new_production`
construct a production-eligible binding, and the owner's acceptance of this
record in its own decision row. `JevBackend::new_production` still
refuses ("direct API mapping awaits owner-authorized qualification (stage
3)"), so no binding is production-eligible yet. Production use also stays
the owner's separate acts (3.1.4).

**Production acceptance (2026-10-29, A-69).** 3.7.5 is amended in, so a
recorded stage 3 can be accepted and a production-eligible binding
constructed, as 3.7.4.4 left to a later change. Pending implementation.
Accepting the first record, for the stage 3 binding of A-68, is the owner's
act in its own decision row; this amendment accepts none.

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

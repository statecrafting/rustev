---
id: "022-jev-production-transport"
title: "Production transport and served identity for the Jev adapter (amends 012)"
status: draft
implementation: deferred
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
  is never relabeled as production evidence. Draft and deferred: blocked on
  owner decisions and external evidence; it authorizes no live call, spend,
  credential use or deployment.
amends:
  - "012-jev-integration"
depends_on:
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
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

Draft, and deferred: a proposal, not a claim about code. It is the
"separately approved spec 012 amendment" that backlog item RUSTEV-010 names,
and it makes spec 012 R-3 and owner decision R-28 item 2 concrete. Spec
012's approved text is not edited; this spec records the change (R-16).

Deferred because it depends on facts that do not exist yet: whether
TypeSafe's direct API is available to the owner with a pinned version, its
request and response shapes, its data-retention terms, and whether the
Vercel AI Gateway can pin a version. No stage of section 3.7 runs as part of
drafting or approving this spec; each live stage needs its own owner
authorization of scope and spend, as R-27 and R-29 did for spec 012.

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
| Credential | The credential's custody class (host-injected at construction, never in identity or evidence); never its value. |

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

## Acceptance (draft)

- Stage 1 of 3.7 passes over loopback with zero external calls.
- Every negative case of section 5 has a failing test before the fix that
  makes it pass.
- Stages 2 and 3 run only under an explicit owner authorization and are
  recorded, with spend, in this spec's implementation record.

## Open questions

1. **Transport choice.** Qualify the direct API (recommended: it is the only
   documented pinning path), or first seek evidence for Gateway pinning?
2. **Access.** Does the owner hold, or intend to obtain, a direct TypeSafe
   API credential, and under which account and terms?
3. **Privacy.** Which data classes may reach the production transport? Spec
   012 3.1.3 still forbids real user mail pending a separate decision.
4. **Requalification dataset.** Reuse travel-memory's synthetic-provenance
   set (limit stated), or wait for a spec 023 admitted dataset?
5. **Testing spend.** Is spec 022's live testing charged to the USD 5
   testing cap of R-29 (about USD 4.99 remaining per spec 012's stage 3
   record), or to a new cap?

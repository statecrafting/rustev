---
id: "012-jev-integration"
title: "Jev integration (remote decision backend)"
status: approved
implementation: complete
created: "2026-09-24"
summary: >
  A remote decision backend under `integrations/` that answers Rustev's
  `proposition`, `classify` and `rubric` steps by calling TypeSafe's Jev
  model, through the Vercel AI Gateway by default or TypeSafe's own API as
  an alternative transport, as an ordinary integration. It conforms to Part
  A of spec 009: explicit served identity (unknown on the Gateway), one
  total budget, unconfirmed cancellation once sent, closed error codes,
  estimated cost, requested privacy options, and exchange records. Many
  Rustev questions over one state go in one request. Provider confidence is
  recorded, never treated as calibrated. `rank` is not declared. No Jev
  wire-compatible surface is exposed. Paid inference for smoke and
  qualification is authorized by R-27 (zero data retention, synthetic and
  independently labeled data only); testing is capped at USD 5 in total and
  production at USD 25 per month (R-29), and production requires a pinned
  model version (R-28). Approved (A-09); implemented.
establishes:
  - { kind: directory, path: "integrations/rustev-jev/" }
extends:
  # Adds the integration crate's manifest and dependencies to the workspace.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 012 to `make verify` once concrete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/discussions/002-user-jev-assessment-and-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "The Jev adapter grants no authority: its outputs are model-derived raw outputs or classified failures, and nothing Jev returns changes a plan, policy, retry, fallback or budget."
    anchor: "3-1-position-and-names"
  - id: "I-2"
    kind: invariant
    text: "Provider-reported confidence, argmax and interpolated score are recorded as provider-reported and are never supplied to the core or labeled calibrated."
    anchor: "3-4-answers-into-rawoutput"
  - id: "I-3"
    kind: invariant
    text: "On the Gateway transport the served model identity is recorded as `unknown`; the requested model id or a routing slug is never recorded as the served identity."
    anchor: "3-6-identity-routing-and-cost"
  - id: "I-4"
    kind: invariant
    text: "The adapter sends each attempt at most once, never retries or switches transport, model or provider on its own; any alternative is a separate adapter instance named as a runtime fallback target in the plan's execution policy."
    anchor: "3-2-transports"
  - id: "I-5"
    kind: invariant
    text: "Rustev exposes no Jev wire-compatible endpoint, and no crate outside `integrations/` names Jev, TypeSafe or Vercel."
    anchor: "3-1-position-and-names"
  - id: "R-1"
    kind: requirement
    text: "The descriptor declares `proposition` (2 options), `classify` (up to 255 options) and `rubric` (up to 10 levels) with output `distribution`, determinism `unspecified`, and no `rank`."
    anchor: "3-3-questions-and-the-descriptor"
  - id: "R-2"
    kind: requirement
    text: "Live calls and paid inference happen only within R-27: smoke and qualification of this spec, key read from the owner's file at runtime and never copied, zero data retention requested, synthetic or independently labeled data only, within R-29's USD 5 total testing cap, enforced as a hard stop before dispatch; no quality claim precedes the R-04 dataset."
    anchor: "3-8-qualification-plan"
  - id: "I-6"
    kind: invariant
    text: "The Gateway API key is never copied into a repository, log, fixture, exchange record or other evidence."
    anchor: "3-2-transports"
  - id: "R-3"
    kind: requirement
    text: "Production binds only a transport with a pinned, verified model version: the direct TypeSafe API with a pin, or the Gateway once its pinning is verified and recorded; `served: unknown` is for development only."
    anchor: "3-6-identity-routing-and-cost"
---

# 012: Jev integration (remote decision backend)

Approved (A-09, 2026-09-24) and implemented. Ordinal: the next free one;
`010` stays reserved for the roadmap's general integrations increment and
`011` is taken. Rationale: owner decision R-26 (Rustev is the decision
engine of the travel-memory product, with Jev through the Vercel AI Gateway
as a remote backend), R-27 (paid inference for smoke and qualification),
R-28 (the owner's answers to this draft's questions), R-29 (spend caps),
R-04, R-05, R-10 and
R-23 in `docs/decisions/00-founding-decisions.md`; design section 1.2 ("a
compatibility adapter may be written later as an ordinary integration");
discussion `002` for background only. Facts about the provider are C-09 in
the decisions record, verified from Vercel's documentation on 2026-09-24;
anything not in C-09 is marked unverified here.

This spec **amends no approved spec**. It needs spec 009 for the
remote adapter obligations and for shared-state batching.

## 1. Purpose

Let a Rustev plan bind a hosted decision model that answers closed-set
questions with probabilities, without giving that model any authority,
without importing its vocabulary into Rustev's core, and without treating
its self-reported confidence as evidence of calibration. The value of the
integration is measured, not assumed (R-04): until qualification, it is a
working adapter with no quality claim.

## 2. Territory

Nothing is claimed until the implementing change. Then it claims
`integrations/rustev-jev/`: a `DecisionBackend` implementation, its binding
document `rustev.jev-binding/1`, the mapping of 3.3 and 3.4, and recorded
test fixtures. It depends on `rustev-contract`, `rustev-core`, spec 009's
adapter machinery, an HTTP client and Tokio. No crate depends on it; the
host (for example the travel-memory application) constructs it and
registers it with the runtime.

## 3. Behavior

### 3.1 Position and names

1. Rustev's primitives stay `proposition`, `classify`, `rubric` and `rank`
   (R-05). Jev's names (`boolean`, `choice`, `score`) appear only inside this
   integration crate. Nothing here claims that Rustev is Jev-compatible, and
   Rustev serves no Jev wire format (design section 19).
2. The adapter is a remote adapter under spec 009 Part A: outputs are
   `model-derived`, it supplies only raw outputs or classified failures, and
   provider metadata goes to exchange records (spec 009, 3.9).
3. Content sent to the provider is exactly the step's projection and
   question material (3.3). Hosts decide which inputs a plan projects;
   under R-27 only the travel-memory synthetic fixture corpus and
   independently labeled evaluation data may reach the provider; real user
   mail may not until a separate privacy decision.

### 3.2 Transports

One adapter instance uses one transport, fixed in its binding document and
therefore in its artifact identity:

| Transport | Endpoint | Status |
|---|---|---|
| `gateway` (default) | `POST https://ai-gateway.vercel.sh/v1/evaluate`, `Authorization: Bearer <AI_GATEWAY_API_KEY>`, model `typesafe-ai/jev` | verified shape (C-09) |
| `gateway_typesafe_base` | the Gateway's TypeSafe-compatible base under `/typesafe` | documented (C-09); request and response details unverified |
| `direct` | TypeSafe's own API, with a version pin such as `jev-1.13.0` | pinning documented by TypeSafe; unverified here |

The Gateway's OpenAI-, Anthropic- and Cohere-compatible endpoints do not
support Jev (C-09) and are not transports. The credential is read by the
host and passed to the adapter at construction; it is never logged, never
in the binding identity and never in an exchange record. For development and
qualification (R-27) the harness reads `AI_GATEWAY_API_KEY` at runtime from
`/Users/bart/.config/statecrafting/infra/vercel/.env`; the key is never
copied into a repository, log, fixture or evidence, and recorded fixtures
keep no request headers. For production, see 3.6.2. Switching
transport, or falling back from one to another, is a runtime fallback
between two adapter instances declared in the plan's execution policy, so
it is in plan identity (R-10) and never happens inside an adapter.

### 3.3 Questions and the descriptor

1. **Operations.**

   | Rustev step | Jev question | Options | Output supplied |
   |---|---|---|---|
   | `proposition`, options exactly `["false", "true"]` | `boolean` | 2 | `distribution` |
   | `classify` | `choice` | 2 to 255 labels | `distribution` over the labels |
   | `rubric`, levels low to high | `score` | 2 to 10 levels (direct API documentation) | `distribution` over the levels |
   | `rank` | none | n/a | not declared |

   The descriptor declares these three operations with output
   `distribution`, `max_options` 2, 255 and 10, determinism `unspecified`,
   and an input limit in canonical projection bytes with `on_excess:
   refuse`, set conservatively below the model's 32,000-token context
   (C-10): 49,152 bytes by default, which is about 20,500 tokens at the
   2.4 bytes per token observed in C-11 and leaves room for question
   material and variance in the ratio. The binding may lower it. Plans require `distribution`,
   `ordinal_distribution` or, with a calibration bound to this adapter's
   artifact, `calibrated_probability` (spec 002, 3.9.3).
2. **Rank is not native** and is not synthesized. The descriptor omits
   `rank`, so a plan that needs a ranking composes it where it is visible and
   identified: a fan-out `rubric` or `proposition` over candidates answered
   by this adapter, and the exact `weighted_rank@1` operator (spec 002, 3.8).
   An adapter-made `scores` output would invent a scale Jev does not return.
   Whether to offer anything more is open question 2.
3. **State.** The Jev `state` is the JSON object holding the projection's
   `values` and `instance` members, as canonical JSON. Content stays in the
   state and the judgment in the question, as the provider's own guidance
   recommends.
4. **Question.** Each Rustev request becomes one entry of `questions`,
   keyed `q0`, `q1` and so on in item order (never by step name, which may
   carry meaning the host does not want to disclose). `instructions` is the
   step's `question`. `criteria` has the shape the Gateway accepts (C-11):
   for `boolean` the object `{"false": d, "true": d}`, for `choice` an object
   from each label, in declared order, to its description, and for `score`
   an array of level descriptions from low to high. Each description comes
   from the binding's option description table when one is given, otherwise
   it is the option label itself.
   The table and the mapping version are in the artifact identity (spec 009,
   3.2.3), so changing a description changes the `PlanId` of dependent
   plans.
5. **Plan check.** Like spec 005 3.11.4, `check_plan(plan)` lists every step
   bound to this adapter whose options fall outside 3.3.1 (for example a
   one-level rubric, or a proposition whose options are not exactly
   `["false", "true"]`). Skipping it moves each mismatch to a `capability`
   failure per request.

### 3.4 Answers into RawOutput

Spec 009 3.5 applies. The Gateway's answer shapes were observed in C-11
(`answers.<key>` with `type` and the fields below); a change of shape
changes only the mapping, under a new mapping version. An answer whose
`type` differs from the question's is `malformed_response`.

1. `boolean`: the field `probability`, the probability `p` that the
   proposition is true, becomes
   `{"false": 1 - p, "true": p}`, the lossless reshaping of spec 009 3.5.4.
2. `choice`: the per-option `probabilities` become the distribution, keyed by
   the labels sent as criteria. Keys are not added, removed or renamed; a
   missing or extra key reaches the core's validation as received.
3. `score`: the per-level `probabilities`, keyed by level index `0` to `k-1`,
   become the distribution keyed by the rubric's level at that index. Keys
   that are not exactly `0` to `k-1` are `malformed_response`. The
   interpolated `score` (the expectation) is not supplied; the core computes
   its own expectation from the distribution (spec 002, 3.5.4).
4. **Provider extras.** The argmax `choice`, any `confidence` (per answer
   or in `providerMetadata.typesafe.confidence`), the interpolated `score`
   and any legend are kept in the exchange record as
   provider-reported. `confidence` is a transform of the distribution's peak
   (discussion `002`), not a probability of being correct, and it is never
   supplied, compared to a threshold, or called calibrated. A calibrated
   probability exists only as spec 002 3.6 defines it: a calibration
   artifact fitted by spec 004 on labeled data and bound to this adapter's
   artifact.

### 3.5 Many questions, one state

Jev answers several questions against one state in one request (C-09). The
adapter uses spec 009's shared-state batching (3.10.4 there): attempts of
one decision whose projections share identical `values` and `instance`
members are sent as one request with one question each, within a declared
coalescing window that never passes the attempt budget. Each attempt keeps
its own id, question key, output, failure and cancellation. A fan-out over
candidates does not batch across candidates, because each candidate's
instance differs; it batches the questions asked about the same candidate.
Answers do not share hidden context across questions, as the provider
documents, but Rustev does not rely on that claim: each answer is validated
on its own. Batching is off by default (R-28); it is enabled for this
adapter only after stage 3 of 3.8 shows batched and unbatched answers
agree within the qualification's stated bounds. Unbatched, each attempt is
one request.

### 3.6 Identity, routing and cost

1. **Artifact identity** is the digest of the `rustev.jev-binding/1`
   document: transport, model id `typesafe-ai/jev`, version pin (direct
   transport only), provider options (3.7), mapping version, option
   description table, request limits and batching configuration.
2. **Served identity.** On the Gateway transports the response does not
   expose a resolved model version (C-09), so `served` is `unknown`. The
   response's `model` field is recorded as `requested_model` echo, and
   `providerMetadata.gateway.routing` (`originalModelId`, `resolvedProvider`,
   `canonicalSlug`, `finalProvider`) as `route`; `generationId` as
   `generation_id`. On the direct transport with a pin, the response's model
   field is recorded as `reported`, and a value other than the pin is
   `identity_mismatch`. Whether the Gateway honors a version pin is
   unverified. `served: unknown` is acceptable for development and
   qualification only; production binds the direct transport with a pinned
   version unless Gateway pinning is verified and recorded in the decisions
   record (R-28).
3. **Consequence for claims.** With `served: unknown`, a change of model
   version behind the Gateway cannot be detected per call. Every evaluation
   report, calibration artifact and qualification record for this adapter
   names the binding identity and the dates of the calls it measured, and
   states that the served version was not observable.
4. **Cost.** `cost_model` is `estimated`; `cost_bound` is an estimate from
   projection and question bytes under the binding's declared bytes-per-token
   ratio and the declared price table; a `hard` cost policy therefore refuses
   plans that reach this adapter (spec 003, 3.5.3), and travel-memory plans
   use `estimated`. The charge is `observed` from
   `providerMetadata.gateway.cost`, a decimal USD string (C-11), when present
   (converted by the declared unit rate, rounded up), else `estimated` from
   `usage.inputTokens` and `usage.outputTokens`, else `unknown`. Reported
   cost, `marketCost` and usage are kept verbatim in the exchange record.
   The default price table is the public one of C-10: USD 0.042 per 1M
   input tokens and no output price; the default estimation ratio is 2
   bytes per token, below the observed 2.4, so estimates err high. The
   default unit is one nano-USD (USD 10^-9), so the caps of R-29 are
   5,000,000,000 and 25,000,000,000 units.
5. **Cancellation.** No cancel operation is documented. Before any request
   byte is sent a cancellation acknowledges `stopped` with `observed{0}`;
   after that it is `unconfirmed` with an `unknown` charge, and the run
   record shows `possibly_continuing` (spec 009, 3.4).

### 3.7 Privacy options

The binding sets, by default, the Gateway provider options
`zeroDataRetention: true` and `only: ["typesafe-ai"]` (C-09). They are sent
on every request and recorded as requested; the adapter does not claim that
retention or routing was honored. `only` is mandatory and must be exactly
`["typesafe-ai"]`: the other listed provider keeps data (C-10), so a
binding that omits or widens it is refused at construction and no request
is sent. `zeroDataRetention` may be set `false` only as an explicit binding
choice under R-30 (the Gateway refuses it on the Hobby plan, C-11), for
synthetic and independently labeled synthetic-provenance data only; the
choice is in the artifact identity. Because they can change routing, they are
in the artifact identity. The attempt id and decision id are not sent
(spec 009, 3.3.3); the exchange record joins `generationId` to the attempt
id locally. Exchange records are digest-only by default under R-19.

### 3.8 Qualification plan

Three stages. Stages 2 and 3 are authorized by R-27 within its scope: the
Gateway transport, zero data retention and the provider allowlist on every
request, the key read at runtime from the owner's file, only synthetic or
independently labeled data. Spend caps are the owner's (R-29): USD 5 in
total for all testing (smoke, qualification and evaluation runs together)
and USD 25 per calendar month for production use. Each cap is held by a
shared cost ledger that persists across runs (spec 003, 3.5.2), and the
adapter's accounting stops before dispatch any attempt whose reservation
would take the ledger over its cap: a hard stop, recorded as
`budget_exhausted{cost}`. Because this adapter's per-call amounts are
estimates, not bounds, a Gateway dashboard budget set to the same caps is
the backstop. No stage runs as part of drafting or approving this spec.

1. **Mechanics (no network, no spend).** Recorded Gateway responses are
   replayed through a local HTTP test server: every mapping in 3.4, every
   spec 009 error code, batching with a cancelled member, late responses,
   cost paths, served identity `unknown`, and both reference plans through
   the runtime with capture and offline replay (spec 004). Recorded responses
   come only from calls made under R-27, or are hand-written and labeled
   synthetic (R-04, R-09). They establish mechanics only.
2. **Smoke (under R-27).** A bounded number of live calls under an
   owner-set spend cap, confirming the unverified shapes of 3.4 and 3.2 and
   recording them as fixtures. No quality statement.
3. **Qualification (under R-27 and R-28).** On the independently labeled,
   synthetic-provenance evaluation set travel-memory supplies (R-28), with
   its provenance, labeling method, splits and limitations recorded under
   R-04 before use, per task: accuracy or macro-F1, log loss,
   Brier score and binned calibration error with every denominator,
   coverage against error for the plan's thresholds, and the same metrics
   by slice, including arithmetic, counting, date comparison, multi-hop and
   adversarial content, where Jev 1.13 documents weaknesses. Spec 005's
   rules backend is the comparison baseline in the same spec 004 report.
   A temperature calibration may be fitted on a calibration split and
   qualified on a separate one (spec 004, 3.6). Provider confidence is
   reported as a provider statistic only. The report states 3.6.3, and that
   the content is of synthetic provenance, so it is not evidence about real
   user data (R-04, R-28). It also compares batched with unbatched answers
   (3.5).
4. **Plan authoring guidance** that follows from the documented weaknesses
   and holds regardless of qualification: counting, arithmetic and date
   comparison are exact steps (spec 002, 3.8), never questions; multi-hop
   judgments are decomposed into steps; untrusted text reaches Jev only as
   state, and no answer grants anything (spec 001, 3.2).

## 4. Out of scope

A Jev wire-compatible API or server (R-05, design section 19). The
travel-memory application's plans and data. Training, fine-tuning or
distillation. Streaming. Any claim of quality, calibration or equivalence
with other backends before stage 3. Paid inference outside R-27, and any
real user data before a separate privacy decision.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A `rank` step requires `scores` and only this adapter is offered | `no_capable_backend` at compile time (spec 002, category 5). |
| A 12-level rubric is bound | Not satisfiable (`max_options` 10); the compiler refuses or takes the declared fallback. |
| A `choice` answer lacks one label's probability | Supplied as received; the core records `invalid_output`. |
| A `score` answer is keyed by level names instead of indices | `malformed_response`. |
| An answer carries `confidence: 0.95` | Recorded as provider-reported; not supplied. |
| The Gateway response has `model: "typesafe-ai/jev"` | `served: unknown`; the value is recorded as the echo. |
| The direct transport pinned `jev-1.13.0` returns another version | `identity_mismatch`. |
| HTTP 429 | `rate_limited`, class `overloaded`; retried only if the step's execution policy says so. |
| Cancellation after the request was written | Ack `unconfirmed`, charge `unknown`, `possibly_continuing`. |
| A plan under a `hard` cost policy reaches this adapter | Refused when prepared (spec 003, 3.5.3). |

## Acceptance

- Stage 1 of 3.8 passes with no network access beyond loopback, and a
  counter shows zero calls to any external host.
- The boundary check passes; no crate outside `integrations/` names the
  provider.
- Stages 2 and 3 run only within R-27 and R-29 and are recorded, with their
  spend against the testing cap, in this spec's implementation record.

## Implementation record

Delivered 2026-09-25 on spec 009's Part A machinery and spec 013's remote
end.

- `integrations/rustev-jev/`: `binding` (the `rustev.jev-binding/1`
  document and its digest as artifact identity; transports `gateway`,
  `gateway_typesafe_base` and `direct` with a pin; provider options refused
  at construction unless `only` is exactly `["typesafe-ai"]`; the option
  description table, request limits with the 49,152-byte default, batching
  off by default; `CostConfig` with the C-10 price table, 2 bytes per token
  and nano-USD units), `mapping` (3.3 and 3.4: `q<i>` keys, criteria shapes
  of C-11, `boolean` probability reshaped losslessly, `choice` keys as
  received, `score` keyed by index or `malformed_response`, provider extras
  kept in the exchange record only), `adapter` (`JevBackend`, the
  `DecisionBackend`: descriptor of R-1, `check_plan`, one send per attempt,
  served `unknown` on the Gateway and `identity_mismatch` on a pinned direct
  transport, shared-state batching when enabled, exchange records to the
  host sink), `spend` (the persistent `SpendJournal`: one file per budget,
  an exclusive lock, a reservation before dispatch refused over the cap,
  settlement to observed, estimated or liability, testing never resets and
  production resets each UTC month) and `net` (a count of exchanges by
  destination). Spec 009's Part A sources are compiled in by path, since
  spec 001 3.4.4 forbids depending on a crate under `integrations/`.
- Stage 1 (no network): `cargo test -p rustev-jev --locked`, against a
  loopback fake Gateway, the recorded calls of C-11 and labeled synthetic
  fixtures: every 3.4 mapping, every spec 009 error code the Gateway can
  produce, cancellation before and after sending, one total budget, cost
  paths, privacy options, the credential only in the bearer header,
  batching with a cancelled member and late answers, both reference plans
  through `rustev-runtime` with capture and offline replay making no
  Gateway call (`tests/runtime.rs`), and the spend journal
  (`tests/spend.rs`: an attempt over the cap is refused unsent in the
  adapter, and through a runtime whose shared ledger is seeded from the
  journal it ends `budget_exhausted{cost}` before reaching the adapter;
  persistence, liability on reopen, reconciliation, one writer, a torn or
  corrupt line, and both periods). The adapter's own refusal is a backstop
  reported as `remote:capability` with the refusal text; a host that wants
  the runtime's `budget_exhausted{cost}` seeds its shared ledger with
  `SpendJournal::shared_runtime_ledger`. Tests that exercise the transport
  assert that only loopback was contacted.
- Stage 2 (smoke, R-27, R-29, R-30): `examples/smoke.rs`, run by hand with
  `RUSTEV_JEV_LIVE=1`, the key read at runtime and never written, every
  attempt reserved against a persistent testing journal. On 2026-09-25 four
  calls on SYNTHETIC fixtures (two `boolean`, one `choice`, one `score`)
  all mapped as 3.4 says, routed to `typesafe-ai`, served `unknown`;
  Gateway `cost` was `0` on each (charged USD 0), `marketCost` totalled USD
  0.00006702 for 1,596 input tokens. The public price after the promotion,
  read 2026-09-25 from the Gateway's model list, is USD 0.000000042 per
  input token and 0 per output token, the default table's. Testing spend to
  date, with the five calls of C-11: USD 0 of the USD 5 cap.
- Stage 3 (qualification, R-27, R-28, R-29, R-30, R-32), recorded below.
  Batching stays off: the batched and unbatched answers agreed on this
  subset (see "Batched against unbatched"), but 11 messages do not qualify
  batching for production. That decision stays with the owner (R-28).

### Stage 3 qualification record (2026-09-26)

**Set.** travel-memory's synthetic-provenance qualification set
(`evaluation/qualification/`, tm spec 012, R-28). Primary identifiers are
the tree `178d6432d3c60bfe8da8f3be0258d3cb17850f96` and the manifest digest
`sha256:6eda107b4ac26323bef6253340aaef8c86cfad881a62212aa941c9608eb78626`,
read at tm commit `c609ce0ded69e41ee2ecc9e4c11ba98239c02188`. That commit
may change when tm's history is rewritten; the tree and digest do not. The
set was verified file by file against its manifest before use. Its
provenance, labeling method, splits and limitations are tm's (R-04). No
fixture text or label is in this repository. The raw records (bundles,
exchanges, reports, the spend journal) stay outside it, under
travel-memory's `handoffs/rustev/qualification-20260925/`. Each case's
snapshot id was computed from its input as `rustev.snapshot/1` before the
dataset id was computed. Task adapters are `tm.relevance`, `tm.message_kind`
and `tm.preference_scope` (one enum rule each).

**Scope run.** The whole-run projection was 73 requests and USD 0.0036
(USD 0.0072 with a 2x margin), from the Gateway price read at
2026-09-25T23:06Z (input USD 0.000000042 per token, output 0). That was
within the cap. The upstream then admitted about one request per 300 s and
answered HTTP 429 otherwise (C-13). The run was therefore cut to a
documented subset, the `final_test` split:

- 11 relevance and 11 message_kind cases, one request per question;
- the same 11 messages batched, with relevance and message_kind as two
  questions on one state;
- 0 preference_scope cases, because the set has none in `final_test`.

The calibration split was not run live, so no temperature was fitted. Every
request used the Gateway transport with `only: ["typesafe-ai"]` and without
`zeroDataRetention` (R-30, R-32). Every attempt was reserved against the
persistent testing journal. Decisions were paced 300 s apart, with retries
60 s apart on `overloaded` and `transient`. The plans used an `estimated`
cost policy.

- Binding identities: unbatched
  `sha256:87093d26ecf3b77c32ceae65fa3aea12f312a0c066c90964d3f14bfbee8a12e0`,
  batched
  `sha256:63efde86dc526f8e9fb16767d796c51fdf1555186255f579c9db59d46ac894f6`.
- Calls measured, all 2026-09-26 UTC: relevance 01:59 to 02:49; message_kind
  02:57 to 03:47; batched 03:57 to 04:47.
- Served identity: `unknown` (3.6.3). A model version change behind the
  Gateway during these windows would not have been visible.
- Routing: every answered request resolved and finished on `typesafe-ai`.
- Content: of synthetic provenance, so none of this is evidence about real
  user data (R-04, R-28).

**Plans.** One per task: a `classify` step, then escalation below 0.5 top
mass, declared `uncalibrated_threshold` (the distribution is provider-made,
I-2), then a proposal with the top label. The spec 005 baseline is an
authored keyword rules program, written from the answer spaces alone and
never tuned on the set. It ran over the same snapshots with the same plans.

**Spec 004 reports.** Each backend has its own baseline report. For a given
task, both reports share the dataset, split, evaluator configuration
(version 2, rules bound, spec 014) and task adapter:

| Task | Dataset | Config |
|---|---|---|
| relevance | `sha256:9a12944c3adaa638c292d518faf4b247d4ee6083e3fdc8d05217405b9bf080e1` | `sha256:a034ffbc2777293e21a6c59cda6f3eaf5179ba4fb6e1d113caccda12f1790434` |
| message_kind | `sha256:7d9e9197e4d3d8ca17a3bd49896783ec855446727de8d2ebb8669b6cba3551db` | `sha256:4d5e6d2b474dc05027dff3547a5d1f44ab2f76205b1bb72c501d550831e12b57` |

3.8 item 3 asks for the rules baseline "in the same spec 004 report". This
record does not meet that literally. Spec 004's candidate mode reuses only
outputs of an identical request, so evaluating the rules plan as a
candidate of the Jev bundles makes every case `incomparable
{request-mismatch}`. `gate` also refuses two baseline reports ("the
baseline and candidate roles do not hold"). The comparison below is
therefore side by side over identical dataset, split, configuration and
adapter identities. A single cross-backend report would need a spec 004
amendment, and none is proposed here.

`final_test` (n = cases; labeled n in parentheses where it differs):

| Metric | relevance, Jev | relevance, rules | message_kind, Jev | message_kind, rules |
|---|---|---|---|---|
| cases | 11 | 11 | 11 (10 labeled) | 11 (10 labeled) |
| coverage (usable bundles) | 1.0 | 1.0 | 1.0 | 1.0 |
| acceptance coverage (proposals) | 11/11 | 11/11 | 11/11 | 9/11 |
| accuracy, top label over labeled | 11/11 | 8/11 | 7/10 | 7/10 |
| macro-F1 over gold classes | 1.00 | 0.61 | 0.87 | 0.83 |
| error among accepted (spec 004) | 0/11 | 3/11 | 3/10 | 3/8 |
| mean log loss (spec 004) | 0.0103 (n 11) | 0.750 (n 11) | unknown: p(gold) = 0 on one case | 1.290 (n 10) |
| log loss, p clamped at 1e-6 | 0.0103 | 0.750 | 2.001 (n 10) | 1.290 |
| Brier (spec 004) | 0.0012 (n 11) | 0.458 (n 11) | 0.544 (n 10) | 0.585 (n 10) |

Binned reliability, from spec 004's bins [0, 0.2, 0.4, 0.6, 0.8, 1]; each
entry is cases, mean top mass and accuracy. Empty bins are unknown.

| Run | Bin 0.4 to 0.6 | Bin 0.6 to 0.8 | Bin 0.8 to 1 |
|---|---|---|---|
| relevance, Jev | none | none | 11, 0.990, 1.00 |
| relevance, rules | 1, 0.576, 1.00 | none | 10, 0.844, 0.70 |
| message_kind, Jev | none | 1, 0.700, 1.00 | 9, 0.953, 0.67 |
| message_kind, rules | none | 8, 0.674, 0.63 | none |

The rules backend's message_kind also has 2 cases in bin 0.2 to 0.4 (mean
0.254, accuracy 1.00).

Coverage against error over labeled answered cases, as accepted/wrong at
top-mass thresholds 0, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95 and 0.99. The plans'
own threshold is 0.5.

| Run | 0 | 0.5 | 0.6 | 0.7 | 0.8 | 0.9 | 0.95 | 0.99 |
|---|---|---|---|---|---|---|---|---|
| relevance, Jev | 11/0 | 11/0 | 11/0 | 11/0 | 11/0 | 11/0 | 10/0 | 9/0 |
| relevance, rules | 11/3 | 11/3 | 10/3 | 10/3 | 10/3 | 0/0 | 0/0 | 0/0 |
| message_kind, Jev | 10/3 | 10/3 | 10/3 | 10/3 | 9/3 | 7/2 | 7/2 | 5/1 |
| message_kind, rules | 10/3 | 8/3 | 8/3 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 |

By slice, as correct/n over labeled cases. Slices overlap, and the slice
tags are single-annotator.

| Slice | relevance, Jev | relevance, rules | message_kind, Jev | message_kind, rules |
|---|---|---|---|---|
| adversarial | 1/1 | 1/1 | 1/1 | 0/1 |
| counting | 2/2 | 2/2 | 1/2 | 1/2 |
| date comparison | 7/7 | 7/7 | 4/6 | 5/6 |
| multi-hop | 6/6 | 5/6 | 3/5 | 3/5 |
| arithmetic | no case | no case | no case | no case |
| certainty clear | 10/10 | 7/10 | 6/9 | 7/9 |
| certainty borderline | 1/1 | 1/1 | 1/1 | 0/1 |

Provider confidence, as a provider statistic only (I-2): the mean was 0.885
over 11 unbatched message_kind answers and 0.926 over 22 batched answers.

**Batched against unbatched** (3.5). All 22 pairs (11 messages, 2 questions)
were comparable:

- the top label agreed on 22/22;
- total variation between the two distributions was at most 0.080, with a
  mean of 0.0077.

**Cost** (testing journal, nano-USD units).

- Stage 3 settled 253 attempts. The unbatched runs, including three
  earlier attempts cut short by HTTP 429 (their bundles are kept apart and
  are not in the reports), account for 231: 26 answered with an observed
  charge of 0 and 205 answered HTTP 429 with an unknown charge. The batched
  run accounts for 22, one per question over 11 requests, all answered: 20
  with an observed charge above 0 and 2 with an observed charge of 0.
- Observed charges: 450,072 units (USD 0.00045), all on the batched run.
  Every unbatched answer, the last at 03:47 UTC, reported Gateway `cost`
  "0"; charging began within the batched run's window.
- Liability held for the unknown charges: 10,672,914 units (USD 0.0107).
- Testing total to date, with smoke and probes: observed USD 0.00045 plus
  liability USD 0.0107 of the USD 5 cap (R-29).
- The raw exchange bodies of the 11 relevance decisions were lost when the
  host process was killed after they finished. Their bundles, run records
  and journal entries are kept. The runner now writes exchanges per case.

**Limitations.**

- The label mapping is under owner review: spam maps to marketing and
  non-travel to other; fixtures 10 and 22 are missing for message_kind
  (hence 10 labeled of 11); preference_scope has no `final_test` case;
  arithmetic has no `final_test` case; the slice tags are
  single-annotator.
- The samples are 10 to 11 cases per task, so no interval is claimed and
  none of these numbers is a production quality claim.
- The calibration split was not run.
- The served version is unknown (3.6.3).
- Zero data retention was not requested (R-30, R-32).

**Owner fact (2026-09-25).** The owner set the Vercel AI Gateway dashboard
budget to USD 5, equal to the testing cap (R-29), as the external backstop
of 3.8 (recorded in R-32).

## Verification

Run by `make verify` (012 is in `VERIFIED_SPECS`).

```verify:cli
# 3.2 to 3.7 and stage 1 of 3.8: mapping, errors, batching, cost, identity,
# against recorded fixtures on a loopback server; no external host.
cargo test -p rustev-jev --locked
# I-5: the provider is named only under integrations/.
sh -c 'if grep -rniE "jev|typesafe|vercel" crates tools backends --include=*.rs; then exit 1; fi'
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-jev --all-targets --locked -- -D warnings
```

## Resolved questions

Decided by the owner on 2026-09-24:

1. Paid inference (former O-02): authorized for smoke and qualification
   within R-27's scope.
2. Rank: composed in plans; no native or adapter-composed operation (R-28).
3. Option descriptions: stay in the adapter binding for now (R-28).
4. Pinning: `served: unknown` is for development only; production uses the
   direct API with a pin unless Gateway pinning is verified (R-28, 3.6.2).
5. Dataset: travel-memory supplies an independently labeled,
   synthetic-provenance set that may go to the provider under zero data
   retention (R-28).
6. Batching: off by default until qualified (R-28, 3.5).
7. Spend caps: USD 5 total for testing, USD 25 per month for production,
   hard stop in the adapter's accounting plus a Gateway budget (R-29).

8. Zero data retention: the Gateway flag may be omitted for synthetic data
   while the team is on the Hobby plan; the provider allowlist stays (R-30).

Settled from the recorded calls of C-11 before approval: the default
projection input limit (3.3.1), the request `criteria` shapes (3.3.4), the
Gateway's per-answer field names (3.4) and the cost fields (3.6.4).

## Open questions

None.

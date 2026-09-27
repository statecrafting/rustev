---
id: "008-reference-package-lodging"
title: "Reference package: lodging recommendation"
status: approved
implementation: complete
created: "2026-09-26"
summary: >
  The second reference domain package reserved by the architecture roadmap:
  `packages/rustev-pkg-lodging`, containing a bounded lodging-recommendation
  definition, canonical documents, synthetic mechanics fixtures, and
  evaluation material. The package preserves exact eligibility, explicit
  trust treatment, bounded semantic fan-out, abstention, lineage, offline
  replay, partial adoption, and backend-swap comparison without claiming
  semantic quality, external readiness, or authority to book lodging.
establishes:
  - { kind: directory, path: "packages/rustev-pkg-lodging/" }
extends:
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "007-support-routing-package"
  - "012-jev-integration"
  - "014-report-integrity"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
---

# 008: Reference package: lodging recommendation

Approved (A-14, 2026-09-26) as its own reviewable change before any code
(R-16); implementation is complete. Approval creates no claim that Aicortex,
Rahi, a semantic backend, real lodging data, or a live provider is ready. It
applies adopted decisions R-01 through R-08, R-31, and R-34, and follows the
package precedent approved in spec 007. The architecture
document, including its lodging example, remains context where an approved
spec or adopted decision does not make it normative.

## 1. Purpose

The lodging definition currently exists as reference-plan and evaluation test
material spread across core, runtime, evaluation, CLI, and integration tests.
No consumable domain package owns it. This spec proposes a package that makes
the definition and its mechanics fixtures available without adding lodging
behavior to the pure core.

The package demonstrates a harder reference shape than support routing:
deterministic eligibility before semantic work, a bounded candidate and claim
cross-product, explicit treatment of attributed claims, a mixed-derived rank,
and visible abstention. It also defines how that shape can be replayed,
evaluated, partially adopted, and compared across eligible backend bindings.

## 2. Territory and dependencies

When implemented, this spec owns
`packages/rustev-pkg-lodging/`. The crate name is
`rustev-pkg-lodging` and its normal dependencies are limited to
`rustev-contract` and `rustev-core`. Runtime, evaluation, CLI, backend, and
integration crates may be dev-dependencies only. The package performs no I/O,
reads no clock, uses no async runtime, and embeds its authored documents at
build time.

The existing `packages/*` workspace membership and package boundary rule apply.
Implementation may add lockfile entries and add 008 to `make verify`. A
backend-swap test may be added under the integration crate owned by spec 012,
but the package may not take an integration dependency.

No contract, core, runtime, evaluation, CLI, backend, or integration behavior
changes under this spec. If delivery discovers that one is required, delivery
stops at the package boundary until a separate amendment is reviewed.

## 3. Package contract

### 3.1 Definition and inputs

The package exposes a typed builder and a reference definition document for
`lodging-recommendation` version `1.0.0`. The reference shape remains the
existing spec 002 lodging definition:

1. `trip.dates`, `trip.party_size`, `trip.budget`,
   `trip.requires_step_free`, and `trip.text` are user-supplied inputs.
2. `traveler.claims` is a bounded list of attributed claims. Each claim has a
   stable identifier, statement, kind, trust class, and validity end. The
   accepted trust classes are `verified`, `stated`, and `inferred`; these names
   describe input provenance posture, not proof that Rustev performed
   verification.
3. `inventory.candidates` is a third-party list with a 15 minute freshness
   bound and a maximum of 200 candidates.
4. `fx.rates` is a system-of-record list with a one hour freshness bound and a
   maximum of 16 rates.
5. Text and list bounds are part of the definition. Oversized, invalid,
   missing, stale, or conflicting inputs become `Unresolved` under specs 002
   and 003. They are not truncated or replaced unless the document explicitly
   declares the permitted, visible runtime truncation behavior.

The builder parameters are limited to deployment choices that already have a
representation in the approved contract: backend constraints and bindings,
calibration references, the semantic request limit, shortlist size, component
weights, and thresholds. The recommended reference values are the existing
golden values: shortlist size 5, at most 128 semantic requests, and ranking
weights 0.5 for preference support, 0.3 for suitability, and 0.2 for price.
Changing any parameter changes canonical definition bytes and therefore the
compiled identity where specs 002 and 003 require it.

### 3.2 Exact eligibility and shortlist

Before semantic dispatch, exact steps:

1. require dates, party size, and budget;
2. filter candidates by availability, converted price within budget,
   occupancy, and the step-free constraint;
3. retain exclusion reason counts;
4. select at most the configured shortlist size by converted price, with the
   stable candidate identifier as the deterministic tie-breaker; and
5. select only current preference claims.

An empty eligible set produces `Propose(no_eligible_lodging)` with exact
eligibility evidence. Missing or stale required evidence produces
`MissingEvidence` or `Unresolved`, never the empty-set proposal. A shortlist is
not a booking choice and exact pre-ranking must not be described as model
quality.

### 3.3 Trusted context and semantic work

The definition contains three semantic tasks:

1. `lodging.intent`: a bounded `classify` task over trip text with labels
   `business`, `leisure`, `family`, and `other`;
2. `lodging.supports_preference`: a `proposition` over each shortlisted
   candidate and each current preference claim; and
3. `lodging.suitability`: a `rubric` over each shortlisted candidate with
   ordinal levels `poor`, `fair`, `good`, and `excellent`.

Only the projected fields declared by those steps may reach a backend. Claims
of kind `constraint` and expired claims do not enter semantic preference
support. A claim's trust class supplies only the explicit ranking weight table
in the definition: 1 for `verified`, 0.6 for `stated`, and 0.3 for `inferred`.
It does not authorize disclosure, establish truth, or override eligibility.

The runtime-planned fan-out and request ceiling are compiled before dispatch.
The number of semantic instances is a function of bounded shortlist and claim
counts, not raw inventory size. Any excess follows the definition's visible
`truncate_visible` policy and is retained in evidence. No hidden batching,
cache, retry, or provider call is introduced by this package.

### 3.4 Ranking, output, and abstention

`weighted_rank@1` combines the mean preference-support proposition weighted by
claim trust class, suitability rubric expectation, and shortlist price
position. Its result is `mixed-derived`. Component values, exclusions,
truncation, input lineage, semantic step lineage, and the compiled `PlanId`
remain available in the judgment or retained evidence as their owning specs
require.

The successful output is `Propose(present_ranked_lodging)` with the ranking,
shortlist, and eligibility evidence. It is a proposal for presentation only.
It is not a reservation, purchase, payment instruction, permission, or proof
of availability after evaluation time. No Rustev crate may turn it into an
authorization token or a `Permitted` value.

Any unresolved semantic value required by ranking propagates as `Unresolved`
under the declared policy. Invalid distributions, absent descriptors,
unsupported capabilities, deadline expiry, cancellation, unavailable backend,
cost refusal, and incomplete semantic results do not receive a neutral score
or a default rank.

### 3.5 Identity and repeatability

The package ships canonical reference documents and tests that its builder and
parsed definition enter one representation. Definition bytes, semantic task
labels, projection, parameters, calibration references, descriptors,
artifacts, bindings, exact-operator registry, and policy participate in
identity exactly as specs 002 and 003 require. A changed backend binding may
produce a different `PlanId` while leaving the policy and output declaration
structurally comparable.

Given one compiled plan, snapshot, evaluation time, and recorded semantic
answers, offline replay must reproduce the judgment or report the precise
identity or evidence mismatch. Replay does not refresh inventory, ask a
provider, or rewrite the historical record.

## 4. Evaluation and evidence

### 4.1 Synthetic mechanics set

The package contains authored, visibly SYNTHETIC fixtures covering all exact
branches, each semantic label or level used by mechanics, trust weighting,
ties, fan-out truncation, unresolved inputs, and backend failures. It includes
a `rustev.task-adapter/1` document and a digest-bound
`rustev.evaluator-config/2` document as required by specs 006 and 014.

Synthetic cases may demonstrate serialization, capture, replay, metric, gate,
and failure mechanics. They do not establish ranking usefulness, calibration,
provider behavior, accessibility accuracy, traveler preference truth, or
external readiness. The package and generated reports retain synthetic
provenance.

### 4.2 Quality evidence is separate

Any quality claim requires a separately governed dataset with retained
provenance, licensing and redistribution posture, privacy basis, split
discipline, exposure accounting, labels, evaluator configuration, and report.
At minimum, a future quality unit must define eligibility precision,
abstention correctness, ranking evaluation against held-out choices with the
exposure caveat, and per-preference calibration where calibrated probabilities
are claimed. This spec supplies none of that evidence and authorizes no use of
real-user data.

### 4.3 Backend swap

A mechanics-only swap test runs the same synthetic cases with:

1. the spec 005 synthetic rules backend; and
2. the spec 012 Jev integration using retained recorded exchanges and no live
   network call.

Both runs use the same dataset, split, task adapter, evaluator configuration,
definition parameters, and policy. Comparison follows the approved identity
and policy rules: plan identity differences are explicit, policy sections and
output declarations remain comparable, and any declared uncalibrated
threshold is matched on both sides. The result may be pass, fail, or unknown
and is mechanics evidence only. It does not establish Jev readiness or lodging
quality.

### 4.4 Partial adoption

Hosts may adopt the package definition and canonical documents without the
CLI, evaluator, Jev integration, or any remote backend. A host may bind a
different backend only when the descriptor satisfies every requested
capability and the resulting plan compiles. Unsupported evaluation or swap
material stays explicitly unavailable; it does not weaken runtime behavior.

## 5. Privacy, isolation, and resources

1. Trip text and attributed claims are untrusted data, not instructions. They
   are projected as data into fixed semantic tasks and cannot modify labels,
   questions, backend constraints, policy, or authority.
2. Synthetic fixtures contain no credentials, provider tokens, live endpoint,
   real inventory, or real traveler data.
3. The package introduces no storage or cache. Runtime evidence and evaluation
   artifacts inherit the retention, erasure, and isolation boundaries of
   specs 003 and 004.
4. Any backend invocation remains within one host-supplied execution context.
   Evidence, answers, and reports from another tenant, principal, artifact,
   definition, plan, snapshot, or dataset identity cannot satisfy the current
   request.
5. Request, projection-byte, deadline, and cost bounds are checked before
   dispatch where their owning specs require. Refusal or exhaustion is
   explicit and is never converted into a lower-cost recommendation.

## 6. Non-goals

- Booking, payment, authorization, inventory refresh, or post-judgment action.
- A production lodging recommender or a quality claim.
- A live provider call or a claim that Aicortex, Rahi, Jev, or another
  semantic backend is ready.
- General batching, duplicate suppression, inference caches, persistent
  caches, live re-execution, or dynamic questions.
- A second local semantic backend, wire-witness ingestion, action facade,
  custom training, distillation, distributed execution, or extension loading.
- Publication to a registry or a release of any crate.

## 7. Observable negative cases

| Boundary | Negative control | Required result |
|---|---|---|
| Authority | Treat `present_ranked_lodging` as booking permission | No compatible authorization value exists; the output remains a proposal. |
| Identity | Change shortlist size, weights, projection, binding, descriptor, or artifact | The applicable definition or plan identity changes; replay under the old identity is refused. |
| Resource | Candidate and claim fan-out exceeds the semantic request ceiling | The declared visible truncation or explicit refusal occurs before excess dispatch and is retained in evidence. |
| Isolation | Supply a recorded answer from another plan, snapshot, tenant, principal, artifact, or instance key | The answer is rejected or remains unresolved; no cross-context reuse occurs. |
| Failure | Required input is missing or stale, or a required semantic answer is invalid or unavailable | `MissingEvidence` or `Unresolved`, never an empty eligible set, neutral score, or default candidate. |
| Privacy | Trip text attempts to alter labels, policy, task question, backend, or authority | It remains projected data; the compiled plan and authority boundary do not change. |
| Replay | Offline replay lacks a recorded semantic answer | Replay reports the missing supply and performs no network call. |
| Evaluation | A synthetic report is presented as quality or calibration evidence | Provenance remains SYNTHETIC and the claim is rejected by review. |
| Dependency | The package gains a normal dependency beyond contract or core | `make boundaries` fails naming the package dependency rule. |
| Backend swap | The two bindings differ in policy or output declaration without a reviewed allowance | The comparison fails or is unknown; it cannot pass silently. |

## Acceptance

The implementation is acceptable when:

1. `cargo test -p rustev-pkg-lodging --locked` covers the package contract,
   canonical bytes, deterministic eligibility and ties, trust weights,
   abstention, identity changes, replay supplies, synthetic provenance, and
   the negative controls in section 7.
2. `cargo test -p rustev-jev --test lodging_swap --locked` exercises the
   recorded-only swap without a live endpoint and checks policy, output,
   identity, and report comparability.
3. `make boundaries` passes and a seeded forbidden normal dependency fails.
4. `make code` and `make verify` pass after 008 is added to the verified-spec
   set.
5. The implementation diff changes no core behavior. Any required change to
   an approved owner is first proposed as a separate amendment.

## Implementation record

Delivered 2026-09-26 as a separate reviewable change.

- `packages/rustev-pkg-lodging/src/lib.rs` owns the typed `Params` and
  `SemanticParams` builders, the canonical `lodging-recommendation` definition,
  its embedded documents and four-way SYNTHETIC mechanics set. The package's
  normal dependencies are only `rustev-contract` and `rustev-core`; it performs
  no I/O, reads no clock and uses no async runtime.
- The reference builder is byte-identical to spec 002's independent lodging
  golden. Tests also prove that the builder and parsed document use one
  representation, and that a shortlist, weight, semantic limit, backend or
  calibration parameter change changes canonical definition identity.
- `data/` contains the canonical definition, a `rustev.task-adapter/1`, its
  digest-bound `rustev.evaluator-config/2`, a `rustev.dataset/1` manifest and
  one snapshot for each of the training, model-selection, calibration and
  final-test splits. Every case and source is visibly SYNTHETIC. The generator
  in `tests/common/mod.rs` is the source for those authored documents, and the
  document suite checks the checked-in canonical bytes against it.
- `tests/behavior.rs` covers exact eligibility, exclusion counts, converted
  price ties and stable identifier tie-breaking; current preference selection
  and the verified, stated and inferred trust weights; mixed-derived ranking;
  missing and stale inputs versus an exact empty eligible set; invalid or
  unavailable semantic answers; visible fan-out truncation; recorded-supply
  replay and missing supplies; and hostile trip text remaining projected data
  under an unchanged plan and policy.
- `integrations/rustev-jev/tests/lodging_swap.rs` runs all four cases through
  spec 005's SYNTHETIC rules backend and the Jev adapter with static authored
  SYNTHETIC recorded responses served only on loopback. It retains one exchange
  record per request, captures replay bundles, evaluates both bindings on every
  split with the same dataset, adapter, evaluator configuration, definition
  parameters and policy, records an allowed pass, fail or unknown gate status,
  and proves that the binding changes `PlanId` without changing the definition
  or policy. It performs no live provider call.
- A transient normal dependency from the package to `rustev-runtime` made
  `make boundaries` fail with both the forbidden Tokio-family path and the
  package dependency rule. Removing the seed restored the passing boundary
  result. No file under `crates/` changed, so this delivery changes no core
  behavior.

## Verification

Run by `make verify` (008 is in `VERIFIED_SPECS`).

```verify:cli
cargo test -p rustev-pkg-lodging --locked
# Static SYNTHETIC recorded responses on loopback only; no live call.
cargo test -p rustev-jev --test lodging_swap --locked
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-pkg-lodging --all-targets --locked -- -D warnings
```

## Open owner questions

No owner decision is required to review the bounded package contract. Any
future use of real or externally sourced lodging data, any redistribution of
such data, any paid or live provider qualification, and any quality or
production claim require their own owner-authorized scope and retained
evidence. Those product, privacy, legal, spend, and external-commitment choices
are intentionally not recommended by this spec.

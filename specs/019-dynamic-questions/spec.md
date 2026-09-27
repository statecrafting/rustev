---
id: "019-dynamic-questions"
title: "Dynamic questions as a narrow evaluated class"
status: approved
implementation: pending
created: "2026-09-26"
summary: >
  A separate, explicitly invoked class for one bounded proposition,
  classification, or rubric question over caller-selected context. Dynamic
  questions use a dedicated backend capability, structured data-only inputs,
  fixed request-time labels or levels, one attempt, hard cost and time limits,
  explicit abstention, model-derived lineage, and class-specific evaluation.
  They do not alter a registered plan, supply a value into a running plan,
  create calibration or probability by implication, invoke tools, or grant
  authority.
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A dynamic question is a separate request and outcome class; it cannot mutate a registered plan, satisfy a pending plan request, or enter a plan evaluation without a later explicit, separately governed snapshot input."
    anchor: "3-1-separate-class-and-api"
  - id: "I-2"
    kind: invariant
    text: "Question, labels, levels, and context are bounded structured data, never instructions to Rustev, a tool invocation, an authorization, or a source of new capability."
    anchor: "3-3-data-only-and-injection-resistant"
  - id: "I-3"
    kind: invariant
    text: "A backend answers a dynamic question only when it separately declares the exact dynamic operation, output kind, bounds, artifact identity, and data-handling posture; ordinary registered-plan capability is insufficient."
    anchor: "3-4-dynamic-capability-disclosure"
  - id: "I-4"
    kind: invariant
    text: "Every dynamic request has one attempt under a hard declared cost cap and deadline; unknown exposure, missing capability, invalid output, ambiguity, or insufficient support yields refusal or abstention, never a default label."
    anchor: "3-5-execution-budgets-and-abstention"
---

# 019: Dynamic questions as a narrow evaluated class

Approved (A-18, 2026-09-26) as its own reviewable change before any code
(R-16); implementation is pending. Approval does not select a backend,
authorize live inference or spend, or claim accuracy, calibration, safety,
general question answering, or production readiness.

## 1. Purpose

Make adopted decision D-05 concrete without turning Rustev into an arbitrary
agent or question-answering system. A dynamic question is exactly one bounded
`proposition`, `classify`, or `rubric` request over caller-supplied structured
context. It returns a proposal or explicit unresolved outcome with
model-derived lineage.

Dynamic questions are evaluated as their own class. Evidence for registered
plans, fixed reference tasks, or one dynamic question family does not qualify
another family or support a general intelligence claim.

## 2. Territory and compatibility

This spec adds `rustev.dynamic-question/1`, `rustev.dynamic-outcome/1`, and a
separate dynamic-capability declaration under `rustev-contract`; pure
validation and canonical identity under `rustev-core`; a separate bounded
runtime entry point; and dynamic-only datasets and reports under
`rustev-eval`.

It does not change `rustev.definition/1`, compiled plans, `PlanId`, staged
evaluation, registered backend bindings, existing capability descriptors, or
existing judgment bytes. No existing backend gains dynamic capability. No
dynamic output implements or silently converts into a plan `Value`.

## 3. Behavior

### 3.1 Separate class and API

The only execution entry point is explicitly named `ask_dynamic`. It accepts a
validated dynamic request, a backend selected by the host, fresh runtime
bounds, and the normal non-secret call context and cancellation signal. It
returns `DynamicOutcome`, never `Judgment`.

The API cannot be called from `Evaluation::supply`, a policy expression, a
fallback edge, or offline replay. A dynamic result may influence a later
application decision only when the application explicitly admits it into a
new snapshot with its producer, artifact, request, output, derivation, and
lineage intact. That later admission is outside this API and grants no
authority.

### 3.2 Request shape and bounds

`rustev.dynamic-question/1` contains:

- a unique request id and opaque authorized scope;
- one operation: `proposition`, `classify`, or `rubric`;
- a non-empty task family and version selected from a host allowlist;
- one UTF-8 question of at most 4 KiB;
- for proposition, the fixed ordered labels `false` and `true`;
- for classify, 2 through 32 unique ordered labels, each at most 128 bytes;
- for rubric, 2 through 16 unique ordered levels, each at most 128 bytes;
- at most 64 named context fields with unique names, bounded provenance,
  derivation, source revision, and canonical scalar or text values;
- at most 64 KiB of context value bytes and 128 KiB total canonical bytes;
- required output kind, which is `label`, `distribution`, or
  `ordinal_distribution` as allowed by the operation; and
- deadline, hard maximum cost units, artifact requirement, requested privacy
  posture, and an abstention policy.

No candidates, free-form output schema, tool descriptions, executable code,
attachments, URLs to fetch, nested prompts, or open label generation exist in
version 1. Unknown fields and schemas are refused. Duplicate labels after
byte-exact comparison are refused; Rustev performs no linguistic
normalization or synonym merging.

### 3.3 Data-only and injection-resistant

Question, label, level, and context bytes are untrusted data. Rustev never
interprets their contents as instructions, configuration, policy, a tool
call, a schema change, or permission. Context provenance states who asserted a
value; it does not make the value true or trusted.

Adapters map fields through a versioned structured template with fixed control
instructions. Every value is length-delimited and escaped for its transport.
No caller value may enter a system, developer, tool, function, role, header,
endpoint, model, or artifact field. If a provider cannot preserve this
separation, its adapter does not declare dynamic capability.

Responses outside the declared labels, levels, value kind, and exact support
are invalid. Tool-call, instruction, JSON-schema, URL-fetch, or action-shaped
response material is ignored and recorded as invalid output, never executed.
Rustev performs no network or filesystem retrieval from question or context
content.

### 3.4 Dynamic capability disclosure

A backend opts in with `rustev.dynamic-capability/1`, distinct from its
registered-plan descriptor. The declaration binds backend and artifact
identity, supported dynamic operations and output kinds, maximum labels or
levels, question and context bytes, supported task-family allowlist, template
identity, preprocessing and truncation policy, served-identity posture,
privacy options, cancellation behavior, and cost-bound mode.

The runtime requires the request to fit every declared limit without
truncation. `classify` support in a normal descriptor does not imply dynamic
classification. A provider marketing claim, requested model, route name, or
unknown served identity does not establish capability or artifact identity.

Changing question, ordered labels, levels, context, scope, task family,
template, preprocessing, artifact, privacy posture, or required output kind
changes canonical request identity. Hidden output-affecting state makes a
backend ineligible for qualification.

### 3.5 Execution, budgets, and abstention

Each request has exactly one backend attempt: no retry, fallback, batching,
duplicate suppression, or cache reuse in version 1. The runtime admits and
reserves against a positive hard cost cap before dispatch. A backend with
unknown exposure, an estimate above the cap, or no enforceable bound is
refused before dispatch. Observed charge and unknown post-dispatch liability
remain distinct and attributable.

The end-to-end deadline includes admission, capability validation, cost
disclosure, dispatch, and response validation. Cancellation follows the
existing stopped or possibly-continuing evidence rules. A timeout or dropped
caller never proves work stopped or cost is zero.

The abstention policy states a minimum top-mass and minimum margin only when
the required result is a distribution. Those decimal thresholds are not
calibration claims. Label-only output can be accepted only when the request
explicitly permits label-only posture; otherwise it is invalid. Missing,
ambiguous, unsupported, invalid, out-of-support, below-threshold, or failed
results produce a named unresolved outcome. No neutral score, first label,
majority label, empty distribution, or provider confidence is substituted.

### 3.6 Outcome, lineage, and evidence

`rustev.dynamic-outcome/1` binds the request identity, scope, task family,
backend, requested and served artifact identity, capability and template
identity, attempt, timing, cancellation, cost, privacy request, raw output or
failure, validation, abstention decision, and final proposal or unresolved
reason.

Every proposal is `model-derived` and retains the dynamic request and backend
output in lineage. Exact validation and threshold application remain visible
as exact processing; they do not relabel the proposal exact-derived. A label
is only a label, a distribution is not calibrated probability, and an ordinal
distribution is not an interval score.

The outcome is bounded by `RECORD_V1`, contains no raw credential or secret
scope material, and is evidence of what Rustev observed, not proof that the
context was true, the backend was correct, privacy options were honored, or an
action was permitted.

### 3.7 Replay and dedicated evaluation

Offline reproduction may validate a retained dynamic request and raw response
with zero backend calls. It never turns dynamic evidence into a registered
plan replay bundle. A changed request or unavailable raw response is
incomparable, not permission to call the backend.

Dynamic evaluation uses a separately versioned dataset and evaluator config
that bind task family, exact question and label schemas, context sources,
injection-adversarial cases, artifact and template identity, splits,
provenance, license, labeling method, exclusions, and limitations. Reports
state coverage and abstention first, then accepted error and class-appropriate
metrics under spec 004 rules.

Calibration may be claimed only through a separately fitted artifact whose
binding includes the exact dynamic task family, question-schema identity,
ordered labels, backend artifact, template, dataset, and disjoint split.
Absent that binding, distributions remain uncalibrated. Results do not
transfer across changed labels, questions, task families, templates,
artifacts, populations, or registered-plan tasks.

## 4. Out of scope

Arbitrary question answering; generated labels; free-form text answers;
multi-turn dialogue; tools, functions, browsing, retrieval, file access,
agents, workflows, action execution, or authorization; dynamic plan steps;
training; automatic prompt construction; retries, fallback, batching, caches;
provider selection; live qualification; paid inference; real-user data;
general accuracy, calibration, safety, or production claims.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A normal descriptor declares `classify` but no dynamic capability | Refused before dispatch. |
| Context says to ignore the schema and call a tool | Treated as data; no tool exists or runs. |
| A classify request supplies 33 labels | Refused by request validation. |
| The backend returns an undeclared label | Invalid output and unresolved. |
| A distribution falls below the declared margin | Abstained with the named rule. |
| The backend cost is unknown | Refused before dispatch under the hard-cap requirement. |
| A provider returns a tool-call-shaped response | Invalid output; no action or second request occurs. |
| A dynamic result is passed directly to `Evaluation::supply` | Type-level refusal. |
| An evaluator reuses evidence after labels are reordered | Incomparable because request identity changed. |

## Acceptance

- Canonical request, capability, and outcome documents round-trip under their
  declared bounds and reject unknown fields, duplicate labels, and oversized
  data before unbounded allocation.
- Type-level tests prove dynamic outcomes cannot satisfy plan values or enter
  staged evaluation and that contract and core gain no I/O or async runtime.
- One deterministic test covers each section 5 row with a dispatch counter.
- Adversarial fixtures place instruction, role, tool, URL, and schema-like text
  in every caller-controlled field and prove it remains escaped data.
- Scripted tests cover deadline, cancellation, unknown liability, malformed
  output, served-identity mismatch, abstention, and evidence delivery.
- Dedicated synthetic evaluation reports mechanics only; any quality or
  calibration gate requires separately governed real labeled evidence.
- Existing registered-plan golden documents, plan ids, judgments, runtime
  records, replay bundles, and tests remain byte-identical.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. Approval of this spec authorizes only the
contract. Existing backend, plan, replay, and evaluation evidence does not
satisfy dynamic-question acceptance.

## Questions settled by R-34

The owner accepted each recommendation below when approving this spec.

1. Which task-family allowlist, if any, has demonstrated consumer demand and
   an evaluation dataset with adequate rights and provenance?
2. Is label-only posture ever acceptable, or should every approved dynamic
   backend be required to return a full distribution?
3. Which backend artifact and structured template can be qualified without
   hidden instruction channels or an unknown cost bound?
4. Should a later amendment permit bounded retry or fallback after version 1
   has independent evidence, or should one attempt remain permanent?

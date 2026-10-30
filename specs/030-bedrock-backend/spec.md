---
id: "030-bedrock-backend"
title: "Hosted model platform backend: AWS Bedrock"
status: approved
implementation: in-progress
created: "2026-10-16"
summary: >
  A remote decision backend under `integrations/rustev-bedrock/` for Amazon
  Bedrock over two wires on the `bedrock-runtime` endpoint, Converse and the
  OpenAI-compatible Responses API, signed with SigV4 over Rustev's existing
  hyper and rustls stack. A versioned binding document puts wire, region,
  model or inference profile, declared destination regions, residency
  policy, parameters, structured output mode, price table and reserved
  members for later operations into the artifact identity. A host-supplied
  `ModelPolicy` (deny wins, no default allowlist) is evaluated at bind time
  and again at runtime start. Wave 1 serves `classify`, `proposition` and
  `rubric` as `Label` outputs, because neither wire documents token
  log-probabilities; `rank`, `generate`, `extract` and Guardrails screening
  are added by their own specs into members reserved here. Amends spec 001
  only by adding a cloud SDK dependency family to the boundary check.
  Approved (A-26) under the 2026-10-16 delivery work order; implementation
  is pending. The work order authorizes paid Bedrock smoke and probe calls
  on synthetic or spec 023 admitted data within R-29's USD 5 total testing
  cap across all providers; the owner supplies AWS credentials, whose
  absence blocks only the live stage. Approval makes no quality,
  residency-compliance or production claim.
amends:
  - "001-boundaries-and-authority"
establishes:
  - { kind: directory, path: "integrations/rustev-bedrock/" }
extends:
  # Adds the integration crate and the AWS signing crates as workspace
  # dependencies usable only under integrations/ (spec 001, 3.4.2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Declared at approval, not by an increment: 030 joins `make verify`
  # (Makefile) with its declared verification, which waits for the live
  # stages (implementation record, increment 2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  # 030 joins the integrations-base acceptance group with `make verify`.
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
  # Adds the cloud SDK family to the boundary check (3.13).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  # Lists the crate as excluded from the release set (spec 031).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Keeps the spend journal it compiles in by path free of the Jev production module (spec 012).
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
  - "020-remote-protocol-refinements"
  - "023-real-data-quality-qualification"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "The Bedrock backend never grants authority: nothing Bedrock returns is authorization, an input fact, a policy parameter, or anything other than a raw output or a classified failure, and every supplied value is model-derived."
    anchor: "3-1-position-authority-and-provenance"
  - id: "I-2"
    kind: invariant
    text: "Every member of `rustev.bedrock-binding/1` is always present and enters the artifact identity; a change of wire, region, model, profile kind, destination set, residency policy, parameters, structured output mode, price table or any reserved member is a different backend."
    anchor: "3-2-the-binding-document"
  - id: "I-3"
    kind: invariant
    text: "A binding whose declared destination regions or profile kind fall outside its residency policy, or whose model id prefix disagrees with its declared profile kind, is refused at construction; no descriptor exists for it, so no plan compiles against it."
    anchor: "3-3-residency-and-inference-profiles"
  - id: "I-4"
    kind: invariant
    text: "Rustev ships no default allowlist: a backend is constructed only with a host-supplied `ModelPolicy`, a binding is admitted only when an allow rule matches and no deny rule matches, and every refusal names the rule or the absence of a match."
    anchor: "3-4-model-policy"
  - id: "I-5"
    kind: invariant
    text: "Without the `aws-config` feature the crate reads no environment variable, profile file or instance metadata; credentials come only from the host's credential provider and never enter identity, logs or exchange records."
    anchor: "3-5-credentials-signing-and-transport"
  - id: "I-6"
    kind: invariant
    text: "A distribution or logits output is supplied only from token log-probabilities the wire returns for every declared option; a self-reported confidence, an argmax or a free-text score is recorded as provider-reported and never supplied or converted into mass."
    anchor: "3-7-operations-and-output-kinds"
  - id: "I-7"
    kind: invariant
    text: "Served identity is explicit: the requested model is never copied into the served field, an echo of the requested id is not a served identity, and the processing region is recorded as unknown unless the response states it."
    anchor: "3-8-served-identity"
  - id: "I-8"
    kind: invariant
    text: "Missing usage yields an `unknown` charge, never zero; usage times the bound price table is an `estimated` charge; and every live call is reserved against the persistent testing journal under R-29 before any request byte is sent."
    anchor: "3-9-cost-and-spend"
  - id: "R-1"
    kind: requirement
    text: "Every Bedrock failure maps to exactly one code of spec 009's closed taxonomy, with the Bedrock error type retained in the detail."
    anchor: "3-10-failure-taxonomy"
  - id: "R-2"
    kind: requirement
    text: "The reachability probe makes several calls under the live flag and the spend journal, and reports reachable only when every call succeeds."
    anchor: "3-11-reachability-probe"
  - id: "R-3"
    kind: requirement
    text: "Later specs add `rank`, `generate`, `extract`, Guardrails screening, log-probability outputs and spec 020 policies only by admitting new values of members reserved here; a new member is `rustev.bedrock-binding/2`, and version 1 documents keep their digests."
    anchor: "3-12-operations-added-by-later-specs"
  - id: "R-4"
    kind: requirement
    text: "The boundary check names the cloud SDK family and refuses it outside `integrations/` and anywhere in the graph of `rustev-contract` and `rustev-core`."
    anchor: "3-13-boundary-check-extension"
---

# 030: Hosted model platform backend: AWS Bedrock

Approved by the owner (A-26, 2026-10-16, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. It implements #76 child C1 (#66). Wave 1 of #76 delivers C1 "for
the existing operations"; this spec defines the backend fully, delivers
`classify`, `proposition` and `rubric`, and reserves the binding members
that C2 (#67, `rank`), B5 (#63, `generate`), B6 (#64, `extract`) and B7
(#65, screening) fill when they land (3.12). The work order authorizes
paid Bedrock smoke and probe calls within the scope of Decision 18 (3.14);
the owner supplies AWS credentials, and their absence blocks only the live
stage. Approval makes no quality, residency-compliance or production
claim.

## 1. Purpose

Amazon Bedrock reaches several model families under one cloud account and
one residency posture. Rustev has no backend for it, so a host whose models
run there cannot bind them to a plan, compare them, or record them honestly.
This spec adds an ordinary remote adapter (spec 009 Part A) for Bedrock that:

1. speaks both chat wires Bedrock exposes on `bedrock-runtime`: Converse and
   the OpenAI-compatible Responses API;
2. puts region, inference profile kind, declared destination regions and
   residency policy into the backend's identity, and refuses a binding
   outside its residency policy before any plan can use it;
3. admits a binding only under a host-supplied model allowlist evaluated at
   bind time and at runtime start;
4. records requested and served identity, usage and cost honestly, with
   unknown as a value; and
5. maps every Bedrock failure into spec 009's closed taxonomy.

## 2. Territory

Nothing is claimed until the implementing change. Then this spec claims:

- `integrations/rustev-bedrock/` (new crate `rustev-bedrock`): the binding
  and policy documents, residency and policy evaluation, the SigV4 signing
  and transport, the Converse and Responses mappings, the `DecisionBackend`
  implementation, the probe binary `rustev-bedrock` (3.11), the live smoke
  example (3.14), and recorded and documented-shape fixtures. Spec 009's
  Part A sources and spec 012's spend journal are compiled in by path, as
  `rustev-jev` does, because spec 001 3.4.4 forbids depending on a crate
  under `integrations/`.
- `Cargo.toml` and `Cargo.lock` (spec 001, additive): the crate as a
  workspace member and the workspace dependencies `aws-sigv4`,
  `aws-credential-types` and, behind the crate's optional `aws-config`
  feature, `aws-config`. The full AWS SDK (`aws-sdk-bedrockruntime`) is not
  a dependency.
- `tools/rustev-boundaries/` (spec 001, additive): the cloud SDK family
  (3.13).
- `Makefile` (spec 001, additive): `030` joins `VERIFIED_SPECS` with the
  implementation.
- `scripts/acceptance/` (spec 001, additive): `030` joins the
  `integrations-base` acceptance group with it.

**Amendment of spec 001.** Spec 001 3.5 says that adding a forbidden family
is a change to that spec. This spec makes exactly that change: 3.5's list
gains the cloud SDK family of 3.13, enforced by rules 3.4.1 and 3.4.2
unchanged. No rule is relaxed and spec 001's approved text is not edited.

**What does not change.** `rustev-contract`, `rustev-core`, `rustev-runtime`,
`rustev-eval` and `rustev-cli` gain nothing: spec 003's seams and spec 009's
vocabulary suffice, and no vendor name enters a crate outside
`integrations/` (spec 009, I-7). No plan construct, document or operator is
added, so every existing plan keeps a byte-identical `PlanId`; the
implementing change keeps the existing reference-plan goldens byte-identical
and adds a golden asserting it. Specs 009, 012, 013 and 020 are not amended.

**Relation to spec 020.** Spec 020 is approved and not implemented. Before
it lands, this backend uses one undivided attempt budget (spec 009, 3.4),
one fresh connection per attempt, no batch seam, and `rustev.run/1`
evidence. Its binding carries spec 020's members with the value `disabled`
(3.2). After spec 020 lands, a binding may set phase ceilings (020, 3.4) and
bounded connection reuse (020, 3.5) as values of those members, which
changes the binding identity as 020 requires; the members themselves do not
change. Bedrock is a third-party endpoint, so spec 020's principal reference
(3.6) is refused at construction before and after.

## 3. Behavior

### 3.1 Position, authority and provenance

1. `BedrockBackend` is a remote adapter under spec 009 Part A, constructed
   explicitly by the host. Its outputs are `model-derived` (spec 001,
   3.3.1). Nothing Bedrock returns (headers, `stopReason`, guardrail trace,
   usage, `additionalModelResponseFields`, a model's self-reported
   confidence, reasoning text) is authorization, becomes an input fact,
   alters a threshold or policy parameter (R-14), or changes retries,
   fallback or budgets.
2. The adapter supplies exactly one of a `RawOutput` or a classified failure
   (3.10). Everything else goes to the exchange record (spec 009, 3.9).
3. Residency enforcement (3.3) is a bind-time refusal and a recorded
   declaration. It is not proof of where data was processed or stored, and
   data handling remains the host's responsibility.
4. One attempt is at most one request (spec 009, I-3). The adapter never
   retries, never follows a redirect, never falls back to another model,
   region or profile, and never batches.

### 3.2 The binding document

The artifact identity is the contract digest (spec 002, 3.3.2) of a
`rustev.bedrock-binding/1` document, parsed under declared limits with
`deny_unknown_fields`. Every member below is required and always present,
including reserved members at their default value, so the canonical bytes
never depend on a member's absence.

| Member | Values in this spec | Notes |
|---|---|---|
| `schema` | `rustev.bedrock-binding/1` | |
| `wire` | `converse`, `responses` | 3.6 |
| `endpoint` | `bedrock-runtime` | `bedrock-mantle` is reserved and refused (Decision 1) |
| `region` | AWS region code, for example `eu-central-1` | the region the request is sent to and signed for |
| `model_id` | a foundation model id or a system-defined inference profile id | ARNs, application inference profiles, prompt routers, provisioned, custom, marketplace and prompt-management targets are refused (Decision 14) |
| `inference_profile` | `none`, `geo`, `global` | must agree with the model id prefix (3.3) |
| `destination_regions` | `{as_of, regions: [...]}`, or `{as_of, any_commercial: true}` for `global` | host-declared from AWS's published or queried profile definition; for `none`, exactly `[region]` |
| `residency_policy` | `{allowed_source_regions, allowed_destination_regions or any, allowed_profile_kinds}` | 3.3 |
| `operations` | list of `{operation, mapping_version, max_options}`; operations `classify`, `proposition`, `rubric` | `rank`, `generate`, `extract`, `screen` reserved (3.12) |
| `mapping` | `{version, prompt_template_digest, option_description_digest}` | question mapping of 3.7 |
| `parameters` | `{max_tokens, temperature or none, top_p or none, stop_sequences, reasoning_effort or none, additional_model_request_fields or none, service_tier or none, performance_latency or none}` | numbers as canonical decimals; `additional_model_request_fields` is canonical JSON, Converse only |
| `structured_output` | `tool_use`, `json_schema`, `none` | 3.7 |
| `logprobs` | `none` | `token_logprobs` reserved (3.7.5) |
| `pricing` | `{table_id, as_of, currency: "USD", unit_nano_usd, input_per_mtok, output_per_mtok, cache_read_per_mtok or none, cache_write_per_mtok or none}` | 3.9 |
| `guardrail` | `none` | `{identifier, version, trace}` reserved for B7 (3.12) |
| `provider_store` | `false` on `responses`; `not_applicable` on `converse` | Decision 2 |
| `request_limits` | `{max_projection_bytes, max_request_bytes, max_response_bytes}` | spec 009, 3.5.1 |
| `transport_phase_policy` | `disabled` | spec 020, 3.4 (reserved) |
| `connection_pool_policy` | `disabled` | spec 020, 3.5 (reserved) |
| `batch_dispatch` | `disabled` | spec 020, 3.3 (reserved) |
| `principal_wire` | `omitted` | spec 020, 3.6; third-party endpoint, never another value |

Credentials, signing keys, network addresses, the credential provider,
concurrency, the `ModelPolicy` and the spend journal path are configuration,
not identity (spec 009, 3.2.3; Decision 8). Construction refuses an
incomplete or inconsistent document (for example `structured_output:
json_schema` with a wire mapping that does not support it,
`additional_model_request_fields` on `responses`, or `provider_store` other
than `false` on `responses`) and sends nothing.

### 3.3 Residency and inference profiles

1. **Profile kind from the id.** The mapping version carries a prefix table
   of system-defined profile prefixes: `global.` is `global`; `us.`, `eu.`,
   `apac.`, `jp.`, `au.`, `in.` and `us-gov.` are `geo` (Facts 6, 7).
   A model id whose first dot-separated segment is in the table is a
   profile of that kind; any other id is a foundation model id, kind
   `none`. A declared `inference_profile` that disagrees is refused
   (`profile_kind_mismatch`). The table is identity, so a prefix AWS adds
   later needs a new mapping version.
2. **Destinations, not prefixes.** AWS documents that the prefix alone does
   not determine routing and that destination regions depend on the source
   region (Fact 7). Residency is therefore judged on the declared
   `destination_regions`, which the host records with an `as_of` date from
   the profile definition for that source region. For `none` the set is
   `[region]`. For `global` the set is `any_commercial`, which only an
   `allowed_destination_regions: any` policy admits.
3. **Refusal before compilation.** At construction the binding is refused,
   naming the failed clause, when `region` is not in
   `allowed_source_regions`, the profile kind is not in
   `allowed_profile_kinds`, or any declared destination is not in
   `allowed_destination_regions`. Plans are compiled against the descriptor
   of a constructed backend (spec 009, 3.2.1), so a binding outside its
   residency policy never yields a descriptor and no plan can be compiled
   against it. This is how "refused at compile time" is realized without a
   vendor-specific hook in `rustev-core` (Decision 5). `check_plan` repeats
   the evaluation for a plan presented later.
4. **What is recorded.** Every exchange record carries region, profile kind,
   the declared destination set with its `as_of`, and `served_region:
   unknown` with the reason `not_disclosed_by_wire` (Fact 5). Rustev does
   not read the host's audit logs to fill it.
5. **Stated limits.** Geographic and global profiles may route to and, for
   abuse detection, store data in destination regions, including opt-in
   regions the account did not enable (Facts 6, 7). The Responses API on
   `bedrock-runtime` requires a cross-region profile for some model
   families (Fact 3); a residency policy that admits only `none` refuses
   such bindings, as intended.

### 3.4 Model policy

1. **Document.** `rustev.bedrock-model-policy/1`:
   `{schema, allow: [Rule], deny: [Rule]}` with at most 256 rules in total.
   A `Rule` is `{model, region, profile_kind, wire}`; every field is
   required. `model` and `region` are patterns: case-sensitive literals in
   which `*` matches any run of characters, matched against the whole
   string; no other metacharacter exists. `profile_kind` and `wire` are a
   nonempty set of values or `*`.
2. **Semantics.** A binding is admitted if and only if at least one `allow`
   rule matches all four fields and no `deny` rule matches all four. Deny
   wins. An empty `allow` admits nothing. Rustev ships no policy and no
   default; the constructor requires one.
3. **Refusals name the rule.** A deny match refuses with
   `policy_denied{index, rule}` for the first matching deny rule in
   document order; no allow match refuses with
   `policy_not_allowed{allow_rules}`. Both carry the policy digest.
4. **When.** At construction (bind time, before any plan can be compiled
   against the backend, 3.3.3), in `check_plan`, and at runtime start: the
   runtime process constructs its own backend with its current policy, and
   a host may call `replace_policy`, which re-evaluates immediately. After a
   failed re-evaluation every `infer` fails `capability` with the refusal
   text and sends no byte.
5. **Not identity.** The policy is host gating, not an output-affecting
   input, so it is not in the artifact identity and editing an allowlist
   changes no `PlanId` (Decision 8). Its digest is recorded in every
   exchange record.

### 3.5 Credentials, signing and transport

1. **Signing.** Requests are signed with SigV4 using the `aws-sigv4` crate:
   service `bedrock`, the binding's region, a signed SHA-256 payload hash,
   and the signing time from an injected clock seam (system clock by
   default). Both wires use the same signer and transport. Signing is
   verified against the published AWS Signature Version 4 test suite
   (Fact 11).
2. **Credentials.** The host supplies a credential provider (a Rust trait
   returning `aws_credential_types::Credentials` asynchronously, with the
   attempt's cancellation signal and remaining budget). A TypeScript host
   implements it through the Node binding of #76 A1. The optional
   `aws-config` feature adds a provider that reads only the environment
   variables `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and
   `AWS_SESSION_TOKEN`; their absence is a credential failure, never a
   panic. It is not the full AWS default credential chain: that chain
   builds an instance metadata client, which needs an HTTP client that
   `aws-config` is built without, because Rustev's one HTTPS stack is the
   transport of item 4 (amended 2026-10-29, A-65). A host that needs
   profiles, single sign-on, role assumption or instance metadata supplies
   its own provider under this item. Without that feature the crate reads
   no environment variable, profile file or instance metadata endpoint, and
   the region comes only from the binding.
3. **Secrecy.** Credentials and session tokens appear only in the signed
   request. Identity, exchange records, logs and error details carry at most
   a domain-separated digest of the access key id (the credential identity
   digest spec 020 3.5 uses in its pool key).
4. **Transport.** Rustev's existing hyper and rustls stack with
   `webpki-roots`, TLS required, no proxy, no redirect. Endpoint
   `https://bedrock-runtime.{region}.amazonaws.com`. Credential acquisition
   and every transport phase draw from the one attempt budget (spec 009,
   3.4.1); spec 020's phase ceilings and connection reuse apply only once
   implemented and enabled in the binding (section 2).
5. **What leaves the process.** The projection and the question material of
   3.7, nothing else. The attempt id and decision id are not sent (spec 009,
   3.3.3); `requestMetadata` is not sent.

### 3.6 The Converse and Responses wires

1. **Converse.** `POST /model/{modelId}/converse` (Fact 1), body with
   `messages`, `system`, `inferenceConfig` from `parameters`,
   `additionalModelRequestFields` when bound, and the structured output
   members of 3.7. No `guardrailConfig` until B7 (3.12).
2. **Responses.** `POST /openai/v1/responses` on the same host (Facts 2, 3),
   signed with SigV4, body with `model`, `input`, `instructions`,
   `max_output_tokens`, `temperature`, `top_p`, `reasoning` when bound, the
   structured output members of 3.7, and `store: false` on every request
   (Decision 2). `background`, `previous_response_id`, server-side tools and
   streaming are never sent.
3. **Mapping version.** Each wire's request and response mapping is part of
   the binding's `mapping.version`; a change of wire shape changes only the
   mapping, under a new version, as spec 012 3.4 does.
4. **Bounded parse.** Responses are read to `max_response_bytes` and parsed
   under spec 009 3.5.1's bounds before typed construction.

### 3.7 Operations and output kinds

1. **Descriptor.** A `rustev.backend/1` descriptor declaring, for each bound
   operation, `output: label` and its `max_options`; input limit in
   canonical projection bytes with `on_excess: refuse`; determinism
   `unspecified`. `cost_model` is `estimated` (3.9). No batch capability.
2. **Questions.** `classify` sends the option keys and their descriptions;
   `proposition` the keys `false` and `true`; `rubric` the level keys low to
   high. The prompt template and option descriptions are identified by
   digest in `mapping`.
3. **Structured output.**
   - `tool_use`: one client tool `rustev_answer` with input schema
     `{choice: enum of the keys}`, forced by tool choice; exactly one tool
     call is expected.
   - `json_schema`: Converse `outputConfig.textFormat` of type
     `json_schema`, or Responses `text.format` of type `json_schema`, with
     the same schema (Facts 1, 2).
   - `none`: the whole output text, with leading and trailing ASCII
     whitespace removed, is the choice; no case folding or substring match.
4. **Into `RawOutput`.** The parsed choice becomes `RawOutput::Label` with
   the string as received. A missing, duplicated or unparseable answer, no
   tool call or several, or a stop reason other than normal completion
   is `malformed_response`, except `model_context_window_exceeded`, which
   3.10 maps to `capability`. A choice outside the keys is supplied as
   received and judged by the core's pre-supply check (`invalid_output`,
   spec 003 3.2.4); the adapter does not repair it (spec 009, 3.5.3). The
   schema asks for no confidence; any extra field a model returns is kept in
   the exchange record as provider-reported and never supplied (spec 009,
   3.5.5; spec 012, 3.4.4).
5. **Distributions and logits.** `Distribution` or `Logits` may be supplied
   only from token log-probabilities the wire returns for every declared
   option, never synthesized from a self-reported confidence, an argmax or
   repeated sampling. Neither wire documents token log-probabilities on
   Bedrock as of the facts below (Fact 9), so `logprobs` admits only `none`
   and every operation is `Label`. A plan whose step requires a
   distribution, calibrated probability or ordinal distribution is
   therefore refused against this backend at compilation (constitution IX),
   or uses a fallback its definition declares. A later change may admit
   `token_logprobs` for a model whose wire documents them, as `Logits` keyed
   by option with values as received, missing options passed to the core
   unfilled; that change records the fact it relies on.

### 3.8 Served identity

1. `requested_model` is the bound `model_id` exactly.
2. **Converse** reports no served model for foundation model ids or
   system-defined profiles (Fact 1); `served` is `unknown` with reason
   `not_disclosed_by_wire`.
3. **Responses** returns a `model` member (Fact 2, to be re-confirmed). A
   value byte-equal to `requested_model` is an echo and recorded as such;
   `served` stays `unknown` with reason `echo_of_requested`. A different
   value is `served{identity, disclosure: reported}`. Disclosure is never
   `pinned`, because Bedrock guarantees no served version per call.
4. `served_region` is `unknown` (3.3.4). `generation_id` is the
   `x-amzn-RequestId` response header on Converse and the response `id` on
   Responses.
5. As spec 009 3.7 requires, every report, calibration artifact and
   qualification record for this backend names the binding identity and the
   dates of the calls it measured, and states that the served version and
   processing region were not observable.

### 3.9 Cost and spend

1. **Cost model** `estimated`. `cost_bound` is an estimate from projection
   and question bytes under the mapping's declared bytes-per-token ratio
   plus `max_tokens` at the output rate, under the bound price table. A
   `hard` cost policy therefore refuses plans that reach this backend (spec
   003, 3.5.3), as for spec 012.
2. **Charge.** Bedrock reports usage, not cost (Facts 1, 2). Usage times the
   bound price table, rounded up to the unit, is an `estimated` charge, as
   spec 009 3.8.1 defines; it is never `observed` (Decision 4). Converse
   `usage.inputTokens`, `outputTokens`, `cacheReadInputTokens`,
   `cacheWriteInputTokens`; Responses `usage.input_tokens`, `output_tokens`
   and their detail members. Missing usage, or a nonzero cache count the
   price table does not price, is `unknown`, never zero. Reported usage is
   kept verbatim in the exchange record.
3. **Price table.** `pricing` is identity with its `as_of` date. Global
   profiles and service tiers may be priced differently from geographic
   ones (Fact 6); a binding states the rates for its own profile and tier.
   Units default to one nano-USD, as in spec 012 3.6.4.
4. **Refusals.** A refusal is `unknown` unless a recorded fact establishes
   that Bedrock does not charge that refusal; this spec records none, so
   throttling, access and validation refusals are `unknown` (spec 009,
   3.6).
5. **Cancellation.** Neither wire offers cancellation of a synchronous
   request (`store: false` responses cannot be cancelled by id). Before any
   request byte the attempt is `stopped` with `observed{0}`; after, it is
   `unconfirmed` with `unknown` charge and `possibly_continuing` (spec 009,
   3.4; spec 013).
6. **Spend.** Live calls (smoke and probe, 3.11, 3.14) reserve against the
   persistent testing journal of spec 012 (`SpendJournal`, budget
   `testing`), compiled in by path, before dispatch: one journal and one USD
   5 total cap for testing across all providers (R-29; Decision 11). A
   reservation over the cap refuses the call unsent. Production spend caps
   for this backend are the host's, through the runtime's shared ledger
   (spec 003, 3.5.2).

### 3.10 Failure taxonomy

The error type is read from the `x-amzn-ErrorType` header or the body's
error type on Converse, and from the HTTP status and OpenAI-style error
object on Responses (Facts 1, 4, to be re-confirmed). The detail begins
`remote:<code>` and then `bedrock:<type>`, within 256 bytes.

| Bedrock condition | Spec 009 code | Notes |
|---|---|---|
| no request byte sent (resolution, connect, TLS, credential provider failed) | `transport_unsent` | `observed{0}` |
| connection lost after request bytes | `transport_interrupted` | `unknown` |
| `ThrottlingException` (429) | `rate_limited` | retry-after recorded, never slept on |
| `ModelNotReadyException` (429) | `overloaded` | distinguished from throttling by type |
| `ServiceUnavailableException` (503), `overloaded_error` (529) | `overloaded` | |
| `AccessDeniedException` (403), `InvalidClientTokenId`, `UnrecognizedClientException`, `ExpiredTokenException`, signature errors, `RequestExpired`, Marketplace agreement pending or failed (403) | `unauthorized` | includes access not yet enabled |
| `ResourceNotFoundException` (404), use-case form not submitted (404), model not available in region or on the wire, stop reason `model_context_window_exceeded` (Fact 1) | `capability` | charge from usage when present |
| `ValidationException` (400), other 4xx | `malformed_request` | includes on-demand throughput not supported for the id |
| `ModelTimeoutException` (408) | `remote_deadline` | |
| `ModelErrorException` (424), `InternalServerException` (500), other 5xx | `remote_error` | |
| stop reason `max_tokens`, `malformed_model_output`, `malformed_tool_use`, `content_filtered`, `guardrail_intervened`, Responses `incomplete`, or an unparseable answer | `malformed_response` | charge from usage when present |
| attempt cancellation signal | `cancelled` | 3.9.5 |
| `replace_policy` refusal in force, residency or binding inconsistency detected at call time | `capability` | sent nothing; `observed{0}` |

Retry and fallback remain the execution policy's (spec 003, 3.6).

### 3.11 Reachability probe

1. The crate ships a binary `rustev-bedrock` with a `probe` subcommand. It
   cannot be `rustev bedrock probe`: `rustev-cli` may not depend on a crate
   under `integrations/` (spec 001, 3.4.4; Decision 6).
2. The probe constructs the backend from a binding, a policy and a
   credential provider, then makes `n` sequential minimal calls (default 3,
   at most 5) of one bound operation on a synthetic fixture, spaced by a
   declared interval, each within its own budget. Several calls are
   required because some model families accept a first call and fail later
   until account enablement completes.
3. It reports `reachable` only when every call returned a mapped output,
   and otherwise the first failing call with its spec 009 code and Bedrock
   type. It is not a qualification and makes no quality claim.
4. It refuses to run without the live flag of 3.14 and an open testing
   journal, and reserves every call before dispatch.

### 3.12 Operations added by later specs

1. **Reserved now.** `operations` admits `classify`, `proposition` and
   `rubric`; the values `rank` (C2, `Scores{scope: request}`), `generate`
   (B5, text records), `extract` (B6, typed records) and `screen` (B7) are
   reserved. `guardrail` admits `none`; `{identifier, version, trace}` is
   reserved for B7's screening with text-unit cost and per-field verdicts.
   `logprobs` reserves `token_logprobs` (3.7.5). Spec 020's members reserve
   their enabled forms. Construction refuses a reserved value with
   `capability` (`reserved_by_spec`) until the owning spec's change admits
   it.
2. **Identity stays stable.** Those changes add admitted values and their
   mappings; they add no member. A version 1 binding that does not use them
   keeps its exact bytes and digest, so no existing `PlanId` changes when
   they land. A need for a new member is `rustev.bedrock-binding/2`, under
   which version 1 documents remain valid with unchanged digests.
3. **Acceptance carried forward.** The issue's fixtures for `rank`,
   `generate`, `extract` and screening on both wires are acceptance of
   those changes, and #66 is complete only when they land.

### 3.13 Boundary check extension

1. `tools/rustev-boundaries` gains a cloud SDK family matched by package
   name: `aws-config`, `aws-credential-types`, `aws-sigv4`, `aws-types`,
   `aws-runtime`, and the prefixes `aws-sdk-` and `aws-smithy-`. The
   cryptographic libraries `aws-lc-rs` and `aws-lc-sys` are not in the
   family.
2. Rule 3.4.2 refuses the family in any crate outside `integrations/`; rule
   3.4.1 refuses it anywhere in the normal and build graph of
   `rustev-contract` and `rustev-core`. Violations are reported with the
   existing clause numbers.
3. Unit tests over synthetic metadata include one violating case per rule
   for the family and one passing case for a crate under `integrations/`.

### 3.14 Exchange records, replay and live smoke

1. **Records.** Each exchange yields a `rustev.remote-exchange/1` record
   (spec 009, 3.9) with the binding identity, the policy digest, region,
   profile kind, declared destinations, the 3.8 identity fields, usage,
   stop reason, provider extras and the supplied charge. Digest-only by
   default (R-19).
2. **Replay.** Offline replay makes no Bedrock call (spec 004, 3.3.5). When
   response bytes are retained, re-running the mapping offline reproduces
   the supplied `RawOutput` byte for byte.
3. **Fixtures.** Default tests use a loopback server and fixtures. Until a
   live smoke records real responses, fixtures are constructed from the
   documented shapes of the facts below and labeled as documented-shape;
   responses recorded by a smoke are added as recorded fixtures with
   synthetic content only. Tests assert that only loopback was contacted.
4. **Live smoke.** `examples/smoke.rs` runs only by hand with
   `RUSTEV_BEDROCK_LIVE=1` and an explicit journal path, never under
   `cargo test` or default CI. Every call is reserved against the R-29
   testing journal (3.9.6) and sends synthetic fixture content or spec 023
   admitted data only, with `store: false` on Responses. Paid calls are
   authorized within that scope (Decision 18). AWS credentials and model
   access are supplied by the owner; their absence blocks only this live
   stage, never the default tests.

### 3.15 How the binding principles are kept

- **Proposals, never grants.** 3.1: the backend supplies raw outputs or
  failures only and defines no authorization or `Permitted` type.
- **Derivation classes and lineage.** Every supplied value is
  `model-derived` and its lineage is the binding identity, attempt id and
  exchange record.
- **Unresolved, never a default; no silent kind conversion.** Missing or
  invalid answers are failures or `invalid_output`; unknown usage is
  `unknown`; no label becomes a distribution and no confidence becomes mass
  (3.7.4, 3.7.5, 3.9.2).
- **Pure core.** `rustev-core` and `rustev-contract` are unchanged; they
  gain no I/O, clock, `unsafe`, async runtime or cloud dependency, and 3.13
  makes the last mechanically checked.
- **New dependency classes only under `integrations/`.** 3.13 extends
  `make boundaries` for the cloud SDK family.
- **No dynamic plugin loading.** The host constructs the backend
  explicitly; the credential provider is a compiled trait or an explicit
  Node binding seam (OD-5).
- **No identity, authority or knowledge store.** Credentials, account,
  model access and residency obligations remain the host's; Rustev stores
  only bounded evidence under the host's retention.

## 4. Out of scope

Implementation; the `bedrock-mantle` endpoint; Bedrock API keys (bearer
authentication); InvokeModel, Chat Completions and the Messages API;
streaming; background, stored or chained Responses; server-side tools;
batch inference jobs; prompt routers, prompt management, application
inference profiles, provisioned, custom, imported and marketplace targets;
reading audit logs to learn the processing region; any production
deployment or production spend cap for this backend; `rank`, `generate`,
`extract` and screening (their own specs, 3.12); quality or calibration
claims (R-04, R-37).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| Constructed without a `ModelPolicy` | Does not compile (required argument); no default policy exists. |
| Policy allows `eu.*` models and denies `wire: responses`; binding is `eu.` on `responses` | Refused `policy_denied` naming the deny rule. |
| Policy allows only `profile_kind: [none, geo]`; binding is `global.` | Refused `policy_not_allowed`; no descriptor; no plan compiles. |
| Policy allows region `eu-*`; binding region `us-east-1` | Refused `policy_not_allowed`. |
| `replace_policy` with a policy that no longer admits the binding | Every later `infer` fails `capability` unsent. |
| Binding declares `inference_profile: none` for a `us.` id | Refused `profile_kind_mismatch`. |
| Declared destinations include a region outside `allowed_destination_regions` | Refused at construction naming the region. |
| `global` profile under a policy without `allowed_destination_regions: any` | Refused at construction. |
| Region, profile, model or any parameter changes | Different binding digest, different descriptor identity, different `PlanId`. |
| Binding sets `operations: [rank]` or `guardrail` other than `none` before their specs land | Refused `reserved_by_spec`. |
| Responses binding with `provider_store: true` | Refused at construction. |
| Plan step requires a distribution | Refused at compilation (IX) unless a declared fallback applies. |
| Model returns `{"choice": "b", "confidence": 0.93}` | `Label("b")` supplied; confidence recorded as provider-reported only. |
| Model answers a key that is not an option | Supplied as received; the core reports `invalid_output`. |
| No tool call, or two | `malformed_response`. |
| `stopReason: max_tokens` | `malformed_response`; charge estimated from usage. |
| 429 `ThrottlingException` | `rate_limited`, `overloaded` class, charge `unknown`. |
| 429 `ModelNotReadyException` | `overloaded`, detail names the type. |
| 403 Marketplace agreement pending | `unauthorized`. |
| Response without `usage` | Charge `unknown`, never zero. |
| Responses `model` equals the requested id | `served: unknown`, reason `echo_of_requested`. |
| Built without `aws-config`, with `AWS_ACCESS_KEY_ID` set and no provider | Construction refused; no environment read. |
| Built with `aws-config`, no credential environment variables set | Credential failure, unsent; no panic and no profile, SSO or instance metadata read. |
| Live smoke without `RUSTEV_BEDROCK_LIVE=1`, or reservation over the cap | No request is sent. |
| `rustev-core` gains an `aws-*` dependency | `make boundaries` fails naming 001 3.4.1. |

## Facts relied on

Each fact was read on 2026-10-16 from the AWS documentation named and is to
be re-confirmed at implementation; a change of shape changes the mapping
version, not this contract.

| # | Fact | Source |
|---|---|---|
| 1 | Converse is `POST /model/{modelId}/converse` on `bedrock-runtime`; `modelId` accepts foundation model ids and inference profile ids or ARNs. Request: `messages`, `system`, `inferenceConfig{maxTokens, stopSequences, temperature, topP}`, `toolConfig`, `outputConfig.textFormat{type: json_schema, structure}`, `guardrailConfig`, `additionalModelRequestFields`, `additionalModelResponseFieldPaths`, `performanceConfig`, `serviceTier`, `requestMetadata`. Response: `output`, `stopReason` (`end_turn`, `tool_use`, `max_tokens`, `stop_sequence`, `guardrail_intervened`, `content_filtered`, `malformed_model_output`, `malformed_tool_use`, `model_context_window_exceeded`), `usage{inputTokens, outputTokens, totalTokens, cacheReadInputTokens, cacheWriteInputTokens, cacheDetails}`, `metrics{latencyMs}`, `trace` (guardrail; `promptRouter.invokedModelId` for prompt routers only). No served-model field otherwise. Errors: `AccessDeniedException` 403, `InternalServerException` 500, `ModelErrorException` 424, `ModelNotReadyException` 429, `ModelTimeoutException` 408, `ResourceNotFoundException` 404, `ServiceUnavailableException` 503, `ThrottlingException` 429, `ValidationException` 400. No log-probability member. | https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html ; https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_OutputFormat.html |
| 2 | Bedrock serves the OpenAI-compatible Responses API on both `bedrock-runtime` (`https://bedrock-runtime.{region}.amazonaws.com/openai/v1`, path `POST /openai/v1/responses`, recommended) and `bedrock-mantle` (`https://bedrock-mantle.{region}.api.aws/v1`), with the same request and response format as the OpenAI Responses API; authentication by SigV4 or a Bedrock API key. `store` defaults to `true` and retains input and output for 30 days; `store: false` retains nothing. The response shape (including `model`, `id`, `usage.input_tokens`, `usage.output_tokens`) is the OpenAI Responses shape, to be confirmed against a recorded response. | https://docs.aws.amazon.com/bedrock/latest/userguide/inference-responses-api.html |
| 3 | On `bedrock-runtime` the Responses API is available in every region of that endpoint, is synchronous only (`background=true` is a 400), has no server-side tools, supports only the default project, rejects application inference profiles, supports Guardrails through `X-Amzn-Bedrock-GuardrailIdentifier` and `X-Amzn-Bedrock-GuardrailVersion` headers, and for closed-weight OpenAI GPT models requires a cross-region profile (`us.`, `global.`, `us-gov.`); in-region inference is not available for them there. GPT OSS models do not support Responses on `bedrock-runtime`. `bedrock-mantle` is available in a subset of regions and supports neither cross-region profiles nor Guardrails. | https://docs.aws.amazon.com/bedrock/latest/userguide/inference-responses-api.html ; https://docs.aws.amazon.com/bedrock/latest/userguide/endpoints.html |
| 4 | Error causes: `AccessDeniedException` 403 (permissions, expired temporary credentials), `FTUFormNotFilled` 404 (model use case details not submitted), Marketplace agreement pending, failed or being created 403, `InvalidClientTokenId` 403, `IncompleteSignature` 400, `RequestExpired` 400, `ThrottlingException` 429 (account quota), `ServiceUnavailable` 503 (capacity, not quota), `overloaded_error` 529, `ValidationError` 400, `ResourceNotFound` 404, `InternalFailure` 500. | https://docs.aws.amazon.com/bedrock/latest/userguide/troubleshooting-api-error-codes.html |
| 5 | The region that processed a cross-region request is logged in CloudTrail (`additionalEventData.inferenceRegion`) in the source region; the Converse response does not state it. | https://docs.aws.amazon.com/bedrock/latest/userguide/cross-region-inference.html |
| 6 | Geographic profiles keep processing within a geography (US, EU, APAC and others); global profiles route to any commercial region at about 10% lower cost; price is computed from the source region; data stored for abuse detection is stored in the destination region. | https://docs.aws.amazon.com/bedrock/latest/userguide/cross-region-inference.html ; https://docs.aws.amazon.com/bedrock/latest/userguide/geographic-cross-region-inference.html |
| 7 | Profile prefixes include `us`, `eu`, `apac`, `in` (geographic) and `global`; `us-gov` is used in the GovCloud partition (Fact 3); country-level `jp` and `au` profiles are announced in an AWS Machine Learning blog post (Japan and Australia cross-region inference for Claude Sonnet 4.5 and Haiku 4.5) and are confirmed against model cards at implementation. Destination regions depend on the source region and may include opt-in regions; AWS warns not to rely on the prefix alone for residency and to verify destinations with `GetInferenceProfile` per source region. Geographic profiles' destination lists do not change; global ones may. | https://docs.aws.amazon.com/bedrock/latest/userguide/inference-profiles-support.html ; https://docs.aws.amazon.com/bedrock/latest/userguide/geographic-cross-region-inference.html |
| 8 | SigV4 for the OpenAI-compatible paths on `bedrock-runtime` uses signing service name `bedrock` (documented example `aws:amz:us-east-1:bedrock`). | https://docs.aws.amazon.com/bedrock/latest/userguide/inference-chat-completions.html |
| 9 | No AWS documentation read on 2026-10-16 states token log-probabilities for Converse or for the Responses API on Bedrock; log-probabilities are documented only for Custom Model Import through InvokeModel (`return_logprobs`). Absence is the fact relied on; it is re-checked at implementation. | https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html ; https://aws.amazon.com/blogs/machine-learning/unlock-model-insights-with-log-probability-support-for-amazon-bedrock-custom-model-import |
| 10 | AWS recommends `bedrock-runtime` for new applications and `bedrock-mantle` only for capabilities available only there; per-token pricing is identical on both. | https://docs.aws.amazon.com/bedrock/latest/userguide/endpoints.html |
| 11 | The published AWS Signature Version 4 test suite is maintained in `awslabs/aws-c-auth` under `tests/aws-signing-test-suite/v4/` (cases such as `get-vanilla`, `post-vanilla`, `get-vanilla-with-session-token`, `post-header-key-sort`, `get-utf8`; each with `context.json`, `request.txt`, `header-canonical-request.txt`, `header-string-to-sign.txt`, `header-signature.txt`, `header-signed-request.txt`). The implementation vendors the cases it runs from a pinned commit and records it. | https://github.com/awslabs/aws-c-auth/tree/main/tests/aws-signing-test-suite/v4 |

## Acceptance

- Documented-shape fixtures, and recorded fixtures once a smoke has run,
  with no network, for both wires: `classify`, `proposition` and `rubric`
  under each `structured_output` mode; every row of 3.10 including
  throttling and model-not-ready; every stop reason; missing usage
  (`unknown`); provider-reported extras retained and not supplied.
- Fixtures for `rank`, `generate`, `extract` and screening on both wires are
  acceptance of C2, B5, B6 and B7 respectively (3.12.3).
- Allowlist refusals for model, region, wire and global-profile cases; deny
  wins; empty allow admits nothing; refusal names the rule; re-evaluation
  through `replace_policy` fails later attempts unsent.
- Residency refusals for source region, profile kind, destination set,
  `global` without `any`, and prefix mismatch.
- Identity changes when region, profile, destinations, residency policy,
  parameters, structured output, pricing or wire change; it does not change
  when the `ModelPolicy`, credentials or journal path change. A golden
  fixes one version 1 binding digest so later admitted values cannot move
  it.
- Existing reference-plan `PlanId` goldens are byte-identical.
- Signing verified against the AWS Signature Version 4 test suite of Fact
  11 (at least `get-vanilla`, `post-vanilla`, `get-vanilla-with-session-token`,
  `post-header-key-sort`, `get-utf8`, `get-vanilla-query-order-key-case`),
  plus a Bedrock-shaped request signed for service `bedrock`.
- Without the `aws-config` feature: the dependency graph contains no
  `aws-config`, a test with AWS environment variables set shows they are not
  read, and no `std::env` access exists outside the feature module and the
  smoke example.
- With the `aws-config` feature: absent credential environment variables
  are a credential failure, never a panic, tested in declared verification
  under that feature (3.5 item 2, A-65).
- Cancellation before and after the first request byte; one total budget;
  credentials absent from every record and error detail.
- `make boundaries` unit tests for the cloud SDK family (3.13).
- Optional live smoke and probe behind `RUSTEV_BEDROCK_LIVE=1` and the R-29
  testing journal, never in default CI.
- `make code`, declared verification, and `make gate` pass.

## Verification

Repository-local and deterministic; no command contacts AWS. `030` joins
`make verify` with the change that lands the recorded fixtures. Live probe
and smoke results are recorded with their journal totals in the
implementation record and are not part of declared verification.

```verify:cli
# 3.2 to 3.14 without network: documents, policy, residency, mappings,
# signing, the loopback transport, identity goldens, spend, and the
# documented-shape and recorded fixtures replayed offline.
cargo test -p rustev-bedrock --locked
# 3.5 item 2 (A-65): the aws-config feature's environment-only provider.
cargo test -p rustev-bedrock --locked --features aws-config
# 3.13: the cloud SDK family in the boundary check.
cargo test -p rustev-boundaries --locked
```

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it.

**Increment 1 (documents, policy, wire mappings and signing; no network).**
`integrations/rustev-bedrock/` is a new workspace crate (`publish = false`)
that compiles spec 009's Part A sources and spec 012's spend journal in by
path, as `rustev-jev` does. `src/binding.rs` defines
`rustev.bedrock-binding/1` (3.2) with every member in the artifact identity
and the members reserved for later specs (3.12); `src/policy.rs` the
host's `ModelPolicy` (deny wins, no default allowlist, 3.4) and residency
and inference-profile evaluation (3.3); `src/mapping.rs` the Converse and
Responses request and response mappings under the binding's mapping
version (3.6, 3.7); `src/signing.rs` SigV4 through `aws-sigv4` with an
injected clock and a host credential provider whose errors never carry key
material (3.5). `tools/rustev-boundaries/` adds the cloud SDK family
(3.13). The workspace gains `aws-sigv4`, `aws-credential-types` and, for
the crate's optional `aws-config` feature, `aws-config`; the full AWS SDK
is not a dependency. Tests: `tests/signing.rs` against vendored vectors of
the published AWS SigV4 test suite (`fixtures/sigv4/`, with their licence
and provenance) and `tests/contract.rs` (binding, policy and mapping
cases of section 5). Charge (3.9.2): Converse's `inputTokens` counts only
non-cached input (AWS: total input is `inputTokens + cacheReadInputTokens
+ cacheWriteInputTokens`), so each count is priced once; Responses'
`input_tokens` includes `cached_tokens`, which are subtracted, and a
nonzero `cache_write_tokens` is `unknown`, because AWS does not state
whether `input_tokens` includes it.

**Increment 2 (the backend, transport and probe; loopback only).**
`src/adapter.rs` implements `DecisionBackend` for `classify`, `proposition`
and `rubric` as `Label` outputs (3.7) over Rustev's hyper and rustls stack
(`src/net.rs`), one undivided attempt budget and one fresh connection per
attempt before spec 020's policies are declared (2), with served identity
(3.8), cost and spend against spec 012's journal (3.9) and the failure
taxonomy (3.10). `src/bin/rustev-bedrock.rs` is the reachability probe
(3.11) and `examples/smoke.rs` the live smoke (3.14); neither runs in
tests. Credentials come only from the host's provider unless the crate's
`aws-config` feature adds `src/aws_config_provider.rs` (3.5). Tests:
`tests/loopback.rs` (a loopback fake over each wire and error code),
`tests/identity.rs` (artifact identity and the unchanged reference-plan
goldens) and `tests/spend.rs`.

Not done, and blocked on the owner: the live probe and smoke against AWS
(3.11, 3.14), which need the owner's AWS credentials within R-29's cap,
and therefore the recorded live fixtures; `030` joins `VERIFIED_SPECS` with
its declared verification once those exist.

**The `aws-config` provider (2026-10-29).** The first live probe run
reached no endpoint. `src/aws_config_provider.rs` called
`aws_config::defaults(..).load()`, which builds the default region chain
and with it an instance metadata client; `aws-config` is built with
`default-features = false`, so it has no HTTP client and panicked
before any credential was read. The panic came after the probe had
reserved its first call (641,190 units) against the testing journal;
nothing was sent, and the reservation stays unsettled, counted as
liability as 3.9 requires. The owner chose an environment-only provider
over enabling a second HTTPS client (A-65), and 3.5 item 2 is amended to
say so. The owner may add the full default chain later as a separate
feature, with its own decision on a second HTTPS client.

**Increment 3 (live probe and smoke, recorded fixtures; 2026-10-29).**
Model `us.anthropic.claude-haiku-5-5` on the Converse wire, source region
`ca-central-1`, `geo` profile, declared destinations `ca-central-1`,
`us-east-1`, `us-east-2` and `us-west-2` as the AWS model card lists them
for that source (the testing role may not call `GetInferenceProfile`),
under the owner's residency decision (A-65). Prices are AWS's public price
list for `ca-central-1`, geographic standard rate (USD 0.11 input, 0.55
output, 0.011 cache read, 0.1375 cache write per million tokens).
`max_tokens` is 1024. The probe (3.11) was reachable on three sequential
calls and the smoke (3.14 item 4) returned a mapped output on one; each
answered `apple`, with 613 input and 34 output tokens charged at 86,130
units estimated.

- Two defects found by the live stage are fixed. The `aws-config`
  provider panicked (see above). The `host` header carried the default
  port (`:443`), and bedrock-runtime refused the signature with
  `InvalidSignatureException` on two calls; `host_header` now drops the
  scheme's default port, as the SDKs sign it, and a control-plane request
  signed both ways confirmed the cause without invoking a model.
- The probe and smoke gain `--out DIR`, which retains request and response
  bodies and refuses to write any part that carries a credential value, and
  print the journal summary before and after. `fixtures/recorded/` holds
  the live binding and policy and the four exchanges, labeled recorded,
  synthetic content only, AWS request ids redacted; `tests/recorded.rs`
  replays them on loopback with the recorded request digest, output digest
  and charge. `tests/credentials.rs` covers the provider; `tests/spend.rs`
  now writes its scratch journal under `CARGO_TARGET_TMPDIR`.
- Testing journal (budget `testing`, nano-USD units): 12,499,578 committed
  before (1,421,364 observed, 11,078,214 liability); 14,767,668 after
  (1,421,364 observed, 344,520 estimated, 13,001,784 liability). Bedrock's
  share is 2,268,090 units (USD 0.002268): 344,520 estimated for the four
  answered calls, and 1,923,570 liability for three calls whose charge is
  unknown (the reservation left by the panic, sent nothing; the two refused
  signatures, which reached AWS).
- Not done: a recorded Responses exchange. The model used does not serve
  the Responses API on bedrock-runtime, so recording one needs another
  model and binding, and therefore another paid run.

## Decisions at approval

Each is decided by the implementer within this spec under the work order,
except where the issue or an approved spec already settles it.

1. **Endpoint.** Both wires use `bedrock-runtime`. Evidence: AWS
   recommends it, Guardrails and cross-region profiles exist only there,
   and both wires then share one signer and host (Facts 3, 10). The issue's
   "OpenAI-compatible Responses API on Bedrock" exists as described (Fact
   2); `bedrock-mantle` is reserved and refused.
2. **`store: false`.** The Responses API stores input and output for 30
   days by default (Fact 2). The issue is silent; storing caller text at the
   provider would contradict R-19's digest-only default, so every Responses
   request sends `store: false` and the binding refuses anything else.
3. **Log-probabilities.** The issue's rule stands (no distribution without
   token log-probabilities), but neither wire documents them (Fact 9), so
   wave 1 outputs are `Label` only and plans requiring distributions are
   refused against this backend.
4. **Charge class.** The issue says `observed` from usage times price. Spec
   009 3.8.1 (approved) says that is `estimated`; the approved spec stands.
5. **Compile-time refusal.** Realized at bind time and in `check_plan`
   (3.3.3, 3.4.4); `rustev-core` gains no vendor-specific hook (spec 009,
   I-7).
6. **Probe command.** `rustev-bedrock probe`, not `rustev bedrock probe`,
   because spec 001 3.4.4 (approved) forbids the CLI from depending on an
   integration; the approved spec stands.
7. **Residency judged on declared destinations,** not on prefixes, because
   AWS says prefixes alone do not determine routing (Fact 7).
8. **Policy not identity.** The `ModelPolicy` is gating, not output, and is
   recorded by digest; editing an allowlist changes no `PlanId`.
9. **Reserved members** for later operations, Guardrails, log-probabilities
   and spec 020 policies keep binding identity stable (3.12).
10. **Refusal charges** are `unknown` until a fact establishes otherwise.
11. **Testing cap.** R-29's USD 5 testing cap is one cap across providers:
    Bedrock's live calls reserve against the same persistent testing journal
    that already holds the spec 012 spend.
12. **Served identity.** Converse: `unknown`; Responses: an echo of the
    requested id is not a served identity (3.8).
13. **Fixtures** are documented-shape until a smoke records responses;
    no live call is made to produce them by this change.
14. **Model targets.** Foundation model ids and system-defined profiles
    only; other target kinds are out of scope.
15. **Authentication.** SigV4 only; Bedrock API keys are out of scope.
16. **Boundary family.** The cloud SDK family is added to spec 001 3.5 by
    this spec's narrow amendment, since 001 says adding a family is a change
    to it.
17. **Guardrails on inference.** `guardrail` stays `none` until B7 defines
    screening; no guardrail is attached to decision calls before then.
18. **Paid live calls (settled by the work order).** The 2026-10-16
    delivery work order: "Live provider calls are limited to smoke tests,
    qualification and E3 live evaluation, on synthetic or spec 023 admitted
    data only. The R-29 caps stay in force: USD 5 in total for all testing
    across all providers unless I raise it in writing. Use the persistent
    spend journal, and refuse before sending any call that would exceed the
    cap." Paid Bedrock smoke and probe calls (3.11, 3.14) are therefore
    authorized within that scope, reserved against the persistent testing
    journal and refused unsent over the cap. AWS credentials and model
    access are supplied by the owner; their absence only blocks the live
    stage.

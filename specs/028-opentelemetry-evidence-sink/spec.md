---
id: "028-opentelemetry-evidence-sink"
title: "OpenTelemetry evidence sink and redaction policy"
status: approved
implementation: pending
created: "2026-10-16"
summary: >
  Approved (A-24) as its own reviewable change before code. A new optional
  integration, `integrations/rustev-otel`, exports each delivered run record
  as OpenTelemetry spans, metrics and optional log records, with GenAI
  semantic-convention attributes on backend attempts, W3C trace-context
  parenting from the host, OTLP over gRPC or HTTP to allowlisted host
  collectors only, and a local JSONL exporter. Every exported field passes a
  declared redaction policy, `rustev.redaction/1`, that defaults to deny for
  caller, third-party, attributed-claim and generated text. The policy
  document, its evaluation, the keyed digest and the `rustev.trace/1` span
  line document are pure additions to `rustev-contract`; `rustev trace tail`
  is added to the CLI with no OpenTelemetry dependency; the boundary check
  gains the telemetry exporter family. Approval does not authorize a release,
  a deployment, an export to any collector outside tests, or real-user data.
amends:
  - "001-boundaries-and-authority"
  - "006-cli-surface"
establishes:
  - { kind: directory, path: "integrations/rustev-otel/" }
extends:
  # The redaction policy and trace span documents, the field catalogue, the
  # policy evaluation and the keyed digest (3.3 to 3.6). Purely additive: no
  # existing document, identity or byte changes.
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: additive }
  # `rustev trace tail` (3.11), an amendment of spec 006.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The telemetry exporter family (3.12), an amendment of spec 001 3.5.
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  # OpenTelemetry workspace dependencies, used only by integrations/rustev-otel.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list (spec 021 3.5's rule, in the directory spec 031 owns).
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
  # Adds 028 to `make verify` at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "009-remote-adapter-protocol"
  - "017-general-runtime-optimization"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No value of a field whose effective redaction action is drop appears, raw or transformed, in any exported span attribute, span name, event, link, log record, metric attribute, resource attribute or JSONL line."
    anchor: "3-4-redaction-policy"
  - id: "I-2"
    kind: invariant
    text: "Attaching the sink changes no judgment and no run record byte; telemetry that is accepted and does not reach a successful export is counted under exactly one loss reason, never silently discarded."
    anchor: "3-8-sink-policies-acknowledgement-and-loss"
  - id: "I-3"
    kind: invariant
    text: "The sink connects only to a collector endpoint that exactly matches the host's allowlist; any other configured, defaulted or environment-supplied destination is refused at construction with no connection attempt."
    anchor: "3-9-export-and-endpoint-allowlist"
  - id: "I-4"
    kind: invariant
    text: "OpenTelemetry SDK, API and exporter crates are dependencies only of crates under integrations/; rustev-contract, rustev-core, rustev-runtime, rustev-eval and rustev-cli reach none of them, and make boundaries refuses a violation."
    anchor: "3-12-dependency-boundary"
  - id: "I-5"
    kind: invariant
    text: "Telemetry is a description of a proposal, never a grant: no exported value, receipt, span status or sampling decision is or implies authorization, and unresolved outcomes are exported as answers, never as defaults or success."
    anchor: "3-13-binding-principles"
  - id: "R-1"
    kind: requirement
    text: "The sink pins a recorded GenAI semantic-conventions version and a Rustev span schema version, records both on the resource and scope, and changes either only through a new span schema version with a configuration-selected window in which the previous version can still be emitted."
    anchor: "3-2-pinned-conventions-and-schema-versions"
  - id: "R-2"
    kind: requirement
    text: "Golden span trees for both reference plans, and for a scripted remote exchange, are committed as rustev.trace/1 JSONL and reproduced byte for byte under a fixed key, anchor and id generator."
    anchor: "3-5-span-model"
  - id: "R-3"
    kind: requirement
    text: "rustev trace tail renders one decision's spans from a rustev.trace/1 JSONL file under a redaction policy no wider than the one it is given, with bounded file access and no OpenTelemetry dependency."
    anchor: "3-11-rustev-trace-tail"
---

# 028: OpenTelemetry evidence sink and redaction policy

Approved by the owner (A-24, 2026-10-16, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation is
pending. It implements child D1 of tracking issue #76 (#70). Approval does not
authorize a release or publication, a deployment, an export to any collector
other than test fixtures on loopback, retaining or exporting real-user data,
or any change to an approved spec beyond the amendments of 001 and 006 stated
here.

## 1. Purpose

A host needs to read a decision while operating it, locally and in shared
environments, and to answer "why was this item chosen", "which backend and
tier ran", "did text fall back" and "why was it unresolved" without reading
code. The run record of spec 003 already carries those facts, mostly as
identities and enumerations, but cannot reach the host's observability
backend, and an ad hoc export would leak caller text.

This spec adds an optional integration that turns each delivered run record
(plus the remote exchange records of spec 009, when the host forwards them)
into OpenTelemetry spans, metrics and optional log records; a redaction
policy that every exported field passes, defaulting to deny for caller,
third-party, attributed-claim and generated text; a local JSONL development
exporter; and a CLI command that renders one decision from that file.

## 2. Territory

- **New, planned:** `integrations/rustev-otel/` (package `rustev-otel`,
  `publish = false` until spec 021's release family is extended by #76 A3).
  Normal dependencies: `rustev-contract`, `rustev-core` (the `EvidenceSink`
  seam), `rustev-runtime` (the `Gauges` and `SinkStats` types it observes),
  `serde`, `serde_json`, `sha2`, `tokio`, and, behind features, the
  OpenTelemetry crates of 3.2.3. No workspace crate depends on it (spec 001
  3.4.4). It depends on no other integration: in particular it does not
  implement `rustev-remote-http`'s `ExchangeSink` trait (3.7.3).
- **`crates/rustev-contract/` (spec 002's), additive:** new modules for the
  `rustev.redaction/1` and `rustev.trace/1` documents, the field catalogue,
  policy evaluation and the keyed digest (3.3 to 3.6), with one new
  dependency, the RustCrypto `hmac` crate (3.4.5). No existing type,
  schema string, limit, canonical byte or identity changes.
- **`crates/rustev-cli/` (spec 006's), amended:** the `trace tail` command
  (3.11). No new dependency.
- **`tools/rustev-boundaries/` and spec 001 3.5, amended:** the telemetry
  exporter family (3.12). `Cargo.toml` and `Cargo.lock` gain the
  OpenTelemetry workspace dependencies and `hmac`; `Makefile` gains `028` in
  `VERIFIED_SPECS` at completion.

**Compatibility.** No definition, plan, snapshot, execution policy, run
record, exchange record or judgment changes. Every `PlanId` and every plan
and definition golden stays byte-identical, because this spec adds no plan
construct; the existing golden tests of specs 002 and 003 prove it, and the
acceptance runs them unchanged. Attaching the sink changes no judgment and no
run record byte (3.8.6).

**What does not change.** The runtime (spec 003) gains no hook, observer or
field: telemetry is derived after the fact from the record the runtime
already delivers, the gauges it already exposes, and the exchange records
adapters already emit (3.7). Spec 003's sink policies apply unchanged.

## 3. Behavior

### 3.1 Placement of the pure parts

1. `rustev-contract` holds, as pure code with no I/O, clock or async
   runtime, and with one new dependency, `hmac` (3.4.5), beside its
   existing `sha2`:
   - the `rustev.redaction/1` document, its parse bounds, validation and
     identity (3.4);
   - the field catalogue: the closed table of exportable field names and
     each one's class rule (3.5, 3.6);
   - policy evaluation: the effective action for a field and class, and its
     application to a value;
   - the keyed digest (3.4.5);
   - the `rustev.trace/1` span line document and its bounds (3.10).
2. Why the contract: the CLI (under `crates/`) must apply the same policy
   and parse the same lines, and may not depend on an integration (spec 001
   3.4.4); an outside reader that holds the host's key must be able to
   compute a correlation digest and read a trace line by depending on
   `rustev-contract` alone (spec 001 3.4.5). These are document semantics
   plus one keyed hash, the same class of content as the contract's
   existing canonical identities and record bounds; R-06's rule that metrics
   and evaluation computation stay out of the contract is kept, because the
   metric instruments, the projection from run records and every exporter
   live in `rustev-otel`.
3. `rustev-otel` holds the projection from run records to span trees (3.5),
   the trace-context table (3.7), metrics and logs (3.6), sampling, the sink
   and its acknowledgement modes (3.8), the OTLP and JSONL exporters and the
   endpoint allowlist (3.9), and the overhead benchmark (3.14).

### 3.2 Pinned conventions and schema versions

1. **Facts as of 2026-10-16, which the implementation re-confirms and
   records in its implementation record before any code depends on them:**
   - The GenAI semantic conventions were last released inside the core
     `open-telemetry/semantic-conventions` repository in **v1.41.1
     (2026-05-11)**, where every `gen_ai.*` attribute used here has stability
     `development`. Release v1.42.0 (2026-06-12) deprecated all `gen_ai.*`
     conventions there and moved them to
     `open-telemetry/semantic-conventions-genai`, which has published no
     release and no schema URL as of 2026-10-16 (its README reads "Schema
     URL: TODO"). The core repository's latest release is v1.44.0
     (2026-08-04).
   - In v1.41.1, `gen_ai.system` is deprecated in favor of
     `gen_ai.provider.name`, and `gen_ai.usage.prompt_tokens` and
     `gen_ai.usage.completion_tokens` are renamed to
     `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`.
   - `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` and
     `opentelemetry-semantic-conventions` are at 0.33.1 (2026-10-09). The
     Rust semantic-conventions crate tracks core v1.42.0 since 0.32.1, so its
     `gen_ai` constants are the deprecated ones.
2. **Pin.** Span schema `rustev.otel/1` uses the GenAI conventions of
   v1.41.1 and the schema URL `https://opentelemetry.io/schemas/1.41.1`. If,
   when implementation starts, the GenAI repository has published a release
   with a schema URL, the implementation pins that release instead, records
   the fact, and keeps every attribute name of 3.5 that the release keeps; a
   name it changes is handled as 3.2.4 states, never silently. The issue's
   `gen_ai.system` is emitted as its pinned successor `gen_ai.provider.name`.
3. **Dependencies.** `rustev-otel` declares its attribute names from the
   contract's field catalogue and does not depend on
   `opentelemetry-semantic-conventions` (whose GenAI constants are
   deprecated). OpenTelemetry crates are workspace dependencies with
   `default-features = false`, pinned to one exact minor line (0.33 today),
   enabling only: `opentelemetry_sdk` with `trace`, `metrics`, `logs` and
   `rt-tokio`; `opentelemetry-otlp` with `grpc-tonic` (feature `otlp-grpc`
   of `rustev-otel`) and `http-proto` with `hyper-client` (feature
   `otlp-http`), TLS through rustls with `ring` as the workspace already
   uses. `reqwest` and the SDK's `internal-logs` are not enabled. The
   crate's default features are empty: projection, redaction and the JSONL
   exporter build with no OpenTelemetry crate at all.
4. **Schema versions.** Every export carries resource attribute
   `rustev.otel.schema_version` (integer, `1` for `rustev.otel/1`) and the
   pinned schema URL as the resource's and instrumentation scope's
   `schema_url` (the issue's `telemetry.schema_url`); the scope is
   `rustev-otel` at the crate version. Moving to a newer semantic-conventions
   version, or renaming, removing or retyping any attribute, span, event or
   metric of 3.5 and 3.6, is a new span schema version `rustev.otel/N+1`,
   announced in release notes. The release that introduces version N+1 and
   the next release both support emitting either version, selected per sink
   by configuration (`schema: 1 | 2`, one version per sink, never mixed in
   one trace); the version after that may remove N. The default stays the
   older version for the whole window, so hosts migrate dashboards on their
   own schedule. Adding a new attribute or a reserved span (3.5.6) is not a
   version change.

### 3.3 Field classes

1. Every exportable field belongs to exactly one class. The classes are:
   - provenance classes, with the contract's names: `user-supplied`,
     `third-party`, `attributed-claim`, `authenticated-app-field`,
     `system-of-record`, `model-derived`;
   - `generated-text`: text produced by a model step (reserved: no such step
     exists until #76 B5 and B6);
   - `free-text-detail`: free text written by an adapter, the runtime or the
     core at run time (failure details, stop reasons, unresolved details),
     which may echo backend or caller bytes;
   - `id`: identifiers that correlate (decision, attempt, instance and
     candidate ids, host source labels, provider generation ids, batch ids);
   - `content-digest`: unkeyed digests of content (snapshot id, exchange
     request and response digests), which a dictionary attack could invert
     for short content;
   - `enum`: closed vocabularies of the contract (outcome, termination,
     unresolved reason, failure class, derivation, step kind, cost mode) and
     boolean or enum values whose option set the plan declares;
   - `number`: counts, durations, costs, token usage and other numeric
     observations that carry no input value;
   - `plan-metadata`: identities and names the plan or definition authored
     (plan, definition and execution policy ids, step and rule ids, action
     names, input field names, notice reasons, backend and artifact ids);
   - `model-identity`: requested and served model identities and routes.
2. **Lineage rule for values.** An outcome parameter, ranking score or other
   value carried in a judgment is classed by what it can contain. A boolean
   or enum value is `enum`. Any other value takes the most restrictive
   provenance class among the judgment lineage's inputs, in the order
   `attributed-claim`, `third-party`, `user-supplied`, `model-derived`,
   `authenticated-app-field`, `system-of-record`; a value whose lineage has
   no input is `plan-metadata`. A text value from a model step is
   `generated-text` once such steps exist. The rule never assigns a less
   restrictive class than an input the value may copy.

### 3.4 Redaction policy

1. **Document.** `rustev.redaction/1`, parsed under `DESCRIPTOR_V1` with
   `deny_unknown_fields`:
   - `schema`;
   - `classes`: a map from class to action, which may name any subset of the
     classes of 3.3.1; an unnamed class takes its default (3.4.3);
   - `fields`: a list of `{field, action}` overrides, each naming one
     catalogue field (3.5, 3.6) exactly, at most once, at most 512 entries;
   - `content_events`: a list of catalogue content fields (3.5.7) whose
     GenAI content events may be emitted; empty by default.
   Actions: `drop`; `digest`; `truncate{bytes}` with `bytes` in `1..=4096`,
   cut at a UTF-8 character boundary; `allow`. The policy identity
   `RedactionPolicyId` is `sha256:` of the canonical bytes.
2. **Validation.** A policy is refused, naming the entry, when: a field is
   not in the catalogue or is named twice; a `content_events` entry is not a
   content field; a `classes` entry gives `allow` or `truncate` to a deny
   class (`user-supplied`, `third-party`, `attributed-claim`,
   `generated-text`, `free-text-detail`): exposing such a value requires
   naming each field in `fields`, never a class-wide grant. The other
   classes that default to `drop` (`authenticated-app-field`,
   `system-of-record`, `model-derived`) may be granted class-wide: the first
   two come from the authenticated application or a system of record, not
   from the caller, and by the lineage rule (3.3.2) a value is
   `model-derived` only when no input it may copy is `user-supplied`,
   `third-party` or `attributed-claim`, while model text is
   `generated-text`, a deny class. A policy is also refused when a field override
   or class entry tries to change a fixed field (3.4.4); or bounds are
   exceeded.
3. **Default policy** (also the effective policy when the host supplies
   none): `drop` for `user-supplied`, `third-party`, `attributed-claim`,
   `generated-text`, `free-text-detail`, `authenticated-app-field`,
   `system-of-record` and `model-derived`; `digest` for `id` and
   `content-digest`; `allow` for `enum`, `number`, `plan-metadata` and
   `model-identity`; `content_events` empty. Whatever issue #70 did not
   name as `digest` or `allow` is `drop`.
4. **Effective action and fixed fields.** A field's action is its `fields`
   override if any, else its class's entry, else the class default. Fixed
   fields are always `allow` and cannot be overridden: the schema and
   policy attributes `rustev.otel.schema_version`,
   `rustev.redaction.policy_id`, `rustev.redaction.key_id`,
   `rustev.redaction.withheld`, and span names. Span names are the constant
   names of 3.5 and never carry a value.
5. **Keyed digest.** `digest` emits
   `hmac-sha256:<64 lowercase hex>` of HMAC-SHA256 (RFC 2104, computed
   with the RustCrypto `hmac` crate over the existing `sha2`, never a
   hand-written construction) with the
   host key, over
   `"rustev.digest/1" 0x00 <class> 0x00 <value bytes>`, where value bytes are
   the UTF-8 string or the canonical JSON of a non-string value. The class,
   not the field, is the domain, so the same id correlates across fields of
   one class. The key is host-supplied, 32 to 64 bytes, never exported,
   logged, embedded in `Debug` output or written to a trace line; the host
   names it by an opaque `key_id` (1 to 64 bytes) recorded on the root span,
   so a reader knows which key produced a digest. Rotating the key changes
   every digest; that is intended. Without a key, the sink refuses
   construction if any effective action is `digest`.
6. **Recording what was withheld.** The root span carries
   `rustev.redaction.policy_id` and `rustev.redaction.key_id`; every span
   carries `rustev.redaction.withheld`, the sorted list of catalogue field
   names (never values) that were dropped on that span. A catalogue field
   name is `plan-metadata` by construction (instance-specific keys such as
   `rustev.outcome.param.<name>` use the plan's parameter name).
   The exception is `rustev.exchange.extra.<key>`, whose key the provider
   supplies: a dropped extra is listed once per span as
   `rustev.exchange.extra.*`, never by its key.
7. **Application is total.** Every value leaves the projection only through
   policy application; there is no code path that writes an attribute,
   event, log body or metric attribute from a record value without it. Log
   bodies are fixed templates over `enum` values (3.6.3). Metric attributes
   are only `enum` and `plan-metadata` fields listed in 3.6.1, whatever the
   policy says, so no `id`, digest or text becomes a metric dimension.

### 3.5 Span model

1. **Tree.** One trace per decision:
   - `rustev.decision` (root; child of the host span when a valid
     `traceparent` was registered, 3.7);
   - one `rustev.step` per step evidence entry, in evidence order, child of
     the root;
   - one `rustev.attempt` per attempt record of that step's request, in
     attempt order, child of its step;
   - one `rustev.policy`, child of the root, last.
   Inputs, notices, transitions and exclusions are span events (3.5.5).
2. **Time.** Run records carry monotonic milliseconds from submission
   (spec 003 3.3). The sink reads its wall clock once per decision as the
   anchor: at `begin` (3.7.1) when the host registered the decision, else at
   delivery minus `elapsed_ms`; `rustev.time_anchor` records `begin` or
   `delivery`. Root: anchor plus `queued_ms` to anchor plus `elapsed_ms`.
   Attempt: anchor plus `dispatched_ms` to anchor plus `ended_ms`. A semantic
   step spans its attempts' earliest dispatch to latest end. Exact steps,
   steps with no attempt (a cache hit, or nothing dispatched) and the policy
   span have no recorded time: they are zero-length at the root's end and
   carry `rustev.timing = not_recorded`. Nothing is invented.
3. **Status and kind.** An unresolved, escalated or cancelled decision is an
   answer (principle VIII): the root's status stays unset. An attempt that
   ended `failed` or `deadline` has status error and `error.type` set to the
   failure class or `deadline`. Attempts with GenAI attributes are kind
   `client`; every other span is `internal`.
4. **Attribute table.** Every attribute below maps to an existing record
   field or is marked as arriving with a later spec, which adds it under
   3.2.4 without a version change. "Run" means `rustev.run/1`; "evidence"
   the core `rustev.evidence/1` inside it; "exchange" a joined
   `rustev.remote-exchange/1`; "plan" the registered plan (3.7.2).
   Every span also carries `rustev.redaction.withheld` (fixed, 3.4.6), and a
   span with no recorded time (3.5.2) carries `rustev.timing` (enum, the
   value `not_recorded`); the tables below list neither.

   `rustev.decision`:

   | Attribute | Class | Source |
   |---|---|---|
   | `rustev.decision.id` | id | run `decision_id` |
   | `rustev.plan.id` | plan-metadata | run `plan_id` |
   | `rustev.definition.id` | plan-metadata | evidence `definition_id` (absent when `not_produced`) |
   | `rustev.snapshot.id` | content-digest | evidence `snapshot_id` |
   | `rustev.execution.policy_id` | plan-metadata | run `execution` (`none` or the id) |
   | `rustev.termination` | enum | run `termination` |
   | `rustev.outcome` | enum | judgment `outcome` variant, or `not_produced` |
   | `rustev.outcome.action` | plan-metadata | `propose.action` |
   | `rustev.outcome.escalate_reason` | plan-metadata | `escalate.reason` |
   | `rustev.outcome.unresolved_reason` | enum | `Unresolved` variant |
   | `rustev.outcome.unresolved_fields` | plan-metadata | fields or facts of the variant |
   | `rustev.outcome.unresolved_detail` | free-text-detail | `detail` of the variant |
   | `rustev.outcome.param.<name>` | lineage rule (3.3.2) | `propose.params` |
   | `rustev.derivation` | enum | judgment `derivation` |
   | `rustev.lineage.inputs`, `rustev.lineage.steps` | plan-metadata | judgment `lineage` |
   | `rustev.cost.mode`, `rustev.cost.final` | enum | run `cost` |
   | `rustev.cost.limit`, `.observed`, `.estimated`, `.liability`, `.bound_violations` | number | run `cost` |
   | `rustev.cost.within_guaranteed_cap` | enum | run `cost` |
   | `rustev.timing.queued_ms`, `.elapsed_ms`, `.deadline_ms` | number | run `timing` |
   | `rustev.timing.deadline_expired` | enum | run `timing` |
   | `rustev.deadline.remaining_ms` | number | `deadline_ms` minus `elapsed_ms`, saturating at 0 |
   | `rustev.time_anchor` | enum | 3.5.2 |
   | `rustev.redaction.policy_id`, `rustev.redaction.key_id` | fixed | sink configuration |
   | `rustev.profile.id`, `rustev.variant` | plan-metadata | arrives with #76 B4 (#62) |
   | `rustev.addressed_to` | enum | arrives with #76 B3 (#61) |
   | `rustev.text_origin` | enum | arrives with #76 B5 (#63) |

   `rustev.step`:

   | Attribute | Class | Source |
   |---|---|---|
   | `rustev.step.id` | plan-metadata | evidence step `step` |
   | `rustev.step.instance` | id | evidence step `instance` |
   | `rustev.step.kind` | enum | plan definition: `exact`, `classify`, `proposition`, `rubric`, `rank`; `unknown` when the plan is not registered |
   | `rustev.step.operator` | plan-metadata | plan step detail `exact.op` at `version` |
   | `rustev.step.backend_id` | plan-metadata | plan step detail `semantic.backend_id` |
   | `rustev.step.status` | enum | evidence step `status` (`resolved`, `unresolved`) |
   | `rustev.step.value_kind` | enum | `resolved.kind` |
   | `rustev.step.derivation` | enum | `resolved.derivation` |
   | `rustev.step.unresolved_reason` | enum | `unresolved` variant |
   | `rustev.step.result` | enum | request `result` (`output`, `failed`, `not_supplied`) |
   | `rustev.step.target` | number | `output.target` |
   | `rustev.step.optimization.outcome`, `.mechanism` | enum | request `optimization` |
   | `rustev.step.reused_from` | id | `RequestRecord::supplied_from().reused_from` |
   | `rustev.step.grade` | enum | arrives with #76 B2 (#60) |

   `rustev.attempt`:

   | Attribute | Class | Source |
   |---|---|---|
   | `rustev.attempt.id` | id | attempt `attempt_id` |
   | `rustev.attempt.ordinal` | number | `n` of the attempt id |
   | `rustev.attempt.retry_index` | number | attempts on the same target before this one |
   | `rustev.attempt.target` | number | attempt `target` (0 primary, k fallback) |
   | `rustev.attempt.backend_id`, `rustev.attempt.artifact` | plan-metadata | attempt `backend_id`, `artifact` |
   | `rustev.attempt.end` | enum | attempt `end` variant |
   | `rustev.attempt.failure_class`, `error.type` | enum | `failed.class` |
   | `rustev.attempt.failure_detail` | free-text-detail | `failed.detail` |
   | `rustev.attempt.cancel` | enum | `cancellation` (`not_requested`, `stopped`, `unconfirmed`, `not_observed`) |
   | `rustev.attempt.remote_state` | enum | attempt `remote` |
   | `rustev.attempt.cost.bound` | enum | `cost.bound` variant |
   | `rustev.attempt.cost.reserved` | number | `cost.reserved` |
   | `rustev.attempt.charge_kind` | enum | `cost.charge` variant |
   | `rustev.attempt.charge_units` | number | `cost.charge` units (absent when `unknown`) |
   | `rustev.batch.id` | id | request `optimization.batch_id` |
   | `rustev.batch.member` | number | request `optimization.batch_member` |
   | `gen_ai.provider.name` | model-identity | host map from backend id (3.7.2); else `unknown` |
   | `gen_ai.operation.name` | enum | the step's operation (`classify`, `proposition`, `rubric`, `rank`) |
   | `gen_ai.request.model` | model-identity | exchange `requested_model`; else the attempt `artifact` |
   | `gen_ai.response.model` | model-identity | exchange `served.identity` when disclosure is `pinned` or `reported`; else `unknown` |
   | `rustev.served.disclosure` | enum | exchange `served.disclosure`; else `unknown` |
   | `gen_ai.response.id` | id | exchange `generation_id` |
   | `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens` | number | exchange `usage` under the binding's declared keys (3.7.2); absent, never 0, when not reported |
   | `rustev.exchange.request_digest`, `.response_digest` | content-digest | exchange digests |
   | `rustev.exchange.status` | number | exchange `status` |
   | `rustev.exchange.bytes_sent`, `.late`, `.truncated` | enum | exchange flags |
   | `rustev.exchange.route` | model-identity | exchange `route` |
   | `rustev.exchange.reported_cost` | number | exchange `reported_cost.amount` |
   | `rustev.exchange.reported_currency` | enum | exchange `reported_cost.currency` |
   | `rustev.exchange.retry_after` | third-party | exchange `retry_after` |
   | `rustev.exchange.extra.<key>` | third-party | exchange `provider_extras` |
   | `rustev.exchange.member_detail` | free-text-detail | exchange member `failure.detail` |

   GenAI attributes are emitted only on an attempt that joined an exchange
   record or whose backend id the host mapped to a provider name; a local
   deterministic backend gets none.

   `rustev.policy`:

   | Attribute | Class | Source |
   |---|---|---|
   | `rustev.policy.rules_fired` | plan-metadata | judgment `trace` |
   | `rustev.policy.excluded_count` | number | `excluded` of `filtered`, `shortlist` and `ranking` params |
   | `rustev.policy.truncated` | number | `shortlist.truncated` |
   | `rustev.policy.position_origin` | enum | arrives with #76 B5 (#63) and C2 (#67) |

5. **Events.** `rustev.input` on the root per input evidence entry: `field`
   (plan-metadata), `status` and `provenance` (enum), `as_of_ms` (number),
   `source` (id), `unresolved_reason` (enum). `rustev.notice` on the root per
   notice: `kind` (enum), `subject` and `reason` (plan-metadata).
   `rustev.transition` on a step per transition: `kind`, `class` (enum),
   `after`, `to`, `delay_ms` (number), `reason` (free-text-detail).
   `rustev.excluded` on the policy span per excluded candidate: `candidate`
   (id), `reasons` (plan-metadata). `rustev.position` per ranking entry:
   `position` (number), `candidate` (id), `score` (lineage rule). Events are
   capped at 128 per span; excess is counted in `rustev.events_cut`.
6. **Reserved spans and step kinds.** Emitted when their specs land, under
   this schema version, with the attributes the issue lists: `rustev.acquire`
   and `rustev.widen.tier` (#76 B1, #59: source, operation, request digest,
   items returned, tier, relaxation); step kinds `acquire` and `widen`
   (B1), `generate` (B5, #63), `extract` (B6, #64), `screen` (B7, #65), and a
   model-backed `rank` with backfill attributes (C2, #67). Each of those
   specs adds its rows to this table and its golden span tree. Until then
   the projection emits none of them, and a record variant it does not know
   is a projection error counted as `projection_unknown`, never a guessed
   span.
7. **Content events.** GenAI content (`gen_ai.input.messages`,
   `gen_ai.output.messages`, `gen_ai.system_instructions`) is emitted only
   for catalogue content fields named in `content_events` and allowed by
   their effective action. Today's records carry no prompt or completion
   content, so no content event exists until B5 or B6 adds content fields.
8. **Goldens.** Committed under `integrations/rustev-otel/tests/golden/` as
   `rustev.trace/1` JSONL, produced under the default policy, a fixed 32-byte
   test key with `key_id` `test`, a fixed anchor and a seeded id generator:
   - `support.trace.jsonl` from spec 006's `support.record.json` and the
     support-routing plan;
   - `lodging.trace.jsonl` from `lodging.record.json` and the lodging plan;
   - `support-remote.trace.jsonl` from the support record joined with
     committed synthetic `rustev.remote-exchange/1` fixtures (requested and
     served model, pinned and unknown disclosure, usage keys, provider
     extras, a failure detail), exercising every GenAI row.
   Each golden is reproduced byte for byte, and a second golden per plan
   under an all-`allow` test policy proves the table's coverage (every
   source field present is exported). Features of B1, B5 and C2 add their
   goldens when they land.

### 3.6 Metrics and logs

1. **Instruments** (metric attributes only from the `enum` and
   `plan-metadata` fields named here):
   - `rustev.decisions` counter: `rustev.outcome`, `rustev.termination`,
     `rustev.plan.id` (and `rustev.addressed_to` with B3);
   - `rustev.unresolved` counter: `rustev.outcome.unresolved_reason` or
     `rustev.step.unresolved_reason`, `scope` (`outcome` or `step`),
     `rustev.plan.id`;
   - `rustev.fallbacks` counter: `kind` = `runtime_fallback` today (from
     `fallback` transitions); `backstop` (B5) and `backfill` (C2) when they
     land; rates are this counter over `rustev.decisions`;
   - `rustev.screening` counter, reserved for B7 (`blocked`, `unavailable`);
   - `rustev.decision.duration` and `rustev.attempt.duration` histograms in
     milliseconds: `rustev.step.kind`, `rustev.attempt.end`,
     `rustev.attempt.backend_id`; `rustev.acquisition.duration` reserved for
     B1;
   - `rustev.decision.cost` histogram in cost units: `rustev.cost.final`;
   - `gen_ai.client.token.usage` histogram (`gen_ai.token.type`,
     `gen_ai.operation.name`, `gen_ai.provider.name`,
     `gen_ai.request.model`) and `gen_ai.client.operation.duration`, for
     attempts with GenAI attributes and reported usage;
   - `rustev.admission.rejected` and `rustev.evidence.sink` observable
     counters (`result` = `acknowledged`, `failed`, `timed_out`, `dropped`),
     read from `Runtime::gauges()` through a host-registered callback,
     because rejected decisions produce no run record;
   - `rustev.telemetry.lost` counter by loss reason (3.8.4) and
     `rustev.telemetry.sampled_out` counter.
2. Metrics are never sampled: every delivered record updates them.
3. **Logs** (`logs: off | on`, default `off`). When on, one log record per
   decision whose outcome is `unresolved` or `escalate` or whose termination
   is `cancelled`, and one per attempt that ended `failed` or `deadline`,
   correlated to the span, with severity `WARN`, a fixed template body over
   `enum` values (for example `decision unresolved: missing_evidence`) and
   the span's redacted attributes. Logs follow the decision's sampling
   result.

### 3.7 Trace context, plans and exchange records

1. **Parenting.** `OtelSink::begin(decision_id, traceparent, tracestate)`
   registers a decision before the host submits it and returns its
   `trace_id` and root `span_id`, so a host (and the #76 A1 Node binding)
   can link its own logs. A W3C `traceparent` that fails to parse, has
   version `ff`, or has an all-zero trace or parent id is ignored as the W3C
   specification requires: the decision starts a new trace, and the count
   `invalid_traceparent` rises. `tracestate` is carried unchanged when the
   parent is valid. The table holds at most `max_pending_contexts` (default
   4 096) entries; `begin` on a full table returns `TableFull`, counted,
   and the decision is exported as a new trace. An entry is removed on
   delivery, on `abandon(decision_id)` (for a rejected decision), or after
   `context_ttl_ms` (default 600 000) by the sink's monotonic clock, counted
   as `context_expired`. A decision delivered without `begin` gets a new
   trace with random ids.
2. **Plans and bindings.** The host registers each compiled plan it runs;
   step kinds and operators come from the plan's embedded definition. An
   unregistered `PlanId` exports with `rustev.step.kind = unknown`, counted
   as `plan_unregistered`. The host may map backend ids to a
   `gen_ai.provider.name` value and declare, per backend id, which exchange
   `usage` keys are input and output tokens. Rustev names no vendor: these
   values come only from host configuration or the exchange record.
3. **Exchange records.** `OtelSink::record_exchange(&ExchangeRecord)`
   accepts spec 009's contract document; the host forwards it from its own
   `ExchangeSink` implementation (a few lines), because one integration may
   not depend on another. Records wait in a bounded join buffer keyed by
   attempt id (default 4 096 entries, same TTL); at delivery each attempt
   joins its record. A record that arrives after its decision was exported,
   or that the full buffer refuses, is counted as `exchange_unjoined`; the
   attempt's GenAI fields are then `unknown` or absent, never guessed.

### 3.8 Sink policies, acknowledgement and loss

1. **The sink.** `OtelSink` implements `rustev_core::seams::EvidenceSink`.
   `deliver` projects and redacts the record (bounded, synchronous), applies
   sampling (3.9.4), updates metrics, and hands the whole span tree to the
   export queue as one unit: all spans of a decision are queued or none are.
2. **Acknowledgement modes.** In every receipt, `<decision digest>` is the
   delivered run record's `record_digest()` (`sha256:` of the schema-tagged
   canonical bytes, spec 004 3.1.6), as the CLI's file sink uses in
   `file:<digest>`.
   - `accepted` (default): the receipt is `otel:accepted:<decision digest>`
     once the tree is in the bounded export queue. It establishes only
     that; a later export failure is telemetry loss, counted (3.8.4). A
     full queue makes `deliver` return an error.
   - `exported`: `deliver` resolves after the exporter reports success for
     the batch holding the tree, with receipt `otel:exported:<decision
     digest>`; an export failure is an error.
   A sampled-out decision is acknowledged with `otel:sampled_out:<decision
   digest>` after its metrics are updated.
3. **Spec 003 policies apply unchanged.** Used as the runtime's sink,
   `fail_decision` makes a telemetry failure (or, in `exported` mode, a
   collector outage) an `evidence_not_delivered` decision, which is the host's
   explicit choice; `backpressure` and `drop_counted` queue and drop as spec
   003 3.9.4 states, and the runtime counts its drops. `TeeSink { primary,
   telemetry, require_telemetry }` lets a host keep its evidence store as the
   sink of record: the receipt is the primary's; telemetry is delivered after
   the primary answers, within its own timeout; when `require_telemetry` is
   false (the default) a telemetry failure never changes the decision and is
   counted as `tee_failed`, and when true it fails the delivery like the
   primary's own failure.
4. **Loss is counted, never silent.** In-memory counters (spec 003 3.9.7:
   they reset with the process), exposed by `OtelSink::stats()` and exported
   as `rustev.telemetry.lost{reason}`, cover every way accepted telemetry
   can fail to arrive: `queue_full`, `export_failed`, `export_timeout`,
   `exporter_panicked`, `shutdown_unflushed`, `tee_failed`, `file_full`,
   `projection_unknown`. Separately counted, not loss: `sampled_out`,
   `invalid_traceparent`, `context_expired`, `plan_unregistered`,
   `exchange_unjoined`, `attributes_cut`, `events_cut`. Conservation holds
   for spans, metric exports and log records alike: accepted equals
   exported plus lost plus in flight, checked in tests with exporters that
   succeed, fail, hang and panic. The crate exports through its own bounded
   queue and a counting exporter wrapper, so no SDK processor can drop an
   item without a count.
5. **Bounds.** Queue capacity (`max_queued_spans`, default 65 536), batch
   size (default 512), export timeout (default 10 000 ms) and per-attribute
   value size (4 096 bytes, cut at a character boundary and counted) are
   configuration with stated ranges; a configuration outside its range is
   refused at construction.
6. **Equivalence.** The sink reads the record and never writes it; the run
   record, judgment and receipt semantics of spec 003 are unchanged. A test
   runs both reference decisions with the file sink alone and with a
   `TeeSink` over the same file sink and the OTel sink, and compares run
   record and judgment bytes.

### 3.9 Export and endpoint allowlist

1. **Exporters.** OTLP over gRPC (feature `otlp-grpc`) or HTTP protobuf
   (feature `otlp-http`) to host-configured collector endpoints, for traces,
   metrics and logs; and the JSONL exporter (3.10). There is no default
   endpoint: Rustev never exports to any destination the host did not
   configure, and so never to a third-party service by default.
2. **Allowlist.** Configuration carries `allowed_endpoints`, a list of
   origins (`scheme://host:port`, port explicit after normalization: scheme
   and host lowercased, default ports made explicit, trailing dot removed).
   Construction refuses, before any connection or DNS lookup, an endpoint
   whose origin is not exactly in the list; an empty list (which refuses
   every OTLP endpoint); a URL with user information; `http` (or plaintext
   gRPC) to a host that is not a loopback IP literal (`127.0.0.0/8`, `::1`; the
   name `localhost` is refused for plaintext, since its resolution is not
   checked); and wildcards. Endpoint, headers and TLS settings are passed
   explicitly to the exporter builders; `OTEL_EXPORTER_OTLP_*` and other
   `OTEL_*` environment variables cannot change the destination, and the
   effective endpoint is re-checked against the list after the exporter is
   built. HTTP redirects are not followed; a redirect response is an export
   failure, counted. Header values (such as a collector token) are host
   secrets: never logged, exported or included in errors.
3. **Refusal test.** A configuration naming an endpoint outside the list,
   or setting the environment variables to one, yields
   `EndpointNotAllowed` naming the origin, and a loopback listener at that
   address records zero accepted connections.
4. **Sampling.** `parent: honor | ignore` (default `honor`: a registered
   valid parent's sampled flag decides) and `root: always | never |
   ratio{per_million}` (default `always`), the ratio decided from the trace
   id as OpenTelemetry's trace-id-ratio sampler does, so it is consistent
   across services. Option `always_sample_on`, a subset of `unresolved`
   (outcome unresolved or any unresolved step), `refusal` (outcome
   `escalate`, and B3's refusal outcomes when they land), `fallback` (any
   runtime fallback transition; backstop and backfill when they land) and
   `cancelled`, overrides a negative decision. Because the sink sees the
   finished record, this needs no tail sampler; such a trace may have an
   unsampled parent, and its spans carry the sampled flag.

### 3.10 The JSONL development exporter and `rustev.trace/1`

1. **Line document.** `rustev.trace/1`, one compact JSON object per span per
   line, keys sorted, at most 64 KiB per line (`TRACE_V1`): `schema`,
   `schema_version`, `trace_id` (32 hex), `span_id` (16 hex),
   `parent_span_id` (16 hex or null), `name`, `kind`, `start_unix_nanos`,
   `end_unix_nanos`, `status`, `attributes`, `events`, and on the root line
   `resource`. Each attribute value is `{"v": <string, integer, number,
   boolean or array of one of those>, "r": "allow" | "digest" |
   "truncated"}`, so a reader knows how the value was produced; a dropped
   field is absent and listed in `rustev.redaction.withheld`.
2. **Exporter.** `JsonlExporter` appends lines to a host-given path, opened
   append-only and created with mode `0600` on Unix if absent, writes one
   decision's lines in one write, and stops at `max_bytes` (default
   256 MiB), counting `file_full` thereafter. It applies the same redaction
   policy (3.4) and per-attribute value bound (3.8.5) as OTLP export;
   there is no unredacted development mode.

### 3.11 `rustev trace tail`

1. **Amendment of spec 006.** A new command, `rustev trace tail`, with
   flags `--file <path>` (required), `--trace-id <32 hex>` (optional; else
   the last complete root in the file), `--redaction <policy file>`
   (optional; else the default policy), `--follow` (switch: wait for a
   matching complete root to be appended), `--timeout-ms` (1 to 3 600 000,
   default 30 000, only with `--follow`) and `--text` (switch).
2. **Reading.** The file is read with spec 006 3.3.1's regular-file and
   non-blocking rules, at most 256 MiB in total (3.3.2), each line under
   `TRACE_V1`. With `--follow` the command polls for appended bytes at a
   fixed 200 ms interval until a matching root and all its spans are
   present or the timeout passes. A malformed line or unknown schema is
   `invalid_input` naming the line number; no match is `invalid_input` with
   reason `trace_not_found`.
3. **Redaction in the CLI.** The CLI holds no key. For each attribute it
   applies its own policy (from `--redaction` or the default): `drop`
   withholds it; `allow` shows it as written; `digest` shows it only if the
   line marks it `digest`, else withholds it; `truncate{bytes}` cuts a value
   marked `allow` or `truncated` and shows a `digest` as is. An attribute
   key outside the catalogue is withheld with a notice. If a line's
   `rustev.redaction.policy_id` differs from the CLI's policy id, the output
   says `policy_mismatch: true`. The CLI therefore never shows more than
   either policy allows.
4. **Output.** By spec 006 3.1.2, one JSON object with `command: "trace
   tail"`, `status: "rendered"` (exit 0), the `trace_id`, `policy_mismatch`,
   and `spans` as an ordered tree of names, timings, statuses and shown
   attributes. With `--text`, and only then, stdout is instead an indented
   human tree, one span per line; this is the one exception to 006 3.1.2
   this amendment makes. Exit codes are those of 006 3.2 (`io_error` 3,
   `invalid_input` 4); no new code is added.
5. **Dependencies.** The command uses `rustev-contract` only; `rustev-cli`
   gains no dependency, and in particular no OpenTelemetry crate (3.12).

### 3.12 Dependency boundary

1. **Amendment of spec 001 3.5.** A new forbidden family, telemetry
   exporters, matched by package name: every package whose name starts with
   `opentelemetry` (including `opentelemetry_sdk`, `opentelemetry-otlp`,
   `opentelemetry-proto`, `opentelemetry-http`, `opentelemetry-stdout`,
   `opentelemetry-semantic-conventions` and the appenders) and
   `tracing-opentelemetry`.
2. **Rules.** No crate outside `integrations/` may depend on the family,
   directly or transitively through its normal or build dependencies (a new
   violation, `TelemetryOutsideIntegrations`, naming the path); the existing
   rule 3.4.1 for `rustev-contract` and `rustev-core` is unchanged. Each new
   rule has a unit test with a violating synthetic workspace (negative
   control), as spec 001 3.5 requires. `tonic`, `hyper`, `http` and `h2`
   are already HTTP families, so `rustev-otel`'s transports are also only
   legal under `integrations/`.
3. `make boundaries` runs the extended check, and `make code` includes it.

### 3.13 Binding principles

How this spec keeps each principle #76 lists as binding:

1. **Proposals, never grants.** The sink exports a description of a
   proposal or unresolved outcome. No type, receipt, span status, sampling
   decision or attribute is an authorization, and none is named
   `Permitted` or `Grant`; a receipt establishes only enqueueing or export.
2. **Derivation classes and lineage.** Every step span exports
   `rustev.step.derivation`; the root exports `rustev.derivation` and the
   lineage lists. Redaction removes values, never derivation or lineage.
3. **Unresolved is an answer; no silent kind conversion.** Unresolved,
   escalated and cancelled outcomes are exported with their reason and an
   unset status, not as errors or defaults; values are exported in their
   recorded kind and never converted (a score stays a score); absent token
   usage is absent, not 0; an unknown served model is `unknown`.
4. **Pure core.** `rustev-core` is not changed. The contract additions do
   no I/O, read no clock, add no `unsafe` and no dependency other than
   `hmac` (3.4.5).
5. **No async runtime in contract or core.** The contract additions are
   synchronous; Tokio and the OpenTelemetry SDK stay in `rustev-otel`.
6. **New dependency classes only under `integrations/`.** 3.12 adds the
   telemetry family to the boundary check.
7. **No dynamic plugin loading.** Exporters are compiled-in features chosen
   at build time and configured explicitly.
8. **No identity, authority or knowledge store.** The sink stores no
   decision beyond its bounded in-memory queues and join tables; the digest
   key and collector credentials are host-owned; retention of exported
   telemetry is the host's collector's.

### 3.14 Overhead benchmark

1. `cargo run -p rustev-otel --release --example overhead` measures, on the
   two reference plans with the deterministic rules backend, 10 000
   decisions per configuration at concurrency 4 under `backpressure`, after
   1 000 warm-up decisions:
   (a) an in-memory acknowledging sink (baseline); (b) `OtelSink` with a
   no-op exporter (projection, redaction and metrics only); (c) the JSONL
   exporter to a temporary file; (d) OTLP gRPC and (e) OTLP HTTP to a
   loopback fake collector.
2. It reports, as one JSON document, per configuration: decision latency
   p50, p95 and p99; `deliver` latency p50 and p99; CPU time per decision;
   exported bytes per decision; peak resident memory; and the loss counters,
   with the machine, OS, toolchain, commit and the exact load.
3. **No fixed pass threshold.** Absolute timings depend on hardware and
   shared CI runners are noisy, so a threshold would either flap or be too
   loose to mean anything (principle XI: measured, not asserted). The
   acceptance requires the benchmark to run, to report zero loss in
   configurations (a) to (e) at this load, and its result to be recorded in
   the implementation record; a smoke run with 100 decisions is part of the
   test suite so the benchmark cannot rot.

## 4. Out of scope

In-flight spans for decisions not yet delivered, and any runtime hook or
observer seam; outbound trace-context propagation to remote backends;
exporting to a destination the host did not allowlist; collector
deployment, retention or access control; vendor-specific exporters; the
Node binding's wiring of `begin` (#76 A1); spans and attributes of features
not yet specified (3.5.6), which their own specs add; a durable loss ledger;
emitting two span schema versions in one trace.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A canary value in a `drop` field of any record or exchange slot | Absent from every exported span, event, log, metric attribute, resource and JSONL line, raw and digested. |
| A policy grants `allow` to class `user-supplied` | Policy refused, naming the entry. |
| A policy names a field not in the catalogue, or a field twice | Policy refused. |
| A policy overrides `rustev.redaction.policy_id` | Policy refused (fixed field). |
| A `digest` action with no key configured | Construction refused. |
| Same value, same key, two fields of class `id` | Identical digests; a different key gives a different digest. |
| Endpoint origin not in the allowlist, or set only through `OTEL_EXPORTER_OTLP_ENDPOINT` | `EndpointNotAllowed`; zero connections to that origin. |
| Plain HTTP to a non-loopback host, or a URL with user information | Construction refused. |
| Collector answers with a redirect | Export failure, counted; redirect not followed. |
| Export queue full under `accepted` | `deliver` errors; the runtime's policy decides; `queue_full` counted. |
| Exporter fails, hangs or panics | Loss counted under its reason; conservation holds. |
| Malformed `traceparent` | New trace; `invalid_traceparent` counted; decision unaffected. |
| Exchange record arrives after its decision was exported | `exchange_unjoined` counted; GenAI fields `unknown` or absent. |
| Record for an unregistered plan | Exported with step kind `unknown`; `plan_unregistered` counted. |
| Exact step or cache hit | Zero-length span with `rustev.timing = not_recorded`. |
| Unresolved outcome | Root status unset; reason exported; always sampled when configured. |
| A crate under `crates/` gains `opentelemetry`, even transitively | `make boundaries` fails naming the path. |
| `rustev trace tail` on a line marked `allow` for a field the CLI policy drops | Field withheld. |
| `rustev trace tail` without key on a raw value its policy says `digest` | Withheld. |
| JSONL line over 64 KiB or wrong schema | `invalid_input` naming the line. |
| Telemetry attached versus not | Identical run record and judgment bytes. |

## Acceptance

- Golden span trees for both reference plans and for the scripted remote
  exchange (3.5.8) are reproduced byte for byte; the all-`allow` goldens
  show every source field of the attribute table that the record holds.
  Goldens with B1, B5 and C2 features are added by those specs.
- Redaction tests: a property test generates bounded arbitrary run records,
  exchange records and valid policies, plants unique canaries in every value
  slot, exports through spans, events, logs, metrics and JSONL to in-memory
  exporters, and asserts no canary of a field whose effective action is
  `drop` appears in any exported byte (I-1), and no raw canary of a
  `digest` field appears either; every default-deny field is absent under
  the default policy; digests are stable for a fixed key (RFC 4231 HMAC
  vectors plus committed Rustev vectors) and change with the key.
- Policy validation tests for every refusal of 3.4.2.
- Endpoint allowlist refusal test (3.9.3), including the environment
  variable case and zero observed connections.
- Sink policy tests: `accepted` and `exported` modes under `fail_decision`,
  `backpressure` and `drop_counted`; `TeeSink` with and without
  `require_telemetry`; conservation of accepted, exported, lost and in-flight
  items with succeeding, failing, hanging and panicking exporters; the
  equivalence test of 3.8.6.
- Trace-context tests: valid parent, malformed parent, full table, expiry,
  abandon, and the returned root span id equal to the exported root's.
- Sampling tests for every mode and each `always_sample_on` trigger;
  metrics recorded for sampled-out decisions.
- `rustev trace tail` end-to-end tests through the binary for every row of
  3.11 and 5 that concerns it; `cargo tree -p rustev-cli` shows no
  `opentelemetry*` package.
- Boundary check unit tests with a violating fixture for the telemetry
  family, directly and transitively; the real workspace passes.
- Existing plan and definition goldens and `PlanId`s are unchanged.
- The overhead benchmark runs and its result is recorded (3.14).
- The pinned facts of 3.2.1 are re-confirmed and recorded.
- `make code`, declared verification, and `make gate` pass.

## Verification

Pending with implementation. The implementing change replaces this
paragraph with a `verify:cli` block covering at least
`cargo test -p rustev-otel --locked --all-features`,
`cargo test -p rustev-contract --locked`, `cargo test -p rustev-cli
--locked`, `cargo test -p rustev-boundaries --locked`, `cargo run -p
rustev-boundaries --locked --quiet`, a `cargo tree` check that no crate
outside `integrations/` reaches an `opentelemetry*` package, and the
benchmark smoke run; and adds `028` to `VERIFIED_SPECS`. Approval creates no
crate and no runnable acceptance.

## Implementation record

Delivered in increments; nothing here claims an acceptance item before the
increment that satisfies it. Plan:

1. The telemetry exporter family in the boundary check (3.12), and the
   re-confirmation of the pinned facts (3.2.1).
2. The pure contract parts: `rustev.redaction/1`, the field catalogue,
   policy evaluation, the keyed digest and `rustev.trace/1` (3.3, 3.4,
   3.10.1).
3. `integrations/rustev-otel`: the projection to span trees, the JSONL
   exporter and the committed goldens (3.5, 3.10.2).
4. The sink: acknowledgement modes, the export queue and loss counters,
   `TeeSink`, trace context, plan and exchange registration, sampling,
   metrics and logs (3.6 to 3.8, 3.9.4).
5. `rustev trace tail` (3.11).
6. The OTLP exporters behind features and the endpoint allowlist (3.2.3,
   3.9.1 to 3.9.3).
7. The redaction property test, the overhead benchmark, the `verify:cli`
   block and `028` in `VERIFIED_SPECS` (3.14, Acceptance).

**Increment 1 (dependency boundary and pinned facts).**
`tools/rustev-boundaries` gains the telemetry exporter family: every
package whose name starts with `opentelemetry`, and `tracing-opentelemetry`
(3.12.1). A crate outside `integrations/` that declares one as a normal or
build dependency (optional ones included, since the resolved graph leaves
them out), or reaches one through its resolved normal and build graph, is a
`TelemetryOutsideIntegrations` violation naming the path (3.12.2); rule
3.4.1 for the pure crates is unchanged. Unit tests cover each named family
member as a normal and a build dependency from `crates/`, `backends/`,
`packages/` and `tools/`, an optional declaration, a transitive path through
a build edge, the contract and core reaching the family, a dev-only
dependency passing, and `rustev-otel` with its OTLP transports passing under
`integrations/`; `make boundaries` passes on the workspace.

The facts of 3.2.1 were re-confirmed on 2026-10-21 and hold unchanged:
`open-telemetry/semantic-conventions-genai` has published no release (so
no schema URL), core v1.42.0's notes deprecate every `gen_ai.*` convention
and move them there, the core repository's latest release is still v1.44.0,
`https://opentelemetry.io/schemas/1.41.1` serves a schema file whose
`schema_url` is itself, and `opentelemetry`, `opentelemetry_sdk`,
`opentelemetry-otlp` and `opentelemetry-semantic-conventions` are at
0.33.1 on crates.io. The pin of 3.2.2 (v1.41.1) therefore stands.

**Increment 2 (the pure contract parts).** `rustev-contract` gains three
modules and one dependency, `hmac` 0.12 (the line built on the `digest`
0.10 that `sha2` 0.10 uses; 0.13 moves to `digest` 0.11):

- `redaction`: the field classes of 3.3.1 with their defaults (3.4.3), the
  lineage rule (3.3.2), the `rustev.redaction/1` document under
  `DESCRIPTOR_V1` with `deny_unknown_fields` and its identity
  `RedactionPolicyId`, validation naming the entry for every refusal of
  3.4.2, the effective action (3.4.4), the keyed digest `DigestKey` (32 to
  64 bytes, `key_id` 1 to 64 bytes, never serialized, `Debug` shows only the
  `key_id`), and `Redactor`, which refuses a missing key when any action is
  `digest` and is the one function that turns a value into an exported one
  (3.4.7). An action is written `"drop"`, `"digest"`, `"allow"` or
  `{"truncate":{"bytes":N}}`. An array of ids is digested element by
  element, so each still correlates.
- `trace_fields`: the closed catalogue, the span and event names, the fixed
  fields, the pinned schema URL and span schema version. Event attributes
  are catalogued as `<event>.<key>` (`rustev.input.source`) and metric-only
  attributes as `<instrument>.<key>` (`rustev.unresolved.scope`), so every
  exported value has one catalogue name for the policy and for
  `rustev.redaction.withheld`. Attributes that arrive with later specs
  (3.5.4 "arrives with") are added by those specs, not reserved here.
- `trace`: the `rustev.trace/1` line under the new limit set `TRACE_V1`
  (64 KiB per line), its invariants and its writer (record canonical
  bytes: compact, keys sorted). The resource on the root line holds the
  resource attributes, the schema URL and the instrumentation scope.
- The reader rule of 3.11.3 (`RedactionPolicy::view`). A reader cannot
  know the class a lineage-rule field (an outcome parameter or a ranking
  score) had at export, so without a `fields` override it applies the most
  withholding action among every class the rule can assign; it never shows
  more than the policy would for the value's actual class.

Tests: every refusal of 3.4.2, the default policy, the effective action,
RFC 4231 vectors (cases 1 to 4, 6, 7), committed Rustev digest vectors
computed independently, equal digests across two `id` fields and a
different digest under another key, key bounds, the reader's view rules,
and the line's round trip and refusals. The three source files join the
reviewed archive list (hence 028's `extends` edge on
`tools/rustev-release/`, as 027 and 033 took). No existing document,
schema string, limit, canonical byte or identity changes. The two new
documents are not added to spec 027's JSON Schema emission: doing so
regenerates the committed binding declarations under
`integrations/rustev-wasm/` and `integrations/rustev-node/`, which this
spec does not extend.

**Increment 3 (projection, JSONL exporter, goldens).**
`integrations/rustev-otel` exists (`publish = false`, listed as excluded in
`release-set.json`; default features empty, no OpenTelemetry crate yet):
`project` turns one delivered `rustev.run/1` record, the registered plan,
the joined exchange records and the host's bindings into the span lines of
3.5, every value through `Redactor` and the 4 096-byte value bound;
`JsonlExporter` appends one decision per write to a file opened
append-only with mode `0600`, stopping at `max_bytes` (`file_full`); ids
come from a seeded or a randomly seeded source. Readings taken within the
spec:

- The projection knows `rustev.run/1` with `rustev.evidence/1` and
  `rustev.judgment/1` (the documents of the specs 028 depends on). A
  record of a later version, a judgment outcome of a later spec, a
  corroboration step, or a step missing from its registered plan is
  `projection_unknown`, never a guessed span; the core's sink seam accepts
  only `rustev.run/1` by default as well.
- A request with no step evidence entry (a decision whose evidence was not
  produced) still gets its step span after the evidence entries, so its
  attempts are not lost; it carries only request attributes.
- An outcome parameter keeps its kind: booleans, integers and strings as
  such, a decimal as its exact string, a timestamp as its milliseconds, a
  composite value (list, record, filtered, shortlist, ranking) as its
  record canonical JSON. A count beyond `i64` is its exact decimal string.
  A lineage input with no recorded provenance counts as `attributed-claim`
  for the lineage rule.
- Root events are timed at the root's start, transitions at their step's
  start, policy events at the root's end. The policy span is always
  emitted. A line over 64 KiB loses events (halving, counted in
  `events_cut`) and then long strings (cut to 256 bytes, counted in
  `attributes_cut`).

Goldens (R-2) under `integrations/rustev-otel/tests/golden/`:
`support.trace.jsonl`, `lodging.trace.jsonl` and
`support-remote.trace.jsonl` under the default policy, and an `.allow.`
golden of each under an all-`allow` test policy (every field allowed by
name, as 3.4.2 requires), in which no span withholds anything and every
GenAI row of 3.5.4 appears. They read spec 006's committed run records and
the committed plans that produced them (`tests/fixtures/`, each checked
against the record's `PlanId`); the remote golden joins three synthetic
`rustev.remote-exchange/1` records (pinned, reported and unknown
disclosure, declared usage keys, provider extras, and a failed member's
detail on an attempt the record shows as an output, which only exercises
that row). Tests also cover timing, unset root status for an unresolved
outcome, error status for a failed attempt, unregistered plans, unknown
records, the event and value bounds, canaries in default-deny and digest
slots, and the JSONL exporter's mode, append and bound.

**Increment 4 (the sink).** `OtelSink` implements `EvidenceSink`: it
projects and redacts synchronously, updates the metrics (never sampled),
samples, and queues the whole tree as one unit in a queue bounded in spans;
a worker exports batches of whole trees through the host's
`TelemetryExporter`, each call its own task under the export timeout, so a
failure, a hang (the task is aborted) and a panic are counted as
`export_failed` (or `file_full`), `export_timeout` and `exporter_panicked`.
Receipts are `otel:accepted:`, `otel:exported:` and `otel:sampled_out:`
over the record digest. `begin`, `abandon`, `register_plan`,
`record_exchange`, `observe_runtime`, `metrics`, `flush_metrics`, `stats`
and `shutdown` are as 3.6 to 3.9.4 state, and `TeeSink` keeps the primary's
receipt. The JSONL exporter is a `TelemetryExporter`. Readings taken within
the spec:

- Conservation is kept per signal in items (`spans`, `logs`,
  `metric_exports`: accepted equals exported plus lost plus in flight).
  `rustev.telemetry.lost{reason}` counts decisions, each under exactly one
  reason (I-2); `TeeSink` counts `tee_failed` only for a telemetry failure
  no other reason counts, so a decision is never counted twice. After
  `shutdown`, a delivery is lost as `shutdown_unflushed`.
- Metric attributes are added only when the policy's action for their
  catalogue field is `allow`; anything else omits the dimension, so no
  digest or truncated value becomes one. `rustev.decision.duration` carries
  no attribute (the three 3.6.1 lists are attempt fields);
  `gen_ai.client.operation.duration` is in seconds, as the GenAI
  conventions state, for attempts with reported usage. Metrics are an
  in-memory snapshot (`metrics()`), exported by `flush_metrics()` when the
  exporter takes metrics; the runtime's gauges are read through the host's
  callback at that moment.
- Logs need an exporter that takes log records; the JSONL exporter takes
  none, so `logs: on` with it is refused at construction.
- Only span schema version 1 exists, so `schema` accepts `1` alone; the
  dual-emit window of 3.2.4 has nothing to select until a version 2 is
  specified.
- Ranges: `max_queued_spans` 1 to 1 048 576, `batch_size` 1 to 8 192,
  `export_timeout_ms` 1 to 600 000, the value bound 16 to 4 096 bytes,
  `max_pending_contexts` and the join buffer 1 to 65 536,
  `context_ttl_ms` 1 to 86 400 000, the ratio 0 to 1 000 000 per million.
  Construction needs a Tokio runtime for the worker. The join buffer
  remembers at least 4 096 exported attempt ids, so a late exchange record
  is `exchange_unjoined` at once rather than at its expiry.
- The trace-id ratio is OpenTelemetry's rule in integer form: the last
  eight bytes of the trace id, shifted right by one, below
  `per_million * 2^63 / 10^6`.

Tests (`tests/sink.rs`): both acknowledgement modes through the runtime
under `fail_decision`, `backpressure` and `drop_counted`, including a
collector outage; conservation with succeeding, failing, hanging and
panicking exporters; a full queue; shutdown; `TeeSink` with and without
`require_telemetry`; the equivalence of 3.8.6 (both reference decisions
with a file sink alone and in a tee with the sink: identical record bytes
up to the runtime's measured times, identical judgment bytes); valid,
malformed, full, expired and abandoned trace contexts with the returned
root span id equal to the exported root's; the join buffer; every sampling
mode and `always_sample_on` trigger with metrics recorded for sampled-out
decisions; metric dimensions; logs; and every configuration refusal.

**Increment 5 (`rustev trace tail`).** `crates/rustev-cli` gains the
command with the flags of 3.11.1, using `rustev-contract` alone (no new
dependency). The argument grammar gains switches (`--follow`, `--text`:
no value, at most once); `--timeout-ms` without `--follow`, a value outside
1 to 3 600 000, and a `--trace-id` that is not 32 lowercase hex digits are
usage errors. Readings taken within the spec:

- A decision is complete in the file when its root line and its policy
  span (which the exporter writes last, in the same write) are both there;
  its spans are found by parent links, so several decisions under one host
  trace id are told apart, and the last complete root wins. A trailing
  line without its newline waits for the next poll; a partial line already
  over 64 KiB is `invalid_input`.
- `--follow` re-reads only what was appended, within the same 256 MiB
  budget, and counts its timeout in 200 ms polls, so it reads no clock.
- Output: `spans` holds the root node; each node has its name, span id,
  kind, status, start and end, the attributes shown (each as written,
  `{"v", "r"}`, or cut further), its events, its children,
  `withheld_by_reader` (catalogue names only) and `not_in_catalogue` (a
  count: a key outside the catalogue is never echoed). The root also shows
  its resource. `policy_mismatch` compares the root line's
  `rustev.redaction.policy_id` with the reader's. `--text` prints one span
  per line, indented by depth, with its status, duration and shown
  attributes.

Tests (`crates/rustev-cli/tests/trace.rs`, through the binary, over
`rustev-otel`'s committed goldens): the last complete root and
`--trace-id`; fields the reader drops and raw values it would digest
withheld, with `policy_mismatch`; a truncating reader policy; a key outside
the catalogue; malformed, foreign-schema and oversized lines naming their
number; `trace_not_found`; I/O errors; `--follow` across an append and to
its timeout; `--text`; every usage rule; an invalid policy file.
`src/trace.rs` joins the CLI's reviewed archive list.

## Decisions at approval

Each open point of #70 and of the work order, with the answer this spec
takes. Points the issue or work order already decides are marked settled;
the rest are decided by the implementer within this spec, recorded here.

1. **Where the pure part of `rustev.redaction/1` lives:** `rustev-contract`
   (documents, catalogue, evaluation, keyed digest, trace line), with the
   projection, exporters and sink in `rustev-otel` (3.1). Decided by the
   implementer within this spec, for the CLI and outside-reader reasons of
   3.1.2.
2. **Where `rustev trace tail` lives:** in `rustev-cli`, reading
   `rustev.trace/1` through the contract, with no OpenTelemetry dependency;
   not a binary or feature of `rustev-otel`, because the issue names the
   `rustev` command and the CLI may not depend on an integration. Decided
   by the implementer within this spec; amends 006 (3.11).
3. **"Runtime instrumentation":** derived after the fact from the delivered
   run record, the runtime's gauges and forwarded exchange records; no
   runtime hook, so spec 003 is not amended and judgments cannot change.
   In-flight spans are out of scope. Decided by the implementer within this
   spec.
4. **Trace-context ingestion and root span id:** a bounded `begin` table in
   the sink, keyed by decision id (3.7.1); the A1 binding calls it. Decided
   by the implementer within this spec.
5. **Semantic-conventions pin:** v1.41.1 with its schema URL, re-confirmed
   at implementation (3.2); `gen_ai.system` is emitted as its pinned
   successor `gen_ai.provider.name`. Pinning and the dual-version window are
   settled by the issue; the version and the successor mapping are decided
   by the implementer within this spec.
6. **Dual-emit window:** either version selectable per sink for the
   introducing release and the next one, never both in one trace (3.2.4).
   Settled by the issue ("can emit either version, selected by
   configuration"); the window length is decided by the implementer within
   this spec.
7. **Span names:** the issue's names (`rustev.decision`, `rustev.step`,
   `rustev.attempt`, `rustev.policy`, reserved `rustev.acquire`,
   `rustev.widen.tier`) are kept, with GenAI attributes on `rustev.attempt`
   rather than GenAI's recommended `{operation} {model}` span name, so span
   names never carry values. Settled by the issue.
8. **Class of numbers and other values:** the lineage rule of 3.3.2, so a
   number copied from a user-supplied input is dropped by default and must
   be allowed by name. Decided by the implementer within this spec, as the
   reading of "drop for user-supplied ... allow for numbers" that never
   exposes a caller value by default.
9. **Acknowledgement:** `accepted` by default, `exported` by choice; a
   `TeeSink` for hosts whose sink of record is elsewhere (3.8). The sink
   policies and "loss counted, never silent" are settled by the issue; the
   modes are decided by the implementer within this spec.
10. **Sampling defaults:** parent-based, root always, `always_sample_on`
    empty unless configured (3.9.4). Settled by the issue in form; defaults
    decided by the implementer within this spec.
11. **Benchmark threshold:** none, for the reasons in 3.14.3. Decided by the
    implementer within this spec, as the work order allows.
12. **Exchange records:** forwarded by the host to `record_exchange`
    because one integration may not depend on another (3.7.3). Decided by
    the implementer within this spec.
13. **Logs:** off by default; fixed-template bodies only (3.6.3). Decided
    by the implementer within this spec.
14. **Dependency family:** `opentelemetry*` and `tracing-opentelemetry`,
    transitive for every crate outside `integrations/` (3.12). Settled by
    #76's binding principle; the matching rule is decided by the implementer
    within this spec.

No point is left open, and no approved spec is contradicted: spec 001 3.5
says adding a family is a change to that spec, which this spec makes by its
`amends` edge; spec 006 gains a command and one stated stdout exception.

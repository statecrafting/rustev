<!-- spec-spine:begin generated id="header" -->
# `028-opentelemetry-evidence-sink` OpenTelemetry evidence sink and redaction policy

- Path: `specs/028-opentelemetry-evidence-sink/spec.md`
- Summary: Approved (A-24) as its own reviewable change before code; implemented in seven increments. A new optional integration, `integrations/rustev-otel`, exports each delivered run record as OpenTelemetry spans, metrics and optional log records, with GenAI semantic-convention attributes on backend attempts, W3C trace-context parenting from the host, OTLP over gRPC or HTTP to allowlisted host collectors only, and a local JSONL exporter. Every exported field passes a declared redaction policy, `rustev.redaction/1`, that defaults to deny for caller, third-party, attributed-claim and generated text. The policy document, its evaluation, the keyed digest and the `rustev.trace/1` span line document are pure additions to `rustev-contract`; `rustev trace tail` is added to the CLI with no OpenTelemetry dependency; the boundary check gains the telemetry exporter family. Approval does not authorize a release, a deployment, an export to any collector outside tests, or real-user data.
- Reference page schema: `1.0.0`
<!-- spec-spine:end generated id="header" -->

<!-- spec-spine:begin generated id="lifecycle" -->
## Lifecycle

| Field | Value |
| --- | --- |
| status | `approved` |
| implementation | `complete` |
| superseded_by | None declared. |
| retirement_rationale | None declared. |
| created | `2026-10-16` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `028-opentelemetry-evidence-sink#I-1` | `invariant` | No value of a field whose effective redaction action is drop appears, raw or transformed, in any exported span attribute, span name, event, link, log record, metric attribute, resource attribute or JSONL line. | `3-4-redaction-policy` | no |
| `028-opentelemetry-evidence-sink#I-2` | `invariant` | Attaching the sink changes no judgment and no run record byte; telemetry that is accepted and does not reach a successful export is counted under exactly one loss reason, never silently discarded. | `3-8-sink-policies-acknowledgement-and-loss` | no |
| `028-opentelemetry-evidence-sink#I-3` | `invariant` | The sink connects only to a collector endpoint that exactly matches the host's allowlist; any other configured, defaulted or environment-supplied destination is refused at construction with no connection attempt. | `3-9-export-and-endpoint-allowlist` | no |
| `028-opentelemetry-evidence-sink#I-4` | `invariant` | OpenTelemetry SDK, API and exporter crates are dependencies only of crates under integrations/; rustev-contract, rustev-core, rustev-runtime, rustev-eval and rustev-cli reach none of them, and make boundaries refuses a violation. | `3-12-dependency-boundary` | no |
| `028-opentelemetry-evidence-sink#I-5` | `invariant` | Telemetry is a description of a proposal, never a grant: no exported value, receipt, span status or sampling decision is or implies authorization, and unresolved outcomes are exported as answers, never as defaults or success. | `3-13-binding-principles` | no |
| `028-opentelemetry-evidence-sink#R-1` | `requirement` | The sink pins a recorded GenAI semantic-conventions version and a Rustev span schema version, records both on the resource and scope, and changes either only through a new span schema version with a configuration-selected window in which the previous version can still be emitted. | `3-2-pinned-conventions-and-schema-versions` | no |
| `028-opentelemetry-evidence-sink#R-2` | `requirement` | Golden span trees for both reference plans, and for a scripted remote exchange, are committed as rustev.trace/1 JSONL and reproduced byte for byte under a fixed key, anchor and id generator. | `3-5-span-model` | no |
| `028-opentelemetry-evidence-sink#R-3` | `requirement` | rustev trace tail renders one decision's spans from a rustev.trace/1 JSONL file under a redaction policy no wider than the one it is given, with bounded file access and no OpenTelemetry dependency. | `3-11-rustev-trace-tail` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"integrations/rustev-otel/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
- `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}`
<!-- spec-spine:end generated id="ownership" -->

<!-- spec-spine:begin generated id="signatures" -->
## Signatures

None declared.
<!-- spec-spine:end generated id="signatures" -->

<!-- spec-spine:begin generated id="relations" -->
## Relations

Not supplied.
<!-- spec-spine:end generated id="relations" -->

<!-- spec-spine:begin generated id="operations" -->
## Operations

Not supplied.
<!-- spec-spine:end generated id="operations" -->

<!-- spec-spine:begin generated id="evidence" -->
## Evidence

### Declared inputs

None declared.

### Acceptance commands (declared, not executed)

```console
cargo test -p rustev-contract --locked
cargo test -p rustev-otel --locked --all-features
cargo clippy -p rustev-otel --all-targets --all-features --locked -- -D warnings
cargo test -p rustev-cli --locked
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 'for d in crates/* backends/* tools/* packages/*; do n=$(basename "$d"); out=$(cargo tree -p "$n" -e normal,build --all-features --prefix none --locked) || exit 1; if printf "%s\n" "$out" | grep -qE "^(opentelemetry|tracing-opentelemetry)"; then echo "$n reaches the telemetry family"; exit 1; fi; done'
sh -c 'if grep -rnE "(struct|enum|trait|type)[[:space:]]+(Permitted|Grant)([^A-Za-z0-9_]|$)" integrations/rustev-otel crates/rustev-contract/src/redaction.rs crates/rustev-contract/src/trace.rs; then exit 1; fi'
```
<!-- spec-spine:end generated id="evidence" -->

<!-- spec-spine:begin generated id="limitations" -->
## Limitations

None declared.

This page restates declared and structurally resolved records. It does not state that code is correct, that a test ran or passed, or that anything was accepted, released, adopted, or observed.
<!-- spec-spine:end generated id="limitations" -->

<!-- spec-spine:begin generated id="changes" -->
## Changes

| Edge | Specs |
| --- | --- |
| amends | `001-boundaries-and-authority`, `006-cli-surface` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `049-opentelemetry-through-the-node-binding` |
| superseded by | None declared. |
| extended by | `031-public-api-and-release-family`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `050-wasm-without-parameterized-definitions` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:028-opentelemetry-evidence-sink` | `sha256:a8f33918e4e5e336f84b01b02329878244aa2b3015d2eafe3fc09ec969dd51f2` |
| `registry:028-opentelemetry-evidence-sink` | `sha256:a8f33918e4e5e336f84b01b02329878244aa2b3015d2eafe3fc09ec969dd51f2` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

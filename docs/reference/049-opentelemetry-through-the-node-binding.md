<!-- spec-spine:begin generated id="header" -->
# `049-opentelemetry-through-the-node-binding` OpenTelemetry export through the Node binding (amends 001, 010, 027 and 028)

- Path: `specs/049-opentelemetry-through-the-node-binding/spec.md`
- Summary: A narrow amendment of approved specs 027 (Node-API binding) and 028 (OpenTelemetry evidence sink), with the boundary exception it needs in specs 001 and 010. A TypeScript host gains an optional `telemetry` option on `Engine.create`: the binding attaches spec 028's `OtelSink` behind the host's own sink of record through 028's `TeeSink`, exports to a JSONL file or over OTLP/HTTP to an allowlisted collector, wires 028's `begin` to each decision's `traceparent` and returns the trace and root span ids, registers compiled plans, forwards exchange records the host passes, reports the sink's counters in the engine's gauges and flushes on shutdown. Every value passes 028's redaction policy; the receipt, run record and judgment bytes are unchanged. `rustev-node` becomes the one integration crate, besides the facade, that may depend on another integration crate, and only on `rustev-otel`. Ratified by the owner on 2026-10-23 (A-51).
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
| created | `2026-10-23` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `049-opentelemetry-through-the-node-binding#I-1` | `invariant` | Telemetry attached through the binding changes no receipt, run record or judgment byte: it is only the telemetry half of a TeeSink behind the host's sink of record, and a telemetry failure changes a decision only when the host set requireTelemetry. | `3-3-composition-behind-the-host-sink` | no |
| `049-opentelemetry-through-the-node-binding#I-2` | `invariant` | The binding exports telemetry only when the host configures it, only to the JSONL path or the allowlisted OTLP/HTTP collector the host names, and every exported value passes spec 028's redaction policy; the digest key and collector header values never cross back to JavaScript in a result, error, gauge or debug output. | `3-2-the-telemetry-option` | no |
| `049-opentelemetry-through-the-node-binding#I-3` | `invariant` | Apart from the facade's exception of spec 031 3.3, the only dependency of a crate on an integration crate is rustev-node's normal dependency on rustev-otel; make boundaries refuses every other edge, including rustev-node on any other integration and rustev-wasm on rustev-otel. | `3-1-the-boundary-exception` | no |
| `049-opentelemetry-through-the-node-binding#R-1` | `requirement` | With telemetry configured, every decide call registers the decision with the sink before admission and returns its trace and root span ids, and a decision that delivers no record leaves no registered context behind. | `3-4-trace-context` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
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
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 'out=$(cargo tree -p rustev-node -e normal --prefix none --locked) || exit 1; printf "%s\n" "$out" | grep -q "^opentelemetry-otlp " || exit 1; if printf "%s\n" "$out" | grep -q "^tonic "; then echo "rustev-node reaches gRPC"; exit 1; fi'
cargo clippy -p rustev-node --all-targets --locked -- -D warnings
cargo test -p rustev-otel --locked --all-features
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
| amends | `001-boundaries-and-authority`, `010-integrations`, `027-node-and-wasm-bindings`, `028-opentelemetry-evidence-sink` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `059-shared-remote-transport-by-dependency` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:049-opentelemetry-through-the-node-binding` | `sha256:af2e558f3389895fdee799f8450ff471ea8fa9170c5d5f7339f41675334d3cad` |
| `registry:049-opentelemetry-through-the-node-binding` | `sha256:af2e558f3389895fdee799f8450ff471ea8fa9170c5d5f7339f41675334d3cad` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

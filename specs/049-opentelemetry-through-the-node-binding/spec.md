---
id: "049-opentelemetry-through-the-node-binding"
title: "OpenTelemetry export through the Node binding (amends 001, 010, 027 and 028)"
status: approved
implementation: complete
created: "2026-10-23"
summary: >
  A narrow amendment of approved specs 027 (Node-API binding) and 028
  (OpenTelemetry evidence sink), with the boundary exception it needs in
  specs 001 and 010. A TypeScript host gains an optional `telemetry`
  option on `Engine.create`: the binding attaches spec 028's `OtelSink`
  behind the host's own sink of record through 028's `TeeSink`, exports to
  a JSONL file or over OTLP/HTTP to an allowlisted collector, wires 028's
  `begin` to each decision's `traceparent` and returns the trace and root
  span ids, registers compiled plans, forwards exchange records the host
  passes, reports the sink's counters in the engine's gauges and flushes on
  shutdown. Every value passes 028's redaction policy; the receipt, run
  record and judgment bytes are unchanged. `rustev-node` becomes the one
  integration crate, besides the facade, that may depend on another
  integration crate, and only on `rustev-otel`. Ratified by the owner on
  2026-10-23 (A-51).
amends:
  - "001-boundaries-and-authority"
  - "010-integrations"
  - "027-node-and-wasm-bindings"
  - "028-opentelemetry-evidence-sink"
extends:
  # The telemetry option, decide's trace fields and the gauges entry (3.2
  # to 3.6), with its tests and declarations.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  # The one allowed integration-to-integration edge (3.1).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: amending }
  # rustev-node's dependency on rustev-otel.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 049 to `make verify` and its CI acceptance group at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "scripts/acceptance/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "010-integrations"
  - "027-node-and-wasm-bindings"
  - "028-opentelemetry-evidence-sink"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Telemetry attached through the binding changes no receipt, run record or judgment byte: it is only the telemetry half of a TeeSink behind the host's sink of record, and a telemetry failure changes a decision only when the host set requireTelemetry."
    anchor: "3-3-composition-behind-the-host-sink"
  - id: "I-2"
    kind: invariant
    text: "The binding exports telemetry only when the host configures it, only to the JSONL path or the allowlisted OTLP/HTTP collector the host names, and every exported value passes spec 028's redaction policy; the digest key and collector header values never cross back to JavaScript in a result, error, gauge or debug output."
    anchor: "3-2-the-telemetry-option"
  - id: "I-3"
    kind: invariant
    text: "Apart from the facade's exception of spec 031 3.3, the only dependency of a crate on an integration crate is rustev-node's normal dependency on rustev-otel; make boundaries refuses every other edge, including rustev-node on any other integration and rustev-wasm on rustev-otel."
    anchor: "3-1-the-boundary-exception"
  - id: "R-1"
    kind: requirement
    text: "With telemetry configured, every decide call registers the decision with the sink before admission and returns its trace and root span ids, and a decision that delivers no record leaves no registered context behind."
    anchor: "3-4-trace-context"
---

# 049: OpenTelemetry export through the Node binding (amends 001, 010, 027 and 028)

Approved: ratified by the owner on 2026-10-23 (decision A-51), as part of
the release path. The approved texts of specs 001, 010, 027 and 028 are not
edited; this spec records what changes in them. Approval does not authorize
a release or publication, an export to any collector outside tests on
loopback, or real-user data.

## 1. Purpose

Spec 028 delivers each run record as redacted OpenTelemetry spans, metrics
and logs, but only to a Rust host: its section 4 leaves "the Node binding's
wiring of `begin` (#76 A1)" out of scope, and its decision 4 says of trace
context "a bounded `begin` table in the sink, keyed by decision id (3.7.1);
the A1 binding calls it". Spec 027 3.8 keeps a decision's `traceparent`
for host seam calls only and says "#76 child D1 decides how spans join it".
A TypeScript service using `@statecrafting/rustev` therefore cannot export
telemetry today except by re-implementing the projection and redaction in
JavaScript, which is exactly the ad hoc export spec 028 exists to prevent.

The obstacle is a boundary rule, not a missing component. Spec 001 3.4.4
reads "No crate depends on a crate under `integrations/`", and spec 031
3.3 makes one exception, for the facade, adding that "integration crates
... still may not depend on one another". `rustev-node` and `rustev-otel`
are both integration crates, so 027 3.3.2 already notes that integrations
are "not linked into the binding: spec 001's boundary rule forbids any
crate depending on an integration crate ... or through a later spec that
changes that rule." This is that later spec, for `rustev-otel` alone.

## 2. Territory

- **Spec 001 3.4.4 and spec 010 I-2, amended (3.1):** one more exception,
  the edge `rustev-node` to `rustev-otel`. `tools/rustev-boundaries/`
  enforces it; `Cargo.toml` and `Cargo.lock` gain the dependency.
- **Spec 027, amended (3.2 to 3.7):** `integrations/rustev-node/` gains the
  `telemetry` option, decide's `tracestate` option and `trace` result field,
  the gauges' `telemetry` entry, `recordExchange` and
  `flushTelemetryMetrics`, with their declarations and tests.
- **Spec 028, amended (3.8):** the Node binding's wiring of `begin` leaves
  028's out-of-scope list; `rustev-otel` gains its first dependent. No
  source of `rustev-otel` changes under this spec, and nothing in 028's
  projection, redaction, catalogue, span model, sink, exporters or
  allowlist changes.

What does not change: the WebAssembly package (no runtime, no sink, no
exporter: 027 3.14.3 keeps its graph free of `rustev-runtime`, executors and
HTTP stacks, and so of `rustev-otel`); the CLI; every document, schema,
generated type, `PlanId`, conformance case and golden; spec 027's error
classes (only new `code` values of existing classes, 3.6); the
`traceparent` that host seam calls receive (027 3.8).

## 3. Behavior

### 3.1 The boundary exception

1. Spec 001 3.4.4 and the last clause of spec 010 I-2, as amended by spec
   031 3.3, are amended again to read, in effect: no crate depends on a
   crate under `integrations/`, except the facade as 031 3.3 states, and
   `rustev-node`, which may depend on `rustev-otel` and on no other
   integration crate, only as a normal (not build or dev) dependency.
2. Why it is safe. Both crates are under `integrations/`, so every family
   `rustev-otel` reaches (the telemetry exporter family of 028 3.12, HTTP
   stacks) is still reached only from `integrations/`, and 028 I-4 and
   #76's principle hold literally. No crate may depend on `rustev-node`
   (it is a `cdylib`, and 3.4.4 still covers it), so the edge adds no path
   from any other crate. `rustev-otel` depends on no integration, so the
   edge creates no chain.
3. `tools/rustev-boundaries` enforces it: the edge passes; `rustev-node`
   depending on any other integration crate, or on `rustev-otel` as a
   build or dev dependency, is the existing `DependsOnIntegration`
   violation; any other crate depending on `rustev-otel` still fails; and
   `rustev-wasm` reaching `rustev-otel` fails both that rule and 027
   3.14.3. Each case has a synthetic-metadata fixture in the existing
   style, and `make boundaries` passes on the workspace.
4. `rustev-node` enables `rustev-otel`'s `otlp-http` feature and not
   `otlp-grpc` (decision 2). It enables no feature of the OpenTelemetry
   crates itself and declares no telemetry family directly.

### 3.2 The telemetry option

`Engine.create` gains one optional field (027 3.3: "option bags may gain
optional fields by later specs"):

```ts
const engine = await Engine.create({
  runtime, sink, backends, contextSource, clock,
  telemetry?: {
    exporter:
      | { kind: 'jsonl'; path: string; maxBytes?: number }
      | { kind: 'otlp-http'; endpoint: string; allowedEndpoints: string[];
          headers?: Record<string, string>; caPem?: Uint8Array; timeoutMs?: number };
    policy?: DocumentInput;                 // rustev.redaction/1; 028's default policy when absent
    key?: { bytes: Uint8Array; keyId: string };
    acknowledgement?: 'accepted' | 'exported';
    requireTelemetry?: boolean;             // false when absent
    telemetryTimeoutMs?: number;
    sampling?: {
      parent?: 'honor' | 'ignore';
      root?: 'always' | 'never' | { ratioPerMillion: number };
      alwaysSampleOn?: ('unresolved' | 'refusal' | 'fallback' | 'cancelled')[];
    };
    logs?: boolean;
    bindings?: {
      providers?: Record<string, string>;   // backend id to gen_ai.provider.name
      usageKeys?: Record<string, { input: string; output: string }>;
    };
    limits?: {
      maxQueuedSpans?: number; batchSize?: number; exportTimeoutMs?: number;
      valueBound?: number; maxPendingContexts?: number; contextTtlMs?: number;
      joinBuffer?: number;
    };
  },
});
```

1. **One-to-one with 028.** Each field maps to the 028 setting of the same
   meaning, with 028's default when absent and 028's stated range:
   `acknowledgement` to 3.8.2's modes; `sampling` to 3.9.4; `logs` to
   3.6.3; `bindings` to 3.7.2; `limits` to 3.8.5, 3.7.1 and 3.7.3; `policy`
   and `key` to 3.4 (the key 32 to 64 bytes, `keyId` 1 to 64 bytes);
   `exporter` to 3.9 (OTLP/HTTP: endpoint, `allowedEndpoints`, headers, CA
   certificates and per-request timeout) and 3.10.2 (JSONL: the host's path
   and `max_bytes`, default 268,435,456). `telemetryTimeoutMs` is the
   `TeeSink`'s telemetry timeout (3.3), 1 to 600,000, default 10,000. The
   span schema is 028's version 1; there is no option to choose it until a
   version 2 exists (028 3.2.4).
2. **No default exporter.** Without `telemetry`, the binding builds no
   `OtelSink` and exports nothing; with it, `exporter` is required.
   Nothing is read from `OTEL_*` environment variables except to refuse,
   as 028 3.9.2 states (its OTLP exporter refuses an environment-supplied
   endpoint outside the list).
3. **Bounded parse.** `policy` crosses as any `DocumentInput` (027 3.2.1)
   and is parsed by the contract's bounded `rustev.redaction/1` parser and
   validated by 028 3.4.2; a refusal is `InvalidDocument`
   (`invalid_document`) naming the entry, as for any document.
4. **Secrets stay in Rust.** The key bytes and header values are copied
   into the sink and the exporter once; no result, error `detail`, gauge
   or `Debug` output contains them (028 3.4.5 and 3.9.2 already keep them
   out of the sink's own errors). An error names a header by its name
   only and an endpoint by its origin only.
5. **Refusals.** Every refusal happens in `Engine.create`, before any
   thread, file or connection is opened beyond what the refused item
   needed, and is a `ConfigRefused` (027 3.4.1) with a `code` and a
   `detail` naming the field (3.6).
6. **WebAssembly.** The WebAssembly package has no `telemetry` option and
   gains nothing.

### 3.3 Composition behind the host sink

1. **Tee, not replacement.** Spec 027 3.3 makes the host's `sink` required,
   with "no default sink". With `telemetry`, the binding passes the runtime
   028's `TeeSink { primary: <the host sink>, telemetry: <the OtelSink>,
   require_telemetry, telemetry_timeout_ms }`. The receipt is the
   primary's ("the receipt is the primary's; telemetry is delivered after
   the primary answers, within its own timeout", 028 3.8.3), so `delivery`
   in the decide result, the run record and the judgment are exactly what
   they are without telemetry (I-1).
2. **Failure.** With `requireTelemetry` false (the default) a telemetry
   failure never changes the decision and is counted as `tee_failed` by
   the sink; with it true, it fails the delivery like the primary's own
   failure, and the decision fails with 027's `SinkFailure` carrying the
   record, as for any delivery failure.
3. **One timeout bounds both.** The binding's runtime runs the
   `fail_decision` sink policy with `sinkTimeoutMs` (027), which bounds the
   whole tee delivery. `telemetryTimeoutMs` must be smaller than
   `sinkTimeoutMs`, else `ConfigRefused` (`out_of_range`, field
   `telemetry.telemetryTimeoutMs`); a host choosing `'exported'` sets
   `sinkTimeoutMs` above its primary's latency plus `telemetryTimeoutMs`.
4. **Runtime.** The sink's export worker runs on the engine's own Tokio
   runtime (028 3.8 needs one at construction), and the sink observes that
   runtime's gauges (028 3.6.1's `observe_runtime`).
5. **Clock.** 027 3.3.4 says the binding reads no clock except through the
   engine's configured `Clock`. Spans need a wall-clock anchor, which the
   runtime's monotonic `Clock` does not give. With `clock: 'system'`, the
   sink uses 028's system clocks, read only for telemetry timestamps and
   table expiry, never for a decision input or a record; this is the one
   exception to 027 3.3.4 this spec makes. With `clock: 'manual'`, the
   sink's anchor and table time come from the manual clock (its
   milliseconds since creation, as Unix nanoseconds from 0), so tests are
   deterministic.

### 3.4 Trace context

1. **Begin.** With telemetry, `plan.decide` calls 028's
   `begin(decisionId, traceparent, tracestate)` once per call, after 027's
   own checks (shutdown, an aborted signal, `traceparent` validity, the
   context source) and immediately before admission. 027 3.8 still refuses
   an invalid `traceparent` with `InvalidTraceContext` first, so the sink
   only receives valid parents and its `invalid_traceparent` count stays
   at zero through the binding.
2. **`tracestate`.** `DecideOptions` gains `tracestate?: string`, carried
   unchanged to `begin` (028 3.7.1: "`tracestate` is carried unchanged when
   the parent is valid"). It is refused with `InvalidTraceContext`, code
   `invalid_tracestate`, before admission when given without
   `traceparent`, longer than 512 bytes, or containing a byte outside
   printable ASCII (0x20 to 0x7e). Without telemetry it is validated the
   same way and then unused. It never reaches a host seam call, a record
   or any identity.
3. **Result.** `DecideResult` gains `trace?: { traceId: string; rootSpanId:
   string }` (32 and 16 lowercase hex digits), 028's `Begun`, present when
   `begin` registered the decision. Without a `traceparent` the trace is
   new, with the sink's random ids. When the table is full (`TableFull`,
   counted by the sink) the field is absent and the decision is exported as
   a new trace, as 028 3.7.1 states; the decision runs either way.
4. **Abandon.** When a decide call ends without the tee delivering
   telemetry (a rejection, cancellation before admission, a deadline in
   the queue, a primary delivery failure, a panic), the binding calls
   `abandon(decisionId)`, so no context waits for its time to live (R-1).
   The binding calls it after every decide call that called `begin`; after
   a delivery it is a no-op.
5. **Host seam calls unchanged.** Host seams still receive the host's
   `traceparent` (027 3.8), not the root span: the root is exported only
   after delivery, and changing what seams receive is outside this spec.

### 3.5 Plans, exchanges, metrics and shutdown

1. **Plans.** Each plan the engine compiles or loads is registered with
   the sink (028 3.7.2's `register_plan`) before its `PlanHandle` is
   returned, so a decision on it never exports as `plan_unregistered`.
2. **Exchange records.** `engine.recordExchange(record: DocumentInput)`
   parses a `rustev.remote-exchange/1` document with its bounded parser and
   forwards it to 028's `record_exchange` (028 3.7.3: "the host forwards it
   from its own `ExchangeSink`"). A parse failure is `InvalidDocument`;
   without telemetry the call is `ConfigRefused`, code
   `telemetry_not_configured`; after shutdown, `EngineShutDown`.
3. **Metrics.** `engine.flushTelemetryMetrics()` resolves with
   `{ outcome: 'exported' | 'not_taken' | 'lost', reason?: string }`:
   `not_taken` when the exporter takes no metrics (the JSONL exporter), and
   `lost` with 028's loss reason when the export failed, which the sink
   has counted. It never rejects for an export failure; without telemetry
   it is `ConfigRefused` (`telemetry_not_configured`).
4. **Gauges.** `engine.gauges()` and `onGauges` (027 3.9.3) gain a
   `telemetry` entry with 028's counters (3.8.4: the span, log and metric
   export flows, decisions lost by reason, and the counted-not-lost
   counts), absent without telemetry. Gauges stay operator views, not
   evidence.
5. **Shutdown.** `engine.shutdown({ drainMs })` drains decisions as 027
   3.9.2 states, then shuts the sink down with what remains of `drainMs`
   (at least 0); telemetry still queued is lost as `shutdown_unflushed`,
   counted. The resolved value gains `telemetryUnflushed`, that count's
   increase during this shutdown (0 without telemetry).

### 3.6 Errors

Spec 027 3.4.1's classes are unchanged; these `code` values are added
(snake case, never changed once released), each with a `detail` naming the
field and never carrying a key, header value or path content:

| Cause | Class | `code` |
|---|---|---|
| A telemetry setting outside its range (3.2.1, 3.3.3), an unknown `exporter.kind` or sampling value | `ConfigRefused` | `out_of_range` |
| An effective `digest` action with no `key` (028 3.4.5) | `ConfigRefused` | `digest_key_required` |
| `logs: true` with an exporter that takes no log records | `ConfigRefused` | `logs_unsupported` |
| An OTLP endpoint refused by 028 3.9.2 (outside the list, empty list, user information, plaintext to a non-loopback host, wildcard, environment) | `ConfigRefused` | `endpoint_not_allowed` |
| A header name invalid or given twice | `ConfigRefused` | `invalid_header` |
| CA certificates that cannot be read, or another exporter build failure | `ConfigRefused` | `exporter_unavailable` |
| The JSONL file cannot be opened or created | `ConfigRefused` | `telemetry_file` |
| `recordExchange` or `flushTelemetryMetrics` without telemetry | `ConfigRefused` | `telemetry_not_configured` |
| An invalid `tracestate` (3.4.2) | `InvalidTraceContext` | `invalid_tracestate` |
| An invalid `policy` document | `InvalidDocument` | `invalid_document` |

### 3.7 Declarations

The telemetry option, `tracestate`, `trace`, the gauges entry, the new
methods and the shutdown count are declared by hand in the package's
`index.d.ts`, as 027 3.2.3's hand-written API declarations are; `policy`
may be typed with the generated `rustev.redaction/1` type. No generated
declaration changes: `rustev.redaction/1` and `rustev.trace/1` are already
emitted (027's R-46 record).

### 3.8 What changes in spec 028

1. 028 section 4's "the Node binding's wiring of `begin` (#76 A1)" is no
   longer out of scope: section 3.4 here is that wiring.
2. 028 section 2's "No workspace crate depends on it (spec 001 3.4.4)"
   reads: no workspace crate depends on it except `rustev-node` (this
   spec, 3.1) and the facade, if spec 031 adds an `otel` feature.
3. Nothing else in 028 changes. Through the binding, its invariants hold as
   stated: I-1 (redaction) and I-3 (allowlist) because the binding uses
   028's sink and exporters unchanged; I-2 (no record byte changes; loss
   counted) by 3.3; I-4 because 3.1 keeps every telemetry family under
   `integrations/`; I-5 because the binding adds no value that is or
   implies an authorization.

### 3.9 Binding principles

- **Proposals, never grants.** `trace` ids, receipts and counters describe
  a decision; none is or implies an authorization, and no type is named
  `Permitted` or `Grant`.
- **Unresolved is an answer.** Telemetry changes no judgment; an
  unresolved outcome is exported as 028 3.13.3 states.
- **Only integrations reach exporter families.** 3.1.
- **No dynamic loading.** The exporter is compiled in and chosen by the
  host's explicit option; nothing is discovered by name or path.
- **No identity, authority or knowledge store.** The key and collector
  credentials are the host's; retention is the collector's or the file's
  owner's.

## 4. Out of scope

OTLP over gRPC through the binding (decision 2); telemetry in the
WebAssembly package; a host-implemented `TelemetryExporter` in TypeScript;
passing the root span to host seam calls; using `OtelSink` as the sole sink
without a host sink of record; selecting a span schema version; any change
to `rustev-otel`'s source or to a document, schema or golden; publication.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `Engine.create` with no `telemetry` | No sink built, nothing exported, no `telemetry` gauge, decide results without `trace`. |
| `exporter.kind: 'otlp-grpc'` or any unknown kind | `ConfigRefused` (`out_of_range`, field `telemetry.exporter.kind`). |
| OTLP endpoint outside `allowedEndpoints`, or only in `OTEL_EXPORTER_OTLP_ENDPOINT` | `ConfigRefused` (`endpoint_not_allowed`) naming the origin; zero connections at it. |
| Default policy (or any `digest` action) without `key` | `ConfigRefused` (`digest_key_required`). |
| A policy granting `allow` to class `user-supplied` | `InvalidDocument` naming the entry. |
| `logs: true` with the JSONL exporter | `ConfigRefused` (`logs_unsupported`). |
| `telemetryTimeoutMs` not below `sinkTimeoutMs` | `ConfigRefused` (`out_of_range`). |
| A header given twice, differing in case | `ConfigRefused` (`invalid_header`) naming it; no value in the error. |
| `tracestate` without `traceparent`, over 512 bytes, or non-printable | `InvalidTraceContext` (`invalid_tracestate`); no admission. |
| A canary in a snapshot field under the default policy | Absent from the JSONL file and from the bytes the collector receives, raw and digested. |
| The collector fails or hangs, `requireTelemetry` false | Decision, receipt and record unchanged; loss counted in `gauges().telemetry`. |
| The collector fails, `requireTelemetry` true | `SinkFailure` carrying the record. |
| A decision rejected for overload after `begin` | Context abandoned; no `context_expired` count later. |
| Telemetry attached versus not, both reference decisions, manual clock | Identical run record, judgment and receipt bytes. |
| `rustev-node` depending on `rustev-remote-http`, or another crate on `rustev-otel` | `make boundaries` fails naming the edge. |

## Acceptance

- `make boundaries` passes on the workspace, and `cargo test -p
  rustev-boundaries` covers the allowed edge and each refused edge of 3.1.3
  with synthetic-metadata fixtures.
- `node --test integrations/rustev-node/test/*.test.mjs` (run by `make
  bindings`) covers, through the built addon: both reference decisions
  with the JSONL exporter under the manual clock and a fixed key, whose
  lines are `rustev.trace/1` lines (schema, span schema version 1, keys
  sorted, each under 64 KiB), carry the `traceparent`'s trace id and the
  returned `trace` ids, and have the span names, tree shape and attribute
  keys of 028's committed default-policy golden for that plan; the
  equivalence row of section 5; OTLP/HTTP
  export of spans, a log record and a metrics flush to a loopback fake
  collector, with the schema URL present and no canary on the wire; every
  other row of section 5 that concerns the binding; `tracestate`; abandon
  after a rejection; `flushTelemetryMetrics` outcomes; the gauges entry;
  and the shutdown count.
- `make bindings` still passes its drift checks: no generated declaration
  or schema changes, and the hand-written declarations type-check.
- Every existing conformance case, CLI golden and `PlanId` is unchanged.
- `make code`, the declared verification and `make gate` pass.

## Verification

Declared for `spec-spine verify 049`; 049 is in `make verify` and the
`runtime-evidence` acceptance group, beside 027 and 028. The binding's
behavior (sections 3.2 to 3.7 and the rows of section 5) is tested through
the built addon by `node --test integrations/rustev-node/test/*.test.mjs`,
which `make bindings` and the CI `bindings` job run on every native
platform (027 3.13.1); the native addon cannot be loaded by a Rust test.

```verify:cli
# 3.1: the one allowed edge and each refused one (synthetic fixtures), and
# the real workspace.
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
# 3.1.4: the binding enables OTLP/HTTP and not gRPC.
sh -c 'out=$(cargo tree -p rustev-node -e normal --prefix none --locked) || exit 1; printf "%s\n" "$out" | grep -q "^opentelemetry-otlp " || exit 1; if printf "%s\n" "$out" | grep -q "^tonic "; then echo "rustev-node reaches gRPC"; exit 1; fi'
# 3.2 to 3.7: the binding builds and lints with the telemetry option.
cargo clippy -p rustev-node --all-targets --locked -- -D warnings
# 3.8.3: rustev-otel's own acceptance is unchanged.
cargo test -p rustev-otel --locked --all-features
```

## Implementation record

Delivered in two increments: (1) the boundary exception, the `telemetry`
option with the JSONL exporter, trace context, plans, exchanges, gauges,
shutdown and declarations; (2) the OTLP/HTTP exporter, the `verify:cli`
block and `049` in `VERIFIED_SPECS`.

**Increment 1.** `tools/rustev-boundaries` names the one edge
(`NODE_TELEMETRY_EDGE`): `rustev-node` on `rustev-otel` as a normal
dependency passes; as a build or dev dependency, `rustev-node` on any other
integration, and any other crate on `rustev-otel` stay
`DependsOnIntegration`, and `rustev-otel` joins the WebAssembly graph's
forbidden list, so `rustev-wasm` reaching it fails both rules (3.1.3, one
fixture per case). `rustev-node` depends on `rustev-otel` by path with no
feature (the root manifest is left to the change that adds `rustev-otel`
to the release family). `Engine.create({ telemetry })` validates every
field before opening the file or starting a thread, opens the JSONL file,
builds the `OtelSink` on the engine's Tokio runtime and gives the runtime a
`TeeSink` over the host sink (3.2, 3.3); under the manual clock the sink's
clock is the manual clock's milliseconds anchored at Unix zero (3.3.5).
`decide` validates `tracestate`, calls `begin` immediately before admission,
returns `trace`, and abandons through a guard on every exit (3.4); compiled
and loaded plans are registered (3.5.1); `recordExchange`,
`flushTelemetryMetrics`, the `telemetry` gauges entry and
`telemetryUnflushed` are as 3.5 states. In this increment
`exporter.kind: 'otlp-http'` is refused as an unknown kind, and
`flushTelemetryMetrics` answers `not_taken` or `lost`, since the JSONL
exporter takes no metrics. Tests (`test/telemetry.test.mjs`): both
reference decisions with the JSONL exporter (trace lines, trace ids,
span names, shape and attribute keys against 028's default-policy goldens,
a snapshot canary absent, and identical evidence, judgment and receipt
bytes with and without telemetry), a full file with and without
`requireTelemetry`, the creation refusals with no secret in any error,
`tracestate`, abandon after a rejection, exchanges, metrics and shutdown.

**Increment 2.** `rustev-node` enables `rustev-otel`'s `otlp-http` feature
alone (3.1.4). `exporter: { kind: 'otlp-http', ... }` builds 028's
`OtlpExporter` with `Protocol::HttpProtobuf` on the engine's runtime; the
allowlist (including the `OTEL_*` endpoint variables) and the header names
and values are checked before any thread starts, so a refused destination
is never dialed (3.2.5). `timeoutMs` is the exporter's per-request timeout:
028 states no range for it, so the binding takes 1 to 600,000 ms with a
default of 10,000, the range and default of 028's `export_timeout_ms`.
Header values and CA certificates cross from JavaScript separately from
the option JSON, like the key, and no error carries them: an endpoint
refusal names the normalized origin only, a header refusal the name only
(3.6). `logs: true` is accepted with OTLP; `flushTelemetryMetrics` answers
`exported` when the exporter takes metrics and the flush succeeded. Tests
(`test/telemetry-otlp.test.mjs`, a loopback collector in plain `node:http`):
both reference decisions exported, with requests on `/v1/traces` (holding
the pinned schema URL), `/v1/logs` (an unresolved decision) and
`/v1/metrics`, no snapshot canary or header value in any body, and
identical evidence, judgment and receipt bytes with and without telemetry;
an endpoint outside the list (configured or through
`OTEL_EXPORTER_OTLP_ENDPOINT`, in a child process), `localhost` in
plaintext, user information and an empty list refused with zero
connections; duplicate and invalid headers, a bad CA and out-of-range
options refused with no secret in the error; a collector answering 500 or
a 307 redirect counted as `export_failed` with the decision unchanged, or
a `SinkFailure` carrying the record under `requireTelemetry`, the redirect
target never dialed. Decision 2 names `h2` as a cost of gRPC, but `h2`
already reaches the binding through `hyper`'s default features under
OTLP/HTTP, so what gRPC would add is `tonic` and its dependencies; the
verification checks for `tonic` alone. `049` joins `VERIFIED_SPECS` and the
acceptance group, and the implementation is complete.

**Release audit fix, 2026-10-26.** The first release rehearsal (spec 053,
run 40279690969) found that `rustev-node`'s `rustev-otel` dependency named
a path without a version, which the release audit refuses as a wildcard
(021 3.6). It now takes the workspace dependency (`workspace = true`,
pinned `=0.1.0`) with the same `otlp-http` feature; `Cargo.lock` and the
binding's behavior are unchanged. The two duplicate majors 3.1.4's
exporter brings in are reviewed in 021's record.

## Decisions at approval

1. **Ratification.** The owner approved this amendment on 2026-10-23 as
   part of the release path (A-51). Settled by the owner.
2. **OTLP/HTTP only, JSONL included.** The native package ships one
   binary per platform (027 3.13.1), so every exporter compiled in costs
   every host. OTLP/HTTP protobuf is accepted by the OpenTelemetry
   Collector and by the common hosted backends, and with JSONL covers
   production and development; gRPC adds `tonic` and `h2` to every
   platform binary for a transport the HTTP one already serves. A later
   amendment can add it behind the same `exporter.kind`. Decided by the
   implementer within this spec.
3. **One option on `Engine.create`, not a separate sink object.** 028's
   sink needs a Tokio runtime at construction and the engine owns the
   runtime, and 027 requires a host sink of record; an option composed as
   a `TeeSink` meets both with no new class and no way to attach the sink
   twice. Decided by the implementer within this spec.
4. **`begin` on every decision, not only with a `traceparent`.** The host
   gets ids to link its logs either way, and 028 3.7.1 gives an unparented
   decision a new trace in both cases. Decided by the implementer within
   this spec.
5. **Validating `tracestate` in the binding.** 028 carries it unchanged and
   does not bound it; the binding refuses what W3C Trace Context forbids
   (over 512 bytes, non-printable) rather than exporting it, with the same
   class 027 uses for an invalid `traceparent`. Decided by the implementer
   within this spec.
6. **The system clock for telemetry timestamps (3.3.5).** Spans without a
   wall-clock anchor are not usable in a collector, the runtime's `Clock`
   has none, and the read affects no decision; the manual clock keeps tests
   deterministic. Decided by the implementer within this spec.
7. **The boundary exception names one edge (3.1).** Narrower than lifting
   "integration crates may not depend on one another", which would also
   admit remote backends into the binding, a different decision with its
   own trade-offs (027 3.3.2). Decided by the implementer within this spec.

No point is left open, and no approved spec is contradicted beyond the
amendments this spec states: 001 3.4.4 and 010 I-2 (3.1), 027 3.3, 3.3.4,
3.4.1, 3.8 and 3.9 (sections 3.2 to 3.6), and 028 sections 2 and 4 (3.8).

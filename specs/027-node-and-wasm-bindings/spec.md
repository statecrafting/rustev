---
id: "027-node-and-wasm-bindings"
title: "Node-API binding and WebAssembly build"
status: approved
implementation: complete
created: "2026-10-16"
summary: >
  Approved (A-23). Two host surfaces over the existing engine: `rustev-node`,
  a napi-rs Node-API binding through which a TypeScript service compiles
  plans, runs decisions, replays and evaluates in-process and implements
  Rustev's seams in TypeScript as explicit composition (R-38); and
  `rustev-wasm`, a wasm-bindgen build of the pure crates for browsers, edge
  runtimes and tooling. Documents cross as the contract's canonical JSON,
  TypeScript types are generated from JSON Schema emitted by
  `rustev-contract`, command rendering moves into one pure crate shared by
  every surface, and every surface (CLI, native binding, WebAssembly, later
  HTTP and gRPC) runs one shared conformance suite. Approval does not
  authorize npm or crates.io publication, name reservation, a release, a
  live provider call, spend, or a claim that cross-target floating point
  agrees beyond what the parity suite measures.
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: additive }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
  # A read-only accessor on the manual clock (its pending delays), so a
  # conformance runner drives it deterministically (3.12.4).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: additive }
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: additive }
  # Each new source file of a release member joins the reviewed archive
  # list, under spec 021 3.5's rule, in the directory spec 031 owns.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
establishes:
  - { kind: directory, path: "integrations/rustev-node/" }
  - { kind: directory, path: "integrations/rustev-wasm/" }
  - { kind: directory, path: "crates/rustev-surface/" }
  - { kind: directory, path: "tools/rustev-conformance/" }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "021-packaging-and-first-release"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No binding output is authorization: a binding returns proposals, unresolved outcomes, refusals and evidence only, defines no permission type, and passes the principal handle through as opaque bytes."
    anchor: "3-15-binding-principles"
  - id: "I-2"
    kind: invariant
    text: "Rust never waits synchronously on JavaScript: every host-implemented seam call is asynchronous through a bounded queue with a typed refusal when full, carries its remaining deadline and cancellation, discards and counts a late resolution, and maps a JavaScript exception or rejection to the seam's typed failure without a panic crossing the boundary."
    anchor: "3-6-host-implemented-seams"
  - id: "I-3"
    kind: invariant
    text: "An unresolved outcome, including one caused by an exhausted deadline or budget during a decision, is a value in the result and never an exception; an exception means that no decision ran or that evidence delivery failed, exactly as the Rust API reports it."
    anchor: "3-4-errors-and-unresolved-results"
  - id: "I-4"
    kind: invariant
    text: "Node-API and wasm-bindgen dependency families occur only under integrations; contract, core and the shared surface crate never reach them, an executor or an HTTP stack; and the WebAssembly crate's normal dependency graph contains no runtime, executor, HTTP stack, randomness source or Node-API crate."
    anchor: "3-14-dependency-boundaries"
  - id: "I-5"
    kind: invariant
    text: "Every host surface runs the one shared conformance suite, and a surface's output for a case equals the case's expected bytes under only the comparison the case declares."
    anchor: "3-12-shared-conformance-suite"
  - id: "R-1"
    kind: requirement
    text: "TypeScript document types are generated from JSON Schema emitted by rustev-contract, are identical in both packages, and a drift check fails when committed schema or types differ from regenerated output."
    anchor: "3-2-typed-boundary-and-generated-types"
  - id: "R-2"
    kind: requirement
    text: "The WebAssembly module stays within the declared compressed size and instantiation budgets, measured in CI as section 3.10 defines; exceeding a budget fails the build, and raising one is an amendment of this spec."
    anchor: "3-10-webassembly-build"
  - id: "R-3"
    kind: requirement
    text: "The native package ships prebuilt binaries for the declared platform matrix with a pinned Node-API version and no install-time download, and each target loads and passes a smoke test in CI."
    anchor: "3-13-packaging"
---

# 027: Node-API binding and WebAssembly build

Approved by the owner (A-23, 2026-10-16, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation is
complete (2026-10-21, with the known limitations R-47 records). It implements #76 child A1 (#56) and rests on R-38 (OD-5 of #76).
Approval does not authorize publication to npm or crates.io, reservation of a
package name or scope, a release or tag (spec 021 and its amendment by #76
child A3 govern those, and each remains a separate owner act), a live provider
call, or spend. It makes no claim that floating-point results agree across
targets beyond the inputs the parity suite actually runs (section 3.11).

## 1. Purpose

Today Rustev is reachable only from Rust and through the CLI. A web service
written in TypeScript would have to spawn a process per decision, which is
slow and loses cancellation, streaming evidence and host-implemented seams.
This spec defines two surfaces over the same engine:

- `rustev-node`, a Node-API binding built with napi-rs: compile plans, run
  decisions, replay and evaluate in-process, and implement Rustev's seams in
  TypeScript;
- `rustev-wasm`, a WebAssembly build of the pure crates (contract, core,
  evaluation) for browsers, edge runtimes and tooling that cannot load a
  native module.

It also defines what keeps the surfaces from drifting apart: one typed
boundary, one pure command renderer, and one shared conformance suite that
every surface runs.

The whole A1 contract is defined here. Wave 1 of #76 implements it over
today's surface (section 3.7); seams and commands that later specs add join
the bindings through the same mechanism and the same suite without amending
the guarantees of this spec.

## 2. Territory

New crates, each a workspace member through the existing directory globs:

| Directory | Holds | Publishable to crates.io |
|---|---|---|
| `crates/rustev-surface/` | The pure command surface: bytes-in, bytes-out functions for `check`, `compile`, `show`, offline `replay`, `eval`, `gate`, `calibrate fit` and `calibrate qualify`, and the result envelope of `run`, rendered exactly as the CLI renders them today. Depends on `rustev-contract`, `rustev-core`, `rustev-eval`, `serde`, `serde_json` and `sha2` only. | Not by this spec (spec 021 and A3 decide) |
| `integrations/rustev-node/` | The napi-rs binding (`cdylib`), its TypeScript API, generated and committed type declarations, and the npm package layout. | No (`publish = false`); shipped through npm |
| `integrations/rustev-wasm/` | The wasm-bindgen build, its JavaScript glue, the same committed type declarations, and the npm package layout. | No (`publish = false`); shipped through npm |
| `tools/rustev-conformance/` | The shared conformance suite: case manifest, fixtures, expected bytes, the Rust runner (CLI and Rust API) and the JavaScript runner (native and WebAssembly). `publish = false`. | No |

Existing territory this spec changes:

- `crates/rustev-contract/` (spec 002, additive): a pure, deterministic JSON
  Schema emitter for every public document type, and the committed schemas
  under `crates/rustev-contract/schema/`. No existing document, parse bound
  or canonical byte changes. No new dependency is added unless it is pure,
  performs no I/O, and passes the boundary check.
- `crates/rustev-core/` and `crates/rustev-eval/` (specs 002 and 004,
  additive): only `cfg` attributes needed to compile for `wasm32-unknown-unknown`,
  with no new dependency and no behavior change on native targets. If none
  is needed, these crates do not change.
- `crates/rustev-cli/` (spec 006, amending its internal structure only): the
  CLI calls `rustev-surface` for every rendering listed above instead of its
  own copy. Spec 006's observable contract (grammar, exit codes, output
  bytes, file handling) is unchanged; its existing goldens under
  `crates/rustev-cli/tests/golden/` prove it, unchanged and without
  re-blessing. No normative text of spec 006 changes.
- `Cargo.toml`, `Cargo.lock` (spec 001, additive): workspace dependency
  entries for napi-rs and wasm-bindgen, pinned to exact versions.
- `Makefile` (spec 001, additive): targets `conformance` and `bindings`
  (section 3.12) and the boundary families of section 3.14.
- `tools/rustev-boundaries/` (spec 001, additive): the new integration-only
  dependency families and graph rules of section 3.14.

What does not change: plan compilation, `PlanId`s and every other content
identity; the runtime's behavior and public Rust API; the remote protocol;
run record, judgment, replay and report bytes; the CLI's contract. This spec
adds no plan construct, so every plan keeps a byte-identical `PlanId`; the
conformance suite asserts the `plan_id` of every golden on every surface
(section 3.12), which is the golden that proves it.

## 3. Behavior

### 3.1 Surfaces and crates

1. **One engine.** Both bindings call the same Rust code the CLI calls:
   `rustev-surface` for pure commands, `rustev-runtime` for decisions (native
   only). A binding renders no document of its own and re-serializes no
   canonical document: judgments, run records, plans, bundles and reports
   cross as the canonical bytes the contract produces.
2. **Result shape.** Every command result exposes both the canonical bytes
   (`raw: Uint8Array`, and for each produced artifact such as a plan, bundle
   or report, its bytes) and a parsed view typed by the generated
   declarations. The parsed view is derived from the bytes; a host that
   needs evidence-grade bytes uses `raw`.
3. **CLI-only presentation.** File paths the CLI prints (such as `out`) and
   process exit codes are CLI presentation. A binding returns artifacts as
   bytes instead. Section 3.12 lists exactly which fields a comparison may
   exclude.

### 3.2 Typed boundary and generated types

1. **Canonical JSON in, bounded parse.** Every document a host passes (a
   definition, profile, calibration, descriptor, snapshot, request, bundle,
   dataset, configuration) crosses as UTF-8 canonical JSON bytes or a string,
   and is parsed on the Rust side by the contract's existing bounded parser
   with the document's existing `ParseLimits`. A JavaScript object argument
   is serialized by the binding with `JSON.stringify` and then parsed the
   same way; no JavaScript-side validation substitutes for the Rust parse.
   Bounds are enforced before unbounded allocation on both 64-bit and
   `wasm32` targets.
2. **Schema emission.** `rustev-contract` gains a function that emits, for
   every public document type and version, a JSON Schema (draft 2020-12)
   document derived from the same type definitions the parser enforces,
   including closed objects where the parser denies unknown fields, enums,
   string formats of identifiers, and every bound a schema can express.
   Emission is pure and deterministic: canonical JSON, sorted keys,
   byte-identical across runs and targets. The schemas are committed under
   `crates/rustev-contract/schema/`, and a contract test fails when emitted
   bytes differ from the committed files. The schema is descriptive: the
   bounded parser remains the only authority on validity.
3. **Type generation.** One code generator, exact-pinned with a committed
   lockfile and run offline from the committed schemas, produces the
   document declarations (`rustev-documents.d.ts`). The hand-written API
   declarations (`Engine`, `PlanHandle`, error classes, seam interfaces)
   import them. Both packages commit byte-identical copies of the document
   declarations, so code can switch between the native and WebAssembly
   packages; napi-rs's generated glue declarations are committed too.
4. **Drift check.** `make bindings` and CI regenerate schemas and
   declarations and fail when any committed file differs, or when the two
   packages' document declarations differ.
5. **Kinds stay distinct.** Generated types keep every value kind distinct
   (score, distribution, calibrated probability, label, ordinal level,
   rank) as tagged shapes; the bindings provide no conversion between kinds
   and no coercion of a missing value to a default.

### 3.3 Engine lifecycle and the TypeScript API

The native package exposes this API (names normative; option bags may gain
optional fields by later specs):

```ts
const engine = await Engine.create({
  runtime: RuntimeConfig,          // spec 003 bounds plus binding worker sizing
  sink: EvidenceSink,              // required; no default sink
  backends: BackendRegistration[], // registered set, each with descriptor and max concurrency
  contextSource?: ContextSource,   // section 3.7; used when a request carries no snapshot
  clock?: 'system' | ManualClock,  // 'system' is TokioClock
});
const plan = await engine.compile({ definition, profile?, backends: BackendBinding[], calibrations? });
// plan: PlanHandle { planId, report, raw }
const result = await plan.decide(request, {
  signal?: AbortSignal, deadlineMs?, principalHandle?: Uint8Array,
  traceparent?: string, capture?: CaptureConfig,
});
// result: { judgment, evidence, origin, notices, delivery, raw }
const replay = await engine.replay(bundleBytes, { nowMs, resolve });
const report = await engine.evaluate({ dataset, split, config, candidates, resolve });
const gate = await engine.gate({ reports, gate });
const fit = await engine.calibrateFit({ ... });
const verdict = await engine.calibrateQualify({ ... });
engine.check(definition); engine.show(planBytes);   // pure, may be synchronous
engine.compileSync(...);                             // optional, documented as blocking
engine.gauges(); engine.onGauges(cb, { everyMs });   // section 3.9
await engine.shutdown({ drainMs });
```

1. **Registration and binding.** The runtime registers its backends when the
   engine is created (spec 003 builds the runtime with its backends).
   `compile`'s `backends` binds the definition's backend ids to registered
   backends by id and descriptor. Preparing a plan whose binding names an
   unregistered backend or a different descriptor is refused with spec 003's
   `PrepareError`, mapped as section 3.4 states.
2. **Native backends available today.** The reference rules backend of spec
   005, constructed from its documents as bytes. Remote backends under
   `integrations/` (`rustev-remote-http`, `rustev-jev`) are not linked into
   the binding: spec 001's boundary rule forbids any crate depending on an
   integration crate. A host reaches a remote backend through a
   TypeScript-implemented `DecisionBackend` (section 3.6) or through a later
   spec that changes that rule.
3. **Result fields.** `judgment` is the judgment (or the cancelled
   completion) with its notices; `evidence` is the run record; `delivery` is
   the sink outcome (acknowledged with receipt, queued, or dropped with the
   dropped count); `origin` is the judgment's derivation class and lineage
   as the contract carries them; `notices` are the judgment's notices. No
   field is computed by the binding beyond what the Rust result holds. Per
   field origin (#76 child B5) joins `origin` when its spec lands.
4. **Time.** Domain evaluation time is always an explicit request field.
   `nowMs` for replay and evaluation is always an explicit argument. The
   binding reads no clock except through the engine's configured `Clock`.
5. **`Engine.create` refusals.** A zero bound or duplicate backend id is
   refused with spec 003's `ConfigError`. A binding-level sizing value or a
   host seam's queue capacity (3.6.2) outside its declared range is refused
   by the binding as `ConfigRefused` with code `out_of_range`, naming the
   field in `detail`. Every refusal happens before any thread is started.
6. **Context source.** A request carries a snapshot or, when the engine was
   created with a `contextSource`, may omit it; the binding then obtains the
   snapshot as section 3.7 states. The binding refuses a request with
   neither before admission, as `AdmissionRejected` (`invalid_request`);
   nothing is defaulted.

### 3.4 Errors and unresolved results

1. **Typed errors.** Every refusal and failure the Rust API returns as an
   error maps to an exported subclass of `RustevError` with a stable
   `code` (snake case, never changed once released), a typed `detail`
   payload parsed from canonical JSON, and no message the host must parse.
   The mapping is exhaustive over today's Rust error types:

   | Rust | Class | `code` |
   |---|---|---|
   | compile or check refusal (spec 002) | `CompileRefusal` | the refusal category name, such as `kind_mismatch` |
   | document parse or bound failure | `InvalidDocument` | `invalid_document` |
   | `ConfigError`; a binding sizing value or seam queue capacity out of range (3.3.5) | `ConfigRefused` | `zero_bound`, `duplicate_backend`, `out_of_range` |
   | `PrepareError` | `PlanPrepareRefused` | `missing_backend`, `descriptor_mismatch`, `hard_budget_unenforceable` |
   | `Rejection::Overloaded`, `Rejection::InvalidRequest` | `AdmissionRejected` | `overloaded`, `invalid_request` |
   | `Rejection::QueueDeadline` | `DeadlineExceeded` | `queue_deadline` |
   | `Rejection::Cancelled` | `CancelledBeforeAdmission` | `cancelled` |
   | `DecideError::EvidenceNotDelivered` | `SinkFailure` | the delivery failure name; `detail` carries the run record bytes |
   | `CaptureConfigError` | `CaptureConfigRefused` | its variant name |
   | context source failure before a decision (3.7) | `ContextUnavailable` | `host_seam_failed`, `host_seam_queue_full`, `invalid_snapshot` |
   | invalid `traceparent` (3.8) | `InvalidTraceContext` | `invalid_traceparent` |
   | engine shut down | `EngineShutDown` | `engine_shut_down` |

   `BudgetExhausted` is exported and reserved for a budget refusal the Rust
   API returns as an error before any work. Today no such path exists
   (enforceability is a prepare refusal; exhaustion during a decision is an
   unresolved outcome), so it is never thrown, and a test proves that budget
   exhaustion during a decision yields a value.
2. **Unresolved is a value.** An unresolved outcome of any kind, including
   deadline, budget, backend unavailable or invalid backend output during a
   decision, is returned in `judgment` and never thrown. A `SinkFailure`
   still carries the record, which is evidence and not a result to act on,
   exactly as `DecideError::EvidenceNotDelivered` states.
3. **No panic crosses the boundary.** Every exported function catches a
   Rust panic and turns it into `InternalError` (`code: internal`); the
   engine that observed it refuses further calls with `EngineShutDown`
   after draining as section 3.9 states. A panic is a defect and a test
   failure, never an expected path.
4. The WebAssembly package uses the same classes and codes for every error
   its commands can produce.

### 3.5 Async execution and cancellation

1. **One runtime per engine.** Each `Engine` owns one Tokio multi-thread
   runtime sized by its configuration (worker threads 1 to 64, blocking
   threads 1 to 512; values outside are refused). Every asynchronous call
   returns a Promise resolved on the Node event loop.
2. **No event-loop blocking.** The asynchronous calls run compilation,
   evaluation, replay, gating and calibration on the engine's blocking
   pool, never on the JavaScript thread. `compileSync` and the pure
   synchronous calls are the only exceptions: they run on the calling
   thread, are documented as blocking and are bounded by the same parse
   limits.
3. **Cancellation.** An `AbortSignal` maps to the decision's `CancelSignal`.
   Aborting before admission yields `CancelledBeforeAdmission`; aborting
   after admission is spec 003 cancellation, and the result reports the
   backends' honest answers (`CancelAck`: not requested, stopped,
   unconfirmed; `RemoteEnd`: finished, possibly continuing) exactly as the
   Rust runtime records them. An already-aborted signal is treated as
   aborting before admission. Dropping the Promise does not cancel; only
   the signal does.
4. `deadlineMs` can only tighten the plan's deadline, as spec 003 states for
   `DecisionRequest.deadline_ms`.

### 3.6 Host implemented seams

TypeScript may implement a seam by passing an object that satisfies the
seam's interface. The binding wraps it in a Rust implementation of the
seam trait that calls JavaScript through a napi `ThreadsafeFunction`. These
rules bind every host-implemented seam, today's and later ones:

1. **Asynchronous only.** Rust never blocks a thread waiting on JavaScript.
   Every seam method that the Rust trait declares synchronous is supplied as
   declared data, not as a callback: a TypeScript `DecisionBackend` declares
   its full `BackendDescriptor`, its `CostModel` and one `CostBound` that
   applies to every projection, and supports no batch capability. A
   projection-dependent cost bound or a batch-capable host backend needs a
   later amendment.
2. **Bounded queue per seam.** Each seam instance has a queue whose capacity
   the host declares with the seam object (1 to 65,536; there is no
   default); `Engine.create` refuses a capacity outside that range (3.3.5).
   When the queue is full the call is not enqueued and returns a typed
   refusal immediately: for a backend, an attempt failure `Overloaded` with
   charge observed zero and remote end finished, so spec 003 retry and
   fallback apply; for a sink, a delivery failure handled by the sink
   policy; for a context source, `ContextUnavailable` with
   `host_seam_queue_full`. Nothing buffers without bound on either side.
3. **Inputs passed in.** Every call carries its remaining deadline in
   milliseconds, its attempt or delivery identity, an `AbortSignal` linked
   to the Rust `CancelSignal`, and, for a backend, the reserved cost bound
   for that attempt. The host decides what to do with them; the runtime
   enforces its own deadline regardless.
4. **Late resolution.** A Promise that settles after the runtime has
   finished the call (deadline, cancellation or shutdown) is discarded and
   counted in the seam's `late_resolutions_discarded` statistic. Its value
   never reaches a judgment or record.
5. **Exceptions.** A thrown exception, a rejected Promise, or a value that
   fails the bounded parse maps to the seam's typed failure. A host may
   throw an exported `SeamFailure` carrying the failure class (`transient`,
   `overloaded`, `permanent`), the charge it observed and the remote end it
   knows. Anything else maps to `permanent` with charge `Unknown` and remote
   end `PossiblyContinuing`, because the binding does not know what the
   host's work did; it never assumes zero cost or stopped work.
6. **Same contract as Rust.** A host backend's output is validated by the
   same core path as any backend's; capability matching, identity, admission,
   budgets, retries, fallback, deadlines and cancellation are the runtime's,
   unchanged. Conformance proves it (section 3.12).
7. **Explicit composition (R-38).** A host seam is an object the host passes
   to `Engine.create` or `compile`. Nothing is discovered, loaded from a
   path or registry, or resolved by name at run time; the binding loads no
   code. This is explicit composition under D-06 as R-38 clarifies it.

### 3.7 Seams available today and seams that join later

Wave 1 implements the binding over today's surface:

| Seam (Rust trait, `rustev-core`) | Host-implementable in wave 1 | Notes |
|---|---|---|
| `DecisionBackend` | yes | Section 3.6. |
| `EvidenceSink` | yes | Delivery under the engine's spec 003 sink policy. |
| `ContextSource` | yes | The runtime takes a snapshot in the request today. When a host passes a context source instead of a snapshot, the binding calls it once, before admission, with the plan's declared input fields, parses the result with the snapshot's bounded parser, and then decides. A failure is `ContextUnavailable` and no decision runs; nothing is defaulted. |

These join the bindings when their owning specs land, through the same seam
mechanism (section 3.6), the same error mapping (section 3.4) and the same
conformance suite (section 3.12), without amending this spec's guarantees:
`Screener` (#76 B7), `Generator` (B5), mid-plan acquisition through the
context source (B1, which replaces the before-admission call with recorded
in-plan acquisitions), `explain` (D2), and `profile check` and `plan diff`
(B4). Each owning spec states its seam's or command's binding surface and
adds its cases to the suite. Until then, no binding exposes a placeholder for
them.

### 3.8 Trace context

`traceparent`, when given, must be a valid W3C Trace Context version `00`
header (55 characters, lowercase hex, non-zero trace and parent ids);
otherwise the call fails with `InvalidTraceContext` before admission. The
binding keeps it for the decision and passes it to every host-implemented
seam call of that decision as `traceparent`. It does not enter `CallContext`,
the run record, the judgment, the remote protocol or any identity, so all
bytes stay unchanged. #76 child D1 decides how spans join it.

### 3.9 Memory, shutdown and observability

1. **Lifetime.** A `PlanHandle` holds a reference-counted prepared plan. The
   plan is released when the last JavaScript handle is garbage-collected
   and no decision using it is in flight. An `Engine` is released the same
   way after `shutdown`.
2. **Shutdown.** `shutdown({ drainMs })` stops admission at once (new calls
   fail with `EngineShutDown`), waits for in-flight decisions up to `drainMs`
   (0 to 600,000), then raises cancellation on the rest, which end with
   spec 003's honest cancellation evidence. Pending sink deliveries follow
   the sink policy. The Promise resolves with counts of drained, cancelled
   and late-discarded work.
3. **Observability.** `engine.gauges()` returns a snapshot of the runtime's
   `Gauges`, its `SinkStats` and every host seam's queue depth, refusals and
   late discards. `onGauges(cb, { everyMs })` (100 to 3,600,000 ms) delivers
   the same snapshot periodically through a bounded, drop-oldest queue of
   capacity 1; it returns an unsubscribe function. Gauges are in-memory
   operator views, not evidence.

### 3.10 WebAssembly build

1. **Scope.** Everything pure: `check`, `compile`, `show`, offline `replay`,
   `eval`, `gate`, `calibrate fit` and `calibrate qualify`, all through
   `rustev-surface`. No runtime, no backends, no acquisition, no sinks, no
   decision execution. `plan diff` and `profile check` (B4) and `explain`
   (D2) join when their specs land (section 3.7).
2. **Build.** `wasm-bindgen` for `wasm32-unknown-unknown`, with the
   `wasm-bindgen-cli` version equal to the `wasm-bindgen` crate version
   (exact pins; `make bindings` refuses a mismatch), outputs for the `web`,
   `bundler` and `nodejs` targets in one package as subpath exports, and the
   document declarations of section 3.2. The module is optimized with a
   pinned `wasm-opt` at `-Oz`.
3. **No ambient capabilities.** The module imports only wasm-bindgen's
   generated glue intrinsics; it has no WASI import, no `env` import, and no
   import of time, randomness, network, storage or DOM. CI inspects the
   import section against an allowlist and fails on anything else. Time is
   an argument (`nowMs`, evaluation time). Bytes a replay or evaluation
   needs are supplied by a synchronous host callback, as `rustev-eval`'s
   resolver already defines; a callback that throws or returns invalid
   bytes yields the same per-case outcome the CLI reports for unusable
   bytes.
4. **Size budget (declared, measured).** For the optimized
   `rustev_wasm_bg.wasm`:
   - gzip (level 9) compressed size at most 786,432 bytes (768 KiB): gated;
   - uncompressed size at most 2,097,152 bytes (2 MiB): gated;
   - Brotli (quality 11) size: recorded, not gated.
5. **Instantiation budget (declared, measured).** Instantiation is the time
   from module bytes in memory to the first successful call of an exported
   `version()`, covering compile, instantiate and glue initialization:
   - Node 22 on the GitHub-hosted `ubuntu-24.04` x64 standard runner: median
     at most 50 ms over 21 runs, each in a fresh process, after 3 discarded
     warm-up processes that warm only the file system and page cache, so
     every measured run is a cold instantiation: gated;
   - headless Chromium (exact-pinned through the test harness) on the same
     runner class: median at most 100 ms under the same procedure: gated;
   - p95 and the raw samples: recorded.
6. **How CI measures.** The `bindings` job (section 3.12) builds the
   release module, measures sizes with the pinned compressors, runs the
   timing procedure, writes a measurement document (sizes, samples,
   medians, runner image, Node and Chromium versions, commit), uploads it as
   a job artifact, and fails when a gated value exceeds its budget. The
   first measurement is recorded in this spec's implementation record. A
   budget is lowered or raised only by amending this spec; a build never
   adjusts it.

### 3.11 Floating point and cross-target determinism

1. **What is promised.** Principle XIII stands: byte determinism is promised
   for canonical compilation and explicitly defined exact computation.
   Plans, identities, canonical documents and exact operators use integer,
   decimal-string and correctly rounded IEEE 754 basic operations, which
   agree bit for bit on every target the bindings build for.
2. **What can differ.** Some pure paths call transcendental functions:
   today `exp` in core's distribution normalization, `ln` in core's
   calibration, and `ln` in evaluation's log loss and temperature fitting.
   On native targets these come from the platform's math library (glibc,
   musl, Apple, Microsoft), and on `wasm32-unknown-unknown` from Rust's
   bundled implementation. They are not required to be correctly rounded,
   so results may differ in the last bit between targets, including between
   two native targets, and a shortest round-trip serialization exposes that
   difference.
3. **What the parity suite proves.** That on each target CI runs (every
   native matrix target of section 3.13 and WebAssembly in Node and
   Chromium), every conformance case produces its expected bytes. In
   addition, a differential corpus runs each transcendental call site
   reachable from a pure command over a fixed, committed input set (at
   least 100,000 inputs per call site, including subnormal, boundary and
   large-magnitude values) on every target and compares bit patterns. NaN
   payloads are out of scope because canonical JSON refuses non-finite
   numbers.
4. **What it does not prove.** Agreement for inputs outside the goldens and
   the corpus, on targets CI does not run, or under future math library or
   toolchain versions. The bindings make no such claim.
5. **A divergence.** Any bit difference in a conformance case or the
   corpus fails the parity job. The remedy, routing those functions through
   one pure Rust implementation on every target, changes core and may
   change existing goldens, so it is a separate amendment of spec 002 with
   a golden migration; this spec does not authorize it silently. Until it
   lands the affected surface is not released.
6. Outputs never depend on hash iteration order, pointer width or
   `usize` size; limits are checked in `u64` before conversion, and the
   suite runs on `wasm32` to prove it.

### 3.12 Shared conformance suite

1. **Rule.** Every host surface runs the shared suite: the CLI, the native
   binding and the WebAssembly package now; the HTTP and gRPC transports of
   #76 child A2 when they land. A surface that does not run it does not
   ship.
2. **Location.** `tools/rustev-conformance/`:
   - `cases/manifest.json`, a `rustev.conformance-manifest/1` document
     listing each case: id, command, the surfaces it applies to, inputs (by
     repository path and SHA-256), expected outputs (by path and SHA-256),
     and its comparison;
   - expected bytes: the existing CLI goldens are referenced in place under
     `crates/rustev-cli/tests/golden/`, never copied; outputs no golden
     covers today (evaluation and gate reports, calibration results,
     WebAssembly-only artifacts) are generated by the CLI once, committed
     under `tools/rustev-conformance/expected/`, and reviewed like any
     golden;
   - the Rust runner (a crate binary and test), which drives the CLI through
     its library entry point and the Rust runtime API;
   - the JavaScript runner, one harness with a native adapter and a
     WebAssembly adapter (Node and Chromium).
3. **Comparison.** A case's comparison is `bytes` (whole output identical)
   unless it declares, exhaustively, (a) CLI-only presentation fields to
   exclude (file paths, exit codes) and (b) observation fields to normalize
   with exactly the normalization the CLI goldens already use for timing.
   No other difference is tolerated. Every case asserts `plan_id`.
4. **Coverage.** At least: every existing CLI golden for `check`,
   `compile`, `show`, `run`, `replay` and the refusal goldens (native);
   every pure command on WebAssembly; `eval` and `gate` on both packaged
   reference tasks; runtime cases for admission, budget, retry, deadline
   and cancellation expressed as data (a scripted backend script per
   attempt: outcome, charge, manual-clock delay, cancel answer, remote end)
   with expected run record bytes under `ManualClock`, each naming the
   existing `crates/rustev-runtime/tests` test it mirrors. The Rust runner
   executes runtime cases with Rust scripted seams; the native runner with
   TypeScript scripted seams; both must produce the same expected bytes.
   Host-seam cases add queue full, late resolution, exception mapping and
   abort mapping.
5. **Make and CI.** `make conformance` runs the Rust runner and needs no
   Node toolchain. `make bindings` builds both packages and runs type drift
   checks, the JavaScript runner (native and WebAssembly), the differential
   corpus, the import allowlist and the WebAssembly budgets; it requires
   Node 20 or later and refuses a tool version other than its exact pins.
   CI runs both through a declared extra required job `bindings` in
   `.github/workflows/bindings.yml`, owned by this spec in the way spec 001
   owns `boundaries.yml`, declared in `project.setup.parameters` of
   `.statecraft/environment.json` and rendered through
   `statecraft-cli init plan|apply`. Adding it needs the owner's approval of
   the `statecraft-review-exception` Environment; this spec does not make
   that change, and its implementation is not complete, and no surface
   ships, until that job runs as required.

### 3.13 Packaging

1. **Native matrix.** Prebuilt binaries for `linux-x64-gnu`,
   `linux-x64-musl`, `linux-arm64-gnu`, `linux-arm64-musl`, `darwin-arm64`,
   `darwin-x64` and `win32-x64` (`x86_64-pc-windows-msvc`), one platform
   package each in napi-rs's optional-dependency layout. The binding pins
   Node-API version 8 (the napi-rs `napi8` feature), which Node 20, 22 and
   24 all provide; `engines.node` is `>=20`. Node 20, 22 and 24 are each
   tested on linux-x64-gnu; every matrix target runs a smoke test (load,
   compile a reference plan, decide with the rules backend, replay) on
   Node 22, on a native runner where one exists and otherwise under a
   recorded emulation (Alpine containers for musl, Rosetta 2 for
   darwin-x64).
2. **No install-time code.** No `postinstall` or other install script, no
   download at install or load time. The loader selects the platform
   package by `process.platform`, `process.arch` and the C library, and
   fails with a typed error naming the missing platform package.
3. **WebAssembly package.** One package with `web`, `bundler` and `nodejs`
   subpath exports; it runs its smoke test in Node and in headless Chromium.
4. **Publication.** Both packages are built as candidates under spec 021's
   lifecycle as amended by #76 child A3 (OD-6), with provenance-signed npm
   publication. Nothing is published, reserved or tagged by this spec.

### 3.14 Dependency boundaries

`tools/rustev-boundaries` gains:

1. Integration-only families: Node-API (`napi`, `napi-derive`, `napi-sys`,
   `napi-build`) and WebAssembly bindings (`wasm-bindgen`,
   `wasm-bindgen-macro`, `wasm-bindgen-futures`, `js-sys`, `web-sys`). A
   crate outside `integrations/` depending on one fails, as for HTTP stacks.
2. The transitive purity check of `rustev-contract` and `rustev-core` also
   refuses these families, and `rustev-surface` joins the checked set.
3. `rustev-wasm`'s normal and build graph may not contain `rustev-runtime`,
   an executor, an HTTP stack, a Node-API crate, `getrandom`, `web-sys` or
   `wasm-bindgen-futures`.
4. Unit tests for each rule, in the existing style, and `make boundaries`
   passes on the workspace.

The two binding crates keep spec 001's rule that no crate depends on an
integration crate. Their hand-written code contains no `unsafe` block; if
macro-generated glue requires it, the crate relaxes `unsafe_code` from
`forbid` to `deny` with the reason at the attribute, and a test fails on any
hand-written `unsafe`. `rustev-core` keeps `forbid(unsafe_code)`.

### 3.15 Binding principles

How this spec keeps each principle #76 lists as binding:

- **Proposals, never grants.** Bindings return judgments, unresolved
  outcomes, refusals and evidence. The generated and hand-written
  declarations define no permission, grant or `Permitted` type.
  `principalHandle` is opaque bytes passed to the runtime as spec 003's
  `principal_handle`; the binding never interprets it.
- **Derivation classes and lineage.** Every judgment and record crosses as
  canonical bytes with its derivation classes and lineage; the parsed view
  keeps them, and `origin` exposes them without recomputation.
- **Unresolved, never a default; no silent kind conversion.** Section 3.4.2,
  3.2.5, 3.6.5 (unknown charge, possibly continuing) and 3.7 (no defaulted
  snapshot).
- **Pure core.** Core and contract gain no I/O, clock, `unsafe` or async
  runtime; only `wasm32` `cfg` attributes are allowed (section 2).
- **New dependency classes only under integrations.** Section 3.14 extends
  `make boundaries`.
- **No dynamic plugin loading.** Section 3.6.7 and R-38.
- **Rustev owns no identity, authority or knowledge store.** The bindings
  hold no credentials, users, sessions or stored knowledge; evidence goes to
  the host's sink and retention stays with the host.

## 4. Out of scope

HTTP and gRPC serving (#76 A2); the screening, generation, acquisition,
explanation and profile seams and commands until their specs land; remote
backends linked into the binding; projection-dependent host cost bounds and
host batch backends; a Deno, Bun or React Native specific build (they may
consume the WebAssembly package, untested); WASI targets; threads or SIMD in
WebAssembly; publication, name or scope reservation, release; OpenTelemetry
spans (#76 D1); any change to plan, identity or evidence bytes.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A host backend's queue is full | Attempt failure `overloaded`, charge observed zero, remote end finished; retry and fallback as the plan declares; queue depth never exceeds capacity. |
| A host seam's declared queue capacity is 0 or above 65,536 | `ConfigRefused` (`out_of_range`) from `Engine.create`; no thread started. |
| A host backend resolves after the deadline | The decision records the deadline outcome; the late value is discarded and counted. |
| A host backend throws a plain `Error` | Attempt failure `permanent`, charge `Unknown`, remote end possibly continuing. |
| A host backend returns output failing the bounded parse | Spec 003 invalid backend output; unresolved, not an exception. |
| Budget is exhausted during a decision | Unresolved value in `judgment`; no exception. |
| The deadline passes while queued for admission | `DeadlineExceeded` (`queue_deadline`); no record. |
| `AbortSignal` aborted after dispatch, backend unconfirmed | Result records `unconfirmed` and possibly continuing work, as the Rust runtime does. |
| The evidence sink rejects delivery under fail-decision policy | `SinkFailure` with the run record bytes in `detail`. |
| The context source throws | `ContextUnavailable`; no decision runs; no snapshot is defaulted. |
| `traceparent` is malformed | `InvalidTraceContext` before admission. |
| A document exceeds its parse bound | `InvalidDocument` before unbounded allocation, on native and `wasm32`. |
| The compiled WebAssembly module imports a clock, randomness or WASI function | The import allowlist check fails the build. |
| The WebAssembly module exceeds its size or instantiation budget | The `bindings` job fails. |
| A transcendental result differs in one bit between native and WebAssembly | The parity job fails; no release of that surface. |
| A committed `.d.ts` differs from regenerated output | The drift check fails. |
| A crate under `crates/` depends on `napi` or `wasm-bindgen` | `make boundaries` fails. |
| No platform package for the host's platform | Typed load error naming the missing package; no download is attempted. |
| A call after `shutdown` | `EngineShutDown`. |

## Acceptance

- Golden parity with the CLI for `check`, `compile`, `show`, `run`,
  `replay` and `eval` (native) and for every pure command (WebAssembly),
  through the shared conformance suite, with every case asserting `plan_id`.
- The CLI's existing goldens pass unchanged after it calls `rustev-surface`.
- Cancellation through `AbortSignal` produces the same `CancelAck` evidence
  as the Rust runtime in scripted-backend tests.
- A TypeScript-implemented backend, source and sink pass the runtime's
  conformance cases (admission, budget, retry, deadline, cancel) with the
  same expected bytes as the Rust runner.
- A queue-full seam yields a typed refusal and never unbounded memory; a late
  resolution is discarded and counted; exceptions map as section 3.6.5
  states.
- Every row of section 5 has a deterministic test without network access or
  wall-clock sleeps (except the instantiation timing of 3.10).
- Schema and type generation drift checks pass in CI for both packages.
- The prebuilt native matrix builds and the smoke test runs on each target
  in CI; Node 20, 22 and 24 run the native suite; the WebAssembly package
  runs its smoke test in Node and in headless Chromium.
- WebAssembly size and instantiation budgets and the import allowlist are
  enforced in CI, and the measurement document is retained.
- The differential floating-point corpus runs on every target and the
  result is recorded.
- `make boundaries` refuses the new families outside `integrations/` and
  the forbidden graph of `rustev-wasm`, with unit tests.
- `make code`, declared verification, and `make gate` pass.

## Verification

Declared for `spec-spine verify 027`; 027 joins `make verify`. The
WebAssembly package, the native matrix and the Node versions are the CI
`bindings` job's (`make bindings` and `.github/workflows/bindings.yml`),
a required job of `ci-gate`; the first measurements and the CI runs are
in the implementation record.

```verify:cli
# 3.12: the shared conformance suite's Rust runner (make conformance).
cargo test -p rustev-conformance --locked
# Section 2: the pure command surface and the CLI goldens it renders.
cargo test -p rustev-surface --locked
# 3.1, 3.10.1: the new families stay in integrations/ (make boundaries).
cargo run -p rustev-boundaries --locked --quiet
cargo test -p rustev-boundaries --locked
```

## Implementation record

Delivered in increments, each its own pull request; nothing here claims an
acceptance item before the increment that satisfies it. Plan:

1. Dependency boundaries (3.14).
2. The JSON Schema emitter and committed schemas (3.2.2).
3. `rustev-surface`, adopted by the CLI with its goldens unchanged (3.1,
   section 2).
4. The conformance suite and its Rust runner, and `make conformance`
   (3.12).
5. `rustev-wasm`, type generation and the JavaScript runner's WebAssembly
   adapter, the import allowlist and the budgets (3.2.3, 3.10, 3.11).
6. `rustev-node`: the engine, errors, cancellation, host seams, trace
   context, shutdown and gauges (3.3 to 3.9).
7. The `bindings` CI job, the native matrix and the differential corpus
   (3.11 to 3.13), after the owner approves the
   `statecraft-review-exception` Environment for it.

**Increment 1 (dependency boundaries).** `tools/rustev-boundaries` gains
the Node-API and WebAssembly binding families (`napi`, `napi-derive`,
`napi-sys`, `napi-build`, `wasm-bindgen`, `wasm-bindgen-macro`,
`wasm-bindgen-futures`, `js-sys`, `web-sys`) as integration-only
families: a crate outside `integrations/` depending on one is a
`ForbiddenFamily` violation, as for HTTP stacks (3.14.1). The transitive
purity check of `rustev-contract` and `rustev-core` also refuses them, and
`rustev-surface` joins the checked set (3.14.2). `rustev-wasm`'s normal and
build graph may not reach `rustev-runtime`, an executor, an HTTP stack, a
Node-API crate, `getrandom`, `web-sys` or `wasm-bindgen-futures`; a reach
is a `WasmGraph` violation naming the path (3.14.3). Unit tests, in the
existing style, cover each family outside `integrations/`, both binding
crates passing under it, a pure crate and the surface reaching a binding
family or executor, each forbidden crate in the WebAssembly graph, and the
dev-only and allowed edges of that graph; `make boundaries` passes on the
workspace.

**Increment 2 (JSON Schema emission).** `rustev-contract` gains
`schema_emit::document_schemas()`, which emits a JSON Schema (draft
2020-12) for every public document type and version: definition 1 and 2,
plan 2 and 3, snapshot 1 and 2, evidence 1 and 2, run 1 and 3, judgment 1
and 2, backend descriptor, calibration, backend output, evaluation report,
execution policy, replay bundle, request, runtime reason, source
descriptor, the request and response of each `rustev.remote/1` exchange,
and the exchange record. Each schema is derived (through `schemars`
=1.2.3, pure, exact-pinned) from the same serde definitions the bounded
parser uses: closed objects where the parser denies unknown fields, tagged
enums, and hand-written schemas for the types with their own serde
(identities and digests by their `sha256:` pattern, decimals by their
string syntax, scope handles by length); `schema` is fixed to the version
by `const`, and the document's `ParseLimits`, which a schema cannot
express, are recorded as `x-rustev-parse-limits`. Emission is canonical
JSON, byte-identical across runs; the schemas are committed under
`crates/rustev-contract/schema/` and `crates/rustev-contract/tests/schemas.rs`
fails when emitted bytes or the file set differ (`RUSTEV_BLESS=1` rewrites
them). The same test validates every committed JSON document under a test
directory of the workspace whose `schema` names an emitted version against
that schema (`jsonschema` =0.59.0 as a dev-dependency, without HTTP or
file resolution), and checks that a schema refuses an unknown member and
another version's `schema`. No document, parse bound or canonical byte
changes. The new source file joins the reviewed archive list
(`tools/rustev-release/archive-policy.json`, spec 021 3.5), hence this
spec's `extends` edge on `tools/rustev-release/`; the release audit
(cargo-deny bans, licenses and sources over the release set) passes with
the new dependency.

**Increment 3 (the command surface).** `crates/rustev-surface` holds the
pure command surface: `plan check`, `compile` and `show`, offline
`replay`, `eval`, `gate`, `calibrate fit` and `calibrate qualify`, the
dependency reading they share, the task adapter, the command output and
exit codes, and the result envelope of `run` (base fields, rejection,
delivery, judged with spec 033's addressing, cancelled, evidence not
delivered, capture status). Its code is the CLI's, moved: each command
takes typed arguments and a `Host` (reads by label, bounded and budgeted
as the host's transport is; outputs published by label, never
overwritten, all or none; label joining), and a hook through which the
host derives a rules program's descriptor, so the surface depends on
`rustev-contract`, `rustev-core`, `rustev-eval`, `serde` and `serde_json`
only. The CLI keeps argument parsing, its file system host (`Reads`
implements `Host`; labels are paths), the content-addressed store, the
rules backend and the runtime: each of its commands checks its flags and
calls the surface, and `run` prints its outcome through the surface's
envelope. Every existing CLI test passes with its goldens unchanged, none
re-blessed. `crates/rustev-surface/tests/memory_host.rs` drives the
surface from an in-memory host (named inputs, outputs in memory) and gets
the CLI's golden bytes for `plan check` and `plan show`, the plan `plan
compile` publishes, the refusal of an existing output, and a rules
program refused by a host without a rules backend. Because the published
CLI depends on it, `rustev-surface` joins the release family as a crate
member in its own layer before the CLI (the release-set file, its test's
member list and the archive allowlists change accordingly; like every
member's, the surface's allowlist names the `Cargo.lock` that
`cargo package` writes into each archive, library or not), under spec 045
(ratified 2026-10-20, decision A-42, in its own change, which adds
`specs/045-command-surface-release-layer/`), which amends spec 031 3.1
for it; this increment merges after that change. Spec 006's mutation
seeds (`crates/rustev-cli/mutation/seeds.py`) follow the code they seed:
the defects in command logic now sit in `rustev-surface`, where the CLI
calls it, and each must still be detected, by the CLI's tests or by the
surface's own (the adapter's unit tests moved with it).

**Increment 4, first change (the suite and its CLI cases).**
`tools/rustev-conformance` (`publish = false`, excluded from the release
set as repository tooling) holds `cases/manifest.json`, a
`rustev.conformance-manifest/1` document listing each case by id with its
command, surfaces, argument vector, exit code, inputs (repository path,
the name the case reads it by, SHA-256) and outputs (printed or written,
the golden it must equal by repository path and SHA-256, and the
normalizations applied first). Its 29 cases are every CLI golden under
`crates/rustev-cli/tests/golden/`, referenced in place, never copied:
`plan check`, `compile` and `show` of both reference plans and of the
actionable plan, every refusal golden (categories 1 to 12, 16 and 17),
`run` of both reference plans (stdout and record) and of the four
actionable outcomes, and `replay` of both. The cases are harvested from
the CLI tests that pin those goldens: with `RUSTEV_HARVEST=1`, the test
harness copies each pinned invocation's inputs, as they were before it
ran, under `tools/rustev-conformance/fixtures/<case>/` and records the
case (`make conformance-harvest` rebuilds the manifest and fixtures). The
normalizations are the goldens' own (`receipt` for a `run` output's sink
receipt, `timing` for a record's timings), in one file the CLI tests and
the runners share. The Rust runner (`make conformance`, no Node
toolchain) runs every `cli` case through `rustev_cli::execute` in a fresh
directory holding exactly its pinned inputs and compares every output and
the exit code; every CLI golden is covered, every pinned input and golden
matches its digest, every printed `plan_id` is compared as bytes (no
declared normalization touches it), and a differing output or exit code
is reported. The CLI cases are harvested into one file each under
`cases/cli/`, and the manifest is the aggregate of every case file, checked
for drift (`RUSTEV_BLESS=1` rewrites it).

`eval` and `gate` on the packaged reference tasks and the
bindings' surfaces come in later changes.

**Increment 4, second change (the runtime cases).** Runtime cases expressed as data (3.12.4) join the same manifest under
`cases/runtime/`, each naming the test under `crates/rustev-runtime/tests`
it mirrors: a full admission queue (`admission.rs`), retries drawing from
a hard budget (`budget.rs`), transient retries with exponential delays
(`retry.rs`), attempts in flight at the deadline (`time.rs`) and a
confirmed stop on cancellation (`cancel.rs`). A case declares its plan,
descriptors and calibrations (fixtures built from spec 002's shared
builders, under `fixtures/runtime/`), the runtime bounds, a scripted
backend per registration (per task, the answer of each attempt in order:
an output with its charge, a classed failure with its charge, or a gate;
the cost model and bound, the remote end, the answer to cancellation, and
the answer a released gate gives), the requests, and the driver's steps on
a manual clock (start a request, wait for gated attempts, queued
decisions or a sleeper, advance the clock, release gates, cancel). The
Rust runner (`rustev_conformance::runtime`) interprets a case with Rust
seams; under the manual clock every timing is fixed, so each request's
result and run-record bytes, committed under `expected/runtime/`, are
compared without normalization. The expected records are checked against
what each mirrored test asserts (the third request is rejected
`overloaded` with no record; two attempts and 8 observed units before the
budget refuses the third; attempts dispatched at 0, 10 and 30 ms; every
request `deadline_exceeded` with unknown charges; every attempt stopped
with its charge of 2), so a blessed record is never merely what the
runner printed. A binding's runner interprets the same data with its
host's seams.

**Increment 5 (the WebAssembly package).** `integrations/rustev-wasm`
(`publish = false`; excluded from the crates.io release set, shipped only
through npm) exports `version`, `check`, `compile`, `show`, `replay`,
`eval`, `gate`, `calibrateFit` and `calibrateQualify` through
`rustev-surface`: a host puts named documents into an `Inputs`, calls a
command naming them, and gets the CLI's exit code, the output object's
bytes and the artifacts the command published by name. The surface gains
`host::Memory`, the in-memory host every binding uses (the surface's own
tests now use it). No runtime, backend, sink or clock: a rules program is
not read (its derived descriptor is), time is an argument, and replay and
evaluation resolve external items through a synchronous host callback, a
throw or invalid answer being inaccessible as for the CLI's store. The
`wasm` cargo profile (size first, link-time optimization, one codegen
unit, no unwinding, stripped) builds it; `make bindings` checks the exact
pins (`wasm-bindgen-cli` 0.2.129, the crate's version, and `wasm-opt`
116), builds the `web`, `bundler` and `nodejs` targets optimized at `-Oz`,
regenerates and type-checks the document declarations, enforces the import
allowlist and the budgets, writes the measurement document, and runs the
JavaScript conformance runner on the package. The declarations
(`rustev-documents.d.ts`) are generated offline from the committed schemas
by `json-schema-to-typescript` 16.0.0 under a committed lockfile, one
namespace per document and version, and type-check under TypeScript 5.9.3
in strict mode; the drift check fails when a committed copy differs. The
module's imports are only `wasm-bindgen`'s glue, each listed by base name
in `tools/rustev-conformance/js/wasm-imports.json` with its reason; the
check fails on any other. The JavaScript runner's WebAssembly adapter runs
every manifest case that names the `wasm` surface (the harvest marks
`plan check`, `compile`, `show` and `replay`, 23 cases, and records each
rules program's derived descriptor for it): all 23 print the CLI's golden
bytes and exit codes, refusals and replays included. Its `package.json` stays `"private": true` until
publication: spec 031 owns the npm release and makes the package
publishable as part of it, so no build of this increment can be published
by accident.

First measurement (3.10.6), local, macOS arm64, Node 24.6.0, commit of this
increment: optimized `rustev_wasm_bg.wasm` raw 2,215,589 bytes, gzip
(level 9) 762,689 bytes, Brotli (quality 11) 543,501 bytes; Node cold
instantiation median 3.5 ms over 21 fresh processes. The gzip and
instantiation budgets hold; the raw budget of 2,097,152 bytes does not
(5.6% over): the contract's document types, most of them added by specs
032 and 033 after this budget was declared, account for most of the
module (their serde code is about 1 MB before optimization), and every
pure command needs them, and no `wasm-opt` option closes the gap (`-Oz`
is the smallest). The owner ratified spec 046 (A-43), which raises the
raw budget to 2,621,440 bytes and keeps every other budget and the gate;
the measurement check applies it, so `make bindings` passes with the
module at 2,216,152 bytes. Headless Chromium timing and the CI
measurement come with the `bindings` job.

**Increment 6, first change (the native crate).** `integrations/rustev-node`
(`publish = false`; excluded from the crates.io release set, shipped as
`@statecrafting/rustev` and its platform packages) is a napi-rs binding
(napi 3.14.2 with Node-API version 8, napi-derive 3.6.12, napi-build
2.6.0). Its native layer: an `Engine` owning one Tokio multi-thread runtime
sized by its configuration (1 to 64 workers, 1 to 512 blocking threads;
refusals before any thread), its registered backends (rules programs as
bytes, and host backends), its sink and optional context source;
`compile` of a definition, or of an already compiled plan as the CLI's
`run` loads it, and `prepare`, on the blocking pool (with blocking
`compileSync`); `decide` running on the engine's runtime (the promise
only awaits it), returning the judgment, run record and delivery with the
contract's canonical bytes, an unresolved outcome being a value; the pure
commands through `rustev-surface` and `host::Memory` on the blocking pool
(`check` and `show` also synchronous); a manual clock option with its
pending delays for tests; gauges; shutdown. Hand-written code has no
`unsafe`: napi-rs's generated glue needs it, so this crate relaxes the
workspace's `forbid` to `deny` with the reason at the manifest, and
`tests/no_unsafe.rs` fails on any hand-written `unsafe`. Host seams
(3.6) are threadsafe functions, weak so an idle engine never holds the
event loop: each seam has a declared queue (1 to 65,536) whose overflow is
refused at once (a backend's attempt fails `overloaded` with nothing
charged and nothing running; a sink's delivery fails; a context source is
`ContextUnavailable`), every call carries its remaining time, identity,
an `AbortSignal` the runtime's cancellation aborts, the reserved bound and
the decision's `traceparent`, a settlement after the runtime finished the
call is discarded and counted, and a rejection that is not a `SeamFailure`
is `permanent` with an unknown charge and possibly continuing work. A
context source is called once before admission with the plan's input
fields when a request carries no snapshot; its failure is
`ContextUnavailable` and nothing is defaulted. A `traceparent` (validated
as W3C version 00) reaches every seam call of its decision and enters no
record. The JavaScript layer over this crate is the second change.

Two findings for the owner. First, the runtime polls a cancelled attempt
once for its answer (spec 003, 3.7.3); a host seam answers across threads
and, not blocking (3.6.1), is never ready then, so a host backend's answer
to an `AbortSignal` is always recorded `not_observed` with possibly
continuing work, never `stopped` or `unconfirmed`: section 5's row
"aborted after dispatch, backend unconfirmed: the result records
`unconfirmed`" holds for Rust backends, and for host backends only as this
more conservative record. Second, section 5's row "a host backend returns
output failing the bounded parse: spec 003 invalid backend output" and
3.6.5's "a value that fails the bounded parse maps to the seam's typed
failure" disagree; the binding follows 3.6.5 (an unparseable answer is
`permanent` with an unknown charge), and an output that parses but the
core refuses is `invalid_backend_output`, which the test shows. Either
needs the owner's reading or an amendment. The native conformance adapter
follows in the third change; the native matrix, prebuilt platform
packages and CI come with increment 7.

**Increment 6, second change (the JavaScript layer).** The JavaScript layer (`index.js`, declarations `index.d.ts`, and
the generated document declarations, byte-identical to the WebAssembly
package's) gives the API of 3.3: `Engine.create`, `compile`,
`PlanHandle.decide` with `AbortSignal` (an already aborted signal is
`CancelledBeforeAdmission`), the typed `RustevError` subclasses and codes of
3.4 (a native failure crosses as `{ class, code, detail }` JSON; a
`SinkFailure` carries the record's bytes), `onGauges` through a bounded
drop-oldest subscription, and `shutdown` that stops admission, waits up to
`drainMs` and raises cancellation on the rest; it loads the platform
package for this process or names the missing one, downloading nothing.
`integrations/rustev-node/test/negative.test.mjs`
tests each section 5 row of the native package, with the refusals the
review rounds added (27 tests).

**Increment 6, third change (the native conformance cases).** Every CLI
case is marked for the native surface too (pure commands also for
WebAssembly), and a `run` case records the derived descriptors a native
runner prepares its plan against. Running the runtime cases against host
seams, which answer asynchronously, required two corrections to their
driver: it advances the clock only when the expected retry delay is
pending and no attempt is in flight (read through
`ManualClock::pending_delays`, the additive accessor the first change of
this increment added to spec 003's crate); and
the cancellation case shared by every runner mirrors
`cancel.rs::no_answer_means_local_waiting_stopped_and_nothing_more`,
while the confirmed-stop case stays a Rust-runner case
(`runtime.cancel-stop`). The JavaScript runner's native adapter is the
fourth change.

**Increment 6, fourth change (the native conformance runner).** The
JavaScript conformance runner gains its native adapter, run by
`make bindings`: every CLI case runs natively (pure commands compared
whole, written outputs against the artifacts the command published; a
`run` compared on the judgment and the run record, the envelope's paths,
receipt and exit code being CLI presentation, with the sink checked to
receive exactly the record's bytes), and every runtime case runs with
TypeScript scripted seams on the engine's manual clock. All 34 produce
their expected bytes, repeatedly.

**Increment 7, first change (the platform packages).** The seven
platform packages of 3.13.1 sit under `integrations/rustev-node/npm/<platform>/`
in napi-rs's optional-dependency layout, one per optional dependency of
the main package: each is `@statecrafting/rustev-<platform>` at the main
package's version, license and engines, with the `os`, `cpu` and (on
Linux) `libc` its name stands for, the addon `rustev.<platform>.node` as
its `main` and only file, no script, and `private` until spec 031's
release makes the family publishable. `tools/rustev-conformance/js/platform-packages.mjs`
checks that agreement (`check`), stages this host's release addon into its
platform package (`stage`, sharing `copy-addon.mjs`'s naming), and packs
the main package and this host's platform package and installs both into a
fresh project from the tarballs alone (`fresh`), omitting optional
dependencies so nothing is fetched for another platform; the installed main
package carries no addon and loads the platform package as a consumer's
install does (3.13.2). `make bindings` runs all three and the native
conformance suite on that install. Locally (macOS arm64) all 34 native
cases pass on it. Building and smoke-testing each matrix target is the CI
job's (a later change of this increment).

**Increment 7, second change (headless Chromium).** The test harness pins
`playwright-core` 1.63.0 exactly (no dependencies; covered by `npm audit
signatures`), and so the Chromium build: `make bindings` installs that
version's headless shell. `tools/rustev-conformance/js/chromium.mjs time`
measures cold instantiation as 3.10.5 defines it, from module bytes in
memory to the first successful `version()`, 3 discarded warm-up runs then
21 measured, each in a fresh browser process, the page served from a
loopback server holding only the package; it writes the browser version,
samples, median and p95. `wasm-check.mjs` gates the median
(`--chromium-ms`) and records the rest in the measurement document
(`--chromium-record`, whose median must be the gated one). The JavaScript
runner's WebAssembly adapter now builds each case as one plain call
(`wasmCall`) run by one self-contained executor (`executeWasm`), so the
same code runs in Node and, by its text, in a Chromium page:
`run.mjs chromium` runs every WebAssembly case there (3.11.3, 3.13.3).
Local measurement (macOS arm64, Chromium 153.0.8010.12 headless shell):
median 3.1 ms, p95 3.3 ms against the 100 ms budget, all 23 WebAssembly
cases passing in Chromium as in Node. The CI runner's measurement comes
with the `bindings` job.

**Increment 7, third change (the differential corpus).** After spec 047
every transcendental call reachable from a pure command is a call of
`rustev_core::math`, at four sites: `exp` in core's softmax, `ln` in core's
calibration, `ln` in evaluation's log loss and `ln` in its temperature fit.
`tools/rustev-conformance/fp-corpus` (crate `rustev-fp-corpus`,
`publish = false`, excluded from the release set as tooling) runs each site
over 131,072 inputs drawn as the site sees them (non-positive arguments of
`exp`; positive finite masses; probabilities in (0, 1] and just above), each
set beginning with its boundary values (zeros, the smallest and largest
subnormals, `f64::MIN_POSITIVE`, one ulp either side of 1, the arguments
where `exp` leaves the normal range and where it rounds to 0, and the
format's extremes) and continued by a seeded SplitMix64 generator that uses
integer arithmetic and correctly rounded basic operations only, so every
target draws the same bits. The input set is fixed by that file and pinned
by its SHA-256 per site; each site's output bits are digested the same
way, and both digests are committed in `fp-corpus/expected.json`. The
native runner (`rustev-fp-corpus check`, and a test running it) and the
WebAssembly module (the same crate for `wasm32-unknown-unknown`, plain
exports and no imports; `js/fp-corpus.mjs` in Node and in headless
Chromium) must each reproduce every digest; any difference fails
(3.11.5). A test fails when the pure crates' `rustev_core::math` calls and
the corpus's sites differ, so a new call site cannot go unmeasured.
`make bindings` runs all three. Locally (macOS arm64) native, Node 24.6.0
and Chromium 153.0.8010.12 agree on all four sites. Seeded with the
platform's `exp` and `ln` in place of `rustev_core::math`, the native run
differs from the committed digests and fails, while WebAssembly, whose
standard library uses the same pure implementation, still matches: the
divergence spec 047 removed. The results on every CI target come with the
`bindings` job.

**Increment 7, fourth change (the `bindings` CI job).** The declared extra
required job `bindings` (3.12.5) is `.github/workflows/bindings.yml`, which
this spec owns as spec 001 owns `boundaries.yml`. It is registered by
adding it to `ci.extra_required_jobs` in `project.setup.parameters` of
`.statecraft/environment.json` and re-rendering with the setup profile's
revision-17 renderer (Statecraft producer commit `2f36691`), together with
spec 031's `release-checks` job in the same change. Its jobs: `wasm` runs
`make bindings` on the reference runner (`ubuntu-24.04` x64, Node 22) and
uploads the measurement document, the Chromium samples and the corpus
results; `node-versions` runs the native suite on linux-x64-gnu under Node
20 and 24; `native` builds the addon on each of the seven platforms (glibc
in Debian bookworm, glibc 2.36, tested on the runner; musl built and tested
in Alpine 3.22 with its Node 22, the recorded emulation; macOS arm64 and
x64 and Windows x64 on native runners, so no Rosetta emulation), runs the
corpus there, stages the addon into its platform package, installs both
packages into a fresh project from their tarballs and runs the native
conformance suite on Node 22, and uploads the addon. Build images are
pinned by digest. Checkout keeps the committed bytes on every platform
(`core.autocrlf` off), because the fixtures and goldens are pinned by
digest and the Windows runner would otherwise rewrite their line endings.

**Completion, 2026-10-21.** CI run 39684537149 on #153's head `093f834`
passed every job of `ci-gate`, among them `bindings / wasm` (the
WebAssembly package in Node and headless Chromium, the drift checks, the
import allowlist and the size and instantiation budgets: 2,251,563 bytes
raw and 772,853 gzip against spec 046's 2,621,440 and 786,432, Node median
7.7 ms, Chromium median 6.8 ms, on linux-x64 with Node 22.23.3), `bindings /
node 20` and `node 24`, and `bindings / native` on the seven platforms,
each running the differential corpus; the measurement document and the
corpus results are that run's `bindings-measurement` artifact. The push
run on `main` at `00a4b7c` (39688068295) passed the same jobs.

Known limitations, recorded by the owner's ruling (R-47) rather than
fixed. The owner chose to complete this spec with these two departures
recorded instead of amending section 5 or 3.6.5: the approved text stands
as written, and this record is the authoritative statement of where the
implementation departs from it. Removing either departure, or amending
the text to match it, is a later change under its own spec.

1. A host backend's answer to an `AbortSignal` is always recorded
   `not_observed` with possibly continuing work, never `stopped` or
   `unconfirmed`: the runtime polls a cancelled attempt once (spec 003,
   3.7.3), and a host seam, answering across threads without blocking
   (3.6.1), is never ready then. Section 5's row "aborted after dispatch,
   backend unconfirmed: the result records `unconfirmed`" holds for Rust
   backends; for host backends the record is this more conservative one.
2. Output failing the bounded parse follows 3.6.5 (the seam's typed
   failure: `permanent`, charge unknown), not section 5's row naming
   spec 003's invalid backend output; output that parses but the core
   refuses is `invalid_backend_output`, as the tests show.

**Follow-up, 2026-10-22 (a test-harness flake).** The waits in
`integrations/rustev-node/test/negative.test.mjs` and the conformance
driver (`tools/rustev-conformance/js/run.mjs`) were bounded by 100,000
event-loop turns; on a loaded CI runner the test "a host backend resolving
after the deadline is discarded and counted" exhausted them before the
engine's threads counted the late resolution, and failed with "waited
forever" (#151's `bindings / wasm`). Both waits are now bounded by 120 s
of wall time, still yielding with `setImmediate` and never sleeping; no
assertion changed.

**Emission extended, 2026-10-22 (R-46).** Implementing the owner's ruling
of 2026-10-21, the
documents specs 019, 026 and 028 added are public document types under
3.2.2 and join the emission: `rustev.dynamic-question/1`,
`rustev.dynamic-capability/1` and `rustev.dynamic-outcome/1` (019),
`rustev.live-reexecution/1` (026), and `rustev.redaction/1` and
`rustev.trace/1` (028, whose types gain the `JsonSchema` derive). Six
schema files join `crates/rustev-contract/schema/`, and both packages'
`rustev-documents.d.ts` gain their six types, regenerated by
`gen-types.mjs` and still byte-identical. No existing schema or generated
type changes. The committed-document check also reads each line of a
committed `.jsonl` file, so the span-line goldens of `rustev-otel` are
validated against `rustev.trace/1`; the other five documents are checked
against their schemas in their own contract tests.

- 2026-10-24 (CI only, no claim of this spec changes): `bindings.yml` caches as spec
  001's record of this date says. The wasm and node jobs restore everywhere and save on a push to main. Workspace crates' artifacts are
  dropped before saving, because Cargo rebuilds them after any checkout.
- 2026-10-24: a late settlement is counted however it races the runtime
  (3.6.4). The seam task counted one only when its send failed, so a reply
  that arrived after the deadline ended the attempt, but before the runtime
  dropped the attempt's future, was discarded uncounted. The reply future now
  closes its channel on drop and counts a reply left unread, so each
  discarded settlement is counted exactly once. This was the cause of the
  intermittent `bindings / node 20` failure of "a host backend resolving
  after the deadline is discarded and counted" in the merge queue.

## Decisions at approval

1. **npm package names and scope** (left to the implementer by #56; decided
   by the implementer within this spec). Recommendation: native
   `@statecrafting/rustev` with platform packages
   `@statecrafting/rustev-<platform>` (for example
   `@statecrafting/rustev-linux-x64-gnu`), and WebAssembly
   `@statecrafting/rustev-wasm`. The scope matches the source organization,
   keeps platform packages from being squatted, and ties npm provenance to
   the repository. Observed on 2026-10-16 against `registry.npmjs.org`:
   `@statecrafting/rustev`, `@statecrafting/rustev-node`,
   `@statecrafting/rustev-wasm`, `@statecrafting/rustev-linux-x64-gnu`,
   `@statecrafting/rustev-darwin-arm64`,
   `@statecrafting/rustev-win32-x64-msvc`, `rustev`, `rustev-node`,
   `rustev-wasm`, `@rustev/node` and `@rustev/wasm` all returned 404 (not
   published); a registry search for scope `statecrafting` returned no
   packages. Whether the `@statecrafting` organization exists on npm and who
   controls it could not be determined anonymously. If the owner cannot
   obtain the scope, the fallback is unscoped `rustev-node` and
   `rustev-wasm`; names change no API. Obtaining the scope and reserving
   names remain owner acts under spec 021.
2. **WebAssembly budgets** (required by #56; decided by the implementer
   within this spec): the declared values and procedure of section 3.10.
   They are ceilings to be measured, not measurements.
3. **Node versions and matrix** (settled by #56): Node 20, 22 and 24 and the
   seven native targets. Observation: Node 20 reached upstream end of life
   on 2026-04-30; it stays supported as #56 requires, and dropping it is an
   amendment of this spec. Node-API version 8 is pinned because all three
   provide it.
4. **Where the conformance suite lives** (decided by the implementer within
   this spec): `tools/rustev-conformance/`, with CLI goldens referenced in
   place.
5. **Shared rendering** (decided by the implementer within this spec): a
   new pure crate `rustev-surface` rather than each surface re-rendering
   CLI output, because #76 names drift between surfaces as a risk; the CLI
   adopts it with byte-identical output.
6. **Host seams use no synchronous callbacks** (decided by the implementer
   within this spec): synchronous trait methods are declared data, which
   rules out deadlock between the event loop and runtime threads.
7. **Seam clarification** (settled by the work order): host-implemented
   seams are explicit composition under D-06 (R-38, OD-5 of #76).
8. **Floating point** (decided by the implementer within this spec):
   section 3.11; a divergence is fixed by a separate spec 002 amendment, not
   by tolerance.

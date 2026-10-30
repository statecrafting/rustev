# rustev-serve

Work in progress for spec 010's serving binding and spec 042 increments 1 and 2.
This implementation is not yet a complete conforming decision service.
Responses are proposals and evidence, never execution authorization.

## Embedding

Construct `Service::new(config, Host { ... })` with host-owned backend,
context-source and evidence-sink implementations. The service validates the
`rustev.serve-config/1` document, expected backend descriptor identities, signing
trust and preloaded registrations before accepting work. `transport::listen`
serves a configured listener. The transport-neutral service methods also support
hosts with their own transport. `serve_authenticated_connection` requires its
caller to authenticate the stream before handing it over.

TCP uses mandatory client certificates, TLS 1.3, the binding's three ring cipher
suites and ALPN for HTTP/1.1 and HTTP/2. Unix listeners reject existing paths and
apply the configured 0600 or 0660 mode. Development bearer tokens are loaded from
host files. There is no network trust-root discovery.

Plan registration validates the bundle and its Ed25519 or offline Sigstore
signature, compiles it against host-installed backends and retains an immutable
registration. Decision requests cannot install a plan, backend, endpoint, policy,
credential or limit. Principal-scoped idempotency reserves bounded memory before
dispatch; disconnect does not cancel the owned decision task. Retained responses
preserve decision status separately from transport status and survive repeated
submissions until expiry. Memory retention is lost on restart.

Replay and evaluation use `rustev-surface` with bounded blocking work and
host-installed replay scopes. Artifact reads are confined to configured roots.
Explanation and invalidation endpoints are unavailable pending spec 040.

Live lookup distinguishes queued and running work using the runtime admission
callback. Cancellation preserves actual backend stop acknowledgements, including
unconfirmed work that may still be continuing. SSE offers value-free step events
without waiting for clients, counts overflow, and reserves a separate final slot.
Semantic steps are reported immediately after recorded supply, using the compiled
step derivation shared with core evidence. Backend determinism does not establish
an exact derivation: core currently classifies all semantic steps as model-derived. Core-only steps
are reported from completed core evidence before sink delivery; the core exposes
no incremental evidence observer. Unresolved core steps use their compiled
conservative derivation bound because their evidence carries no dynamic class.
Observer panics are contained and counted separately from decision evidence.

Normal shutdown stops accepting connections and starts Hyper graceful shutdown
while decisions drain and sink tickets settle. Existing streams can send their
final event; connection flushing is bounded. A second signal skips these waits.

## gRPC

The same authenticated listeners route `application/grpc` requests over HTTP/2
(ALPN `h2` on TCP) to `rustev.serve.v1.Serve`. Every protobuf message has one
`bytes document` field; canonical document bytes pass through unchanged. Empty
HTTP bodies remain empty, and Metrics carries the same OpenMetrics bytes.
Methods and schema comments come from the contract operation table.

Request headers retain their names as metadata. HTTP path parameters use
`rustev-plan-id` or `rustev-decision-id`; Cancel's optional query wait uses
`rustev-wait-ms`. IDs are percent-escaped UTF-8 path components, also for the
`rustev-decision-id` failure metadata; `grpc::path_id` provides that encoding.
Refusals carry their exact document in `rustev-refusal-bin` with the contract
status code. A failed evidence delivery returns INTERNAL and its decision ID;
Lookup returns the retained document with OK. Stream sends each canonical event,
including its reserved final event, before terminal status trailers.

The development generator uses protoc (29.3 was used) from `$PROTOC`, or `protoc` on `PATH`; without one, the test still checks the `.proto` and the stub's recorded `.proto` digest:

```sh
cargo test -p rustev-serve --locked --test grpc_generation
RUSTEV_BLESS=1 cargo test -p rustev-serve --locked --test grpc_generation
```

The second command rewrites the checked-in proto and Rust output. There is no
build script or generation during normal builds. Tests check both outputs for
drift. Socket tests stay enabled; equivalent in-memory HTTP/2 tests cover the
same framing and handler path when the environment forbids binding sockets.

## Conservative choices

- Client subject allowlist entries use `der:` followed by lowercase hexadecimal
  DER of the complete X.509 subject Name. WebPKI validates the chain first; this
  avoids ambiguous textual distinguished-name normalization.
- Unix socket creation requires a parent directory with mode 0700. This prevents
  exposure between bind and chmod. Host-managed access is necessary for a 0660
  shared socket; this adapter does not change directory ownership or group.
- Existing socket paths are never unlinked automatically, including on exit.
- Public Ed25519 key files are exactly 32 raw bytes. Config and trusted-root files
  are bounded to 4 MiB. Signature verification grants no authority.
- Root directories must be host-controlled and read-only to untrusted processes.
  Canonical-path confinement, regular-file checks and inode checks do not provide
  a safe concurrent hostile-writer filesystem abstraction.
- Timed-out blocking replay/evaluation retains its worker permit until the
  synchronous function exits. A timeout does not claim that computation stopped.
- Unexpected task failure marks the service stopping and preserves the uncertain
  reservation, preventing a second dispatch. It returns an empty HTTP 500 rather
  than falsely asserting a pre-dispatch refusal.

## Remaining work and integration seams

These gaps are not claims of spec acceptance:

- Runtime live decision IDs are global. Concurrent equal IDs under different
  principals can conflict even though service retention keys are principal-scoped.
  Fixing this requires a runtime identity seam without changing evidence IDs.
- Context fetching uses the host ContextSource, but remote disposition detail,
  cancellation acknowledgement and late-callback accounting are not fully mapped.
- Readiness requires host-reported describe completion for remote components and
  observes configured degradation and unconditional invalidation degradation.
  `report_component_health` rejects unknown names and impossible queue depths.
  Runtime/sink gauges, backend concurrency, callback health and stream-drop metrics
  are exposed. Host adapters must report actual callback health; protocol describe
  probes and invalidation application are not implemented here. Hyper can reject
  parser-level oversized headers before a typed refusal can be produced.
- Replay repeat execution and rules-backed evaluation return endpoint unavailable;
  the available surface API has no repeat hook, and a rules backend is not wired.
  Evaluation output artifacts are local to that evaluation invocation.
- The standalone `--config FILE` binary constructs configured remote evidence
  sinks and completes their describe handshake before serving. Delivery includes
  canonical record bytes, digest, registration, signer and service build. Receipts
  must match the digest and advertised durability states. Delivery uses the
  configured fail-decision, backpressure or drop-counted runtime policy. No sink
  configured means explicit delivery failure. Ecosystem backends and context
  clients still require host construction. OpenTelemetry startup is refused
  because no exporter feature is built.
- The shared conformance suite, remote HTTP context bindings, and specs 034/036
  integration are not implemented yet.

## Image

`image/Dockerfile` builds the binary on the pinned Rust toolchain image and
copies it alone onto the pinned `gcr.io/distroless/cc-debian12:nonroot` base,
as user and group 65532, with entrypoint
`/usr/local/bin/rustev-serve --config /etc/rustev/serve.json`. Mount the
configuration there; the root filesystem can be read-only. From the
repository root:

```sh
docker buildx build --platform linux/arm64 --load -t rustev-serve:dev \
  -f integrations/rustev-serve/image/Dockerfile .
integrations/rustev-serve/image/check.sh rustev-serve:dev linux/arm64
```

Release builds, SBOMs, signatures and publication belong to the release
tooling (spec 031), not this file.

## Verification

Run the six checks in the increment 2 task brief. Tests cover handler refusals, signed
registration, retention, idempotency, disconnect ownership, admission limits,
shutdown accounting, root confinement, HTTP/1.1 and HTTP/2, and TLS authentication.
The in-memory transport tests exercise real Hyper and rustls connections without
binding sockets. Separate TCP loopback and Unix socket tests remain enabled.

The current sandbox denies TCP and Unix `bind` with EPERM, so socket tests
cannot pass here. They must run in an environment that permits local sockets.
Sigstore fixtures verify both an upstream artifact and a synthetic Rustev plan
registration offline, including identity, issuer, inclusion proof and tampering.
Remote sink tests cover startup describe, delivery identity, idempotency,
conflicting and wrong-digest receipts, and timeout. Remote context callback cases
remain excluded with their HTTP binding. See
`tests/COVERAGE.md` for the negative-case matrix. See `tests/fixtures/README.md` for fixture provenance.

Follow-up checks also cover all three configured sink policies, queued ticket
settlement, counted drops and failed/unflushed shutdown records. The owner ran the
initial 19 tests outside the sandbox and reported all passing; socket tests remain
enabled. The larger updated suite still needs that external socket-capable run.

<!-- spec-spine:begin generated id="header" -->
# `011-semantic-backend` Local semantic backend: frozen embeddings and a linear head

- Path: `specs/011-semantic-backend/spec.md`
- Summary: The first local model backend, `backends/rustev-backend-embed-linear/`: an ordinary `DecisionBackend` that embeds projected text with a frozen, host-provisioned sentence encoder run by ONNX Runtime through `ort`, and answers `classify`, `proposition` and `rubric` with logits from an externally produced, identified task-specific linear head. One canonical `rustev.embed-linear/1` manifest binds checkpoint, graph, tokenizer, preprocessing, truncation, pooling, precision, runtime, heads, labels, cost and tolerance into the artifact identity. Implementation is gated by a staged feasibility record (`rustev.embed-feasibility/1`) that passes or refuses each selection of #69 against measured limits before any inference dependency lands. No network, no download, no training, no provider; repeatability is a declared tolerance, not bitwise. Optional `local-embed` features in the Node binding, the service and the facade, and an opt-in in-process embedding cache. Amends spec 001 only by adding a local inference runtime family and one placement rule to the boundary check. Ratified (A-28) under R-36, which amends R-17 for this spec; R-43 authorizes the feasibility stages and pins the revision that ships an ONNX graph; implemented and complete (2026-11-06).
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
| created | `2026-09-23` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `011-semantic-backend#I-1` | `invariant` | The backend never grants authority: it supplies logits or a classified failure only, every supplied value is model-derived, and no output, embedding, cache entry or setup result is authorization or an input fact. | `3-1-position-authority-and-provenance` | no |
| `011-semantic-backend#I-2` | `invariant` | Every behavior-affecting input (checkpoint, graph, tokenizer, preprocessing, truncation, pooling, precision, runtime version and settings, operators, heads, labels, text templates, limits, cost and tolerance) is in the `rustev.embed-linear/1` manifest, whose digest is the artifact identity; a mutable name, alias or path is never an identity. | `3-5-identity` | no |
| `011-semantic-backend#I-3` | `invariant` | The crate performs no network access and no download at build, setup or inference; it reads only host-supplied absolute paths, verifies every byte it uses against the manifest before use, and refuses a missing, oversized or mismatched artifact with a typed code. | `3-7-setup-and-refusals` | no |
| `011-semantic-backend#I-4` | `invariant` | The descriptor declares exactly the operations and options of the bundled heads, output `logits` only and determinism `tolerance`; no `rank`, no label outside a head's trained options, no distribution or calibration claim. | `3-10-capabilities-and-the-descriptor` | no |
| `011-semantic-backend#I-5` | `invariant` | Cancellation is acknowledged `stopped` only when the backend has established that the run ended; otherwise `unconfirmed`, and the worker slot stays occupied until the run ends. | `3-12-deadlines-and-cancellation` | no |
| `011-semantic-backend#I-6` | `invariant` | Repeatability is declared as a tolerance measured under recorded conditions; no bitwise or cross-platform claim is made beyond what the feasibility record measured. | `3-13-numerical-repeatability` | no |
| `011-semantic-backend#R-1` | `requirement` | No inference dependency enters the workspace until the feasibility record passes stages 1 to 3 for at least one platform; a platform is supported only when all four stages pass for it and the Node binding builds with `local-embed` on its native matrix target, and a refusal is recorded, never hidden. | `3-2-the-feasibility-gate` | no |
| `011-semantic-backend#R-2` | `requirement` | An embedding cache hit supplies exactly the bits a miss computed in the same process for the same artifact, principal handle and token sequence, never crosses a principal handle, and is refused on a platform without measured in-process bitwise repeatability. | `3-14-embedding-cache` | no |
| `011-semantic-backend#R-3` | `requirement` | Builds of the Node binding, the service and the facade without `local-embed` contain no crate of the local inference runtime family, and the boundary check refuses that family outside the backend crate except through an optional dependency on it. | `3-18-boundary-check-extension` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"backends/rustev-backend-embed-linear/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `010-integrations` | `{"kind":"directory","path":"integrations/rustev-serve/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"tools/rustev-boundaries/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
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
cargo test -p rustev-backend-embed-linear --locked --test feasibility_record
cargo test -p rustev-backend-embed-linear --locked --test bundle
cargo test -p rustev-backend-embed-linear --locked --lib
cargo test -p rustev-backend-embed-linear --locked --test offline
cargo test -p rustev-serve --locked --features local-embed --lib local_embed
cargo clippy -p rustev-serve --locked --all-targets --features local-embed -- -D warnings
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
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
| amends | `001-boundaries-and-authority` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | None declared. |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:011-semantic-backend` | `sha256:a6112ec53e46e4196064c70b395cff1bc453b45a20d92ad378f4d8ded736291e` |
| `registry:011-semantic-backend` | `sha256:a6112ec53e46e4196064c70b395cff1bc453b45a20d92ad378f4d8ded736291e` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

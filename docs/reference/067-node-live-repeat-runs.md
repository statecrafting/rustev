<!-- spec-spine:begin generated id="header" -->
# `067-node-live-repeat-runs` Node binding: live repeat runs over host backends (amends 006, 027, 043 and 063)

- Path: `specs/067-node-live-repeat-runs/spec.md`
- Summary: An amendment of approved specs 063 (3.9), 027 (3.3, 3.7) and 043 (section 2, 3.2.2, 3.5.1, 3.11.4, 3.13), for release 0.6.0 (#539), ratified by the owner on 2026-11-06 (A-86). A Node host whose model backends are TypeScript `DecisionBackend`s (027 3.6) cannot run fresh repeat runs of its plans through Rustev: the binding's repeat commands refuse the `live` source (063 3.9.4), and `rustev eval live` links only the rules backend (043 3.13.2). So the host keeps its own harness, outside Rustev's spend journal and caps. This spec adds `engine.liveRun`, an asynchronous method over `rustev_live::run` with the CLI's inputs, the engine's registered host backends and the same documents as `rustev eval live`, all or none for the final files. The journal is a file path the binding opens under 043 3.5's lock and durability rules, and no attempt is sent before its full worst-case exposure is durably reserved. A host backend's exposure is the cost model and bound it already declares (027 3.6.1); an `unknown` model is refused. Probe-only, resume and rerun-interrupted are 043's; an `AbortSignal` or `shutdown` ends the run `interrupted` with every reserved, unsettled attempt kept as liability. The cumulative cap holds per journal file, so CI must persist the file; a journal created by a run records an id and creation time, and the run report names them, so a fresh journal is visible, never silent. Tests use scripted host backends only; CI makes no live or paid call. This fulfills 043 section 2's "the Node binding exposes it in its own spec".
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
| created | `2026-11-06` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `067-node-live-repeat-runs#I-1` | `invariant` | No attempt of a `liveRun` is handed to a host backend unless its full worst-case exposure, from the backend's declared cost model and bound, is durably reserved in the journal first and fits both the run cap and the journal's cap; a binding whose cost model is `unknown`, or that has no entry in the live configuration, is refused before the journal is opened. | `3-3-host-backend-cost-declarations` | no |
| `067-node-live-repeat-runs#I-2` | `invariant` | Cancellation by `AbortSignal`, the run deadline or `shutdown` never releases a reservation for an attempt that was handed to a host backend and has not settled; its charge stays liability in the journal, its item is `interrupted`, and a late Promise settlement is discarded and never settles the ticket. | `3-6-cancellation-and-shutdown` | no |
| `067-node-live-repeat-runs#I-3` | `invariant` | For the same inputs, scripted backends and clock, `liveRun` writes the same documents, bytes, status and exit code as `rustev_live::run` called from Rust, and the binding adds no authority: its authorizer is 043 3.12's declarative rule list, and no binding value is a permission. | `3-2-the-entry-point` | no |
| `067-node-live-repeat-runs#I-4` | `invariant` | A journal created by a live run records a stable `journal_id` and `created_at_ms` in its header, and every `rustev.live-run/1` names the journal's id, creation time, whether this run created it, and the cumulative spend before the run, so a run that started its cumulative cap at zero on a new file is visible and never silent. | `3-4-the-journal-and-the-output-directory-are-file-paths` | no |
| `067-node-live-repeat-runs#R-1` | `requirement` | Every test and conformance case of this spec uses scripted host backends and a temporary `testing` journal; no CI job makes a live or paid provider call, and the WebAssembly package gains nothing. | `3-8-tests-and-conformance` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `043-live-re-execution-at-scale` | `{"kind":"directory","path":"crates/rustev-live/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
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
cargo test -p rustev-node --locked
cargo test -p rustev-conformance --locked
cargo test -p rustev-surface --locked --all-features
cargo test -p rustev-live --locked
cargo test -p rustev-cli --locked --test eval_live
cargo clippy -p rustev-node -p rustev-conformance -p rustev-live --all-targets --locked -- -D warnings
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
| amends | `006-cli-surface`, `027-node-and-wasm-bindings`, `043-live-re-execution-at-scale`, `063-node-binding-parity` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `069-node-host-nonblocking-open` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:067-node-live-repeat-runs` | `sha256:c4bd24d7740781714f7479f20f66978f1fdec11226a738f2be2e800ab81db2c6` |
| `registry:067-node-live-repeat-runs` | `sha256:c4bd24d7740781714f7479f20f66978f1fdec11226a738f2be2e800ab81db2c6` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

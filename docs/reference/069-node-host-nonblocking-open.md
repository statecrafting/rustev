<!-- spec-spine:begin generated id="header" -->
# `069-node-host-nonblocking-open` The Node binding's filesystem host opens inputs without blocking (amends 006, 027 and 067)

- Path: `specs/069-node-host-nonblocking-open/spec.md`
- Summary: An amendment of approved specs 067 (3.4.2 and its implementation record's residual race), 027 (the Node binding's host) and 006 (3.3.1's non-blocking open, which moves to a shared home), for #566, ratified by the owner on 2026-11-08 (A-89). The Node binding's filesystem host for `engine.liveRun` opens every input file and store item with the standard library's blocking open, so a regular file swapped for a FIFO or device between its metadata check and the open can block the call; the CLI closes that window with a non-blocking open. This spec moves the CLI's open into `rustev-surface`, has the CLI and the Node host both use it, and requires a test that a FIFO at an input path or store item is refused without blocking. Nothing read changes: the check on the open handle already refused a non-regular file.
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
| created | `2026-11-08` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `069-node-host-nonblocking-open#I-1` | `invariant` | On Unix, every input file and store item the Node binding's filesystem host reads is opened with `O_NONBLOCK` (store items also with `O_NOFOLLOW`) through `rustev_surface::host::open_input`, so a FIFO or device swapped in after the metadata check opens at once and is refused by the check on the handle; no read through that host waits for a FIFO writer. | `3-2-the-node-filesystem-host` | no |
| `069-node-host-nonblocking-open#I-2` | `invariant` | The CLI and the Node host share one implementation of the non-blocking open; the CLI's `rustev_cli::io::open_input` behavior (006 3.3.1) is unchanged. | `3-1-one-shared-open` | no |
| `069-node-host-nonblocking-open#R-1` | `requirement` | A Unix test creates a FIFO with no writer at an input path and at a store item, and asserts that the Node host's read returns the not-a-regular-file refusal (input) or `Inaccessible` (store item) within a bounded time instead of blocking. | `3-3-tests` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
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
cargo test -p rustev-surface --locked --lib open_tests
cargo test -p rustev-node --locked --lib live_fs
cargo test -p rustev-cli --locked
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
| amends | `006-cli-surface`, `027-node-and-wasm-bindings`, `067-node-live-repeat-runs` |
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
| `index:069-node-host-nonblocking-open` | `sha256:adc53fd218c35083471eb59e6136f136636b029972b8050713d9709029022a6d` |
| `registry:069-node-host-nonblocking-open` | `sha256:adc53fd218c35083471eb59e6136f136636b029972b8050713d9709029022a6d` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

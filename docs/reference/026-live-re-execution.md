<!-- spec-spine:begin generated id="header" -->
# `026-live-re-execution` Live re-execution with fresh observations

- Path: `specs/026-live-re-execution/spec.md`
- Summary: A separately invoked live re-execution path that creates a new decision from a newly identified snapshot, obtains fresh host authorization for every dispatch, applies new deadlines and budgets, and records new backend, cancellation, cost, retention, and delivery observations. It may compare the new proposal with a retained historical proposal, but never reuses a historical decision identity, supplies retained backend outputs as fresh, rewrites a replay bundle, or lets offline replay contact a backend.
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
| created | `2026-09-26` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `026-live-re-execution#I-1` | `invariant` | Offline reproduction and candidate comparison remain unable to dispatch a backend; only the explicit live re-execution entry point may request fresh external work. | `3-1-separate-live-entry-point` | no |
| `026-live-re-execution#I-2` | `invariant` | Every live re-execution has a new decision id, a newly constructed identified snapshot, a fresh evaluation time, fresh admission and budgets, and new runtime observations; it never mutates or relabels a historical record. | `3-2-new-decision-and-snapshot` | no |
| `026-live-re-execution#I-3` | `invariant` | Before each external dispatch, the host independently authorizes the exact scope, backend target, artifact, operation, maximum exposure, and remaining time; missing, expired, mismatched, or refused authorization prevents dispatch. | `3-3-host-authorization-at-dispatch` | no |
| `026-live-re-execution#I-4` | `invariant` | Every attempted external effect, cancellation answer, observed or unknown charge, delivery result, retained reference, and comparison limitation is attributable to the new decision and cannot be concealed by the historical source. | `3-6-evidence-cost-and-external-effects` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test live
cargo test -p rustev-runtime --locked --test live
cargo test -p rustev-runtime --locked --doc
cargo test -p rustev-eval --locked --test live
cargo test -p rustev-eval --locked --test dependencies
cargo test -p rustev-eval --locked --test reproduce
cargo run -p rustev-boundaries --locked --quiet
sh -c 'if grep -rnE "std::(fs|net|env|process|thread|time)|tokio|SystemTime|Instant" crates/rustev-eval/src; then exit 1; fi'
sh crates/rustev-runtime/mutation/seeds.sh live:
sh crates/rustev-eval/mutation/seeds.sh live-comparison
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
| amends | `003-runtime-execution-and-evidence`, `004-evaluation-and-replay` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `043-live-re-execution-at-scale` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:026-live-re-execution` | `sha256:cc9f52600a69ca9cadc56345542950aea986fdd9508d4c98c00494e8892670b3` |
| `registry:026-live-re-execution` | `sha256:cc9f52600a69ca9cadc56345542950aea986fdd9508d4c98c00494e8892670b3` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

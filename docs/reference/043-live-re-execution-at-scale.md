<!-- spec-spine:begin generated id="header" -->
# `043-live-re-execution-at-scale` Live re-execution at dataset scale with spend caps (amends 026 and 006)

- Path: `specs/043-live-re-execution-at-scale/spec.md`
- Summary: An amendment of approved spec 026 (live re-execution) and of spec 006 (CLI), for #76 child E3 (#74). A new crate `rustev-live` runs a dataset, or the inputs (never the outputs) of retained replay bundles, through one or more candidate plans live, through spec 026's `reexecute_live`, under an explicit `--live` opt-in, a per-run spend cap and the persistent spend journal of spec 012, generalized to any backend with a cost model and moved into `rustev-live`. A runtime spend gate durably reserves every attempt's full worst-case exposure before any byte is sent and refuses an attempt that would pass the run cap or the journal's cap (R-29: USD 10 in total for all testing across providers since A-75 raised it from USD 5). Acquisitions are declared live or captured per source; a probe checks reachability and served identity of every candidate binding before any dataset case is sent; scheduling is bounded per binding; runs resume from a checkpoint without re-sending a completed case or counting a charge twice. Every result is new evidence marked `live_reexecution` with a link to its source case or record, and per-run outputs are spec 029 repeat outputs with source `live`, so E1 and E2 consume them unchanged. Approved (A-40); approval authorizes no live run, provider call or spend by itself, raises no cap, and makes no quality claim.
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
| created | `2026-10-17` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `043-live-re-execution-at-scale#I-1` | `invariant` | Offline replay, evaluation, comparison and gate code paths still cannot dispatch a backend: `rustev-eval` has no normal dependency on `rustev-runtime` or `rustev-live`, and only `rustev-live`'s run entry point, reached through `reexecute_live`, may request external work. | `3-1-live-evaluation-entry-points` | no |
| `043-live-re-execution-at-scale#I-2` | `invariant` | No attempt of a live run is sent unless its full worst-case exposure is durably reserved in the persistent journal first and fits both the run cap and the journal's cap; an attempt whose exposure is unknown, whose cost unit is undeclared, or that would pass either cap is refused before any request byte, with its refusal code. | `3-6-spend-gate-and-refusal-before-sending` | no |
| `043-live-re-execution-at-scale#I-3` | `invariant` | Resumption never re-sends a case whose result was checkpointed and never counts one charge twice: an attempt reserved but not settled before an interruption remains liability in the journal and its case is reported interrupted, re-run only under an explicit flag with new attempt identities. | `3-10-checkpoints-and-resumption` | no |
| `043-live-re-execution-at-scale#I-4` | `invariant` | Every live result of a `rustev_live::run` work item (a dataset case or retained bundle, never a probe call) is a new run record plus a `rustev.live-reexecution/1` document and a `rustev.live-case/1` document with `mode` `live_reexecution` and a link to its source case or record; no dataset, bundle, capture, run record, judgment or report used as a source is modified. | `3-9-evidence-of-live-re-execution` | no |
| `043-live-re-execution-at-scale#I-5` | `invariant` | Without `rustev eval live`, every existing CLI command, library function, document, golden and plan identity is byte-identical, and the journal file format is unchanged. | `3-15-compatibility-and-identities` | no |
| `043-live-re-execution-at-scale#R-1` | `requirement` | An acquisition declared captured is served only from a capture keyed by its request digest; a missing capture is missing evidence for that acquisition and never falls back to a live request. | `3-4-live-or-captured-acquisitions` | no |
| `043-live-re-execution-at-scale#R-2` | `requirement` | No dataset case is dispatched until every candidate binding has passed its probe; a failed probe ends the run with `probe_failed` and no case sent. | `3-7-probe` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-live/"}` | no | `resolved` |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `030-bedrock-backend` | `{"kind":"directory","path":"integrations/rustev-bedrock/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
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
cargo test -p rustev-live --locked
cargo test -p rustev-live-crash --locked
cargo test -p rustev-runtime --locked --test live --test acquisition
cargo test -p rustev-eval --locked --test observed --test live_separation --test reports
cargo test -p rustev-jev --locked --test spend
cargo test -p rustev-cli --locked --test eval_live --test repeat
cargo clippy -p rustev-live -p rustev-live-crash -p rustev-cli --all-targets --locked -- -D warnings
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
| amends | `006-cli-surface`, `026-live-re-execution` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `067-node-live-repeat-runs` |
| superseded by | None declared. |
| extended by | `067-node-live-repeat-runs` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:043-live-re-execution-at-scale` | `sha256:0b967da3823d5f283e4e54ec95833d8fe80ed7b8b4f9ec4486189e82ba4f3deb` |
| `registry:043-live-re-execution-at-scale` | `sha256:0b967da3823d5f283e4e54ec95833d8fe80ed7b8b4f9ec4486189e82ba4f3deb` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

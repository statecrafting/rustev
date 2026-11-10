<!-- spec-spine:begin generated id="header" -->
# `006-cli-surface` CLI surface

- Path: `specs/006-cli-surface/spec.md`
- Summary: Increment 2: `rustev`, a command-line host over the library. Plan check, compile and show against descriptor, rules-program, calibration and execution-policy files; run one snapshot through `rustev-runtime` on rules-program backends with a file evidence sink and optional bounded capture into a replay bundle; offline replay under an explicitly supplied trusted scope; evaluation reports with their detail and configuration companions, regression gates, and temperature fitting with its lineage record, through `rustev-eval`. Bounded file access, stable JSON output and distinct exit codes. No network, no hosting, no live inference beyond the deterministic rules backend.
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

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

None declared.
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
cargo test -p rustev-cli --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 't=$(cargo tree -p rustev-cli -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -xE "hyper|reqwest|axum|actix-web|warp|tonic|ureq|isahc|surf|h2|http|clap|aicortex.*|rahi.*|statecraft.*" | grep -q .; then exit 1; fi'
cargo clippy -p rustev-cli --all-targets --locked -- -D warnings
cargo fmt -p rustev-cli --check
sh crates/rustev-cli/mutation/seeds.sh
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
| amends | None declared. |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `021-packaging-and-first-release`, `028-opentelemetry-evidence-sink`, `029-evaluation-repeatability`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `040-decision-explanation`, `043-live-re-execution-at-scale`, `056-release-0-1-1-and-consumer-qualification`, `067-node-live-repeat-runs`, `069-node-host-nonblocking-open` |
| superseded by | None declared. |
| extended by | `014-report-integrity`, `015-per-case-bundle-failures`, `021-packaging-and-first-release`, `027-node-and-wasm-bindings`, `028-opentelemetry-evidence-sink`, `029-evaluation-repeatability`, `031-public-api-and-release-family`, `032-evidence-quality`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `041-ranking-and-rubric-metrics`, `042-decision-service-and-remote-seams`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `050-wasm-without-parameterized-definitions`, `056-release-0-1-1-and-consumer-qualification`, `061-host-scored-repeat-sets`, `063-node-binding-parity`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity`, `067-node-live-repeat-runs`, `069-node-host-nonblocking-open` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:006-cli-surface` | `sha256:ae450b1de904dabd6afe9d05c10500e49bc79b0837b822d7d5e9b7986475b9d8` |
| `registry:006-cli-surface` | `sha256:ae450b1de904dabd6afe9d05c10500e49bc79b0837b822d7d5e9b7986475b9d8` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

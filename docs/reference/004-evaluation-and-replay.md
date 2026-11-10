<!-- spec-spine:begin generated id="header" -->
# `004-evaluation-and-replay` Evaluation and replay

- Path: `specs/004-evaluation-and-replay/spec.md`
- Summary: Increment 2: offline reproduction from bounded, host-retained replay bundles; configurable tenant-only or principal-scoped equivalence for candidate comparison; explicit availability, coverage and abstention; named dataset splits and temperature fitting. Amends 002 with replay and request documents and typed loading diagnostics, and 003 with opt-in bounded capture of supplied values. Live re-execution is deferred. Synthetic fixtures establish mechanics only, never empirical quality.
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
| `establishes` | None declared. | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 't=$(cargo tree -p rustev-eval -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -E "^(rustev-|tokio$)" | grep -vxE "rustev-(contract|core|eval)" | grep -q .; then exit 1; fi'
sh -c 'if grep -rnE "std::(fs|net|env|process|thread|time)|tokio|SystemTime|Instant" crates/rustev-eval/src; then exit 1; fi'
cargo clippy -p rustev-eval --all-targets --locked -- -D warnings
cargo fmt --all --check
sh crates/rustev-eval/mutation/seeds.sh
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
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `014-report-integrity`, `015-per-case-bundle-failures`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `026-live-re-execution`, `029-evaluation-repeatability`, `036-mid-plan-acquisition`, `037-generation-steps`, `040-decision-explanation`, `041-ranking-and-rubric-metrics` |
| superseded by | None declared. |
| extended by | `013-remote-state-on-failed-attempts`, `014-report-integrity`, `015-per-case-bundle-failures`, `017-general-runtime-optimization`, `019-dynamic-questions`, `020-remote-protocol-refinements`, `021-packaging-and-first-release`, `023-real-data-quality-qualification`, `026-live-re-execution`, `027-node-and-wasm-bindings`, `029-evaluation-repeatability`, `031-public-api-and-release-family`, `033-provenance-rules-and-actionable-outcomes`, `034-parameterized-definitions`, `035-screening-seam`, `036-mid-plan-acquisition`, `037-generation-steps`, `038-structured-extraction`, `039-model-backed-rank`, `040-decision-explanation`, `041-ranking-and-rubric-metrics`, `043-live-re-execution-at-scale`, `044-runtime-optimization-increment-2`, `047-target-independent-transcendentals`, `050-wasm-without-parameterized-definitions`, `061-host-scored-repeat-sets`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity`, `066-segment-ancestry-predicate` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:004-evaluation-and-replay` | `sha256:11889f02a0e5c29cc81e5825d941c342aede990578dd697e217c05a6f5f82cff` |
| `registry:004-evaluation-and-replay` | `sha256:11889f02a0e5c29cc81e5825d941c342aede990578dd697e217c05a6f5f82cff` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

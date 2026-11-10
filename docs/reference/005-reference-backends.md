<!-- spec-spine:begin generated id="header" -->
# `005-reference-backends` Deterministic rules backend

- Path: `specs/005-reference-backends/spec.md`
- Summary: Increment 2: `rustev-backend-rules`, a deterministic, in-process backend that implements the backend seam of spec 003 by evaluating a bounded, versioned rules program (`rustev.rules/1`): typed field access into the canonical projection, ordered rules with explicit precedence and no-match behavior, constant, linear and lookup contributions to authored logits in exact fixed-point arithmetic, and a descriptor derived from the program so the program's digest is the artifact identity plans bind. Metered logical cost units, bounded work, local cancellation observed at declared points. Narrowed by owner decision R-13; the semantic backend is spec 011.
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
| `establishes` | None declared. | `{"kind":"directory","path":"backends/rustev-backend-rules/"}` | no | `resolved` |
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
cargo test -p rustev-backend-rules --locked
cargo run -p rustev-boundaries --locked --quiet
sh -c 't=$(cargo tree -p rustev-backend-rules -e normal --prefix none --locked) || exit 1; if printf "%s\n" "$t" | sed "s/ .*//" | grep -E "^(rustev-|tokio$)" | grep -vxE "rustev-(contract|core|backend-rules)" | grep -q .; then exit 1; fi'
sh -c 'if grep -rnE "std::(fs|net|env|process|thread|time)|tokio" backends/rustev-backend-rules/src; then exit 1; fi'
cargo clippy -p rustev-backend-rules --all-targets --locked -- -D warnings
cargo fmt -p rustev-backend-rules --check
sh backends/rustev-backend-rules/mutation/seeds.sh
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
| amended by | `039-model-backed-rank` |
| superseded by | None declared. |
| extended by | `013-remote-state-on-failed-attempts`, `021-packaging-and-first-release`, `031-public-api-and-release-family`, `035-screening-seam`, `037-generation-steps`, `039-model-backed-rank`, `050-wasm-without-parameterized-definitions` |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:005-reference-backends` | `sha256:9f68468193da5f6969ea72e3dfb6c72b42a66911bd3005d426e5c66df172eef5` |
| `registry:005-reference-backends` | `sha256:9f68468193da5f6969ea72e3dfb6c72b42a66911bd3005d426e5c66df172eef5` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

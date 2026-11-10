<!-- spec-spine:begin generated id="header" -->
# `047-target-independent-transcendentals` One implementation of exp and ln on every target (amends 002 and 027)

- Path: `specs/047-target-independent-transcendentals/spec.md`
- Summary: Approved (A-44). The remedy spec 027 3.11.5 names for a cross-target floating-point divergence. The pure crates' four transcendental call sites (`exp` in core's softmax, `ln` in core's calibration, `ln` in evaluation's log loss and temperature fit) call the platform's math library on native targets and Rust's bundled one on WebAssembly; measured on 2026-10-21 they differ by one unit in the last place on 0.08% to 7.1% of a million inputs per input domain (macOS arm64 against `wasm32-unknown-unknown`). This spec routes all four through one pure Rust implementation, the `libm` crate exact-pinned at 0.2.16 without default features, exposed as `rustev_core::math`, so each produces the same bits on every target; no existing golden changes. Ratified by the owner on 2026-10-21 (A-44). Approval does not authorize a release.
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
| created | `2026-10-21` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `047-target-independent-transcendentals#I-1` | `invariant` | Changing the implementation's pin is a reviewed change that runs spec 027's differential corpus and every golden on every target; it never happens through an unpinned update. | `3-one-implementation` | no |
| `047-target-independent-transcendentals#R-1` | `requirement` | Every exp and ln reachable from a pure command is computed by rustev_core::math, one pure Rust implementation (libm =0.2.16, default features off) on every target; no pure crate calls f64::exp, f64::ln or another platform math function, and a test fails when one does. | `3-one-implementation` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.lock"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Cargo.toml"}` | no | `resolved` |

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

No declared acceptance block.
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
| amends | `002-decision-contract-and-pure-core`, `027-node-and-wasm-bindings` |
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
| `index:047-target-independent-transcendentals` | `sha256:aa2e61fe62d4de2ae35c996f21872faa398543e04eb75ad8e40c93d25539c728` |
| `registry:047-target-independent-transcendentals` | `sha256:aa2e61fe62d4de2ae35c996f21872faa398543e04eb75ad8e40c93d25539c728` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

<!-- spec-spine:begin generated id="header" -->
# `029-evaluation-repeatability` Evaluation repeatability, baselines, diffs and selection gates (amends 004 and 006)

- Path: `specs/029-evaluation-repeatability/spec.md`
- Summary: An amendment of approved specs 004 and 006 (R-16), for #76 child E2. Evaluation runs each configuration N times from a declared repeat source (deterministic replay, captured historical run sets, or live runs once spec 026 is implemented), reports per metric and per slice the mean, standard deviation, minimum, maximum, a pooled estimate and a case-clustered percentile bootstrap interval computed with a named, versioned, seeded generator, and derives declared noise bands. Adds named baselines (`rustev.baseline/1`), a reproducibility check, diffs in JSON and Markdown, multi-candidate comparison, held-out accounting against a host-kept ledger file, and a declarative selection gate (`rustev.selection-gate/1`) with stated CI exit codes. Every existing document, gate and CLI output keeps its bytes. Answers spec 023's open question 3. Approved (A-25); approval authorizes no live run, provider call or spend, no quality claim, and no change to `rustev.eval-report/1` or any plan identity.
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
| created | `2026-10-16` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `029-evaluation-repeatability#I-1` | `invariant` | Without a repeat configuration, every existing evaluation, gate and CLI invocation produces byte-identical documents, stdout and exit codes, and no plan identity changes. | `3-15-compatibility` | no |
| `029-evaluation-repeatability#I-2` | `invariant` | Bootstrap intervals and per-run seeds are fully determined by the declared configuration, the case measures and the seed, through the named generator and fixed arithmetic order, and are bit-identical across platforms for identical case measures. | `3-4-bootstrap-intervals` | no |
| `029-evaluation-repeatability#I-3` | `invariant` | A noise band derived from fewer than two independent runs is unknown, never zero, except for deterministic replay; an unknown band, statistic or measurement makes every clause that needs it unknown, never a pass. | `3-5-noise-bands` | no |
| `029-evaluation-repeatability#I-4` | `invariant` | A held-out evaluation states its count only from a host-supplied ledger or an explicit new-ledger statement; a missing count is unknown, never zero, and Rustev keeps no store. | `3-7-split-roles-and-held-out-discipline` | no |
| `029-evaluation-repeatability#R-1` | `requirement` | Latency and cost observed by a historical run are never presented as a candidate's own; a selection clause over them for an offline candidate is unknown. | `3-11-multi-candidate-comparison` | no |
| `029-evaluation-repeatability#R-2` | `requirement` | A selection gate yields per candidate exactly one of qualifies, rejected with its failing clause, or unknown with its clause, and the command's exit code is 0, 11 or 12 as section 3.14 states. | `3-12-selection-gates` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"tools/rustev-conformance/"}` | no | `resolved` |
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
python3 -I crates/rustev-eval/tests/data/bootstrap/reference.py --check
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
cargo clippy -p rustev-eval -p rustev-cli --all-targets --locked -- -D warnings
sh crates/rustev-eval/mutation/seeds.sh "spec 029"
sh crates/rustev-cli/mutation/seeds.sh repeat029
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
| amends | `004-evaluation-and-replay`, `006-cli-surface` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `041-ranking-and-rubric-metrics`, `061-host-scored-repeat-sets`, `063-node-binding-parity`, `064-selection-gate-case-regressions`, `065-selection-gate-run-integrity` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:029-evaluation-repeatability` | `sha256:0d43c76a7f0728bd6cc09f3f1eee38d95db37a3408740a5e9c2a717bc651f1a4` |
| `registry:029-evaluation-repeatability` | `sha256:0d43c76a7f0728bd6cc09f3f1eee38d95db37a3408740a5e9c2a717bc651f1a4` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

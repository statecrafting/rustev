<!-- spec-spine:begin generated id="header" -->
# `034-parameterized-definitions` Parameterized definitions, variants and tenant profiles (amends 002 and 006)

- Path: `specs/034-parameterized-definitions/spec.md`
- Summary: An amendment of approved specs 002 and 006 (R-16), for #76 child B4. A definition may declare, as optional members this spec adds to the #76 track version `rustev.definition/2` (introduced by spec 032), typed, bounded parameters with lock levels (`fixed`, `bounded`, `free`), reference them from a closed set of literal positions, declare named variants and scope inputs, steps, handlers, rules and adjustments to them. A versioned profile document (`rustev.profile/1`) binds parameter values, may extend one other profile, and names its definition by a version range. Instantiation, a pure phase before compilation, resolves the range against the supplied definitions, resolves the overlay chain, checks every lock and bound, substitutes values and prunes the variant, yielding a definition without parameterization that the compiler compiles into a `rustev.plan/3` (the #76 track version, introduced by spec 032, to which this spec adds the optional member `instantiation`) whose `PlanId` binds the template, the variant, the profile chain and every resolved value. Adds `rustev plan diff` (`rustev.plan-diff/1`) and `rustev profile check`. A `rustev.definition/1` compiled without a profile still yields a byte-identical `rustev.plan/2` and `PlanId`. Approved (A-31); implementation is complete (see the implementation record).
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
| `034-parameterized-definitions#I-1` | `invariant` | A `rustev.definition/1` compiled without a profile and without a variant yields a `rustev.plan/2` byte-identical to today's, with the same `PlanId`, and every existing CLI output for it keeps its bytes. | `3-16-compatibility` | no |
| `034-parameterized-definitions#I-2` | `invariant` | An instantiated `rustev.plan/3` records the exact template document and its identity, the variant, the identity of every profile in the resolved chain and every resolved parameter value with its source, so its `PlanId` never depends on a version range, a pin or which other definitions were supplied. | `3-10-plan-version-3-and-identity` | no |
| `034-parameterized-definitions#I-3` | `invariant` | A profile that sets a `fixed` parameter, sets a `bounded` parameter outside its bounds, sets an undeclared parameter or sets an ill-typed value is refused with the parameter named; no plan is produced. | `3-7-overlay-resolution` | no |
| `034-parameterized-definitions#I-4` | `invariant` | A profile carries values for declared parameters only: it cannot add or remove an input, step, operator, expression, handler, rule, adjustment or action name, and no parameter value is interpreted as code, an expression or a reference. | `3-5-profiles` | no |
| `034-parameterized-definitions#I-5` | `invariant` | Rustev never selects a profile or variant from a principal, tenant handle, scope or request content; the host names both for every compilation. | `3-15-library-surface` | no |
| `034-parameterized-definitions#R-1` | `requirement` | A profile's definition range resolves to the highest supplied definition version that satisfies it, or to the host's pin when the pin satisfies it; an unsatisfiable range or a pin outside the range is refused, listing the supplied versions. | `3-6-definition-version-binding` | no |
| `034-parameterized-definitions#R-2` | `requirement` | `rustev.plan-diff/1` is a deterministic function of the two plan documents, and `rustev plan diff` output is golden-tested. | `3-12-plan-diff` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"crates/rustev-surface/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
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
cargo test -p rustev-contract --locked --test parameterized_wire
cargo test -p rustev-core --locked --features parameterized --test instantiate_analysis --test instantiate_compile --test plan_diff --test profile_check
cargo test -p rustev-eval --locked --features parameterized --test instantiated_replay
cargo test -p rustev-cli --locked --test parameterized
cargo test -p rustev-core --locked
cargo test -p rustev-cli --locked
sh crates/rustev-core/mutation/seeds.sh "a lock not checked on a base profile" "an inclusive bound made exclusive" "highest version replaced by first supplied" "the pin entered into identity" "a dangling reference pruned silently" "a handler kept for a pruned target" "a value source dropped from the plan"
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
| amends | `002-decision-contract-and-pure-core`, `006-cli-surface` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `063-node-binding-parity` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:034-parameterized-definitions` | `sha256:1aba8424a419c21943b450bdf0d6ed1aff2aef5795b7d5f9e59a5d8fec019833` |
| `registry:034-parameterized-definitions` | `sha256:1aba8424a419c21943b450bdf0d6ed1aff2aef5795b7d5f9e59a5d8fec019833` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

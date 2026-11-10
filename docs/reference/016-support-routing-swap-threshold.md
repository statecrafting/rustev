<!-- spec-spine:begin generated id="header" -->
# `016-support-routing-swap-threshold` Declared uncalibrated threshold in support-routing swap test (amends 007)

- Path: `specs/016-support-routing-swap-threshold/spec.md`
- Summary: A proposed amendment of approved spec 007 (R-16). Spec 007 3.4.3 and section 5 require the backend swap test to assert identical compiled plans' policy sections across the rules backend and Jev backend. Under owner decision R-31 (approving Q-1), Jev returns an uncalibrated distribution and declares `uncalibrated_threshold` on `topic`'s `top_mass_below` condition with a margin rule. The reference definition keeps `uncalibrated_threshold: not_declared` to match the golden document byte-for-byte (3.1.3). Because `uncalibrated_threshold` is part of the condition inside the policy section, the compiled plans' policy sections cannot be strictly identical under equality. This amendment amends 3.4.3 and section 5 so that the swap test asserts identical policy rules, conditions, thresholds, adjustments and output declarations, accounting for Jev's declared uncalibrated threshold on `topic`. It also resolves two further conflicts found while preparing 007's delivery: where the swap test can live given spec 001 3.4.4, and how priority can be a separately labeled field given the declarative task adapter's one-label-per-case shape (spec 006, 3.8). Approved (A-13, 2026-09-26) with both open questions settled as recommended (R-33); implementation complete, delivered with spec 007.
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
| created | `2026-09-25` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

None declared.
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

None declared.

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
| amends | `007-support-routing-package` |
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
| `index:016-support-routing-swap-threshold` | `sha256:715e8e99fffe363ca90d98d6408e85daa7e058308886fabff513861192c457a3` |
| `registry:016-support-routing-swap-threshold` | `sha256:715e8e99fffe363ca90d98d6408e85daa7e058308886fabff513861192c457a3` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

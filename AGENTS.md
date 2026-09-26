@.statecraft/AGENTS.md

# Rustev: repository instructions

Rustev is an embeddable Rust decision engine: versioned decision definitions
compiled into plans that combine exact computation, semantic backends behind
capability contracts, and explicit abstention. Start with
`docs/design/001-decision-engine-architecture.md` (proposed design) and
`docs/decisions/00-founding-decisions.md` (what is adopted versus recommended).
`docs/design/discussions/` is rationale, never contract.

## Governance

- spec-spine is pinned in `spec-spine.toml` (`required_version`). Install it
  with `make tools`; it lands in `.tooling/bin` and the Makefile prefers it.
  A different version on `PATH` proves nothing.
- `make gate` is read-only and must pass before every commit. `make refresh`
  regenerates `.statecraft/derived/`; commit the shards with the spec edit that
  staled them. Never run `refresh` just to make `gate` pass.
- A behavior change starts with a spec change. `draft` specs are proposals
  and are never claims about code; ratification (`approved`) is the owner's act.
- One owning spec per crate. Ordinals are build order.

## Boundaries that bind every change

Normative text: spec `001` and constitution principles VI onward.

- Proposals, never grants: no Rustev crate defines or produces a value an
  action executor accepts as authorization, and none defines a `Permitted`
  type. Excluding `Judgment` from a signature is a restriction, not proof
  against laundering; derivation classes are the mitigation.
- Every value carries a derivation class (`exact-derived`, `model-derived`,
  `mixed-derived`) and its lineage.
- Value kinds (scores, distributions, calibrated probabilities, labels,
  ordinal levels, ranks) never convert silently; missing, stale, conflicting
  or invalid evidence is `Unresolved`, never a default.
- Only `integrations/*` may depend on Aicortex, Rahi, statecraft-cli or an
  HTTP stack; `rustev-contract` and `rustev-core` also take no async runtime.
  `make boundaries` checks this.
- `rustev-core` performs no I/O, reads no clock, and forbids `unsafe`.

## Code checks

`make code` runs build, test, clippy (`-D warnings`), fmt and the boundary
check across the workspace. `spec-spine verify <id>` runs a spec's declared
acceptance.

## Continuous integration

CI is the Statecraft setup profile `github-actions-rust` (revision 7): one
required check, `ci-gate`, over governance, code, the AI review and the
declared extra jobs `boundaries` and `verify`
(`.github/workflows/boundaries.yml`, `.github/workflows/verify.yml`, owned by
spec 001). The managed files (`.github/workflows/statecraft-*.yml`,
`scripts/statecraft/*`, `.statecraft/setup/*`, `.github/CODEOWNERS`) change
only by editing `project.setup.parameters` in `.statecraft/environment.json`
and re-rendering with `statecraft-cli init plan|apply`, never by hand. Any
change under `.github/workflows/` or `scripts/statecraft/`, and to
`scripts/check-authored-content.sh`, needs the owner's approval of the
`statecraft-review-exception` Environment once.

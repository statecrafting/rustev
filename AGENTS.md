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

- Judgment is not authority: no Rustev crate defines or produces a value an
  action executor accepts as authorization, and no authority function takes a
  judgment as input.
- Value kinds (scores, distributions, calibrated probabilities, ordinal levels,
  ranks) never convert silently; missing or invalid evidence is `Unresolved`,
  never a default.
- Only `integrations/*` may depend on Aicortex, Rahi or an HTTP stack.
- `rustev-core` performs no I/O, reads no clock, and forbids `unsafe`.

## Code checks

`make code` runs build, test, clippy (`-D warnings`) and fmt across the
workspace once a crate exists.

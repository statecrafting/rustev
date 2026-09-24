# rustev

An embeddable Rust engine for making, evaluating and recording decisions over
evolving, partially trusted information. A versioned decision definition is
compiled into a plan: exact computation in Rust, semantic questions answered by
interchangeable backends that disclose what they can return, explicit
abstention, bounded execution, and an evidence record for every decision.

Rustev produces proposals, never grants. It does not authorize, own knowledge,
or execute effects; those belong to the application and to neighbouring
components (Aicortex, Rahi, statecraft-cli), each optional. Rustev is usable
without any of them.

## Status

Increment 1 is implemented: `rustev-contract` (versioned documents, bounded
parsing, canonical identities) and `rustev-core` (value kinds, exact
operators, the plan compiler with typed refusals, staged evaluation and
selection policy). The first runtime slice is implemented in
`rustev-runtime`: bounded admission and concurrency, one end-to-end deadline
on an injectable clock, cost budgets reserved before dispatch, declared
retries and runtime fallback that are part of the plan's identity, honest
cancellation, and run records delivered under a declared sink policy.
`rustev-backend-rules` is a deterministic rules backend: authored,
identified rules programs, not a model. `rustev-eval` reproduces retained
decisions offline from bounded replay bundles, compares candidate plans,
and produces evaluation reports, regression gates and temperature fits.
`rustev` (`crates/rustev-cli`) does all of this from files: plan check,
compile and show; rules-backed runs with bounded capture into replay
bundles; offline replay; evaluation reports, gates and temperature fits.
There is no model backend yet, and no quality or calibration claim: every
dataset and report is synthetic. Batching, duplicate suppression and
caches are deferred. Nothing is released.

- Design (proposed): [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specs: `001` boundaries and authority, `002` decision contract and pure core, `003` runtime execution and evidence, `004` evaluation and replay, `005` rules backend, `006` CLI (approved and complete); `011` semantic backend (draft).

## Governance

Governed by [spec-spine](https://github.com/statecrafting/spec-spine), pinned
in `spec-spine.toml`, and initialized with statecraft-cli.

```sh
make tools   # install the pinned spec-spine into .tooling/bin
make gate    # read-only corpus gate, including ownership coverage
make code    # build, test, clippy, fmt, boundary check
make verify  # run each approved spec's declared acceptance
```

Apache-2.0.

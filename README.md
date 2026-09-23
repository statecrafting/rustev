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
selection policy). There is no runtime, backend, CLI or model yet, and no
quality or calibration claim: the reference plans run on synthetic fixtures.
Nothing is released.

- Design (proposed): [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specs: `001` boundaries and authority, `002` decision contract and pure core (both approved and complete).

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

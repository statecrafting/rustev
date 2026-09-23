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

Spec `001` (boundaries and authority) is approved. Nothing is released.

- Design (proposed): [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specs: `001` boundaries and authority (approved), `002` decision contract and pure core.

## Governance

Governed by [spec-spine](https://github.com/statecrafting/spec-spine), pinned
in `spec-spine.toml`, and initialized with statecraft-cli.

```sh
make tools   # install the pinned spec-spine into .tooling/bin
make gate    # read-only corpus gate
```

Apache-2.0.

# rustev

An embeddable Rust engine for making, evaluating and recording decisions over
evolving, partially trusted information. A versioned decision definition is
compiled into a plan: exact computation in Rust, semantic questions answered by
interchangeable backends that disclose what they can return, explicit
abstention, bounded execution, and an evidence record for every decision.

Rustev produces judgments. It does not grant authority, own knowledge, or
execute effects; those belong to the application and to neighbouring
components (Aicortex, Rahi, statecraft-cli), each optional.

## Status

Specified in draft, not implemented. Nothing is released.

- Design (proposed): [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specs: `001` boundaries and authority, `002` decision contract and pure core, both `draft`.

## Governance

Governed by [spec-spine](https://github.com/statecrafting/spec-spine), pinned
in `spec-spine.toml`, and initialized with statecraft-cli.

```sh
make tools   # install the pinned spec-spine into .tooling/bin
make gate    # read-only corpus gate
```

Apache-2.0.

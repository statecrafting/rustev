# rustev

An embeddable Rust engine for making, evaluating and recording decisions over
evolving, partially trusted information.

You write a versioned **decision definition**. Rustev compiles it into an
identified **plan** that combines exact computation in Rust, semantic
questions answered by interchangeable backends (rules, local models, hosted
models) behind capability contracts, and explicit abstention. Every run is
bounded in time and cost and leaves an evidence record you can replay,
evaluate and explain later.

Rustev produces proposals, never grants. It does not authorize, own
knowledge or execute effects; those belong to your application.

## What Rustev guarantees

- **Proposals, not permissions.** An outcome is a proposal, an escalation or
  an unresolved result. No Rustev type is an authorization an executor
  should accept.
- **Abstention is explicit.** Missing, stale, conflicting or invalid
  evidence yields `Unresolved` with a reason, never a silent default.
- **Every value says where it came from.** Each value carries a derivation
  class (`exact-derived`, `model-derived`, `mixed-derived`) and its lineage.
- **Value kinds never convert silently.** Scores, distributions, calibrated
  probabilities, labels, ordinal levels and ranks stay distinct.
- **Identities are content hashes.** Definitions, plans, snapshots and
  records are canonical documents; the same inputs give the same `plan_id`
  and the same judgment.
- **Bounded execution.** One end-to-end deadline, cost budgets reserved
  before dispatch, declared retries and fallbacks that are part of the
  plan's identity, and honest cancellation.
- **A pure core.** `rustev-core` performs no I/O, reads no clock and forbids
  `unsafe`.

## Install

Rustev ships as one release family: every Rust crate, the CLI binaries, the
npm packages and the service image share a version. See
[Releases](https://github.com/statecrafting/rustev/releases) for the current
one.

**Rust library.** Depend on the `rustev` facade. It has no default features;
with none, you get the contract and the pure core only.

```sh
cargo add rustev --features runtime,rules
```

| Feature | Module | Adds |
| --- | --- | --- |
| (none) | `contract`, `decision_core` | Documents, parsing, identities, compiler, exact evaluation |
| `runtime` | `runtime` | Bounded execution through backend seams, run records |
| `rules` | `rules` | The deterministic rules backend |
| `eval` | `eval` | Replay bundles, offline reproduction, evaluation and gates |
| `remote-http` | `remote_http` | The `rustev.remote/1` remote backend protocol over HTTP |
| `otel` | `otel` | An OpenTelemetry evidence sink with redaction |

No feature implies another.

**Command line.**

```sh
cargo install rustev-cli   # installs the `rustev` binary
```

Prebuilt binaries for Linux (gnu and musl, x64 and arm64), macOS and Windows
are attached to each GitHub release with checksums, SBOMs and build
attestations.

**Node.js and WebAssembly.**

```sh
npm install @rustev/rustev        # Node-API binding, prebuilt per platform
npm install @rustev/rustev-wasm   # pure commands as a WebAssembly module
```

npm packages are published from CI with provenance.

**Decision service.** `ghcr.io/statecrafting/rustev-serve` serves
host-registered, signed plans over HTTP/1.1, HTTP/2 and gRPC. See
[integrations/rustev-serve](integrations/rustev-serve/README.md).

## Quick start

The lodging reference package and the rules backend's fixtures make a complete
decision. From a clone, with the toolchain from `rust-toolchain.toml`:

```sh
cargo build -p rustev-cli --locked
alias rustev=target/debug/rustev   # or the installed binary
DATA=packages/rustev-pkg-lodging/data
RULES=backends/rustev-backend-rules/tests/fixtures/lodging.rules.json

# Validate and compile the definition into an identified plan.
rustev plan check   --definition $DATA/lodging-recommendation.definition.json --rules $RULES
rustev plan compile --definition $DATA/lodging-recommendation.definition.json --rules $RULES \
                    --out lodging.plan.json

# Decide over a snapshot of evidence, at a given evaluation time.
rustev run --plan lodging.plan.json --rules $RULES \
           --snapshot $DATA/snapshots/lg-c01.json \
           --evaluation-time 1790164740000 \
           --decision-id demo-1 --record-out demo-1.record.json
```

Each command prints one JSON object and exits with its status. The run above
is judged with a `propose` outcome (`present_ranked_lodging`) and writes a
run record whose digest is the receipt. Run it again a few months later
(`--evaluation-time 1800000000000`) and the same plan answers `unresolved`
with `stale_evidence` on `fx.rates` and `inventory.candidates`: the evidence
has aged out, so Rustev abstains instead of guessing.

From there:

- `rustev explain` answers why an item was selected, excluded or ordered,
  from a record or a replay bundle.
- `rustev run --capture-bytes ... --bundle-out ...` captures a bounded replay
  bundle; `rustev replay` reproduces the decision offline.
- `rustev eval`, `rustev gate` and `rustev calibrate` compare candidate plans
  on a dataset, enforce selection gates and fit calibrations.

`rustev --help` lists every command.

## Concepts

| Term | Meaning |
| --- | --- |
| Definition | The authored, versioned decision: inputs, steps, policies, outcomes. |
| Plan | A definition compiled against its backends and calibrations; identified by `plan_id`. |
| Backend | Answers semantic steps (classify, rank, extract, generate) behind a declared capability descriptor. |
| Snapshot | The evidence a decision reads, each field with its freshness. |
| Judgment | The outcome with its derivation class, lineage and trace. |
| Run record | The delivered evidence of one run; its digest is the receipt. |
| Replay bundle | A bounded capture that reproduces a decision offline. |

## Repository layout

| Path | Contents |
| --- | --- |
| `crates/` | Contract, pure core, runtime, evaluation, live re-execution, the command surface and the CLI |
| `backends/` | The rules backend and the local semantic backend (frozen embeddings and a linear head) |
| `integrations/` | The `rustev` facade, remote HTTP protocol, OpenTelemetry sink, Node and WASM bindings, decision service, and adapters for Bedrock, Jev, Aicortex and Rahi |
| `packages/` | Reference decision packages: support routing and lodging |
| `tools/` | Boundary checker, cross-host conformance suite, release tooling |
| `specs/` | The governed specifications, one owning spec per crate |
| `docs/` | Architecture design, adopted decisions, backlog |

Only `integrations/*` may depend on an HTTP stack or a neighbouring system,
and `rustev-contract` and `rustev-core` take no async runtime; `make
boundaries` enforces this.

## Limits

- **No quality claim.** Datasets, calibrations and reports in this
  repository are synthetic or of synthetic provenance. Real labeled data is
  admitted only under spec `023`.
- **Not every crate is published.** The local semantic backend and the
  Bedrock, Jev, Aicortex and Rahi adapters are in the repository but not on
  crates.io.
- **Pre-1.0.** The public API follows the compatibility policy of spec `031`;
  read the release notes before upgrading across a minor version.

## Documentation

- Architecture (proposed design):
  [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Adopted decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specifications: [`specs/`](specs/). The specs are the contract;
  `spec-spine registry list` prints each one's status.
- Unscheduled and research work: [docs/backlog.md](docs/backlog.md)
- API docs: [docs.rs/rustev](https://docs.rs/rustev); Node typings in
  [integrations/rustev-node/index.d.ts](integrations/rustev-node/index.d.ts)
- Security reports: [SECURITY.md](SECURITY.md)

## Development

The repository is governed by
[spec-spine](https://github.com/statecrafting/spec-spine), pinned in
`spec-spine.toml`. A behavior change starts with a spec change; draft specs
are proposals and never claims about code.

```sh
make tools     # install the pinned spec-spine into .bin
make gate      # read-only governance gate; must pass before every commit
make code      # build, test, clippy (-D warnings), fmt, boundary check
make verify    # run each approved spec's declared acceptance
make refresh   # regenerate .statecraft/derived after a spec edit
```

Commit regenerated `.statecraft/derived/` shards with the edit that staled
them. Contributor and agent rules are in [AGENTS.md](AGENTS.md).

## Releases

A release is one signed tag on `main`. CI publishes the crates, the npm
packages (with provenance), the CLI binaries and the service image, then a
fresh consumer qualifies every artifact against the public registries. Each
GitHub release carries the signed release manifest, changelog, SBOMs and
in-toto attestations.

## License

Apache-2.0. See [LICENSE](LICENSE).

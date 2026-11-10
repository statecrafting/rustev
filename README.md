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

This runs a lodging recommendation from the reference package, with the
`rustev` binary from [Install](#install). It needs only `curl`; the three
files come from the release tag of the version you installed.

```sh
REF=https://raw.githubusercontent.com/statecrafting/rustev/v0.7.0
curl -fsSLO $REF/packages/rustev-pkg-lodging/data/lodging-recommendation.definition.json
curl -fsSLO $REF/backends/rustev-backend-rules/tests/fixtures/lodging.rules.json
curl -fsSLO $REF/packages/rustev-pkg-lodging/data/snapshots/lg-c01.json

# Compile the definition, with its rules, into an identified plan.
rustev plan compile --definition lodging-recommendation.definition.json \
  --rules lodging.rules.json --out lodging.plan.json

# Decide over a snapshot of evidence, at a given evaluation time.
rustev run --plan lodging.plan.json --rules lodging.rules.json \
  --snapshot lg-c01.json --evaluation-time 1790164740000 \
  --decision-id demo-1 --record-out demo-1.record.json
```

`plan compile` prints the plan's identity:

```json
{"command":"plan compile","out":"lodging.plan.json","plan_id":"sha256:369bc95a561aed48d0e42fdfa5c4ae1bf0662bed2be738075bbe8bec177a1036","status":"compiled"}
```

`run` prints the judgment (abridged here) and exits 0:

```json
{
  "command": "run",
  "completion": "judged",
  "decision_id": "demo-1",
  "receipt": "file:sha256:…",
  "judgment": {
    "derivation": "mixed-derived",
    "outcome": { "propose": { "action": "present_ranked_lodging", "params": {
      "eligibility": { "filtered": { "eligible": ["c-quiet", "c-budget", "c-euro"],
        "excluded": [ { "id": "c-small", "reasons": ["occupancy"] },
                      { "id": "c-dear",  "reasons": ["within_budget"] } ] } },
      "ranking": { "ranking": { "entries": [
        { "position": 1, "candidate": "c-quiet",  "score": 0.588… },
        { "position": 2, "candidate": "c-budget", "score": 0.395… },
        { "position": 3, "candidate": "c-euro",   "score": 0.353… } ] } } } } },
    "lineage": { "inputs": ["fx.rates", "inventory.candidates", "trip.budget", "…"],
                 "steps": ["eligible", "ranking", "shortlist", "…"] }
  }
}
```

The outcome is a proposal: three eligible places, ranked, with the two
excluded ones and why. `derivation` is `mixed-derived`: the outcome combines
exact computation with answers from a semantic backend (here the rules
backend standing in for a model). `lineage` lists every input and step the
outcome depends on. The run record
written to `demo-1.record.json` is the evidence; `receipt` is its digest.

Now decide the same request later, after the exchange rates and inventory in
the snapshot have aged out:

```sh
rustev run --plan lodging.plan.json --rules lodging.rules.json \
  --snapshot lg-c01.json --evaluation-time 1800000000000 \
  --decision-id demo-2 --record-out demo-2.record.json
```

```json
{"completion":"judged","judgment":{"outcome":{"unresolved":{"stale_evidence":{"fields":["fx.rates","inventory.candidates"]}}}, "…": "…"}}
```

Same plan, same evidence, a later time: Rustev abstains and names the stale
fields instead of guessing. It still exits 0, because the decision was made.

From there:

- `rustev explain` answers why an item was selected, excluded or ordered,
  from a record or a replay bundle.
- `rustev run --capture-bytes ... --bundle-out ...` captures a bounded replay
  bundle; `rustev replay` reproduces the decision offline.
- `rustev eval`, `rustev gate` and `rustev calibrate` compare candidate plans
  on a dataset, enforce selection gates and fit calibrations.

`rustev --help` lists every command.

## Command line essentials

Every command prints one JSON object on stdout, with a `status` (or
`completion`) field, and exits with a code that scripts can rely on:

| Exit | Status | Meaning |
| --- | --- | --- |
| 0 | `ok`, `compiled`, `judged`, `reported`, `pass`, `selected`, … | The command did what it states. A judged escalation or unresolved outcome is still 0. |
| 2 | (stderr) | Usage error. |
| 3 | `io_error` | A file could not be read or written, or an output path already exists. Names the path. |
| 4 | `invalid_input` | An input document was refused, naming the file and the reason. |
| 5 | `refused` | The plan cannot be used: the compiler or the runtime refused it, with a category. |
| 6 | `rejected` | The decision was not admitted; nothing ran. |
| 7 | `cancelled` | The decision was cancelled; a run record was still written. |
| 8 | `evidence_not_delivered` | The run record could not be written; it is embedded in stdout instead. |
| 9 | `diverged` | A replay produced a different judgment. |
| 10 | `incomparable` | A replay could not be compared, with the reason. |
| 11 | `fail`, `none_qualified`, … | A gate failed, or no candidate qualified. |
| 12 | `unknown` | A gate, qualification or selection is unknown. Unknown is never a pass. |
| 13 | `not_reproducible` | An evaluation did not reproduce its baseline. |
| 14–16 | `spend_capped`, `probe_failed`, `interrupted` | Live re-execution stopped (`rustev eval live`). |

Files the CLI writes are canonical JSON, never overwritten (an existing
path is `io_error`), and output directories must already exist:

| File | Written by | Holds |
| --- | --- | --- |
| Plan | `plan compile --out` | The compiled plan; its digest is the `plan_id`. |
| Run record | `run --record-out` | One decision: inputs used, backend requests, timing, cost and the judgment. |
| Replay bundle | `run --bundle-out` | A bounded capture of a run that `replay` and `eval` reproduce offline. |
| Evaluation directory | `eval --out` | The report, per-case detail, and with repeats, per-run reports and a repeat report. |
| Baseline | `baseline record --out` | A frozen evaluation that later runs and candidates are compared with. |
| Selection | `gate select --out` | Each candidate's verdict with every rule result, and the ranking. |

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
  repository are synthetic or of synthetic provenance, so nothing here says
  how well a plan decides on real traffic.
- **Not every crate is published.** The local semantic backend and the
  Bedrock, Jev, Aicortex and Rahi adapters are in the repository but not on
  crates.io.
- **Pre-1.0.** A minor version may change the public API; read the
  changelog attached to each release before upgrading.

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
packages, the CLI binaries and the service image, then a fresh consumer
qualifies every artifact against the public registries. Each GitHub release
carries the signed release manifest, changelog, SBOMs (`*.cdx.json`),
checksums (`*.sha256`) and build provenance.

To verify a CLI binary you downloaded:

```sh
V=0.7.0 T=linux-x64-gnu   # your version and platform
gh release download v$V -R statecrafting/rustev -p "rustev-$V-$T.tar.gz*"
sha256sum -c rustev-$V-$T.tar.gz.sha256            # shasum -a 256 -c on macOS
gh attestation verify rustev-$V-$T.tar.gz --repo statecrafting/rustev
```

The last command checks the SLSA provenance: the archive was built by this
repository's release workflow at the `v$V` tag. For the npm packages,
`npm audit signatures` in your project verifies their registry signatures
and provenance.

## License

Apache-2.0. See [LICENSE](LICENSE).

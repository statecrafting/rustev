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

Implemented, under approved and complete specs:

- `rustev-contract` and `rustev-core` (`002`): versioned documents, bounded
  parsing, canonical identities, value kinds, exact operators, the plan
  compiler with typed refusals, staged evaluation and selection policy.
- `rustev-runtime` (`003`, amended by `013`): bounded admission and
  concurrency, one end-to-end deadline on an injectable clock, cost budgets
  reserved before dispatch, declared retries and fallback that are part of
  the plan's identity, honest cancellation, remote state on attempts that end
  without a stop, and run records delivered under a declared sink policy.
- `rustev-eval` (`004`, amended by `014` and `015`): offline reproduction of
  retained decisions from bounded replay bundles, candidate comparison,
  evaluation reports with task adapter rules bound into their identity,
  per-case reporting of unusable bundles, regression gates and temperature
  fits. Live re-execution is not implemented (spec `026`).
- `rustev-backend-rules` (`005`): a deterministic rules backend; authored,
  identified rules programs, not a model.
- `rustev` (`crates/rustev-cli`, `006`): plan check, compile and show; run;
  replay; eval; gate; calibrate fit and qualify.
- `integrations/rustev-remote-http` (`009`): the versioned remote adapter
  protocol over HTTP, client and server, including adapter-scoped
  shared-state batching that is off by default until qualified.
- `integrations/rustev-jev` (`012`): a remote decision backend over TypeSafe's
  Jev through the Vercel AI Gateway. Its evidence is development evidence: the
  Gateway reports the served model as unknown, and the production transport
  (spec `022`) is approved but not implemented.
- Reference packages `rustev-pkg-support-routing` (`007`, amended by `016`)
  and `rustev-pkg-lodging` (`008`); neither amends a core crate.

Runtime optimization (spec `017`) is in progress: increment 1 (runtime-planned
batching and a bounded memory cache with invalidation, opt-in per plan) is
implemented; increment 2 (concurrent duplicate suppression and persistent
caches) is not. There is no local model backend (spec `011` is a draft), and
no quality or calibration claim: datasets and reports are synthetic or of
synthetic provenance, and real labeled data is admitted only under spec `023`
(R-04, R-37). Nothing is released; every
manifest is `0.1.0` with `publish = false` (spec `021` is approved, not
implemented).

Approved specs not yet implemented: `010` optional integrations and serving,
`019` dynamic questions, `020` remote protocol refinements, `021` packaging
and first release, `022` Jev production transport, `023` real labeled data
qualification, `026` live re-execution. Drafts: `011` first local semantic
backend, `024` second local semantic backend, `025` wire-witness replay input.
`017` is approved and in progress. Every other spec (`000` to `009`, `012` to
`016`, `018`) is approved and complete.

The current roadmap is tracking issue
[#76](https://github.com/statecrafting/rustev/issues/76); it describes planned
work, not current behavior. Remaining unscheduled and research items are in
[docs/backlog.md](docs/backlog.md).

- Design (proposed): [docs/design/001-decision-engine-architecture.md](docs/design/001-decision-engine-architecture.md)
- Decisions: [docs/decisions/00-founding-decisions.md](docs/decisions/00-founding-decisions.md)
- Specs: [`specs/`](specs/); `spec-spine registry list` shows each status.

## Governance

Governed by [spec-spine](https://github.com/statecrafting/spec-spine), pinned
in `spec-spine.toml`, and initialized with statecraft-cli.

```sh
make tools   # install the pinned spec-spine into .bin
make gate    # read-only corpus gate, including ownership coverage
make code    # build, test, clippy, fmt, boundary check
make verify  # run each approved spec's declared acceptance
```

Apache-2.0.

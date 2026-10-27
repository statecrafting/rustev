---
id: "001-boundaries-and-authority"
title: "Ecosystem boundaries, proposals versus authority, and product principles"
status: approved
implementation: complete
created: "2026-09-23"
summary: >
  What Rustev owns and refuses to own within the ecosystem (spec-spine,
  Aicortex, Rahi, statecraft-cli, applications), the rule that Rustev produces
  proposals and never grants, the provenance classes of Rustev outputs, the
  dependency layout that keeps contracts transport-neutral and the core free of
  integrations, and the product principles that are the constitution's
  principles VI onward. Owns the workspace manifest, the Makefile and the
  boundary check that enforces the dependency rules.
establishes:
  - "docs/design/001-decision-engine-architecture.md"
  - { kind: section, file: "standards/spec/constitution.md", anchor: "vi-onward-the-principles-of-the-system-you-are-specifying" }
  - "Cargo.toml"
  - "Cargo.lock"
  - "Makefile"
  - { kind: directory, path: "scripts/acceptance/" }
  - { kind: directory, path: "scripts/landing/" }
  - { kind: directory, path: "tools/rustev-boundaries/" }
depends_on:
  - "000-bootstrap"
---

# 001: Ecosystem boundaries, proposals versus authority, and product principles

Rationale: `docs/design/001-decision-engine-architecture.md` sections 1 to 5,
12 to 15. This spec is the normative subset; the design and the discussion
archive are not.

## 1. Purpose

Fix Rustev's responsibilities and non-responsibilities so every later spec can
be judged against them; fix the rule that makes a decision engine safe to
embed; and make the dependency rules mechanically checked rather than
remembered.

## 2. Territory

- The constitution section `VI onward`, whose principles are section 3.6.
- The design document it normalizes.
- The workspace manifest `Cargo.toml` and its lockfile `Cargo.lock`, the
  `Makefile`, and the boundary check `tools/rustev-boundaries/` (a
  `publish = false` workspace member).
- The delivery tooling `scripts/acceptance/` (the CI acceptance partition) and
  `scripts/landing/` (the read-only landing report). Neither is product code. A spec that adds a crate directory
  extends `Cargo.toml` and `Makefile` with an `extends` edge and claims its
  own directory.

## 3. Behavior

### 3.1 Responsibilities

| Component | Owns |
|---|---|
| spec-spine | Governance semantics and contract identity. |
| Rustev | Decision computation and selection policy: validated inputs, exact computation, semantic steps behind capability contracts, selection policy, bounded execution, evidence emission, evaluation machinery. |
| Aicortex | Attributed knowledge and its lifecycle. |
| Rahi | Optional operational infrastructure and enforcement. |
| statecraft-cli | Development governance and independent acceptance. |
| Applications | Authorization, action execution, and domain-specific policy. |

Rustev does not own a knowledge store, authentication or identity, authority
policy, action execution, arbitrary agent workflows, or hosting. Rustev is
usable without any of the other ecosystem projects.

### 3.2 Proposals, never grants

1. A Rustev output is a proposal or an unresolved outcome. No Rustev crate
   defines an authorization type (`Permitted`, `Grant` or an equivalent) or
   presents any output as authorization. What an application's executor
   accepts is the application's decision (4 below).
2. Before any effect, the application validates the requested action, resource,
   parameters, principal, scope and current revision against its own authority
   policy. Rustev supplies none of these as trusted facts.
3. Model output cannot create or broaden permissions. A judgment may cause the
   application to choose a narrower disposition (act, confirm, escalate,
   refuse) within what its authority policy permits; no judgment value,
   however confident, enlarges it.
4. Limitation, stated rather than hidden: keeping `Judgment` out of an
   authority function's signature is a design restriction on Rustev-supplied
   interfaces. It does not prove that a model-derived value was not copied
   into another input the application treats as a trusted fact. Rustev's
   mitigation is provenance: every input carries a provenance class and every
   step value and judgment a derivation class (3.3), so an application can
   refuse model-derived values as authority inputs. Enforcing that refusal is
   the application's obligation.
5. An optional executor interface, if ever supplied, is generic over an
   authorization type the application owns (`ActionExecutor<A, Auth>`), and
   Rustev never constructs an `Auth`. It is not part of increment 1.

### 3.3 Provenance of Rustev outputs

1. Every step value and every judgment carries a derivation class:
   - `exact-derived`: computed only by exact operators from inputs none of
     whose provenance class is `model-derived`;
   - `model-derived`: produced by a semantic backend;
   - `mixed-derived`: computed from at least one model-derived value and at
     least one exact-derived value, or by an exact operator over a
     model-derived input.
2. Every step value and judgment retains its lineage: the input fields and
   steps it was computed from.
3. A decision Rustev produced is evidence that the system produced that
   decision, under the recorded plan and inputs. It is not independent evidence
   that an underlying preference or factual claim is true, and no Rustev
   component admits it as such.

### 3.4 Dependency layout

1. `rustev-contract` and `rustev-core` depend on no crate of Aicortex, Rahi or
   statecraft-cli, no HTTP client or server stack, and no async runtime
   executor, directly or transitively.
2. Only crates under `integrations/` may depend on Aicortex, Rahi,
   statecraft-cli or an HTTP stack. HTTP clients and servers, including the
   future remote adapter's, live under `integrations/`.
3. Transport-neutral backend contracts (the seam traits in `rustev-core`, the
   remote protocol messages in `rustev-contract`) are separate from any
   network implementation of them.
4. No crate depends on a crate under `integrations/`.
5. `rustev-contract` depends on no other workspace crate; `rustev-core`
   depends on no workspace crate other than `rustev-contract`. An outside
   project reads every Rustev record and report by depending on
   `rustev-contract` alone.

### 3.5 The boundary check

`tools/rustev-boundaries` reads `cargo metadata` for the workspace and exits
non-zero naming each violation of 3.4.1, 3.4.2, 3.4.4 and 3.4.5. Rule 3.4.3
(contracts kept apart from network code) is not visible in dependency metadata
beyond 3.4.2 and is held in review. `make boundaries` runs it and `make
code` includes it. Its rules are unit-tested against synthetic metadata,
including one violating case per rule.

`cargo metadata` resolves features once for the whole workspace, so one
member's dependencies can switch on an optional edge of a shared crate
that another member's own build never takes. A transitive violation (3.4.1
reached through a dependency, and spec 027 3.14.3 for the WebAssembly
binding) therefore stands only when the crate's own resolution confirms
it: `cargo tree -p <crate> -e normal,build --locked`, for every target
(`--target all`), or for `wasm32-unknown-unknown` for the WebAssembly
binding, also lists a package the violated rule forbids. The check prints
how many paths it dismissed this way. It fails closed: when the crate's
own graph cannot be read (`cargo tree` cannot run or exits non-zero), the
violation stands. A violation of a declared edge (3.4.1 direct, 3.4.2,
3.4.4, 3.4.5) needs no confirmation. The confirmation rule is unit-tested
with its negative control (R-45).

Forbidden families are matched by package name: `aicortex*`, `rahi*`,
`statecraft*`, `spec-spine*`; HTTP stacks `hyper`, `reqwest`, `axum`,
`actix-web`, `warp`, `tonic`, `ureq`, `isahc`, `surf`, `h2`, `http`; async
executors `tokio`, `async-std`, `smol`, `futures-executor`, `async-executor`.
The list is a conservative denylist, not a proof of absence of I/O; adding a
family is a change to this spec.

### 3.6 Product principles (constitution VI onward)

The constitution carries the text. In summary:

- VI. Proposals, never grants.
- VII. Values carry their semantics.
- VIII. Unresolved is an answer.
- IX. Capabilities are disclosed, not assumed.
- X. Every decision is identified, attributed and evidenced.
- XI. Measured, not asserted.
- XII. Partial adoption.
- XIII. Determinism is scoped.

### 3.7 Partial adoption obligations

Each row of the design's section 3.3 table becomes a build that passes without
the omitted crates, owned by the spec of the increment that first makes it
possible. Increment 1 makes one row possible: `rustev-contract` builds and is
usable with no other workspace crate (3.4.5). The boundary check enforces the
rule here; the build of the crate alone is spec `002`'s acceptance, because
002 creates the crate.

## 4. Out of scope

Crate-level behavior (spec `002` onward). The choice of semantic backend.
Aicortex's, Rahi's and statecraft-cli's own contracts, and any claim about
their readiness.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A crate outside `integrations/` gains a dependency on an `aicortex*`, `rahi*` or `statecraft*` package | The boundary check exits non-zero naming the edge. |
| `rustev-core` or `rustev-contract` gains `tokio` or `reqwest`, even transitively | The boundary check exits non-zero naming the path. |
| Any crate depends on a crate under `integrations/` | The boundary check exits non-zero. |
| `rustev-contract` depends on `rustev-core` | The boundary check exits non-zero. |
| A Rustev crate defines a `Permitted` authorization type | Refused in review; no such type exists (checked by `grep` in acceptance). |
| A judgment is written as a user-preference claim | Refused in review; only observations carrying their derivation class are proposed. |

## Acceptance

- The boundary check passes on the workspace, and each rule has a unit test
  with a violating synthetic workspace that fails (negative control).
- No source file under `crates/` or `tools/` declares a type named
  `Permitted` or `Grant`.
- `make gate` passes.

## Verification

```verify:cli
# 3.4, 3.5: rule unit tests, each with a violating fixture.
cargo test -p rustev-boundaries --locked
# 3.4 on the real workspace.
cargo run -p rustev-boundaries --locked --quiet
# 3.2.1: no Rustev-owned authorization type in any Rust source that exists.
sh -c 'for d in crates tools; do [ -d "$d" ] || continue; if grep -rnE "(struct|enum|trait|type)[[:space:]]+(Permitted|Grant)([^A-Za-z0-9_]|$)" "$d"; then exit 1; fi; done'
```

## Implementation record

- 2026-10-27: `make landing-status` caps stack depth per chain. A PR's
  depth is the number of unmerged PRs beneath it; the cap is 1 by default
  and 2 for the 037, 038 and 039 chains (the owner's 2026-10-26 choice), and
  the report prints both. The single cap of 2 it replaces let every chain
  run two PRs above its base. Over the 217 PRs merged from 2026-10-19 to
  2026-10-27, a stacked PR landed about 1.2 hours after its base at depth 1
  or 2 alike, while AI reviews per PR rose from 5.8 at depth 1 to 7.8 at
  depth 2 and 10.7 at depth 3 or more.
- 2026-10-24: `make landing-status` orders each action class by cost of
  delay (1, plus the specs a spec unblocks, plus its place in the owner's
  release order: 031 4, 034 3, 035 and 036 2), then by age. It lists the
  approved specs being built with their open PRs, most costly first, and
  marks one in progress with no open PR as stalled. It caps speculative work
  (at most 4 stacked PRs and stacks 2 deep): past the cap it names the PRs
  to land before anything new is opened on a stack. It also counts PRs
  failing only the AI review, which usually means the reviewer is failing.
- 2026-10-24: `make landing-status` reads the default branch's merge queue.
  A pull request in the queue is `merging` with its position and state, or
  `fix` when its merge group failed, instead of "enable auto-merge" (GitHub
  reports no auto-merge request for a queued pull request).
- 2026-10-24: CI caches are restored on every run and saved only by a push
  to main in every hand-owned workflow (`verify.yml`, `boundaries.yml`, and
  spec 027's `bindings.yml` and spec 031's `release-checks.yml`, which no
  claim of theirs covers). Measured over 25 runs: saves from merge-queue and
  pull-request branches, which nothing else can read, kept main's entries
  evicted (`code` restored 1 time in 19, verify 20 in 76). Before saving,
  workspace crates' artifacts are dropped (Cargo rebuilds them after any
  checkout), the MSRV job keeps only registry and git (its target was 4 GB),
  and verify restores the preflight job's spec-spine binary instead of
  racing four saves of its own.
- 2026-10-27: 040 moves from `integrations-core` (34 minutes measured) to
  `integrations-base` (17), about 25 minutes each. Same commands, every
  spec still exactly once.
- 2026-10-27: a pull request's own CI run checks the acceptance partition
  but runs no acceptance group; the merge-queue entry, which tests the
  change against the latest `main`, and every push to `main` run all of
  them, so nothing reaches `main` unverified. The label
  `statecraft:stacked-ci` runs the groups on a pull request. A pull
  request's run is about 25 minutes shorter; a defect only acceptance
  finds now ejects the queue entry instead of failing the pull request.
- 2026-10-27: `integrations-core` reached the acceptance job's 45-minute
  limit once 037 joined it, so 002, 010, 012, 015, 017, 019, 020 and 033
  move to a fifth group, `integrations-base` (about 22 minutes each). Same
  commands, every spec still exactly once.
- 2026-10-23: CI acceptance groups rebalanced on measured per-spec time
  (eight runs): spec 004 alone took about 24 of the `evaluation` group's
  36 minutes, so 004 runs alone and 023 and 029 join `runtime-evidence` and
  `cli-backends`; 019 and 020 move to `integrations-core`. Same four groups,
  same commands, every spec still exactly once.
- 2026-10-23: `make landing-status` (`scripts/landing/status.py`) orders the
  open pull requests by what they unblock and names the next action for each:
  retarget, owner approval with its reason, failed checks, update, enable
  auto-merge, or waiting for a parent. It also reports delivery measures
  (approval wait, oldest blocked PR, merges to main versus intermediate
  branches, specs complete on main, release rehearsals). Read-only except
  `--cancel-superseded`, which cancels pull-request runs waiting on the
  exception Environment for a head no open pull request still has.

- 2026-10-22: Spec 023's repository-local synthetic refusal acceptance joins
  `VERIFIED_SPECS` and the `evaluation` group in `scripts/acceptance/verify.py`
  beside spec 004. This adds no real-data qualification claim.
- 2026-10-22: CI declared acceptance runs in four isolated groups of whole
  specs. A checked partition covers the Makefile `VERIFIED_SPECS` list exactly
  once; all groups must succeed. Local `make verify` still runs the full list.
  Per-spec elapsed times are recorded to support measured rebalancing.
  Verification commands and mutation checks are unchanged.

- The boundary check, the workspace manifest and the Makefile targets
  (`boundaries`, `verify`, coverage in `gate`) landed with this spec marked
  `complete`. Ownership coverage (`require_ownership`, `governed_scope`) is
  enabled in `spec-spine.toml` in the same change.

## Decision history

- 2026-09-23: approved by the owner with bounded corrections (authority as
  proposals with a stated limitation, derivation classes, dependency layout
  with network implementations under `integrations/`, ownership of workspace
  machinery). Recorded in `docs/decisions/00-founding-decisions.md` as A-01.
- 2026-09-23: the owner closed O-01 (R-07). Principle XIII's closing
  sentence, in the constitution section this spec claims, now records that
  reading instead of the pending question. No principle changed.
- 2026-10-16: R-35. The design document this spec owns refers to the
  downstream host generically ("the host application"). Editorial only; no
  principle, boundary or check changed.
- 2026-10-16: RUSTEV-004. `README.md`, the design document this spec claims
  and `docs/backlog.md` were reconciled with the governed specs and tracking
  issue #76: status notes added to the design, backlog states point at their
  owning specs or #76 children. Editorial only; no principle, boundary or
  check changed.

- 2026-10-21: R-45. 3.5 amended: a transitive violation found in the
  workspace-wide `cargo metadata` resolution stands only when the crate's
  own `cargo tree -p` resolution confirms it, and it stands when that graph
  cannot be read. Found while adding `rustev-rahi` (spec 010): Rahi's graph
  switches on `crypto-common`'s optional `rand_core` edge, and the
  unconfirmed checker reported seven paths ending `rand_core -> getrandom`
  (and `js-sys`) for the pure crates and the bindings that none of their
  own builds hold. Approved by the owner on 2026-10-21.

## Profile upgrade record (2026-10-02)

The Statecraft github-actions-rust profile moves to revision 13 before the
spec-spine pin moves to 0.28.0. This bridge keeps the exact 0.26.0 pin while
CI adopts the regular-file commit walk and repository-local `.bin` installer.
Existing required reusable jobs remain declared in the profile parameters.
Local installation derives the version from `spec-spine.toml`.

## Engine upgrade record (2026-10-02)

After the revision 13 bridge, the repository exact pin moves to spec-spine
0.28.0. The registry and index are regenerated with the pinned engine,
including the committed codebase-index input manifest. The CI profile and
required reusable jobs remain revision 13.

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
  `publish = false` workspace member). A spec that adds a crate directory
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

- The boundary check, the workspace manifest and the Makefile targets
  (`boundaries`, `verify`, coverage in `gate`) landed with this spec marked
  `complete`. Ownership coverage (`require_ownership`, `governed_scope`) is
  enabled in `spec-spine.toml` in the same change.

## Decision history

- 2026-09-23: approved by the owner with bounded corrections (authority as
  proposals with a stated limitation, derivation classes, dependency layout
  with network implementations under `integrations/`, ownership of workspace
  machinery). Recorded in `docs/decisions/00-founding-decisions.md` as A-01.

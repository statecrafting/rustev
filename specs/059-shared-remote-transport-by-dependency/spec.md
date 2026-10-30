---
id: "059-shared-remote-transport-by-dependency"
title: "Shared remote transport by dependency, not by source link (amends 001, 010, 012, 030 and 049)"
status: approved
implementation: pending
created: "2026-10-29"
summary: >
  An amendment of approved specs 001 (3.4.4), 010 (I-2 and the
  `rustev-serve` note), 012, 030 and 049 (I-3). `rustev-jev`,
  `rustev-bedrock` and `rustev-serve` compile spec 009's Part A machinery
  from `integrations/rustev-remote-http/src/` through `#[path]`, because
  001 3.4.4 forbids a crate depending on an integration crate. A crate
  whose sources sit outside its own directory cannot be packaged, so spec
  031 3.4 keeps `rustev-jev` (and, for the same reason, `rustev-bedrock`)
  out of the release family until a reviewed change removes the source
  link, and leaves open how. This spec chooses the boundary amendment:
  those three crates may depend on `rustev-remote-http`, only as a normal
  dependency, and re-export or use its modules instead of compiling copies.
  No behavior, record, document or public path of any crate changes.
  Admission of any crate to the release family stays with a later change.
amends:
  - "001-boundaries-and-authority"
  - "010-integrations"
  - "012-jev-integration"
  - "030-bedrock-backend"
  - "049-opentelemetry-through-the-node-binding"
extends:
  # The three crates replace their source links with the dependency (3.2).
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: amending }
  - { spec: "030-bedrock-backend", unit: { kind: directory, path: "integrations/rustev-bedrock/" }, nature: amending }
  - { spec: "010-integrations", unit: { kind: directory, path: "integrations/rustev-serve/" }, nature: amending }
  # The allowed edges (3.1).
  - { spec: "001-boundaries-and-authority", unit: { kind: directory, path: "tools/rustev-boundaries/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 059 to `make verify` at completion.
  # Spec 049 is amended only in its own text (I-3, see 3.1.1): it
  # establishes no unit, so the `amends` edge carries that amendment and no
  # `extends` entry names 049.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "009-remote-adapter-protocol"
  - "010-integrations"
  - "012-jev-integration"
  - "030-bedrock-backend"
  - "031-public-api-and-release-family"
  - "049-opentelemetry-through-the-node-binding"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Apart from the facade's exception of spec 031 3.3 and rustev-node's edge to rustev-otel of spec 049 3.1, the only dependencies of a crate on an integration crate are the normal dependencies of rustev-jev, rustev-bedrock and rustev-serve on rustev-remote-http; make boundaries refuses every other edge, including these three as build or dev dependencies, any of them on another integration, and any other crate on rustev-remote-http."
    anchor: "3-1-the-boundary-exception"
  - id: "I-2"
    kind: invariant
    text: "No source file of rustev-jev, rustev-bedrock or rustev-serve is compiled from outside that crate's own directory; each uses spec 009's Part A machinery through its dependency on rustev-remote-http."
    anchor: "3-2-the-source-links-become-a-dependency"
  - id: "R-1"
    kind: requirement
    text: "The change alters no behavior: every existing test, conformance case, golden, PlanId, recorded fixture and exchange record of the three crates and of rustev-remote-http is unchanged, and the public module paths of rustev-jev and rustev-bedrock still resolve."
    anchor: "3-3-what-does-not-change"
---

# 059: Shared remote transport by dependency, not by source link (amends 001, 010, 012, 030 and 049)

Ratified by the owner on 2026-10-30 (A-70). The approved texts of specs
001, 010, 012, 030 and 049 are not edited; this spec records what changes
in them. Ratification does not admit any crate to the release family,
authorize a release or publication, or authorize a live call.

## 1. Purpose

Spec 009's Part A machinery (budgets, cancellation, cost, exchange records,
the HTTP client and pool, identity, phases, principals, the error taxonomy
and the wire) lives in `integrations/rustev-remote-http/src/`. Three other
integration crates need it, and spec 001 3.4.4 says "No crate depends on a
crate under `integrations/`". They therefore compile the same files into
themselves through `#[path]`:

| Crate | Source-linked modules |
|---|---|
| `rustev-jev` (spec 012) | `budget`, `cancel`, `cost`, `exchange`, `http`, `identity`, `phases`, `pool`, `principal`, `taxonomy`, `wire` (public) |
| `rustev-bedrock` (spec 030) | the same eleven (public) |
| `rustev-serve` (spec 010, 042) | `budget`, `http`, `phases`, `pool`, `wire` (private) |

Spec 010 records the pattern for `rustev-serve` ("Standalone sinks reuse
`rustev-remote-http`'s sources by path, as `rustev-jev` does (spec 001
3.4.4)"). Its cost is packaging: `cargo package` archives only the crate's
own directory, so a crate that compiles files outside it cannot be
published. Spec 031 3.4 states that "the `typesafe` feature therefore waits
for a reviewed change that removes that source-linking (a further boundary
amendment or moving the shared machinery into an admissible crate); this
spec does not decide which", and `tools/rustev-release/release-set.json`
excludes `rustev-jev` and `rustev-bedrock` for the same reason.

This spec makes that reviewed change, choosing the boundary amendment
(decision 2).

## 2. Territory

- **Spec 001 3.4.4, spec 010 I-2 and spec 049 I-3, amended (3.1):** three
  more allowed edges, each to `rustev-remote-http`. `tools/rustev-boundaries/`
  enforces them; `Cargo.lock` records the dependencies.
- **Specs 012, 030 and 010, amended (3.2):** `rustev-jev`, `rustev-bedrock`
  and `rustev-serve` replace their `#[path]` modules with the dependency.
  Their crate documentation that explains the source link is updated.

What does not change: `rustev-remote-http`'s source, features and public API
(its eleven modules are already public); every other crate; every document,
schema, generated type, `PlanId`, conformance case, golden and recorded
fixture; the release set and the facade's features.

## 3. Behavior

### 3.1 The boundary exception

1. Spec 001 3.4.4 and the last clause of spec 010 I-2, as amended by spec
   031 3.3 and spec 049 3.1, are amended again to read, in effect: no crate
   depends on a crate under `integrations/`, except the facade as 031 3.3
   states, `rustev-node` on `rustev-otel` as 049 3.1 states, and
   `rustev-jev`, `rustev-bedrock` and `rustev-serve`, each of which may
   depend on `rustev-remote-http` and on no other integration crate, only
   as a normal (not build or dev) dependency. Spec 049 I-3 is amended to
   name these edges beside its own.
2. Why it is safe.
   - All four crates are under `integrations/`, so every family
     `rustev-remote-http` reaches (HTTP stacks, TLS, the async runtime) is
     still reached only from `integrations/`, and 001 3.4.2 holds
     literally.
   - `rustev-remote-http` depends on no integration crate, so no edge forms
     a chain. Rule 3.4.4 still covers `rustev-jev`, `rustev-bedrock` and
     `rustev-serve`, so no other crate gains a path through them.
   - Each dependent already compiles exactly these files today. The
     dependency changes how they are built, not what they contain.
   - Spec 010 I-2 asks that each integration stay "independently optional
     and removable". `rustev-remote-http` is the shared transport and
     cannot be removed while a remote integration is present; that was
     already true of its source files. Each of the three dependents can
     still be removed alone, and none needs another.
3. `tools/rustev-boundaries` enforces it in the style of 049 3.1.3:
   - each of the three edges passes as a normal dependency;
   - the same edge as a build or dev dependency, any of the three on
     another integration crate, `rustev-node` or `rustev-otel` on
     `rustev-remote-http`, and any crate outside `integrations/` on it,
     each stay the existing `DependsOnIntegration` violation;
   - each case has a synthetic-metadata fixture, and `make boundaries`
     passes on the workspace.

### 3.2 The source links become a dependency

1. Each of the three crates declares `rustev-remote-http.workspace = true`
   under `[dependencies]`.
2. `rustev-jev` and `rustev-bedrock` replace each public `#[path]` module
   with a re-export of the same name, `pub use rustev_remote_http::<name>;`,
   so `rustev_jev::<name>` and `rustev_bedrock::<name>` still resolve.
3. `rustev-serve` replaces its private `#[path]` modules with private
   `use rustev_remote_http::{...};` imports of only the modules it uses. It
   exposes none of them, as today.
4. No source file under `integrations/rustev-remote-http/src/` changes.
   Each crate's documentation that says the modules "are included rather
   than depended on" is rewritten to name the dependency and this spec.

### 3.3 What does not change

1. One consequence is intended: the shared types become one type. Today
   `rustev_jev::exchange::ExchangeSink` and
   `rustev_remote_http::exchange::ExchangeSink` are distinct traits compiled
   twice; after this change they are the same trait. No published crate is
   affected, because `rustev-jev`, `rustev-bedrock` and `rustev-serve` are
   all outside the release family.
2. Every existing test of the four crates passes unchanged, including
   `rustev-bedrock`'s recorded-fixture replay and `rustev-jev`'s swap tests.
3. Test-only `#[path]` links (for example `tests/common/mod.rs` reaching
   `rustev-runtime`'s test helpers) are left alone. Whether a test link
   bears on admission is spec 031's question, not this spec's.

## 4. Out of scope

- Admitting `rustev-jev` or `rustev-bedrock` to the release family, adding
  the facade's `bedrock` or `typesafe` feature, or changing
  `release-set.json` (spec 031 3.4). Self-containment is necessary for
  admission and not sufficient. Each crate also depends on `rustev-live`,
  which is `publish = false` and excluded from the family ("spec 043
  increment 1 establishes the journal; not yet admitted"). That is a
  separate change.
- Moving the machinery into a new crate (decision 2).
- Any live call, and any change to spec 030's or spec 022's live stages.
- Publication.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `rustev-jev`, `rustev-bedrock` or `rustev-serve` on `rustev-remote-http` as a build or dev dependency | `make boundaries` fails naming the edge. |
| `rustev-jev` on `rustev-bedrock`, or any of the three on `rustev-otel` | `make boundaries` fails naming the edge. |
| `rustev-node` or `rustev-otel` on `rustev-remote-http` | `make boundaries` fails naming the edge. |
| A crate outside `integrations/` on `rustev-remote-http` | `make boundaries` fails (3.4.2 and 3.4.4). |
| A `#[path]` in any source file under `src/` of the three crates that reaches outside that crate's directory | The declared verification fails. |

## Acceptance

- `make boundaries` passes on the workspace, and `cargo test -p
  rustev-boundaries` covers each allowed edge and each refused edge of
  3.1.3 with synthetic-metadata fixtures.
- No `#[path]` attribute in any source file under `src/` of `rustev-jev`,
  `rustev-bedrock` or `rustev-serve` reaches outside that crate's own
  directory (I-2). A `#[path]` that stays inside the crate, such as a test
  module included from the crate's `tests/`, is not a source link.
- `cargo test` of the three crates and of `rustev-remote-http`, with all
  features, passes with no test changed.
- `make code`, the declared verification and `make gate` pass.

## Verification

Declared for `spec-spine verify 059`; 059 joins `make verify` at completion.

```verify:cli
# 3.1: the allowed edges and each refused one (synthetic fixtures), and the
# real workspace.
cargo test -p rustev-boundaries --locked
cargo run -p rustev-boundaries --locked --quiet
# 3.2 (I-2): no source file under the three crates' src/ links a path
# outside its crate (two or more parent steps leave the crate directory).
sh -c 'if grep -rnE "#\[path[^]]*\.\./\.\./" integrations/rustev-jev/src integrations/rustev-bedrock/src integrations/rustev-serve/src; then exit 1; fi'
# 3.3: behavior is unchanged.
cargo test -p rustev-remote-http --locked --all-features
cargo test -p rustev-jev --locked --all-features
cargo test -p rustev-bedrock --locked --all-features
cargo test -p rustev-serve --locked --all-features
```

## Implementation record

Not started. A prototype of 3.2 on 2026-10-29 (not committed) built all
three crates, with every target and feature, after replacing the 27
`#[path]` modules. Its only change beyond 3.2 was dropping two imports
`rustev-serve` did not use (`phases`, `pool`).

## Decisions at approval

1. **Ratification.** Ratified by the owner on 2026-10-30 (A-70).
2. **Boundary amendment, not a new crate.** Spec 031 3.4 names two ways:
   amend the boundary, or move the machinery into an admissible crate. A
   new crate would need its own exception anyway, since the machinery
   carries an HTTP stack and 001 3.4.2 keeps those under `integrations/`.
   It would also move eleven public modules out of `rustev-remote-http`,
   a published crate, which is a breaking change to its API. The amendment
   changes no published API and no file of `rustev-remote-http`. Proposed
   by the implementer; accepted with the ratification (A-70).
3. **Normal dependencies only.** As in 049 3.1, the narrowest edge that
   meets the need. Proposed by the implementer; accepted with the
   ratification (A-70).

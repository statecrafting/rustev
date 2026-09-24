---
id: "013-remote-state-on-failed-attempts"
title: "Remote state on attempts that end without a stop (amends 003)"
status: draft
implementation: pending
created: "2026-09-24"
summary: >
  A separately reviewable amendment of approved spec 003 (R-16), chosen by
  the owner in R-28 for the gap spec 009 5.1 records: the runtime derives
  `possibly_continuing` only from cancellation, so an attempt whose request
  left the process and whose connection then failed is recorded as
  `finished` although remote work and charges may continue. This adds one
  field to the attempt report through which an adapter states that remote
  work may continue, and one derivation rule in the runtime. The
  `rustev.run/1` schema and every existing record's bytes are unchanged.
  Draft: claims no code.
amends:
  - "003-runtime-execution-and-evidence"
extends:
  # The attempt report of the backend seam gains one field (spec 003, 3.2.6).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  # The runtime's remote-state derivation (spec 003, 3.7.3).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # The rules backend and the eval test fixtures fill the new field with the
  # value that keeps their behavior; no behavior of theirs changes.
  - { spec: "005-reference-backends", unit: { kind: directory, path: "backends/rustev-backend-rules/" }, nature: additive }
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: additive }
depends_on:
  - "003-runtime-execution-and-evidence"
  - "009-remote-adapter-protocol"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "An attempt whose adapter reports that remote work may continue is recorded `possibly_continuing` and its charge is settled as unknown liability, never as finished or refunded."
    anchor: "3-2-runtime-derivation"
  - id: "I-2"
    kind: invariant
    text: "Every existing backend reports `finished`, so every run record it produces is byte-identical to the one produced before this amendment."
    anchor: "3-3-compatibility"
---

# 013: Remote state on attempts that end without a stop (amends 003)

Draft. An amendment proposal, reviewable on its own before any code, as
R-16 requires for a substantive change to approved behavior. The owner
chose it over accepting the limitation (R-28, item 4). Spec 003's approved
text is not edited; this spec records the change.

## 1. Purpose

Spec 003 3.7.3 promises that dropping a future is never recorded as
stopping remote work. It does not cover a remote adapter that returns a
failure of its own accord after its request left the process and before any
answer came back (spec 009, `transport_interrupted`). The runtime records
such an attempt as `finished`. Its charge is already honest, because the
adapter reports `unknown` and the reservation stays liability, but the
remote-state field claims more than anyone knows. This amendment lets the
adapter say so.

## 2. Territory

When approved and delivered: `AttemptReport` in `crates/rustev-core/src/seams.rs`,
the remote-state derivation in `crates/rustev-runtime/`, and the mechanical
updates to every place that constructs an `AttemptReport` (the rules
backend and the runtime and eval test fixtures).

## 3. Behavior

### 3.1 The attempt report

`AttemptReport` gains `remote: RemoteEnd`, where `RemoteEnd` is
`Finished` (the remote work, if any, is over as far as the adapter can
establish) or `PossiblyContinuing` (request bytes left the process and
nothing establishes that the remote side stopped). An in-process backend
always reports `Finished`. A remote adapter reports `PossiblyContinuing`
exactly when spec 009 3.6 prescribes charge `unknown` because the request
was sent and no complete answer arrived.

### 3.2 Runtime derivation

For a report returned of the adapter's own accord (spec 003, 3.3.4):

1. `remote: Finished` is recorded as today: `finished`, with the reported
   charge.
2. `remote: PossiblyContinuing` is recorded `possibly_continuing`, and the
   charge is settled as `unknown` whatever the report says, so the
   reservation stays liability (spec 003, 3.5.4) until the host reconciles
   it (spec 003, 3.5.5).

The cancellation path (a report obtained after the runtime raised the
signal) is unchanged: `CancelAck` decides as spec 003 3.7.3 says, and the
new field is ignored there, because an unconfirmed acknowledgement already
records `possibly_continuing`.

### 3.3 Compatibility

- `rustev.run/1` is unchanged: `possibly_continuing` is an existing value.
  Only the rule that selects it changes, for reports that carry the new
  value. Spec 004 5.8's promise of unchanged wire schemas holds.
- Every existing backend and fixture sets `Finished`, so its records are
  byte-identical, and every existing runtime, eval and rules-backend test
  passes unchanged apart from constructing the field.
- The failure class is unchanged: the attempt still ends `transient` (or as
  the adapter classed it), and retries and fallback still follow the
  execution policy (spec 003, 3.6). A retry after a possibly-continuing
  attempt is a new attempt id, as before; this amendment does not make the
  runtime wait for, or deduplicate against, the earlier one.

## 4. Out of scope

Any other run-record member, a `rustev.run/2` (deferred by R-28), and any
change to cancellation acknowledgements.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A scripted adapter returns `transient` with `remote: PossiblyContinuing` and charge `observed{5}` | Recorded `possibly_continuing`, charge settled `unknown`, reservation kept as liability. |
| The rules backend runs both reference plans | Run records byte-identical to those before the amendment. |
| A report with `remote: PossiblyContinuing` arrives after the runtime raised the signal | Derived from `CancelAck` exactly as before. |

## Acceptance (planned)

- Runtime tests cover each row of section 5 with scripted backends.
- The existing runtime, eval, rules-backend and CLI suites pass, and a
  recorded golden run record for each reference plan is byte-identical.

## Verification

Planned; not run until this amendment is approved and delivered.

```verify:cli
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-backend-rules --locked
cargo test -p rustev-eval --locked
cargo test -p rustev-cli --locked
```

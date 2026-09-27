---
id: "024-second-local-semantic-backend"
title: "Second local semantic backend (deferred)"
status: draft
implementation: deferred
created: "2026-09-27"
summary: >
  Reserves backlog item RUSTEV-012 and the architecture's increment 3
  "second semantic backend": a second local, offline `DecisionBackend`
  added only after the first local backend (spec 011) is complete and
  comparable evaluation reports show a concrete need it would answer. It
  declares its actual capabilities, artifact identity, resource use and
  numerical behavior, and must be swappable with spec 011's backend and the
  Jev adapter without any authority-path or core contract change. Claims no
  code, names no model, and adds no dependency.
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "011-semantic-backend"
references:
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Swapping this backend for another changes no plan authority path and requires no change to rustev-contract or rustev-core unless separately approved."
    anchor: "3-3-swap-evidence"
  - id: "R-1"
    kind: requirement
    text: "Work under this spec starts only when spec 011 is implementation-complete and a retained comparison report names a need the first backend does not meet."
    anchor: "3-1-entry-criteria"
---

# 024: Second local semantic backend (deferred)

Draft, and deferred: a proposal, not a claim about code, and not offered as
work. Backlog item RUSTEV-012 has no owning spec; this reserves one so the
item is governed rather than remembered. The design's increment 3 names "a
second semantic backend or remote adapter"; spec 012 delivered the remote
adapter, so this backend is no longer on the increment's critical path.

## 1. Purpose

A second local backend is justified only by evidence: a task, capability,
platform, resource or quality gap that the first local backend (spec 011)
demonstrably leaves. Its value is then measured by the same spec 004
reports, so the comparison, not the addition, is the product.

## 2. Territory

None. A crate under `backends/` is claimed only when this spec is made
concrete and approved, after 3.1 holds. No inference dependency, model
download, training or paid inference is authorized (R-17 stands).

## 3. Behavior (to be made concrete)

### 3.1 Entry criteria

1. Spec 011 is approved and implementation-complete.
2. A retained comparison report (spec 004, paired under identical dataset,
   configuration and adapter ids) or a qualification record (spec 023)
   shows the gap: for example a capability spec 011's backend does not
   declare, a platform its feasibility record refused, a resource bound it
   exceeds, or measured quality below a stated gate.
3. The owner records the gap as the reason for this backend.

### 3.2 Obligations carried from spec 011

The backend meets every obligation spec 011 sets for the first local
backend: a bounded feasibility record before a dependency is added;
artifact identity over every behavior-affecting input; declared
capabilities returning only what it computes (no synthesized `rank`);
`Tolerance` repeatability with measured bounds; honest local cancellation;
bounded workers, queue and memory; no implicit network; typed refusal on a
missing or mismatched artifact; `model-derived` lineage; and no quality
claim without R-04 data.

### 3.3 Swap evidence

1. The same reference plans and datasets run against this backend, spec
   011's backend and, where capabilities overlap, the Jev adapter, with
   reports paired under identical dataset, configuration and adapter ids.
2. The swap changes no plan authority path, value-kind conversion, policy
   control or core contract. A needed core change is a separate approved
   amendment, never part of this spec.
3. Partial-adoption builds (design section 3.3) pass with only one of the
   local backends present.

## 4. Out of scope

Choosing a model now. Training or distillation (backlog RUSTEV-019). Remote
backends (spec 009). Any claim of equivalence with Jev (R-05).

## Acceptance (draft)

- 3.1's entry criteria are recorded before any crate is claimed.
- 3.3's swap reports exist for every task both local backends declare.

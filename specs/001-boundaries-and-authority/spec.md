---
id: "001-boundaries-and-authority"
title: "Ecosystem boundaries, judgment versus authority, and product principles"
status: draft
created: "2026-09-23"
summary: >
  What Rustev owns and refuses to own within the ecosystem (spec-spine,
  Aicortex, Rahi, statecraft-cli, domain packages), the separation of
  judgment from authority as a property of signatures rather than convention,
  the partial-adoption obligations, and the product principles that become
  constitution principles VI onward when this spec is approved.
establishes:
  - "docs/design/001-decision-engine-architecture.md"
  - { kind: section, file: "standards/spec/constitution.md", anchor: "vi-onward-the-principles-of-the-system-you-are-specifying" }
depends_on:
  - "000-bootstrap"
---

# 001: Ecosystem boundaries, judgment versus authority, and product principles

Rationale: `docs/design/001-decision-engine-architecture.md` sections 1 to 5
and 12 to 14. This spec is the normative subset.

On approval this spec claims the constitution's `VI onward` section and
replaces its placeholder with the principles in section 3.4. Until then the
principles below govern nothing.

## 1. Purpose

Fix Rustev's responsibilities and its non-responsibilities so that every later
spec can be judged against them, and fix the authority rule that makes a
decision engine safe to embed.

## 2. Territory

Owns no code. Claims the design document it normalizes, and the
constitution section named above, whose placeholder it replaces on approval.

## 3. Behavior

### 3.1 Responsibilities

Rustev compiles and executes typed decision plans: validated inputs, exact
computation, semantic steps behind capability contracts, selection policy,
bounded runtime execution, evidence emission, and evaluation machinery.

Rustev does not own: a knowledge store, authentication or identity, authority
policy, action execution, arbitrary agent workflows, or hosting.

### 3.2 Dependency direction

1. No crate outside `integrations/` depends on Aicortex, Rahi, statecraft-cli
   or an HTTP server stack.
2. No crate depends on a crate under `integrations/`.
3. An outside project can read every Rustev record and report by depending on
   `rustev-contract` alone.

### 3.3 Judgment versus authority

1. Rustev produces judgments and proposals. It never produces a value an
   action executor accepts as authorization.
2. The authority function's inputs exclude every judgment type. What is
   permitted is computed from trusted facts: principal, scope, revision.
3. A judgment may select within, or narrow, the permitted set (act, confirm,
   escalate, refuse). No judgment value, however confident, enlarges it.
4. A Rustev output carries `model-derived` provenance and is never admitted,
   by any Rustev component, as evidence for the claim it was computed from.

### 3.4 Product principles (constitution VI onward, on approval)

- **VI. Judgment is not authority.** Confidence never enlarges what an actor
  may do.
- **VII. Values carry their semantics.** Scores, distributions, calibrated
  probabilities, ordinal levels and ranks are distinct kinds that do not
  silently convert.
- **VIII. Unresolved is an answer.** Missing, stale or invalid evidence and
  exhausted budgets produce a typed unresolved outcome, never a default.
- **IX. Capabilities are disclosed, not assumed.** A backend states what it
  returns; an unsupported plan is refused or uses a declared fallback.
- **X. Every decision is identified and evidenced.** Plans, artifacts,
  snapshots and datasets are content-identified, and every decision emits a
  record under a declared durability policy; loss is counted, never silent.
- **XI. Measured, not asserted.** Quality, calibration and latency claims
  name the dataset, split and artifact that measured them; an absent
  measurement is unknown.
- **XII. Partial adoption.** Each major part is usable without the others and
  replaceable behind its seam.

### 3.5 Partial adoption obligations

Each row of the design's section 3.3 table becomes a build that passes
without the omitted crates, owned by the spec of the increment that first
makes it possible.

## 4. Out of scope

Crate-level behavior (specs `002` onward). The choice of semantic backend.
Aicortex's and Rahi's own contracts.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A crate outside `integrations/` gains a dependency on an Aicortex or Rahi crate | The workspace dependency test fails. |
| An authority function takes a `Judgment` parameter in any Rustev-supplied helper | Refused in review; no such helper exists. |
| A Rustev component writes a judgment as a user-preference claim | Refused in review; only observations with `model-derived` provenance are proposed. |

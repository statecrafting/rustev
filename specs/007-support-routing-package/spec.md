---
id: "007-support-routing-package"
title: "Reference package: support routing"
status: approved
implementation: pending
created: "2026-09-25"
summary: >
  The first domain package of increment 3 (design section 16.1, roadmap
  ordinal 007): `packages/rustev-pkg-support-routing`, a library that
  publishes the support-routing decision definition, its golden canonical
  documents, a declarative task adapter and evaluator configuration, and a
  synthetic labeled dataset, depending only on `rustev-contract` and
  `rustev-core`. It demonstrates that a domain is a package and not a core
  change, and that swapping the semantic backend (the rules backend's
  synthetic head against the Jev integration, spec 012) produces comparable
  reports with no change in any authority path. Approved (A-12, 2026-09-25); implementation pending.
establishes:
  # Created on delivery (see section 2).
  - { kind: directory, path: "packages/rustev-pkg-support-routing/" }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "012-jev-integration"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
---

# 007: Reference package: support routing

Approved (A-12, 2026-09-25) as its own reviewable change before any code
(R-16); implementation is pending. Drafted on 2026-09-25 under the
owner decision of 2026-09-24 to draft package 007 once 012 was merged, and
to stop for approval. Rationale: design sections 15, 16.1 and 17
(increment 3); owner decisions R-02 (typed builder and canonical JSON),
R-04 (synthetic fixtures establish mechanics only) and R-05 (the four
semantic operations).

## 1. Purpose

Today the support-routing definition exists only as test code: spec 002
3.13 builds it in `crates/rustev-core/tests/common/`, pins its golden
definition and plan, and the runtime, eval, CLI and Jev tests reach it
through their own copies or paths. No host can depend on it, and nothing
shows that a domain can be added without touching the core.

This package makes support routing a consumable, versioned artifact: the
definition a host compiles against its own backend descriptors, the
evaluation material that defines what a correct route is, and a synthetic
dataset that exercises the evaluation mechanics. It is also the first half
of increment 3's gate: no core change is needed for a package, and a
backend swap yields a comparable report with no change to any authority
path.

## 2. Territory

When approved and delivered:

- `packages/rustev-pkg-support-routing/` (established by this spec).
- The workspace manifest (spec 001) gains the `packages/*` member glob.
- `tools/rustev-boundaries` (spec 001) gains the design's package rule
  (3.3); an additive check, since no package exists today.
- `Makefile` (spec 001) adds 007 to `make verify`.
- Test-only moves in `crates/rustev-core/tests/` (spec 002) if the core's
  golden test is pointed at the package's golden documents (open question
  Q-3); the goldens' bytes do not change.

No contract, core, runtime, eval or backend behavior changes. If the
package needs one, delivery stops, and that change is its own reviewed
amendment first (R-16); increment 3's gate records that it was needed.

## 3. Behavior

### 3.1 The definition

1. The package exposes the support-routing definition of design 16.1
   through the typed builder (R-02): inputs `ticket.message`
   (`user-supplied`, untrusted), `account.tier` (`authenticated-app-field`,
   1 day) and `payments.events[]` (`system-of-record`, 5 minutes); exact
   steps `failed_payments_30d` and `hours_since_first_failure`; semantic
   steps `topic` (`classify` over billing, integration_defect,
   account_access, other), `frustration` (`rubric` over calm, frustrated,
   very_angry) and `explicit_deadline` (`proposition`); and the
   deterministic selection table of 16.1. The output is a proposal to
   route a ticket to a queue with a priority; it is never an authorization
   (spec 001, principles VI onward).
2. The definition is parameterized only by what differs per deployment:
   the `topic` calibration reference and the task thresholds. Everything
   else is fixed by the package version.
3. The package ships the canonical definition document for its reference
   parameters, byte-identical to spec 002's golden
   `support-routing.definition.json`, and a test that the builder and the
   parsed document enter one representation. The golden plan and
   `PlanId` stay spec 002's, since a plan binds descriptors the package
   does not own.
4. A change to the definition's bytes is a new package version and a new
   golden, never a silent edit.

### 3.2 Evaluation material

1. A `rustev.task-adapter/1` document (spec 006, 3.8) defining a correct
   route: the queue label equals the proposed queue; priority is a separate
   labeled field with its own rule. It is bound by content digest where
   spec 014 (approved) requires it.
2. A `rustev.evaluator-config/1` document (version 2 if spec 014 is
   approved first) naming that adapter, agreement on queue and priority,
   `topic` as the probability step, reliability bins, subgroups by
   account tier, and one declared regression gate on error among accepted
   with minimum comparable and labeled coverage.
3. A `rustev.dataset/1` manifest with SYNTHETIC provenance and disjoint
   training, model-selection, calibration and final-test splits, with
   invented tickets written for the package. It establishes mechanics
   only (R-04): no report over it supports a quality or calibration claim,
   and the package says so where it exposes the dataset.

### 3.3 Dependencies

1. Normal dependencies: `rustev-contract` and `rustev-core` only. Runtime,
   backends, eval, the CLI and integrations may appear only as
   dev-dependencies (design section 15). `make boundaries` enforces this
   for every crate under `packages/`.
2. No clock, I/O, async runtime or network in the package's normal code.
   Its documents are embedded at build time.

### 3.4 The backend swap

1. Tests run the synthetic dataset's cases through the runtime twice: once
   with the rules backend's synthetic head (spec 005) and once with the Jev
   integration (spec 012) replaying RECORDED exchanges, with no network in
   tests or CI. Each run is captured into bundles (spec 004) and evaluated
   with the package's adapter and configuration.
2. Both reports are produced under the same dataset, split and evaluator
   configuration; the gate between them is computed and its verdict (pass,
   fail or unknown) recorded as mechanics evidence, never as quality.
3. The authority path is unchanged by the swap: the selection policy, the
   output type and the set of values any executor could receive are
   identical, which the tests assert by comparing the compiled plans'
   policy sections and output declarations across both bindings.

## 4. Out of scope

The lodging package (008); new semantic operations or value kinds; a
calibration fitted on real data; any quality claim; live Jev calls beyond
the recording of fixtures within the R-29 testing cap; publication of the
package to crates.io; `rank`, which Jev does not declare.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The package gains a normal dependency on `rustev-runtime` | `make boundaries` fails naming the package rule. |
| The builder's output differs by one byte from the shipped definition document | The golden test fails. |
| A report over the synthetic dataset | Carries synthetic provenance; no empirical claim is printed. |
| Stale `payments.events[]` (older than 5 minutes) | `Unresolved` missing evidence, never a default route. |
| `topic` unresolved, or its top calibrated probability below threshold | `Escalate(ambiguous-topic)`. |
| The Jev fixture run and the rules run | Two comparable reports; identical policy sections and output declarations. |

## Acceptance

- `cargo test -p rustev-pkg-support-routing --locked` covers section 5.
- `make boundaries` passes with the package rule active and fails on a
  seeded normal dependency on the runtime.
- No file under `crates/` other than spec 002's test moves (Q-3) changes;
  increment 3's "no core change" is checked by the diff.

## Open questions

Owner decision of 2026-09-25 (R-31), answering Q-1 to Q-4: "Specs 014, 015 and 007 are APPROVED, accepting the recommendation written in each draft for every open question".
Each question below is therefore settled as its recommendation states; the
text is kept as the record of what was asked.

- Q-1: the `topic` threshold needs a calibrated probability. Jev returns a
  provider distribution that spec 012 never labels calibrated. With no
  real calibration split, should the Jev binding declare
  `uncalibrated_threshold` with a margin rule (visible on every judgment),
  or should the Jev swap test fit a synthetic temperature on the synthetic
  calibration split (mechanics only)? Recommendation: the declared
  `uncalibrated_threshold`, because a synthetic fit invites being read as
  calibration.
- Q-2: priority labels. Design 16.1 raises priority by one step from
  `frustration` and `explicit_deadline`; should the dataset label priority
  at all, or evaluate the queue only? Recommendation: label both, with
  priority as its own agreement payload, so a frustration change shows up.
- Q-3: whether spec 002's reference-plan tests should consume the
  package's builder instead of their own copy. Recommendation: no; the
  core's goldens stay independent of any package, and the package's test
  asserts byte equality with them.
- Q-4: package documents embedded in the crate versus files under
  `packages/rustev-pkg-support-routing/data/`. Recommendation: files,
  embedded with `include_bytes!`, so the CLI can use them directly.

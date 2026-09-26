---
id: "016-support-routing-swap-threshold"
title: "Declared uncalibrated threshold in support-routing swap test (amends 007)"
status: draft
implementation: pending
created: "2026-09-25"
summary: >
  A proposed amendment of approved spec 007 (R-16). Spec 007 3.4.3 and
  section 5 require the backend swap test to assert identical compiled
  plans' policy sections across the rules backend and Jev backend. Under
  owner decision R-31 (approving Q-1), Jev returns an uncalibrated
  distribution and declares `uncalibrated_threshold` on `topic`'s
  `top_mass_below` condition with a margin rule. The reference definition
  keeps `uncalibrated_threshold: not_declared` to match the golden document
  byte-for-byte (3.1.3). Because `uncalibrated_threshold` is part of the
  condition inside the policy section, the compiled plans' policy sections
  cannot be strictly identical under equality. This amendment amends 3.4.3
  and section 5 so that the swap test asserts identical policy rules,
  conditions, thresholds, adjustments and output declarations, accounting
  for Jev's declared uncalibrated threshold on `topic`. It also resolves
  two further conflicts found while preparing 007's delivery: where the
  swap test can live given spec 001 3.4.4, and how priority can be a
  separately labeled field given the declarative task adapter's
  one-label-per-case shape (spec 006, 3.8).
amends:
  - "007-support-routing-package"
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
  - "006-cli-surface"
  - "007-support-routing-package"
  - "012-jev-integration"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
---

# 016: Declared uncalibrated threshold in support-routing swap test (amends 007)

Proposed amendment of approved spec 007 (R-16); drafted on 2026-09-25.
Spec 007's approved text is not edited; this spec records the proposed
change.

## 1. Purpose

Spec 007 3.4.3 requires that when swapping the semantic backend in the
support-routing package between the rules backend's synthetic head and the
Jev remote backend, "the authority path is unchanged by the swap: the
selection policy, the output type and the set of values any executor could
receive are identical, which the tests assert by comparing the compiled
plans' policy sections and output declarations across both bindings." Section
5 similarly lists: "Two comparable reports; identical policy sections and
output declarations."

However, owner decision R-31 (approving Q-1 of spec 007) resolved the
`topic` calibration question for Jev by selecting "the declared
`uncalibrated_threshold`, because a synthetic fit invites being read as
calibration." Under this decision, Jev declares `uncalibrated_threshold` with
a margin rule on the `topic` probability threshold in the `ambiguous` rule.

The reference definition of support-routing (spec 007, 3.1.3) ships the
canonical definition document byte-identical to spec 002's golden
`support-routing.definition.json`, where `uncalibrated_threshold` on `topic`
is `not_declared` because `topic` is calibrated by `topic_calibration("1.5")`.

Because `uncalibrated_threshold` is an explicit field of
`Cond::TopMassBelow` in the selection rules of `PolicyDecl`, the compiled
plan for Jev carries `UncalibratedThreshold::Declared { reason }` in its
policy section, while the compiled plan for the rules backend carries
`UncalibratedThreshold::NotDeclared`. Under `PartialEq` and byte
comparison, the compiled plans' policy sections are not identical.

This amendment clarifies section 3.4.3 and section 5 so that the swap test
asserts identical selection rules, predicates, thresholds, adjustments and
output declarations, while permitting the declared uncalibrated threshold on
`topic` for Jev.

## 2. Territory

When approved:
- `packages/rustev-pkg-support-routing/tests/swap.rs`: the backend swap test
  asserts equality of the policy sections modulo the declared uncalibrated
  threshold on `topic`, and asserts identical output declarations.
- `integrations/rustev-jev/tests/support_routing_swap.rs` instead, if Q-1
  below is accepted (3.3): the Jev crate gains a dev-dependency on the
  package.
- `packages/rustev-pkg-support-routing/data/`: a second task adapter,
  evaluator configuration and dataset manifest for priority, if Q-2 below
  is accepted (3.4).

No contract, core, runtime, eval or backend behavior changes.

## 3. Behavior

### 3.1 Selection policy comparison across bindings

1. The compiled plan for the reference rules backend has:
   `Cond::TopMassBelow { step: "topic", threshold: "0.6", uncalibrated_threshold: not_declared }`.
2. The compiled plan for the Jev backend has:
   `Cond::TopMassBelow { step: "topic", threshold: "0.6", uncalibrated_threshold: declared { reason: ... } }`.
3. The swap test asserts that:
   - All rules, rule order, rule targets, rule actions and proposal parameters
     are identical.
   - All conditions, predicates, and threshold values are identical.
   - All adjustments, adjustment orders, scales and conditions are identical.
   - All unresolved handlers and actions are identical.
   - The output declarations and types are identical.
   - The only difference in the compiled policy sections is the declaration of
     `uncalibrated_threshold` on the `topic` step, which is `not_declared` for
     the calibrated rules head and `declared` for the uncalibrated Jev binding.

### 3.2 Amendment of spec 007 normative text

Spec 007 section 3.4.3 is amended as follows:
- The compiled plans' policy sections are identical in selection logic, rules,
  conditions, thresholds, adjustments and output declarations, with the sole
  exception that Jev's `topic` condition declares `uncalibrated_threshold` per
  Q-1 / R-31, whereas the reference plan declares `not_declared`.
- The test asserts equality of the policy sections modulo this declared
  uncalibrated threshold difference, and asserts identical output declarations.

Spec 007 section 5 (negative cases row 6) is amended as follows:
- Expected: "Two comparable reports; identical policy rules, thresholds and
  output declarations with Jev's declared uncalibrated threshold notice."

### 3.3 Where the swap test lives

1. Spec 007 3.3.1 lets integrations appear as dev-dependencies of the
   package, but spec 001 3.4.4 says "No crate depends on a crate under
   `integrations/`", with no exception for dev-dependencies, and
   `make boundaries` enforces it for every dependency kind. A package test
   that replays recorded Jev exchanges needs `rustev-jev`, so it cannot be
   a package test.
2. Proposed: the swap test is a test of `integrations/rustev-jev`, which
   takes the package as a dev-dependency (an integration may depend on a
   package; nothing forbids that direction). The package's own normal and
   dev-dependencies never include a crate under `integrations/`. Spec 007
   3.3.1's allowance for integrations as package dev-dependencies is
   withdrawn; spec 001 is unchanged.

### 3.4 Priority as a separately labeled field

1. Spec 007 3.2.1 asks for one `rustev.task-adapter/1` document in which
   the queue label equals the proposed queue and priority is "a separate
   labeled field with its own rule". The declarative adapter (spec 006,
   3.8) selects the first rule whose action matches and compares one mapped
   parameter with the case's single label; a `rustev.dataset/1` case has
   one label. One document cannot judge queue and priority separately, and
   the package cannot ship a library adapter, because `TaskAdapter` lives
   in `rustev-eval`, which is not a normal dependency of a package (007
   3.3.1).
2. Proposed: the package ships two evaluation sets over the same cases and
   snapshots: `support-routing.queue` (adapter rule on `queue`, dataset
   labeled with queues) and `support-routing.priority` (adapter rule on
   `priority`, dataset labeled with priorities), each with its own version
   2 evaluator configuration bound to its adapter's rules digest (spec
   014). A frustration or deadline change then shows up in the priority
   report while the queue report is unaffected, which is Q-2's intent. Both
   datasets keep SYNTHETIC provenance and identical split membership.

## 4. Observable negative cases

| Case | Expected |
|---|---|
| A rule or adjustment differs between the rules plan and the Jev plan | The swap test fails. |
| A threshold value differs between the rules plan and the Jev plan | The swap test fails. |
| An output declaration differs between the rules plan and the Jev plan | The swap test fails. |
| Jev does not declare `uncalibrated_threshold` on `topic` | Compiler refuses with `C::UncalibratedThreshold`. |
| The package gains any dependency (normal or dev) on a crate under `integrations/` | `make boundaries` fails (spec 001 3.4.4). |
| A priority change with an unchanged queue | The priority report's error changes; the queue report's does not. |

## Open questions

Found on 2026-09-25 (night) while preparing 007's delivery; the owner
decides them with the rest of this amendment.

- Q-1: move the swap test to `integrations/rustev-jev/tests/` (3.3), or
  amend spec 001 3.4.4 to allow dev-dependencies on integrations?
  Recommendation: move the test; 001's rule stays absolute and simple to
  check.
- Q-2: two evaluation sets, queue and priority (3.4), or evaluate the queue
  only and drop priority labels? Recommendation: two sets, which keeps
  R-31's answer to 007 Q-2 (label both).

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
  for Jev's declared uncalibrated threshold on `topic`.
amends:
  - "007-support-routing-package"
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
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

## 4. Observable negative cases

| Case | Expected |
|---|---|
| A rule or adjustment differs between the rules plan and the Jev plan | The swap test fails. |
| A threshold value differs between the rules plan and the Jev plan | The swap test fails. |
| An output declaration differs between the rules plan and the Jev plan | The swap test fails. |
| Jev does not declare `uncalibrated_threshold` on `topic` | Compiler refuses with `C::UncalibratedThreshold`. |

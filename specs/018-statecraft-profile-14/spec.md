---
id: "018-statecraft-profile-14"
title: "Adopt Statecraft profile revision 14 with the ordinary Rust code gate"
status: approved
implementation: pending
created: "2026-10-02"
summary: >
  Authorizes the narrow migration from Statecraft github-actions-rust
  revision 13 to revision 14 after the spec-spine 0.28.0 bridge and pin
  sequence. Rustev keeps its exact engine pin and ordinary Rust code gate,
  boundaries and verify jobs. The managed authority files are regenerated
  by the revision 14 producer using the retained setup parameters.
amends:
  - "001-boundaries-and-authority"
depends_on:
  - "001-boundaries-and-authority"
obligations:
  - id: "R-1"
    kind: requirement
    text: "Profile revision 14 replaces revision 13 while the exact spec-spine 0.28.0 pin, ordinary Rust code gate and required specialized jobs remain in force."
    anchor: "3-migration-contract"
---

# 018: Statecraft profile revision 14

## 1. Purpose

The owner approved Statecraft's external-code applicability proposal and
continued revision 14 fleet rollout on 2026-10-02. This spec records only
Rustev's corresponding migration consent. It amends the revision 13 binding
in spec 001's engine upgrade record without rewriting that approved history.
No Rustev product behavior or acceptance authority is added.

## 2. Territory

This amendment owns no product code and grants no implementation claim.
It changes the adopted setup-profile requirement only. Existing owning specs,
workspace boundaries and verifier acceptance remain unchanged.

## 3. Migration contract

After the revision 13 bridge and spec-spine pin PRs have merged, regenerate
managed setup artifacts with the released Statecraft github-actions-rust
revision 14 producer. The exact spec-spine pin remains `=0.28.0`.

Rustev selects the ordinary Rust code gate, not the externally supplied code
lane. Preserve existing setup parameters and the required reusable boundaries
and verify jobs. Those jobs and governance must still pass through the
required `ci-gate`; cancellation, failure and an unexpected skip do not pass.
Keep the owner Environment approval path and existing secret references.

The renderer's profile revision and identity are recorded in the managed
policy and environment declaration. Record the concrete producer identity and
local and hosted verification in the migration PR. Managed files are produced
through reviewed plan and apply, never hand-edited. The old `.tooling` ignore
and resolver exclusion remain alongside `.bin` during this transition.

## 4. Acceptance criteria

- The committed managed profile declares revision 14 and agrees with the
  recorded renderer identity.
- The exact engine pin stays 0.28.0 and the ordinary Rust code job remains
  required with the boundaries and verify jobs.
- Local governance, postcommit coupling and every required hosted check pass
  for the integration candidate before merge.
- No unrelated product code, trust root, secret value, spec lifecycle or
  branch-protection rule is changed by the rendered migration.

## 5. Approval basis

Owner consent is the explicit approval of Statecraft proposal 032 and the
continuing revision 14 fleet rollout on 2026-10-02. This spec grants only that
scoped migration. `implementation: pending` is deliberate: adoption is not
claimed until the separate managed-profile migration lands.

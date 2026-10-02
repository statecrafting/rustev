---
id: "018-statecraft-profile-14"
title: "Adopt Statecraft profile revision 14 with the ordinary Rust code gate"
status: approved
implementation: complete
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
scoped migration. The ratification PR left implementation pending. The separate migration
candidate records the scoped implementation and must pass its declared local
and hosted acceptance before merge.

## 6. Implementation record

The migration candidate uses Statecraft producer commit
`b7e44d7187c9720c5252c897eb3c717d3c2409ca`, qualified by its normal PR and
merge-group checks. The frozen renderer SHA-256 is
`916d9c4044a8420cf400103a2373e6a10f1be65881dacfa816b944f574a01d98`.
The rendered profile is revision 14 with identity
`9eb589951883f6a94ad174678b50a683982915ac463e0782cfc0efbab8c960c9`.

The reviewed setup plan identity is
`ee2635bd4368a3f9ad7ac7467a6483e4d9ab0bc43e296547a30155554da4942c`.
It retains the recorded setup parameters, ordinary Rust code gate and
required specialized jobs. The exact spec-spine pin remains `=0.28.0`, and
the reviewer CLI remains 2.1.116. Local governance passed for the rendered
candidate; its signed commit and regenerated lifecycle shards are checked
again before submission. Actual hosted checks, AI review, scoped owner
Environment approval and any required merge group must pass before landing.

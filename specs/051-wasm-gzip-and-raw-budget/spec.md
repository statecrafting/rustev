---
id: "051-wasm-gzip-and-raw-budget"
title: "The WebAssembly module's gzip and uncompressed budgets for the #76 track (amends 027)"
status: approved
implementation: complete
created: "2026-10-24"
summary: >
  A narrow amendment of approved spec 027 3.10.4, after specs 046 and 050.
  Spec 046 kept the gzip budget of 786,432 bytes on purpose so that the
  next #76 specs adding document types would meet it first, and spec 050
  kept it by leaving spec 034's code out of the WebAssembly module. Spec
  035's first contract increment then measured 789,931 bytes gzip, 3,499
  over, from screening's descriptor, output and required-kind types alone;
  every remaining #76 contract increment adds document types the module
  must parse. This spec raises the gzip budget to 1,048,576 bytes (1 MiB)
  and the uncompressed budget to 3,145,728 bytes (3 MiB), and keeps the
  instantiation budgets, the procedure, the gate and spec 050's exclusion.
  Ratified by the owner on 2026-10-24 (A-53).
amends:
  - "027-node-and-wasm-bindings"
extends:
  # The gated values in the WebAssembly check (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
depends_on:
  - "027-node-and-wasm-bindings"
  - "046-wasm-size-budget"
  - "050-wasm-without-parameterized-definitions"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "The optimized rustev_wasm_bg.wasm is at most 3,145,728 bytes uncompressed and at most 1,048,576 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job."
    anchor: "3-budget"
---

# 051: The WebAssembly module's gzip and uncompressed budgets for the #76 track (amends 027)

Approved: ratified by the owner on 2026-10-24 (decision A-53).
The approved texts of specs 027, 046 and 050 are not edited; this spec
records what changes in 027 3.10.4 after 046.

## 1. Purpose

Spec 027 3.10.4 gates the optimized WebAssembly module under gzip level 9
and uncompressed. Spec 046 (A-43) raised the uncompressed budget to
2,621,440 bytes and kept the gzip budget of 786,432 bytes on purpose: "the
next #76 specs that add document types will meet it first, and that is the
gate meant to make their cost visible". Spec 050 (A-52) met it for spec
034 by leaving parameterized definitions out of the module.

Spec 035's increment 1 part 1 (the screen profile, output and required
kind, pull request #233) measured 2,301,576 bytes uncompressed and 789,931
bytes gzip in CI, 3,499 bytes over the gzip budget. Spec 046's record left
13,579 bytes of gzip headroom; spec 035's first contract part used all of
it. Specs 035 and 036 and the later #76 children add further members to
the definition, plan, evidence, judgment and run documents the module
parses, so leaving each out of WebAssembly the way spec 050 did would
remove the track from the browser surface piece by piece. The owner chose
to raise both budgets once, to cover the track, over shrinking the module.

## 2. Territory

Spec 027 3.10.4's gzip and uncompressed budgets only. The `make bindings`
and CI checks that enforce them are spec 027's
(`tools/rustev-conformance/js/wasm-check.mjs`); this spec changes their two
numbers.

## 3. Budget

1. The optimized `rustev_wasm_bg.wasm` is at most 1,048,576 bytes (1 MiB)
   under gzip level 9: gated. This replaces 786,432 bytes.
2. It is at most 3,145,728 bytes (3 MiB) uncompressed: gated. This replaces
   spec 046's 2,621,440 bytes.
3. Unchanged: Brotli (quality 11) recorded, not gated; the instantiation
   budgets of 3.10.5; the procedure and measurement document of 3.10.6;
   spec 050's exclusion of spec 034's code from the module, which only a
   later amendment of 027 lifts; and that a budget changes only by amending
   spec 027.
4. The measured cost of each later change remains visible: the
   measurement document of 3.10.6 records the sizes on every run, and a
   change that crosses either budget fails the `bindings` job as before.

## 4. Out of scope

Any change to the contract's types or serialization; exposing spec 034
through WebAssembly; the instantiation budgets; the native package.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The optimized module exceeds 1,048,576 bytes under gzip level 9 | The `bindings` job fails, naming the measured and budgeted sizes. |
| The optimized module exceeds 3,145,728 bytes uncompressed | The `bindings` job fails, naming the measured and budgeted sizes. |

## Acceptance

- `make bindings` and the CI `bindings` job gate the gzip size at
  1,048,576 bytes and the uncompressed size at 3,145,728 bytes, and the
  measurement document records both budgets.
- `make gate` passes.

## Verification

`make bindings` runs `tools/rustev-conformance/js/wasm-check.mjs`, which
fails the run when the optimized module exceeds either gated size, and the
CI `bindings` job (a required job of `ci-gate`) runs it on every pull
request and push to `main`.

## Implementation record

**2026-10-24.** `tools/rustev-conformance/js/wasm-check.mjs` gates
`gzip_bytes` at 1,048,576 and `raw_bytes` at 3,145,728; the measurement
document's `budgets` field records both, as before. This change's own
`bindings` run is the first measurement against them. Both acceptance items
hold at this change, so `implementation` is `complete`.

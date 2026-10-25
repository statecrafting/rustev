---
id: "052-wasm-budget-for-specs-035-to-037"
title: "The WebAssembly module's gzip and uncompressed budgets for specs 035 to 037 (amends 027)"
status: approved
implementation: complete
created: "2026-10-25"
summary: >
  A narrow amendment of approved spec 027 3.10.4, after specs 046, 050 and
  051. Spec 051 raised the budgets to 1,048,576 bytes gzip and 3,145,728
  bytes uncompressed to cover the #76 track; after spec 036 increment 1
  (#243) main measures 1,043,813 bytes gzip, and spec 036 increment 2
  (#246) fails at 1,084,956. The open stacks of specs 036 and 037 measure
  up to 1,219,036 bytes gzip and 3,640,136 bytes uncompressed before spec
  035's remaining increments. This spec raises the gzip budget to
  1,572,864 bytes (1.5 MiB) and the uncompressed budget to 4,718,592 bytes
  (4.5 MiB), and keeps the instantiation budgets, the procedure, the gate
  and spec 050's exclusion.
  Ratified by the owner on 2026-10-25 (A-54).
amends:
  - "027-node-and-wasm-bindings"
extends:
  # The gated values in the WebAssembly check (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
depends_on:
  - "027-node-and-wasm-bindings"
  - "046-wasm-size-budget"
  - "050-wasm-without-parameterized-definitions"
  - "051-wasm-gzip-and-raw-budget"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "The optimized rustev_wasm_bg.wasm is at most 4,718,592 bytes uncompressed and at most 1,572,864 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job."
    anchor: "3-budget"
---

# 052: The WebAssembly module's gzip and uncompressed budgets for specs 035 to 037 (amends 027)

Approved: ratified by the owner on 2026-10-25 (decision A-54).
The approved texts of specs
027, 046, 050 and 051 are not edited; this spec records what changes in
027 3.10.4 after 051.

## 1. Purpose

Spec 051 (A-53) raised spec 027 3.10.4's budgets to 1,048,576 bytes under
gzip level 9 and 3,145,728 bytes uncompressed, "once, to cover the track",
after spec 035's first contract increment crossed the earlier gzip budget.
The estimate was short. Measured in CI on the optimized module:

| Change | Uncompressed | gzip |
|---|---|---|
| main after spec 036 increment 1 (#243, run 40177114802) | 3,097,315 | 1,043,813 |
| spec 036 increment 2 (#246, run 40180577976) | 3,220,531 | 1,084,956 |

Measured locally with the pinned `wasm-bindgen` 0.2.129 and `wasm-opt`
116 under the same flags, at the top of each open stack:

| Stack top | Uncompressed | gzip |
|---|---|---|
| spec 036 (increment 5) | 3,348,094 | 1,123,384 |
| spec 037 (increment 7, on spec 036 increment 2) | 3,640,136 | 1,219,036 |

Spec 035's remaining increments (3 to the closure) are not in either
stack and add to both. The build already uses the smallest settings
(`opt-level = "z"`, LTO, one codegen unit, `panic = "abort"`, strip,
`wasm-opt -Oz`). Leaving each spec's code out of the module as spec 050
did for spec 034 would remove the track from the browser surface piece by
piece, which spec 051 already declined. The owner chose to raise both
budgets with headroom for specs 035 to 037 and the next specs, over
shrinking the module or excluding these constructs.

## 2. Territory

Spec 027 3.10.4's gzip and uncompressed budgets only. The `make bindings`
and CI checks that enforce them are spec 027's
(`tools/rustev-conformance/js/wasm-check.mjs`); this spec changes their two
numbers.

## 3. Budget

1. The optimized `rustev_wasm_bg.wasm` is at most 1,572,864 bytes
   (1.5 MiB) under gzip level 9: gated. This replaces spec 051's 1,048,576
   bytes.
2. It is at most 4,718,592 bytes (4.5 MiB) uncompressed: gated. This
   replaces spec 051's 3,145,728 bytes.
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
| The optimized module exceeds 1,572,864 bytes under gzip level 9 | The `bindings` job fails, naming the measured and budgeted sizes. |
| The optimized module exceeds 4,718,592 bytes uncompressed | The `bindings` job fails, naming the measured and budgeted sizes. |

## Acceptance

- `make bindings` and the CI `bindings` job gate the gzip size at
  1,572,864 bytes and the uncompressed size at 4,718,592 bytes, and the
  measurement document records both budgets.
- `make gate` passes.

## Verification

`make bindings` runs `tools/rustev-conformance/js/wasm-check.mjs`, which
fails the run when the optimized module exceeds either gated size, and the
CI `bindings` job (a required job of `ci-gate`) runs it on every pull
request and push to `main`.

## Implementation record

**2026-10-25.** `tools/rustev-conformance/js/wasm-check.mjs` gates
`gzip_bytes` at 1,572,864 and `raw_bytes` at 4,718,592; the measurement
document's `budgets` field records both, as before. This change's own
`bindings` run is the first measurement against them. Spec 035 increment 3
(#244) measured 1,058,464 bytes gzip and 3,139,870 uncompressed (run
40186879792) against spec 051's budgets. Both acceptance items hold at
this change, so `implementation` is `complete`.

---
id: "057-wasm-raw-budget-for-spec-039"
title: "The WebAssembly module's uncompressed budget for spec 039 (amends 027)"
status: approved
implementation: complete
created: "2026-10-28"
summary: >
  A narrow amendment of approved spec 027 3.10.4, after specs 046, 050,
  051, 052 and 055. Spec 052 raised the uncompressed budget to 4,718,592
  bytes (4.5 MiB) and spec 055 kept it gated. Main measures 4,628,645
  bytes; spec 039 increment 6 part b (#325, rank and select evaluation)
  measures 4,724,936 bytes, 6,344 over. This spec raises the uncompressed
  budget to 5,242,880 bytes (5 MiB) and keeps everything else of 3.10.4:
  gzip and Brotli recorded and not gated, the instantiation budgets, the
  procedure and spec 050's exclusion.
  Ratified by the owner on 2026-10-28 (A-60).
amends:
  - "027-node-and-wasm-bindings"
extends:
  # The gated value in the WebAssembly check (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
depends_on:
  - "027-node-and-wasm-bindings"
  - "052-wasm-budget-for-specs-035-to-037"
  - "055-wasm-gzip-measured"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "The optimized rustev_wasm_bg.wasm is at most 5,242,880 bytes uncompressed, measured in CI as spec 027 3.10.6 defines; exceeding it fails the bindings job."
    anchor: "3-budget"
---

# 057: The WebAssembly module's uncompressed budget for spec 039 (amends 027)

Approved: ratified by the owner on 2026-10-28 (decision A-60).
The approved texts of specs 027,
046, 050, 051, 052 and 055 are not edited; this spec records what changes
in 027 3.10.4 after 055.

## 1. Purpose

Spec 052 (A-54) raised spec 027 3.10.4's uncompressed budget to 4,718,592
bytes, and spec 055 (A-57) kept it as the only gated size. Spec 039 adds
the `rank` and `select` steps to the core, which the module carries.
Measured in CI on the optimized module, with the pinned `wasm-bindgen`
0.2.129 and `wasm-opt` 116:

| Change | Uncompressed | gzip (recorded) |
|---|---|---|
| main at #339 (run 40608958105) | 4,628,645 | 1,545,307 |
| spec 039 increment 6 part b (#325, run 40605712490) | 4,724,936 | 1,582,610 |

The build already uses the smallest settings (`opt-level = "z"`, LTO, one
codegen unit, `panic = "abort"`, strip, `wasm-opt -Oz`). Leaving spec
039's code out of the module, as spec 050 did for spec 034, would remove
ranking from the browser surface; spec 051 declined that course for the
#76 track. This spec raises the budget with headroom of about 0.5 MiB
over #325 for the remaining increments of the open specs.

## 2. Territory

Spec 027 3.10.4's uncompressed budget only. The `make bindings` and CI
checks that enforce it are spec 027's
(`tools/rustev-conformance/js/wasm-check.mjs`); this spec changes one
number there.

## 3. Budget

1. The optimized `rustev_wasm_bg.wasm` is at most 5,242,880 bytes (5 MiB)
   uncompressed: gated. This replaces spec 052's 4,718,592 bytes.
2. Unchanged: the gzip level 9 size and the Brotli quality 11 size are
   measured and recorded, not gated (spec 055); the instantiation budgets
   of 3.10.5; the procedure and measurement document of 3.10.6; spec 050's
   exclusion of spec 034's code from the module; and that a budget changes
   only by amending spec 027.
3. The measured cost of each later change remains visible: the
   measurement document records the sizes on every run, and a change that
   crosses the budget fails the `bindings` job as before.

## 4. Out of scope

Any change to the contract's types or serialization; exposing spec 034
through WebAssembly; the instantiation budgets; gating gzip again; the
native package.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The optimized module exceeds 5,242,880 bytes uncompressed | The `bindings` job fails, naming the measured and budgeted sizes. |

## Acceptance

- `make bindings` and the CI `bindings` job gate the uncompressed size at
  5,242,880 bytes, and the measurement document records the budget.
- `make gate` passes.

## Verification

`make bindings` runs `tools/rustev-conformance/js/wasm-check.mjs`, which
fails the run when the optimized module exceeds the gated size, and the
CI `bindings` job (a required job of `ci-gate`) runs it on every pull
request and push to `main`.

## Implementation record

**2026-10-28.** `tools/rustev-conformance/js/wasm-check.mjs` gates
`raw_bytes` at 5,242,880; the measurement document's `budgets` field
records it, as before, and gzip and Brotli stay recorded and ungated.
This change's own `bindings` run is the first measurement against it;
spec 039 increment 6 part b (#325) measured 4,724,936 bytes uncompressed.
Both acceptance items hold at this change, so `implementation` is
`complete`.

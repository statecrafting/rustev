---
id: "046-wasm-size-budget"
title: "The WebAssembly module's uncompressed size budget (amends 027)"
status: draft
implementation: pending
created: "2026-10-20"
summary: >
  A narrow amendment of approved spec 027 3.10.4. The uncompressed size
  budget of the optimized `rustev_wasm_bg.wasm` was declared at 2,097,152
  bytes before specs 032 and 033 added their document types; the first
  measurement is 2,215,589 bytes, 5.6% over, while the gzip and
  instantiation budgets hold. This spec raises the uncompressed budget to
  2,621,440 bytes (2.5 MiB) and keeps every other budget, the procedure and
  the gate unchanged. Draft: a proposal for the owner's ratification; it
  claims nothing about code.
amends:
  - "027-node-and-wasm-bindings"
depends_on:
  - "027-node-and-wasm-bindings"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "The optimized rustev_wasm_bg.wasm is at most 2,621,440 bytes uncompressed and at most 786,432 bytes under gzip level 9, measured in CI as spec 027 3.10.6 defines; exceeding either fails the bindings job."
    anchor: "3-budget"
---

# 046: The WebAssembly module's uncompressed size budget (amends 027)

Draft: a proposal for the owner's ratification, not a claim about code.
The approved text of spec 027 is not edited; this spec records what
changes in 027 3.10.4.

## 1. Purpose

Spec 027 3.10.4 gates the optimized WebAssembly module at 2,097,152 bytes
(2 MiB) uncompressed and 786,432 bytes (768 KiB) under gzip level 9; its
decisions at approval call the values "ceilings to be measured, not
measurements", and say a budget is raised only by amending spec 027.

Spec 027's increment 5 made the first measurement (macOS arm64, Node
24.6.0): 2,215,589 bytes uncompressed, 762,689 bytes gzip, 543,501 bytes
Brotli, and a Node cold-instantiation median of 3.5 ms against a 50 ms
budget. Only the uncompressed budget fails, by 118,437 bytes (5.6%).

The module is mostly the contract's document types: their
deserialization code is about 1 MB before optimization, and the definition,
plan, evidence, judgment and run types of specs 032 and 033, added after
027's budget was declared, are a large share of it. Every pure command
needs them (a plan embeds its definition; replay and evaluation read
bundles that embed runs and judgments). Shrinking the module below 2 MiB
would mean restructuring the contract's serialization, which changes
spec 002's code for a size goal and risks its canonical bytes; it is not
proposed here.

## 2. Territory

Spec 027 3.10.4's uncompressed budget only. The `make bindings` and CI
checks that enforce it are spec 027's; their implementation changes the
one number when this spec is approved.

## 3. Budget

1. The optimized `rustev_wasm_bg.wasm` is at most 2,621,440 bytes (2.5 MiB)
   uncompressed: gated. This replaces 2,097,152 bytes.
2. Unchanged: gzip (level 9) at most 786,432 bytes, gated; Brotli (quality
   11) recorded, not gated; the instantiation budgets of 3.10.5; the
   procedure and measurement document of 3.10.6; and that a budget changes
   only by amending spec 027.
3. The gzip budget, the size a browser downloads, is kept on purpose: the
   first measurement is within it by 23,743 bytes (3%), so the next #76
   specs that add document types will meet it first, and that is the gate
   meant to make their cost visible.

## 4. Out of scope

Any change to the contract's types or serialization; the instantiation
budgets; the native package.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The optimized module exceeds 2,621,440 bytes uncompressed | The `bindings` job fails, naming the measured and budgeted sizes. |
| The optimized module exceeds 786,432 bytes under gzip level 9 | The `bindings` job fails, as before. |

## Acceptance

- `make bindings` and the CI `bindings` job gate the uncompressed size at
  2,621,440 bytes and the gzip size at 786,432 bytes, and the measurement
  document records both budgets.
- `make gate` passes.

## Verification

Pending with implementation: spec 027's increment 5 applies the new value
and records it.

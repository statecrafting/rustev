---
id: "055-wasm-gzip-measured"
title: "The WebAssembly module's gzip size is recorded, not gated (amends 027)"
status: approved
implementation: complete
created: "2026-10-25"
summary: >
  A narrow amendment of approved spec 027 3.10.4, after specs 046, 050, 051
  and 052. As the #76 track added
  document types, the gzip budget was met four times: specs 046 and 050
  kept it, and specs 051 and 052 raised it, and the last time it stopped
  every open chain at once. This spec keeps measuring and recording
  the optimized module's gzip size (level 9) on every run, as the Brotli
  size already is, and removes it from the gated budget. The uncompressed
  budget of 4,718,592 bytes (spec 052), the instantiation budgets, the
  procedure and spec 050's exclusion are unchanged. Ratified by the owner
  on 2026-10-25 (A-57).
amends:
  - "027-node-and-wasm-bindings"
extends:
  # The gated values in the WebAssembly check (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: amending }
depends_on:
  - "027-node-and-wasm-bindings"
  - "052-wasm-budget-for-specs-035-to-037"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "R-1"
    kind: requirement
    text: "The optimized rustev_wasm_bg.wasm's gzip level 9 size is measured and recorded in the measurement document on every run and gates nothing; its uncompressed size stays gated at 4,718,592 bytes."
    anchor: "3-measurement"
---

# 055: The WebAssembly module's gzip size is recorded, not gated (amends 027)

Approved: ratified by the owner on 2026-10-25 (decision A-57).
The approved texts of specs 027, 046, 050, 051 and 052 are not edited; this
spec records what changes in 027 3.10.4 after 052.

## 1. Purpose

Spec 027 3.10.4 gates the optimized WebAssembly module under gzip level 9
and uncompressed. As the #76 track added document types, the gzip budget
was met four times: spec 046 raised only the uncompressed budget and kept
gzip on purpose, spec 050 left spec 034's code out of the module to stay
within it, and specs 051 and 052 raised it. On 2026-10-25 the base pull
request of every open #76 chain (#244 for spec 035, #246 for specs 036 and
037) failed the gzip budget at once, and nothing reached the merge queue
for about six hours until spec 052 landed.

The owner chose (2026-10-25) to record the gzip size as a measurement
instead of gating it. Spec 046 kept the gzip budget so that each #76 spec
would see its cost; the measurement document keeps that cost visible on
every run without stopping the queue.

## 2. Territory

Spec 027 3.10.4's gzip budget only. The check is spec 027's
(`tools/rustev-conformance/js/wasm-check.mjs`); this spec removes one gated
value from it. No workflow file changes.

## 3. Measurement

1. The optimized `rustev_wasm_bg.wasm`'s size under gzip level 9 is
   measured on every run and recorded in the measurement document of
   3.10.6 (`gzip_bytes`), and gates nothing. This replaces spec 052's gzip
   budget of 1,572,864 bytes.
2. Its uncompressed size stays gated at 4,718,592 bytes (spec 052); a
   larger module fails the `bindings` job as before.
3. Unchanged: Brotli (quality 11) recorded, not gated; the instantiation
   budgets of 3.10.5; the procedure of 3.10.6; spec 050's exclusion of spec
   034's code from the module; and that a gated budget changes only by
   amending spec 027.

## 4. Out of scope

The uncompressed budget; the instantiation budgets; the module's contents;
the native package.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| The optimized module exceeds 1,572,864 bytes under gzip level 9 | The `bindings` job passes; the measurement document records the size. |
| The optimized module exceeds 4,718,592 bytes uncompressed | The `bindings` job fails, naming the measured and budgeted sizes. |
| A run omits `gzip_bytes` from the measurement document | The check fails: the measurement is required even though it gates nothing. |

## Acceptance

- `tools/rustev-conformance/js/wasm-check.mjs` records `gzip_bytes` on
  every run, gates only `raw_bytes` among the sizes, and its measurement
  document's `budgets` no longer lists a gzip budget.
- `make bindings` and `make gate` pass.

## Verification

`make bindings` runs `wasm-check.mjs`; the CI `bindings` job (a required
job of `ci-gate`) runs it on every pull request and push to `main`.

## Decisions at approval

Ratified as proposed (A-57):

1. **No gzip ceiling at all.** The gzip size is recorded and gates nothing;
   a runaway module is caught by the uncompressed gate.

## Implementation record

**2026-10-25.** `tools/rustev-conformance/js/wasm-check.mjs` drops
`gzip_bytes` from `BUDGET`, so the measurement document's `budgets` no
longer lists a gzip budget, and gates only `raw_bytes` among the sizes, at
4,718,592. It still measures `gzip_bytes` and `brotli_bytes` on every run
and fails the run when either was not measured (section 5's third case).
This change's own `bindings` run is the first measurement under the new
rule. Both acceptance items hold at this change, so `implementation` is
`complete`.

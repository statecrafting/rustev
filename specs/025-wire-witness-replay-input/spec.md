---
id: "025-wire-witness-replay-input"
title: "Wire-witness exchanges as remote-exchange and replay evidence (deferred)"
status: draft
implementation: deferred
created: "2026-09-27"
summary: >
  Reserves backlog item RUSTEV-013: how a redacted, attempt-bound
  `wire-witness.exchange/1` record, captured by an independent witness and
  admitted by its host, may corroborate a Rustev `rustev.remote-exchange/1`
  record and supply retained response bytes for spec 009's offline mapping
  check, while preserving requested versus served identity, explicit
  unknowns, the stricter retention, privacy and cost provenance, and
  host-owned authorization. Captured traffic that no Rustev attempt produced
  is never a Rustev decision or replay case. Draft and deferred: depends on
  wire-witness's exchange record being implemented and on a Rustev remote
  backend that a witness can observe.
depends_on:
  - "004-evaluation-and-replay"
  - "009-remote-adapter-protocol"
  - "012-jev-integration"
references:
  - { unit: { kind: file, path: "docs/backlog.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "A witness record is joined to a Rustev attempt only by an exact match of request byte digests to exactly one exchange record; any other outcome is recorded as unjoined, never guessed."
    anchor: "3-2-joining-a-witness-record"
  - id: "I-2"
    kind: invariant
    text: "Witness evidence never overwrites a Rustev field: requested and served identity, usage and cost from the witness are recorded beside Rustev's as corroborating or conflicting testimony."
    anchor: "3-3-corroboration-not-substitution"
  - id: "I-3"
    kind: invariant
    text: "Captured traffic that no Rustev attempt produced never becomes a decision, replay bundle or evaluation case, and no imported record grants authority."
    anchor: "3-5-what-is-not-imported"
---

# 025: Wire-witness exchanges as remote-exchange and replay evidence (deferred)

Draft, and deferred: a proposal, not a claim about code. Backlog item
RUSTEV-013 has no owning spec, and wire-witness's own backlog carries the
matching item ("use captured exchanges as Rustev replay input", deferred
until Rustev's semantic backend contract is implemented and an explicitly
authorized capture corpus exists). Wire-witness spec 002 already pins
Rustev spec 009 sections 3.1, 3.7 and 3.8 as interface references; this
spec is Rustev's side of that interface. Nothing here changes a Rustev
behavior today.

## 1. Purpose

A Rustev remote adapter reports its own exchanges (spec 009 3.9). A witness
that observes the same wire from outside the process is independent
testimony: it can corroborate served identity, usage and cost, and it may
retain response bytes that Rustev's digest-only default (R-19) did not.
This spec defines how that testimony is used without letting it become a
second source of truth, a decision, or an authority.

## 2. Territory

None. When made concrete, an importer crate under `integrations/` (it names
an external product and parses its record), depending on
`rustev-contract` and `rustev-eval` and no HTTP stack. Its placement is
question 1 of section 6.

## 3. Behavior (to be made concrete)

### 3.1 Inputs

1. Only `wire-witness.exchange/1` records whose canonical digest verifies
   (`wire-witness.exchange/1+keysort-json+sha256`) and that the host states
   were admitted under its own policy. Rustev does not admit witness
   testimony; the host does.
2. The matching `rustev.remote-exchange/1` records and, for replay, the
   `rustev.replay/1` bundle (spec 004 3.2).
3. Records are bounded before parsing, with limits no looser than
   `REPLAY_V1`.

### 3.2 Joining a witness record

Rustev never sends attempt or decision ids on the wire (spec 009 3.3.3), so
the join key is the request: the witness's request byte digest
(`file-bytes-sha256` over the exact observed bytes) must equal the Rustev
exchange record's request digest, and match exactly one record. No match,
several matches, a digest over different bytes (for example after a proxy
rewrote them), or an incomplete witness direction is recorded as unjoined
with its reason. Whether the two digests are over identical bytes is
question 2 of section 6.

### 3.3 Corroboration, not substitution

For a joined pair, the importer reports field by field:

| Field | Rule |
|---|---|
| Requested identity | Compared; a difference is a conflict. |
| Served identity | Witness `known` beside Rustev's value; Rustev's `unknown` stays `unknown` in the run record and exchange record. A witness-known value is reported as witness testimony only. |
| Usage | Compared under provider field names; never summed across sources. |
| Cost | Reported, estimated and unknown slots kept apart; a witness-reported cost does not make Rustev's estimate observed. |
| Completeness | A witness `incomplete` or `absent` direction is carried with its closed reason. |

Conflicts are findings for the host; they never change a judgment, charge
or historical record.

### 3.4 Retained bytes and the mapping check

When the witness retained the response bytes and both retention policies
permit, the bytes may be supplied to spec 009 3.9.4's offline mapping
check: re-running the adapter's mapping must reproduce the supplied
`RawOutput` byte for byte. This checks the mapping, not the model. Replay
itself still reproduces from retained `RawOutput` and makes zero remote
calls (spec 004 3.3.5). The stricter of the two retention policies governs
every retained byte, and imported bytes never outlive the replay bundle
they support (R-19).

### 3.5 What is not imported

Witness records of traffic no Rustev attempt produced (for example a coding
agent's own provider calls) are not decisions, replay bundles or evaluation
cases. They may enter an evaluation only as a dataset under spec 023's
admission, with their own provenance, and never as evidence of a Rustev
judgment. Nothing imported authorizes an action.

## 4. Out of scope

Capture, normalization, redaction and custody (wire-witness). Admission and
policy (the host and statecraft-cli). Live re-execution (spec 018 when
delivered). Training on captured outputs.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| A witness record's digest does not verify | Refused before use. |
| Two Rustev exchange records share the witness's request digest | Unjoined, reason `ambiguous`. |
| The witness reports a served model where Rustev recorded `unknown` | Rustev's record stays `unknown`; the witness value is reported as testimony. |
| Witness bytes are retained but the Rustev bundle is digest-only with no explicit retention | Bytes not imported; the stricter policy wins. |
| A coding-agent exchange with no Rustev attempt is offered as replay input | Refused (I-3). |

## 6. Open questions

1. **Placement.** An importer under `integrations/` (recommended), or a
   witness-side exporter with Rustev accepting only its own record shape?
2. **Digest equality.** Do the Rustev adapter's request digest and the
   witness's request byte digest cover identical bytes (headers excluded,
   body exact)? If not, which side changes, under which versioned schema?
3. **Provider family.** Wire-witness normalizes Anthropic and OpenAI
   families; Jev traffic is `unknown` family there. Is byte-level testimony
   enough, or does wire-witness need a Jev normalization first?

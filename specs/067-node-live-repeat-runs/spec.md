---
id: "067-node-live-repeat-runs"
title: "Node binding: live repeat runs over host backends (amends 027, 043 and 063)"
status: approved
implementation: pending
created: "2026-11-06"
summary: >
  An amendment of approved specs 063 (3.9), 027 (3.3, 3.7) and 043
  (section 2, 3.2.2, 3.5.1, 3.11.4, 3.13), for release 0.5.1 (#539),
  ratified by the owner on 2026-11-06 (A-86). A Node host whose model
  backends are TypeScript `DecisionBackend`s (027 3.6) cannot run fresh
  repeat runs of its plans through Rustev: the binding's repeat commands
  refuse the `live` source (063 3.9.4), and `rustev eval live` links only
  the rules backend (043 3.13.2). So the host keeps its own harness,
  outside Rustev's spend journal and caps. This spec adds
  `engine.liveRun`, an asynchronous method over `rustev_live::run` with
  the CLI's inputs, the engine's registered host backends and the same
  documents as `rustev eval live`, all or none for the final files. The
  journal is a file path the binding opens under 043 3.5's lock and
  durability rules, and no attempt is sent before its full worst-case
  exposure is durably reserved. A host backend's exposure is the cost
  model and bound it already declares (027 3.6.1); an `unknown` model is
  refused. Probe-only, resume and rerun-interrupted are 043's; an
  `AbortSignal` or `shutdown` ends the run `interrupted` with every
  reserved, unsettled attempt kept as liability. The cumulative cap holds
  per journal file, so CI must persist the file; a journal created by a
  run records an id and creation time, and the run report names them, so
  a fresh journal is visible, never silent. Tests use scripted host
  backends only; CI makes no live or paid call. This fulfills 043 section
  2's "the Node binding exposes it in its own spec".
amends:
  - "027-node-and-wasm-bindings"
  - "043-live-re-execution-at-scale"
  - "063-node-binding-parity"
extends:
  # `liveRun`, its options, rule authorizer, cancellation, shutdown and declarations.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  # The `eval live` input resolution moves here behind `Host`.
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: amending }
  # The CLI calls the moved resolution and gains `--require-existing-journal`.
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # Journal identity, the run report's `journal` member and the existing-journal check (3.4.5, 3.4.6).
  - { spec: "043-live-re-execution-at-scale", unit: { kind: directory, path: "crates/rustev-live/" }, nature: amending }
  # The live conformance cases (3.8).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "tools/rustev-conformance/" }, nature: additive }
  # Archive entries for moved source files.
  - { spec: "031-public-api-and-release-family", unit: { kind: directory, path: "tools/rustev-release/" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "003-runtime-execution-and-evidence"
  - "006-cli-surface"
  - "012-jev-integration"
  - "026-live-re-execution"
  - "027-node-and-wasm-bindings"
  - "029-evaluation-repeatability"
  - "031-public-api-and-release-family"
  - "043-live-re-execution-at-scale"
  - "063-node-binding-parity"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "No attempt of a `liveRun` is handed to a host backend unless its full worst-case exposure, from the backend's declared cost model and bound, is durably reserved in the journal first and fits both the run cap and the journal's cap; a binding whose cost model is `unknown`, or that has no entry in the live configuration, is refused before the journal is opened."
    anchor: "3-3-host-backend-cost-declarations"
  - id: "I-2"
    kind: invariant
    text: "Cancellation by `AbortSignal`, the run deadline or `shutdown` never releases a reservation for an attempt that was handed to a host backend and has not settled; its charge stays liability in the journal, its item is `interrupted`, and a late Promise settlement is discarded and never settles the ticket."
    anchor: "3-6-cancellation-and-shutdown"
  - id: "I-3"
    kind: invariant
    text: "For the same inputs, scripted backends and clock, `liveRun` writes the same documents, bytes, status and exit code as `rustev_live::run` called from Rust, and the binding adds no authority: its authorizer is 043 3.12's declarative rule list, and no binding value is a permission."
    anchor: "3-2-the-entry-point"
  - id: "I-4"
    kind: invariant
    text: "A journal created by a live run records a stable `journal_id` and `created_at_ms` in its header, and every `rustev.live-run/1` names the journal's id, creation time, whether this run created it, and the cumulative spend before the run, so a run that started its cumulative cap at zero on a new file is visible and never silent."
    anchor: "3-4-the-journal-and-the-output-directory-are-file-paths"
  - id: "R-1"
    kind: requirement
    text: "Every test and conformance case of this spec uses scripted host backends and a temporary `testing` journal; no CI job makes a live or paid provider call, and the WebAssembly package gains nothing."
    anchor: "3-8-tests-and-conformance"
---

# 067: Node binding: live repeat runs over host backends (amends 027, 043 and 063)

Approved: ratified by the owner on 2026-11-06 (A-86) for release 0.5.1;
the implementation is pending. The approved texts of 027, 043 and 063
are not edited; this spec records what changes in them (3.10). It
authorizes no live run, provider call or spend, and raises no cap.

## 1. Purpose

A Node host needs fresh repeat runs of its own plans with its own model
backends: for example several candidate models per flow, each run against
a baseline, then gated with `gateSelect` (063 3.9). Today it cannot do
this from Rustev:

- the binding's repeat commands keep the `live` source refused
  (`live-source-unavailable`, 063 3.9.4);
- `rustev eval live` runs live, but links only the rules backend (043
  3.13.2), so a host whose backends are TypeScript callbacks (027 3.6)
  cannot run them through the CLI;
- 043 section 2 names `rustev_live::run` the binding equivalent of the
  CLI command and leaves the Node exposure to "its own spec". 063 did not
  take it.

The host therefore runs live repeats in its own harness, outside Rustev's
spend journal, caps, probe and checkpoints, and imports the results
afterwards (061). This spec is the missing exposure.

## 2. Territory

`integrations/rustev-node/` (027): the `liveRun` method, its options,
the rule authorizer, cancellation and shutdown, hand-written declarations
and tests; the crate gains a normal dependency on `rustev-live`.
`crates/rustev-surface/` (027): the input resolution `rustev eval live`
does (live configuration, dataset, candidates, snapshots or bundles,
captures, admission), moved from `crates/rustev-cli/` behind `Host`, as
063 3.9.3 moved the repeat renderings. `crates/rustev-cli/` (006,
internal structure only): it calls the moved resolution; grammar, bytes,
exit codes and goldens unchanged. `tools/rustev-conformance/` (027): the
live cases of 3.8.

Ratification added the `extends` edges to `integrations/rustev-node/`
and `crates/rustev-surface/` (027, amending), `crates/rustev-cli/` (006,
amending), `crates/rustev-live/` (043, amending, for 3.4.5 and 3.4.6),
`tools/rustev-conformance/` (027, additive) and `tools/rustev-release/`
(031, additive, archive entries for moved files); the edges to
`Makefile` and `scripts/acceptance/` (001, additive) are added by the
change that completes this spec.

Unchanged: `rustev-contract`, `rustev-core`, `rustev-runtime`,
`rustev-eval`, and `rustev-live` behavior except 3.4.5's journal
identity and run report member (every other function used exists); the
journal's line format, existing headers and lock (043 3.5); every other
document, schema, generated declaration, `PlanId` and golden; `rustev
eval live` apart from 3.4.5; the
WebAssembly package; `make boundaries` (`rustev-live` is not an
integration crate, and `rustev-node` is under `integrations/`).

### 2.1 Binding principles

- **Proposals, never grants.** Live results are evidence; the rule
  authorizer's allow covers one dispatch inside Rustev, is not exported,
  and is not a `Permitted` value.
- **Derivation and lineage.** Every live result is 043 3.9's new run
  record with its `rustev.live-reexecution/1` and `rustev.live-case/1`
  documents; the binding relabels nothing.
- **Unresolved, never a default.** Unknown exposure is refused; an
  unknown charge is liability; a thrown or late host answer is never zero
  cost or stopped work (027 3.6.5).
- **Pure core; no async runtime in contract and core.** Untouched.
- **No dynamic loading; explicit composition.** Backends are the
  engine's registered host objects (027 3.6.7); nothing is resolved by
  name at run time.

## 3. Behavior

### 3.1 What changes for 063's commands

1. `liveRun` joins 063 3.9.1's commands. 063 3.9.4's "the `live` source
   stays refused (`live-source-unavailable`) as in the CLI" stands for
   `eval` with `repeatConfig`: that command still cannot produce a live
   set. A live set is produced only by `liveRun`, whose repeat outputs
   (043 3.11.2) `baselineRecord`, `evalReproducibility`, `evalDiff` and
   `gateSelect` read unchanged, as the CLI's spec 029 commands read `eval
   live` output.

### 3.2 The entry point

1. `engine.liveRun(options): Promise<LiveRunResult>`, with these options,
   one per `rustev eval live` flag (043 3.13), in camel case:
   `live: true` (required, as `--live`), `liveConfig`, `journal` (a file
   path, 3.4), `runId`, `dataset`, `split`, `config`, `adapter`, the
   scope handles, `nowMs` (required), `repeatConfig`, `candidates`
   (043's candidate layout), exactly one of `snapshots` or `bundles`,
   optional `captures`, `admission`, spec 029's ledger options,
   `runDeadlineMs`, the booleans `probeOnly`, `resume`,
   `rerunInterrupted` and `requireExistingJournal` (3.4.6, default
   false), `out` (a directory path, 3.4), and `signal`
   (`AbortSignal`, 3.6). `repeat` is refused, as `--repeat` is.
2. Usage rules (043 3.13.1, 3.13), checked before any input is read,
   throw `InvalidDocument` naming the rule: `live` not `true`, a missing
   `liveConfig`, `journal`, `out` or `nowMs`, both or neither of
   `snapshots` and `bundles`, `split: "final_test"`.
3. The binding resolves the inputs with the function the CLI uses
   (section 2), prepares the run (`rustev_live::run::prepare`, every
   refusal before the journal is opened), opens the journal (3.4) and
   executes on the engine's runtime, with the engine's registered
   backends and evidence sink, the engine's clock (`system` or
   `ManualClock`) and the rule authorizer of 3.5. Work runs on the
   engine's runtime and blocking pool, never on the JavaScript thread
   (027 3.5.2).
4. **Outcomes are values.** `LiveRunResult` is `{ code, out, status }`:
   `code` the exit code, `out` the CLI's stdout line bytes (043 3.13.4),
   `status` one of `evaluated_live`, `probed`, `probe_failed`,
   `spend_capped`, `interrupted`, with codes 0, 0, 15, 14 and 16 (043
   3.14, whose numbers stay those `rustev-live` assigns). A refusal of
   043's `invalid_input` codes is `code` 4 with the code named in `out`;
   `journal-locked` and a write that cannot be made durable are `code` 3.
   None is an exception.
5. **Documents.** `out` receives exactly 043 3.13.3's files: the
   configuration, `checkpoint.json`, `cases/`, `probe/`, and at the end
   `live-run.json` with the repeat outputs of 043 3.11.2, all or none for
   the final files. An existing `out` without `resume` is `code` 3. For
   the same inputs, backends and clock the bytes equal what
   `rustev_live::run` writes when called from Rust (I-3).

### 3.3 Host backend cost declarations

1. A TypeScript `DecisionBackend` already declares its full descriptor,
   its `CostModel` and one `CostBound` for every projection (027 3.6.1).
   That bound is the attempt's worst-case exposure: `bounded{max_units}` or
   `estimated{units}` is reserved in full before the attempt is enqueued
   to the host (043 3.6.3). Each retry and fallback reserves anew (043
   3.6.7).
2. The cost **unit** is the live configuration's per-binding `cost_unit`
   (043 3.2.1), keyed by the descriptor's backend and artifact identity,
   so it is part of the `LiveConfigId`; the backend registration
   declares no unit of its own. `nano_usd`
   states that the backend's units are nano-USD; Rustev cannot verify it,
   and the run records the statement.
3. Refused before the journal is opened (`invalid_input`): a backend whose
   `CostModel` is `unknown` (`spend-unit`); a binding any candidate
   reaches with no entry in the live configuration, or an entry no
   candidate reaches (`live-config`); a registered descriptor that differs
   from the binding entry's identity (`live-config`). A projection-dependent
   bound stays out (027 section 4).
4. `cost_unit: none` is accepted on the host's declaration, as 043 3.6.3
   allows for the library, reserved at zero and recorded as
   `declared_no_spend` in every per-case document and in
   `rustev.live-run/1`.
5. **Charges.** The charge an attempt ends with settles its ticket: the
   charge the host's result reports, or the charge a thrown `SeamFailure`
   carries; any other throw, a rejection or a malformed value is charge
   `Unknown` (027 3.6.5), kept as liability, never zero. A full seam queue
   (027 3.6.2) sends nothing and releases its ticket (043 3.6.6).

### 3.4 The journal and the output directory are file paths

1. `journal` is the path of a spec 012 / 043 3.5 journal file that the
   binding opens through `rustev_live::journal`, with every rule of 043
   3.5 unchanged: append-only lines, `fsync` per line, one writer by
   `<journal>.lock` (a journal another process or another `liveRun` holds
   is `journal-locked`, `code` 3), a `testing` header only
   (`journal-budget`), the cap from the header, the A-75 header migration,
   and the run spend computed from the journal. There is no default path
   and no default cap.
2. `out` is a directory path the binding writes directly, because 043
   3.10.1's checkpoint durability (temporary file, `fsync`, rename) needs
   a real directory. Inputs may be paths or 063 3.9.4 input prefixes under
   the filesystem host; the memory host is refused for `liveRun`
   (`InvalidDocument`, `durable-paths-required`).
3. **Reserve before send.** Whatever the journal's form, 043's I-2 holds:
   the reservation line is durable (written and `fsync`ed) before the
   attempt is enqueued to the host backend's threadsafe function, and an
   attempt whose reservation fails is never enqueued. No host journal
   seam is offered: it would need a synchronous durable reservation the
   binding cannot obtain from JavaScript (027 3.6.1).
4. **The total cap is per journal file.** The journal's cap (043 3.5.4)
   is cumulative only over the lines of one file. A new file starts the
   cumulative spend at zero. A host running live jobs in CI must keep the
   journal on persistent storage (a persistent runner, or durable storage
   the job restores before the run and saves after it, lock released) or
   accept that each fresh journal restarts the cumulative cap at zero.
   Rustev cannot tell a lost journal from a first one; it makes the
   difference visible (3.4.5) and refuses to create one when asked
   (3.4.6).
5. **A new journal is visible (amends 043 3.5.1 and 3.11.4).**
   - **Header.** A journal created by this version records two members
     in its header line, beside `schema`, `budget` and `cap`:
     `journal_id`, the `tagged_digest("rustev.spend-journal-id/1",
     created_at_ms || 0x00 || run_id)` of the creating run, as lowercase
     hex, and `created_at_ms`, read from the run's injected clock. Both are
     written once, at creation, and never change; the A-75 migration (043
     3.5.6) keeps them. The schema name stays `rustev-jev.spend-journal/1`
     and the members are optional. A journal created before this version has neither
     member; it is accepted, never stamped (no creation time is invented),
     and is reported as `legacy`.
   - **Run report.** `rustev.live-run/1` (043 3.11.4) already carries
     `journal_before` and `journal_after`, the journal's cumulative
     observed, estimated, liability and outstanding units and its cap
     before and after the run; they are kept. It gains one member,
     `journal: { id: hex | null, created_at_ms: integer | null, created:
     "this_run" | "earlier" | "legacy" }`: the header's identity, and
     whether this run created the file. A host or reviewer reading a run
     that reports `created: "this_run"` with zero `journal_before` spend
     sees that the cumulative cap started at zero for it.
   - **Result.** `LiveRunResult` gains `journal: { id, createdAtMs,
     created, spendBefore }` from the same values, and the stdout line
     (043 3.13.4) gains `new_journal` when `created` is `this_run`, for
     the CLI and the binding alike.
   - **Resume.** The checkpoint already binds the journal header digest
     (043 3.10.1). Today every fresh `testing` header has the same digest,
     so a resume against a replaced journal is not detected; with
     `journal_id` in the header it is `checkpoint-mismatch` before the
     journal is used. 043 3.10.2's superseded-cap exception keeps working
     for a header that differs only in its cap.
   - **Compatibility.** The members are written only for journals created
     by this version and the run report's `journal` member is new, so
     existing journal files, run reports and checkpoints keep their
     bytes. A journal created by this version is refused by an earlier
     release (unknown header member), never read as a journal without an
     identity. `rustev eval live` goldens that create a journal gain the
     header members, the `journal` member and `new_journal`, and are
     re-blessed in the change that implements this; no `PlanId`, plan
     golden or other document changes.
6. **Requiring an existing journal (amends 043 3.13).** The option
   `requireExistingJournal` (CLI `--require-existing-journal`, a switch),
   off by default so that a first local run can create its journal,
   refuses a journal path that does not exist (`invalid_input`,
   `journal-missing`) before any file, lock or output is created and
   before anything is sent. With it, a CI job pointed at a persisted
   journal that was not restored fails instead of starting a fresh
   cumulative cap. It is the documented setting for CI jobs. On an
   existing journal it changes nothing.

### 3.5 Authorization

1. `rustev_live::run` takes a synchronous `LiveAuthorizer`. Under 027
   3.6.1 a synchronous seam method is supplied as declared data, not as a
   callback, so the binding takes no authorizer callback. It builds the
   CLI's authorizer from the live configuration's `authorize` rules (043
   3.12): ordered rules naming a backend target, artifact, operation set
   and `max_exposure_units`; an empty list refuses all external work;
   `cost_unit: none` bindings still need a rule.
2. This amends 043 3.2.2 for the Node binding: like the CLI, `liveRun`
   requires `authorize` present, and a configuration without it is
   refused (`live-config`). A Rust host calling the library keeps 043's
   rule (absent, host authorizer). Requiring the rules in the
   configuration makes the approval of each paid run an explicit, recorded
   part of its `LiveConfigId`. No asynchronous host authorizer is offered.

### 3.6 Cancellation and shutdown

1. `signal` (an `AbortSignal`) and `runDeadlineMs` map to the run's
   `CancelSignal` (027 3.5.3). An already-aborted signal ends the call
   before the journal is opened, with nothing written.
2. On abort or run deadline, no new attempt is reserved or started;
   in-flight attempts are cancelled under 026 3.5: each host call's
   `AbortSignal` is aborted (advisory to the host), the runtime stops
   waiting at the attempt's deadline, and each attempt settles with the
   charge it reports or, when its host answer never arrives, keeps its
   reservation as liability (I-2). Completed items are checkpointed; items
   with a reservation and no checkpointed result are `interrupted`. The
   run ends `interrupted` (16) with `checkpoint.json` written and resumes
   with `resume: true`.
3. A host Promise that settles after the runtime stopped waiting is
   discarded and counted in the backend seam's
   `lateResolutionsDiscarded` (063 3.4.5); it never settles a ticket, so a
   late charge never lowers liability.
4. `engine.shutdown({ drainMs })` raises cancellation for a running
   `liveRun` as for decisions (063 3.4.6). It waits up to `drainMs` for
   in-flight attempts to settle and the checkpoint to be written, then
   resolves with `abandonedPending` counting host calls still unsettled;
   their reservations stay liability. The journal lock is released when
   the run ends. A process that exits without shutdown leaves the journal
   under 012's crash rules: every reservation already durable is
   liability, counted once, and the items are `interrupted` at resume.

### 3.7 Probe, resume and rerun

1. `probeOnly: true` runs only 043 3.7 and writes `rustev.live-run/1` with
   the probe section; status `probed` (0) or `probe_failed` (15). A full
   run whose probe fails sends no dataset case (043's R-2).
2. `resume: true` with the same `out` reopens the checkpoint under 043
   3.10.2, refuses `checkpoint-mismatch` before the journal opens, never
   re-sends a `completed` item and reads the run spend from the journal,
   so a reached cap stays reached.
3. `rerunInterrupted: true` re-runs `interrupted` items as 043 3.10.4
   states: decision id suffix `/rerun-<k>`, `rerun{k}` recorded durably
   before the first reservation, a new reservation, the extra exposure
   reported, `refused{rerun-limit}` above 99. Without it they are
   incomparable with `live-interrupted`.

### 3.8 Tests and conformance

1. Every test uses scripted TypeScript backends (fixed outputs, scripted
   delays, throws, `SeamFailure`s, never-settling Promises) declaring
   `bounded` or `estimated` cost models, a `ManualClock`, synthetic
   datasets and a temporary `testing` journal. No test or CI job sends a
   request to a live or paid provider (R-1).
2. **Conformance.** Cases with surfaces `rust-api` and `native`: a full
   run, a probe-only run, a probe failure, `spend_capped` by the run cap
   and by the journal cap, and a resume after a mid-run abort; each
   mirrors an existing `rustev-live` test and compares every written
   document's bytes and the status between both runners (I-3).
3. **Host-seam cases** (`node --test`): an `unknown` cost model refused
   before the journal exists; a reservation line present before the
   scripted backend observes the call; abort with a never-settling
   in-flight attempt (liability kept, item `interrupted`, late settlement
   discarded); shutdown during a run; a second `liveRun` on a locked
   journal (`code` 3); the memory host refused.

### 3.9 Errors, declarations and position

1. No new error class. Usage rules use `InvalidDocument` (3.2.2); all run
   outcomes are values (3.2.4).
2. `liveRun`, its options and `LiveRunResult` are declared by hand in
   `index.d.ts`; no generated declaration changes.
3. The WebAssembly package gains nothing: it has no runtime, backends or
   file system (027 3.10.1, 063 3.12).

### 3.10 What changes in specs 027, 043 and 063

1. **027:** 3.3's API gains `liveRun`; section 4's "remote backends linked
   into the binding" is unchanged (host backends are TypeScript objects).
2. **043:** section 2's "the Node and WebAssembly bindings ... expose it
   in their own specs" is fulfilled for Node by this spec; 3.2.2's
   "`authorize` ... the library requires absent" does not apply to the
   Node binding (3.5.2); 3.5.1's "header ... unchanged" gains the two
   creation members for new journals, and 3.11.4's `rustev.live-run/1`
   gains `journal` (3.4.5), for the CLI and the binding alike; 3.13 gains
   `--require-existing-journal` (3.4.6).
3. **063:** 3.9.1's table gains `liveRun`; 3.9.4's `live` refusal stands
   for `eval` with `repeatConfig` (3.1).

## 4. Out of scope

A host journal seam; an asynchronous host authorizer; projection-dependent
cost bounds and batch host backends (027 section 4); live acquisitions
through a host source in a live run beyond what 063's source seam and 043
3.4 already allow; live runs in the WebAssembly package or the service;
raising any cap; any live or paid provider call in CI; quality claims.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `live` omitted or not `true` | `InvalidDocument`; nothing read. |
| `repeat` given | `InvalidDocument`. |
| `split: "final_test"` | `InvalidDocument`. |
| A host backend with cost model `unknown` | `code` 4, `spend-unit`; journal not opened. |
| A reached binding with no live configuration entry | `code` 4, `live-config`. |
| A configuration without `authorize` | `code` 4, `live-config`. |
| A non-`testing` journal | `code` 4, `journal-budget`. |
| A journal locked by another run | `code` 3, `journal-locked`. |
| The memory host | `InvalidDocument`, `durable-paths-required`. |
| `requireExistingJournal: true` on a missing journal path | `code` 4, `journal-missing`; no file created, nothing sent. |
| An attempt whose reservation would pass the run cap | Not enqueued; run ends `spend_capped`, `code` 14. |
| A probe call failing | `probe_failed`, `code` 15; no dataset case sent. |
| Abort with an attempt in flight whose Promise never settles | `interrupted`, `code` 16; its reservation stays liability; later settlement discarded. |
| A thrown non-`SeamFailure` error | Charge `Unknown`, liability; never zero. |
| `resume` with another live configuration | `code` 4, `checkpoint-mismatch`; journal not opened. |
| `resume` over a completed item | Not re-sent. |
| An interrupted item without `rerunInterrupted` | Incomparable, `live-interrupted`. |
| `eval` with `repeatConfig` naming the `live` source | Still `live-source-unavailable` (3.1). |
| A run on a journal path that does not exist | Journal created with `journal_id` and `created_at_ms`; `live-run.json` reports `created: "this_run"` and zero `journal_before` spend; stdout `new_journal`. |
| A run on an existing journal | `created: "earlier"` with the header's id and creation time and the prior cumulative spend. |
| A journal created before this version | Accepted; `created: "legacy"`, id and creation time null; header not rewritten. |
| `resume` after the journal file was replaced by a fresh one | `code` 4, `checkpoint-mismatch`. |

## Acceptance

- `liveRun` with every option of 3.2.1 and every usage rule of 3.2.2.
- Byte equality with `rustev_live::run` from Rust over the same inputs,
  scripted backends and `ManualClock`, for each status (I-3).
- The cost rules of 3.3 and the journal rules of 3.4, including the
  reservation-before-enqueue ordering, with scripted backends.
- Cancellation, deadline and shutdown as 3.6 states, with liability kept
  and late settlements discarded.
- Probe-only, resume and rerun-interrupted as 3.7 states.
- Journal visibility (3.4.5), with scripted backends and a `ManualClock`
  only: a first run on a missing path reports `created: "this_run"`,
  the id and creation time, zero `journal_before` spend and
  `new_journal`; a second run on the same file reports `earlier`, the
  same id and the first run's spend before it; a run on a fresh file
  after deleting the first reports a new id and zero spend; a legacy
  journal reports `legacy` and keeps its header bytes; a resume against a
  replaced journal is `checkpoint-mismatch`; `requireExistingJournal`
  (and the CLI's `--require-existing-journal`) on a missing path refuses
  with `journal-missing` and creates no file, and on an existing journal
  runs as without it.
- The conformance and host-seam cases of 3.8; every row of section 5 has
  a test; no test makes a live or paid call.
- Compatibility: every CLI, eval, live and conformance golden and every
  plan golden byte-identical; `rustev eval live` unchanged.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

Planned increments, each a separately reviewed pull request from `main`
of about 1,000 lines (the AI review times out near 1,000 to 1,800
reviewed lines), so this spec's 0.5.1 risk is its size: four increments
must land in order. Nothing here claims an acceptance item before the
increment that satisfies it.

1. **Journal, cost declarations and authorization.** `rustev-node`
   depends on `rustev-live`; the host backend cost rules of 3.3 checked
   against a live configuration, the journal opened by path under 3.4,
   and the rule authorizer of 3.5, as Rust functions of the binding with
   tests over scripted backends; no JavaScript method yet. The journal
   identity and report members of 3.4.5 and `--require-existing-journal`
   (3.4.6) land here, in `rustev-live` and the CLI, with the re-blessed
   `eval live` goldens.
2. **The entry point.** The input resolution moved into `rustev-surface`
   (the CLI calls it, goldens unchanged); `engine.liveRun` for full and
   probe-only runs, outcomes, documents and declarations (3.2, 3.7.1),
   with `requireExistingJournal`; abort and run deadline (3.6.1 to
   3.6.3); `node --test` cases. Increments 1 and 2 together hold the
   journal and the Node entry point, the part that must ship in 0.5.1 if
   not all four fit (owner answer 5).
3. **Resume, rerun and shutdown.** `resume`, `rerunInterrupted` (3.7.2,
   3.7.3), shutdown during a run (3.6.4), and the conformance cases of
   3.8.2 with both runners.
4. **Closure.** The host-seam cases left, declared verification, 067 in
   `VERIFIED_SPECS` and an acceptance group, and the implementation
   record.

## Decisions at approval

Decided in this text, with the reason beside each: the exposure is the
cost model and bound a host backend already declares (3.3.1, 027 3.6.1);
outcomes are values with 043's statuses and exit codes (3.2.4, 063
3.9.2); `eval` with `repeatConfig` still refuses `live` (3.1); tests use
scripted backends only (R-1); the cap is per journal file and a new
journal is made visible (3.4.4, 3.4.5).

Ratified by the owner on 2026-11-06 (A-86), with every question the
draft left open answered as follows; the normative text above states
the adopted behavior.

1. **Journal form: a file the binding opens** (3.4.1, 3.4.3). Live runs
   are evaluation jobs started by hand, locally or in CI, never inside a
   partner API service, so a local file fits; the cap holds per file,
   and a new journal is visible (3.4.4, 3.4.5).
2. **`requireExistingJournal` / `--require-existing-journal`,** off by
   default, the documented CI setting (3.4.6).
3. **The Node binding requires `authorize`** in the live configuration,
   reversing 043 3.2.2 for the binding only (3.5): it matches the owner's
   rule of approval before each paid run. No asynchronous host authorizer.
4. **The memory host is refused for `liveRun`** (3.4.2): the owner's
   evaluation jobs run on a real filesystem.
5. **Release scope.** All four increments are targeted for 0.5.1. If
   only part fits, the shipped part must contain the Node entry point and
   the journal: increments 1 and 2 of the plan already contain both (the
   journal, its visibility and `--require-existing-journal` in 1;
   `liveRun` with `requireExistingJournal` in 2), so they ship first and
   in that order; increments 3 and 4 (resume, rerun, shutdown,
   conformance and closure) may follow.
6. **The cost unit** lives only in the live configuration's binding
   entry, inside the `LiveConfigId` (3.3.2).
7. **`cost_unit: none` for host backends** is accepted on the host's
   declaration and recorded as `declared_no_spend` (3.3.4, 043 3.6.3's
   library rule).
8. **Header form.** `journal_id` and `created_at_ms` are optional header
   members under the unchanged schema name, written only at creation;
   existing journals keep their bytes, and an older release refuses a new
   journal rather than misreading it (3.4.5).

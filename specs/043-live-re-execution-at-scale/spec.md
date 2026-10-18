---
id: "043-live-re-execution-at-scale"
title: "Live re-execution at dataset scale with spend caps (amends 026 and 006)"
status: approved
implementation: pending
created: "2026-10-17"
summary: >
  An amendment of approved spec 026 (live re-execution) and of spec 006
  (CLI), for #76 child E3 (#74). A new crate `rustev-live` runs a dataset,
  or the inputs (never the outputs) of retained replay bundles, through one
  or more candidate plans live, through spec 026's `reexecute_live`, under
  an explicit `--live` opt-in, a per-run spend cap and the persistent spend
  journal of spec 012, generalized to any backend with a cost model and
  moved into `rustev-live`. A runtime spend gate durably reserves every
  attempt's full worst-case exposure before any byte is sent and refuses
  an attempt that would pass the run cap or the journal's cap (R-29: USD 5
  in total for all testing across providers). Acquisitions are declared
  live or captured per source; a probe checks reachability and served
  identity of every candidate binding before any dataset case is sent;
  scheduling is bounded per binding; runs resume from a checkpoint without
  re-sending a completed case or counting a charge twice. Every result is
  new evidence marked `live_reexecution` with a link to its source case or
  record, and per-run outputs are spec 029 repeat outputs with source
  `live`, so E1 and E2 consume them unchanged. Approved (A-40); approval
  authorizes no live run, provider call or spend by itself, raises no cap,
  and makes no quality claim.
amends:
  - "026-live-re-execution"
  - "006-cli-surface"
establishes:
  - { kind: directory, path: "crates/rustev-live/", planned: true }
extends:
  # `rustev.live-reexecution/1` gains mode, purpose, source and run members (3.9).
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  # The spend gate seam on the live entry point (3.6).
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  # Evaluation of observed live results and the no-runtime dependency test (3.11, 3.1).
  - { spec: "004-evaluation-and-replay", unit: { kind: directory, path: "crates/rustev-eval/" }, nature: amending }
  # `rustev eval live` and three new exit codes (3.13, 3.14).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # The journal source moves to `rustev-live`; `rustev-jev` re-exports it unchanged (3.5).
  - { spec: "012-jev-integration", unit: { kind: directory, path: "integrations/rustev-jev/" }, nature: additive }
  # The new crate in the workspace, and 043 in `make verify` at completion.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "006-cli-surface"
  - "012-jev-integration"
  - "013-remote-state-on-failed-attempts"
  - "014-report-integrity"
  - "015-per-case-bundle-failures"
  - "023-real-data-quality-qualification"
  - "026-live-re-execution"
  - "029-evaluation-repeatability"
  - "036-mid-plan-acquisition"
references:
  - { unit: { kind: file, path: "docs/design/001-decision-engine-architecture.md" }, role: context }
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "Offline replay, evaluation, comparison and gate code paths still cannot dispatch a backend: `rustev-eval` has no normal dependency on `rustev-runtime` or `rustev-live`, and only `rustev-live`'s run entry point, reached through `reexecute_live`, may request external work."
    anchor: "3-1-live-evaluation-entry-points"
  - id: "I-2"
    kind: invariant
    text: "No attempt of a live run is sent unless its full worst-case exposure is durably reserved in the persistent journal first and fits both the run cap and the journal's cap; an attempt whose exposure is unknown, whose cost unit is undeclared, or that would pass either cap is refused before any request byte, with its refusal code."
    anchor: "3-6-spend-gate-and-refusal-before-sending"
  - id: "I-3"
    kind: invariant
    text: "Resumption never re-sends a case whose result was checkpointed and never counts one charge twice: an attempt reserved but not settled before an interruption remains liability in the journal and its case is reported interrupted, re-run only under an explicit flag with new attempt identities."
    anchor: "3-10-checkpoints-and-resumption"
  - id: "I-4"
    kind: invariant
    text: "Every live result is a new run record plus a `rustev.live-reexecution/1` document with `mode` `live_reexecution` and a link to its source case or record; no dataset, bundle, capture, run record, judgment or report used as a source is modified."
    anchor: "3-9-evidence-of-live-re-execution"
  - id: "I-5"
    kind: invariant
    text: "Without `rustev eval live`, every existing CLI command, library function, document, golden and plan identity is byte-identical, and the journal file format is unchanged."
    anchor: "3-15-compatibility-and-identities"
  - id: "R-1"
    kind: requirement
    text: "An acquisition declared captured is served only from a capture keyed by its request digest; a missing capture is missing evidence for that acquisition and never falls back to a live request."
    anchor: "3-4-live-or-captured-acquisitions"
  - id: "R-2"
    kind: requirement
    text: "No dataset case is dispatched until every candidate binding has passed its probe; a failed probe ends the run with `probe_failed` and no case sent."
    anchor: "3-7-probe"
---

# 043: Live re-execution at dataset scale with spend caps (amends 026 and 006)

Approved by the owner (A-40, 2026-10-17, under the 2026-10-16 delivery work
order) as its own reviewable change before any code (R-16); implementation
is pending. This spec implements and extends spec 026 for #76 child E3
(#74), which supersedes backlog item RUSTEV-008. The approved texts of
specs 026 and 006 are not edited; this spec records what changes, as 029
does for 004 and 006. Approval authorizes no live run, provider call or
spend by itself, raises no cap, admits no dataset, and makes no quality,
calibration, latency or cost claim (R-04, R-37). Live runs under this spec
are bound by the work order's terms (Decisions at approval, 12).

## 1. Purpose

Offline replay reproduces history; it cannot measure a new model, prompt
template or profile. Model and prompt selection (spec 029's gates) need
fresh executions with real latency and cost, over a whole dataset, without
ever being confusable with replay and without ever contacting a provider by
accident or spending past a cap.

Spec 026 defines one live re-execution of one decision. This spec runs
many: every case of a dataset (or every retained bundle's inputs), for
every candidate, for N repeats, under a hard spend cap held in a
persistent journal, with a probe before spending, bounded scheduling,
resumption, explicit acquisition posture and evidence that names itself.

## 2. Territory

When delivered:

- `crates/rustev-live/` (established here, owner spec 043): the run
  engine, the configuration, checkpoint and run documents
  (`rustev.live-config/1`, `rustev.live-checkpoint/1`,
  `rustev.live-run/1`), the probe, the per-binding scheduler and the
  persistent spend journal (3.5). Normal dependencies: `rustev-contract`,
  `rustev-core`, `rustev-runtime`, `rustev-eval`, `serde`, `serde_json`,
  `sha2` and `tokio` (features `sync`, `time`, `rt`). No HTTP stack, cloud
  SDK or telemetry exporter, so it is not under `integrations/`; it does
  file I/O only on paths its caller names.
- `crates/rustev-contract/` (owner spec 002): the members 3.9 adds to
  `rustev.live-reexecution/1`, which spec 026 places there and which is
  not yet implemented.
- `crates/rustev-runtime/` (owner spec 003): the spend gate seam of 3.6,
  consulted only on the `reexecute_live` path.
- `crates/rustev-eval/` (owner spec 004): `evaluate_observed` (3.11), a
  pure function, and the dependency test of 3.1.
- `crates/rustev-cli/` (owner spec 006): `rustev eval live` (3.13) and
  three new exit codes (3.14).
- `integrations/rustev-jev/` (owner spec 012): its `spend` module becomes a
  re-export of `rustev_live::journal`, with no behavior or format change
  (3.5).
- `Cargo.toml`, `Cargo.lock`, `Makefile` (owner spec 001): the crate joins
  the workspace; 043 joins `make verify` at completion.

The library function `rustev_live::run` is the binding equivalent of the
CLI command; the Node and WebAssembly bindings (#76 A1) and the service
(A2) expose it in their own specs.

Spec 036 (#76 B1, mid-plan acquisition) defines the acquisition seam,
`rustev.replay/2` and `rustev.run/3` this spec reads, and is in `depends_on`.
Nothing in this spec requires spec 036 to function for
plans without acquisitions.

### 2.1 Binding principles of #76

- **Proposals, never grants.** A live result is a proposal and evidence,
  exactly as under spec 026. The spend gate's allow and the CLI's
  authorization rules (3.12) authorize one dispatch inside Rustev and
  nothing else; neither is a `Permitted` value, neither is exported as
  permission, and a selection over live results remains evidence for the
  host's release process.
- **Derivation classes and lineage.** Live values carry the derivation
  class and lineage the runtime gives them; a captured acquisition keeps
  the derivation and lineage it was captured with, plus the capture
  digest. No value is relabeled because it came from a live run.
- **Unresolved, never a default; no silent kind conversion.** Unknown
  exposure is refused, never treated as zero; an unknown charge is
  liability, never zero; a missing capture is missing evidence; a missing
  source item makes a case unavailable, never filled from history.
- **Pure core; no async runtime in contract and core.** Core is untouched;
  contract gains data types only. The journal, clock and scheduler live in
  `rustev-live`.
- **New dependency classes only under `integrations/`.** None is added;
  `make boundaries` is unchanged. `rustev-live` depends on no crate under
  `integrations/` (spec 001, 3.4.4); provider backends reach it by being
  passed in by a host program, a binding or a service.
- **No dynamic plugin loading.** Backends are values the caller constructs
  and passes; candidates are files the host names; nothing is loaded as
  code.
- **No identity, authority or knowledge store.** The journal, checkpoint
  and outputs are files at host-named paths; Rustev keeps no store and
  owns no authorization (spec 026, 3.3).
- **Plan identities.** No plan construct is added. Every plan and
  definition golden keeps its bytes and `PlanId`.

## 3. Behavior

### 3.1 Live evaluation entry points

1. `rustev_live::run(config, inputs, candidates, journal, authorizer,
   clock, cancel)` is the only function that performs a live evaluation.
   Every case it executes goes through spec 026's `reexecute_live`, with
   that spec's new decision, snapshot, authorization, budget, cancellation
   and evidence rules, as amended here.
2. Offline replay, `evaluate`, `evaluate_gate`, candidate comparison and
   every spec 029 function keep spec 026 3.1's separation: they take
   retained documents only and have no path to a backend, the runtime or
   `rustev-live`. A test reads `crates/rustev-eval/Cargo.toml` and fails if
   its normal dependencies name `rustev-runtime`, `rustev-live` or `tokio`.
3. `rustev replay`, `rustev eval` and every spec 029 command are unchanged
   and never enter the live path; `rustev eval live` is a distinct command
   (3.13) and is matched before `rustev eval`.

### 3.2 Live configuration

1. `rustev.live-config/1` (`REPLAY_V1`, canonical, identity-bearing, no
   binary64 numbers; its digest is the `LiveConfigId`) declares:
   - `budget`: `testing` (the only value this spec admits; 3.5.4);
   - `run_cap_units`: the run's cap, an integer of the journal's unit
     (one nano-USD), at least 1;
   - `repeats`: N, 1 to 64 (spec 029 3.1);
   - `evaluation_time`: `source` (each case's recorded evaluation time,
     3.3.3) or `fixed{ms}`;
   - `bindings`: per backend target any candidate reaches (by the
     descriptor's backend and artifact identity): `cost_unit`
     (`nano_usd` or `none`, 3.6.3), `max_in_flight` (1 to 64),
     `min_interval_ms` (0 to 600 000), `case_deadline_ms` (1 to
     3 600 000) and `rate_limit_pause_max_ms` (0 to 600 000);
   - `acquisitions`: per acquisition source the plans declare (3.4),
     `live` or `captured`;
   - `probe`: `calls` (1 to 5, default 3), `interval_ms` (0 to 60 000),
     `snapshot` (the path of the probe snapshot file), its `digest` and
     its `provenance`, which must be `synthetic` (3.7);
   - `authorize`: the CLI's authorization rules (3.12), absent for the
     library, where the host supplies the authorizer;
   - `compare_with_source`: whether bundle inputs get spec 026 3.8
     comparison (default false).
2. Every member above is required except where a default is stated
   (`probe.calls`, `compare_with_source`) and except `authorize`, which
   the CLI requires and the library requires absent. Unknown members, a
   missing required member, `authorize` missing in the CLI or present in
   the library, duplicate binding entries, a binding no candidate
   reaches, or a candidate binding with no entry are refused
   (`invalid_input`, `live-config`) before any journal is opened.

### 3.3 Inputs and snapshots

1. **Dataset cases.** A `rustev.dataset/1` manifest and its split; each
   case's snapshot bytes come from `--snapshots <dir>` (a regular file
   whose computed `SnapshotId` equals the case's) or from a bundle (3.3.2).
   A case whose snapshot is not found is `unavailable` with reason
   `snapshot-missing`, never sent.
2. **Retained bundle inputs.** From a `rustev.replay/1` or
   `rustev.replay/2` bundle (the latter introduced by spec 036) a live run
   reads only `decision_id` (as the source link), `scope`,
   `evaluation_time_ms`, `snapshot_id`, `snapshot` and `expires_at_ms`,
   and, under 3.4, `acquisition_pages`. It never reads `run`, `supplies`,
   `plan`, `descriptors` or `calibrations`: candidate plans and
   their bindings are explicit (spec 026, 3.1). A bundle that is expired
   at the run's `now_ms`, inaccessible or invalid makes its case
   `unavailable` with spec 015's reason; the run does not restore it
   (spec 026, 3.7).
3. **Amends spec 026 3.2 (snapshot and time).** For a dataset-scale run
   the new snapshot may be built from a dataset case's or a bundle's
   retained snapshot bytes. They are parsed and validated anew through the
   normal snapshot contract and their `SnapshotId` is computed anew; it
   equals the source's only when the bytes are equal, and a mismatch is
   refused (`identity-mismatch`). The evaluation time is the case's
   recorded time when `evaluation_time` is `source`, or the fixed value;
   with `--snapshots` inputs, which carry no time, `source` is refused
   (`evaluation-time-unavailable`). Either choice is recorded per case.
   Holding snapshot and time to the source isolates a model change from
   context drift; the decision id, admission, budgets, authorization and
   every observation remain new, as spec 026 3.2 requires.
4. **Decision ids.** Each case of candidate `c` and repeat `r` runs as
   decision `live/<run_id>/<c>/<r>/<case_id>`, with `/rerun-<k>`
   appended for a rerun (3.10.4). Every component is free of `/`:
   `run_id` is host-supplied, 1 to 64 bytes of `[A-Za-z0-9._-]`; a
   candidate name follows spec 029 3.13.5 (spec 006 3.9.1's case id rule,
   `[A-Za-z0-9._-]` starting with an alphanumeric) and is limited here to
   32 bytes; `r` is the decimal repeat index, 1 to 64; `case_id` follows
   spec 006 3.9.1 (at most 128 bytes); `k` is decimal, 1 to 99. The
   longest id is 243 bytes, within spec 003 3.9.2's 256. A name or id
   outside these rules is refused (`invalid_input`, `live-config`) before
   any journal is opened. Because `run_id` contains no `/`, the prefix
   `live/<run_id>/` used for run spend (3.5.3) matches exactly that run's
   attempts. A decision id equal to any source decision id is refused
   before admission (spec 026, 5).
5. **Data admitted.** A dataset with `synthetic` provenance is accepted. A
   `real` dataset needs `--admission <file>` (the spec 023 admission
   record), whose digest is recorded; without it the run is refused
   (`data-not-admitted`) before any journal is opened. Rustev checks
   presence and bounds, not the record's truth (spec 023 3.1). The
   `final_test` split needs spec 029 3.7's ledger flags; a live send of a
   final-test case is recorded as exposure under spec 023 3.2.

### 3.4 Live or captured acquisitions

1. A plan's acquisitions (spec 036, #76 B1) are declared per source
   (`SourceId`) `live` or `captured` in the configuration. The initial
   snapshot is always the case's (3.3); a live run never fetches an
   initial snapshot.
2. A `captured` acquisition's pending page is served by spec 036 3.15.5's
   candidate rule: from a retained page of the source bundle's
   `acquisition_pages` (`rustev.replay/2`), or of `--captures <dir>`
   (files holding spec 036 page documents), whose `AcquisitionRequestId`
   equals the request's, recomputed, never trusted. Retained pages with
   one identity and different bytes make the case
   `incomparable{ambiguous-output}`. A page with no equal identity, or a
   digest-only one, is supplied as `missing_evidence{fields: [<step>]}`
   (spec 036 3.11.4) and the case is `incomparable{acquisition-not-captured}`
   for metrics; no source is called. Historical failure reasons are never
   reused.
3. A `live` acquisition goes through spec 036's `AcquisitionSource`
   seam (3.12 there) with the same authorization, deadline and evidence
   rules as any external attempt; its `cost_bound` is reserved through
   the spend gate like a backend attempt (3.6), and `unknown` is refused.
4. With every acquisition `captured` (or for a plan without
   acquisitions), a live run of a case with `evaluation_time: source` differs from
   offline replay of the source bundle only in backend outputs: same plan
   (when the candidate is the source plan), snapshot id, evaluation time
   and acquisition pages. Acceptance tests that statement.
5. A configuration naming a source the candidate plans do not declare, or
   omitting one they declare, is refused (`acquisition-undeclared`).

### 3.5 The persistent spend journal

1. **Moved, not changed.** Spec 012's `SpendJournal` (3.8 there; append
   only JSON lines, header schema `rustev-jev.spend-journal/1`, `fsync`
   per line, one writer by `<journal>.lock`, crash rules, `testing` never
   resets) moves to `rustev_live::journal`. `rustev_jev::spend` re-exports
   it. The line format, header, schema name and every behavior are
   unchanged, so the journal that already holds the recorded testing
   spend opens and continues; the schema name is kept as recorded because
   existing files carry it.
2. **Generalized.** The journal knows units, reservations, charges and
   attempt ids, not providers: any backend whose descriptor discloses a
   cost model (`bounded` or `estimated`) can be held by it through the
   gate of 3.6. Spec 030's journal reservations use the same source.
3. **Run spend.** A run's spend is the sum, over journal lines whose
   attempt id starts with `live/<run_id>/`, of observed, estimated,
   liability and outstanding units; it is computed from the journal at
   open and kept current, so it survives a restart.
4. **Budget.** Live evaluation is testing under R-29 ("smoke,
   qualification and evaluation runs together"), so the journal must be a
   `testing` journal; any other header is refused (`journal-budget`). Its
   cap is the header's (USD 5 in total, 5 000 000 000 units); this spec
   raises nothing.
5. **One writer.** A journal another process holds is `io_error`
   (`journal-locked`), before any work. A backend that keeps its own
   journal on the same file cannot open it while a run holds the lock, so
   a double reservation fails closed. Spec 012's adapter gains an
   additive constructor option taking the run's journal handle, under
   which its own check confirms that the runtime's reservation for the
   attempt id exists instead of making a second one (no reservation, no
   send); its default construction is unchanged.

### 3.6 Spend gate and refusal before sending

1. **The seam (amends spec 026 3.4).** `reexecute_live` takes a spend
   gate: `reserve(attempt) -> Ticket | Refused`, `settle(ticket, charge)`
   and `release(ticket)`, synchronous and durable before return. The
   attempt presented carries the decision id, attempt id, backend target,
   artifact, operation, the descriptor's cost bound and the binding's
   cost unit. `decide` and every non-live path take no gate.
2. **Order.** For every attempt, including retries and fallbacks:
   decision budget reservation (spec 003 3.5), shared ledger, host
   authorization (spec 026 3.3), then the gate. Only when all four allow
   is any request byte sent. A refusal at any step sends nothing, and the
   earlier reservations are released.
3. **Exposure.** The reserved amount is the attempt's full worst-case
   exposure: `bounded{max_units}`, or `estimated{units}` (R-29: the hard
   stop acts on estimates, the provider's dashboard budget is the
   backstop). `unknown` exposure is refused (`spend-exposure-unknown`).
   `cost_unit: none` declares that the binding's cost units are not money
   and that it causes no provider spend (for example spec 005's rules
   backend, whose units count work); it is reserved at zero in the
   journal, and the declaration is recorded in every live document and in
   `rustev.live-run/1` as `declared_no_spend`. The CLI accepts `none` only
   for the backends it links (3.13.2); the library accepts it on the
   host's declaration, which is the host's statement, not Rustev's. A
   binding whose descriptor's cost model is `unknown` is refused
   (`spend-unit`) at configuration time whatever its unit.
4. **Caps.** The gate refuses when the reservation would take the run's
   spend over `run_cap_units` (`run-cap-exceeded`) or the journal over its
   cap (`spend-cap-exceeded`). Liability and outstanding reservations
   count in full.
5. **After a refusal.** The first spend refusal stops dispatch of every
   new attempt of the run: a cheaper case is not run in place of a
   refused one, which would bias the sample. In-flight attempts finish or
   are cancelled under spec 026 3.5, settle, and are checkpointed; the run
   ends `spend_capped` (exit 14) with no repeat report (3.11).
6. **Settlement.** The charge an attempt ends with settles its ticket:
   `observed`, `estimated`, or `unknown` (kept as liability). A cancelled
   attempt that sent nothing releases its ticket.
7. **Cost of retries.** Each retry and fallback is its own reservation;
   nothing is reserved once and reused.

### 3.7 Probe

1. Before any dataset case is sent, every binding any candidate reaches is
   checked. **Static checks** (no dispatch): the plan prepares against the
   backend (`check_plan`), the descriptor's backend and artifact identity
   equals the binding entry, and the cost unit rule of 3.6.3 holds.
2. **Calls.** Then each binding receives `probe.calls` sequential live
   decisions of the first candidate plan reaching it, on the configured
   probe snapshot, spaced by `probe.interval_ms`, each with purpose
   `probe` (3.9), through the same authorization and spend gate. Several
   calls are made because some providers accept a first call and fail
   later until account enablement completes (spec 030 3.11). The probe
   snapshot is read from `probe.snapshot` and its digest recomputed,
   never trusted. It is refused (`probe-snapshot`) before any journal is
   opened when the recomputed digest differs from `probe.digest`, when
   `probe.provenance` is absent or is not `synthetic`, or when the digest
   equals any case snapshot's digest, so no dataset content is sent by
   the probe. As for datasets (3.3.5, spec 023 3.1), Rustev checks the
   declaration, not its truth: declaring a real snapshot `synthetic` is
   the operator's false statement, and the declared provenance and digest
   are recorded in the probe section (3.11.4).
3. **Verdict.** A binding is `reachable` when every call's attempts to it
   returned a mapped output. Its served identity is evaluated in this
   order: `mismatched` when any call reports an identity that is neither
   the binding's artifact identity nor `unknown`; otherwise `unverified`
   when any call reports `unknown` (spec 009; for example spec 012's
   Gateway or spec 030's Converse); otherwise `matched`. A mix of
   `unknown` and a different identity is therefore `mismatched`. A
   binding fails its probe when it is not reachable or its identity is
   mismatched; the first failing call is reported with its code.
4. **Spend.** A probe spends no evaluation budget on dataset cases. Its
   own calls are reserved and charged like any attempt, against the run
   cap and the journal; a `cost_unit: none` binding's calls are
   reserved and charged at zero in the journal (3.6.3), so they spend
   nothing.
5. `--probe-only` runs only 3.7 and writes `rustev.live-run/1` with the
   probe section; it ends `probed` (exit 0) or `probe_failed` (exit 15).
   A full run whose probe fails ends `probe_failed` with no case sent.

### 3.8 Scheduling: concurrency, rate limits and deadlines

1. Work items are dispatched in a fixed order: repeat, then candidate in
   name order, then case in manifest order. Per binding, at most
   `max_in_flight` attempts are outstanding and successive dispatch starts
   are at least `min_interval_ms` apart, measured on the injected clock.
2. A `rate_limited` answer with a retry-after pauses new dispatches to
   that binding for the stated time, capped at
   `rate_limit_pause_max_ms`; the retry itself follows the plan's
   execution policy (spec 003 3.6), with a new reservation.
3. Each case runs under `case_deadline_ms` as its end-to-end deadline
   (spec 003). An optional `--run-deadline-ms` bounds the run; on expiry
   no new attempt starts, in-flight work is cancelled under spec 026 3.5,
   and the run ends `interrupted` (exit 16), resumable.
4. Completion order may vary; every output is ordered by the fixed order
   of 3.8.1, so outputs depend only on the observations, not on timing.

### 3.9 Evidence of live re-execution

1. **Amends spec 026 3.6.** `rustev.live-reexecution/1` gains, as
   required members, `mode` (always `live_reexecution`), `purpose`
   (`single`, `evaluation` or `probe`), `source` (`none`,
   `dataset_case{dataset_id, case_id, split}`, or
   `replay_bundle{bundle_digest, decision_id}`), `run` (`null`, or
   `{run_id, live_config_id, candidate, repeat}`), `evaluation_time`
   (`source` or `fixed`), `acquisitions` (per source, `live` or
   `captured` with capture digests) and `spend` (per attempt, the journal
   sequence number, reserved units and settlement, or the refusal code).
   Spec 026's document is not yet implemented, so no existing bytes
   change.
2. The run record is the ordinary record the runtime writes
   (`rustev.run/1`, or `rustev.run/3` for a plan using #76 constructs,
   introduced by spec 036), delivered through
   the host's sink. It is linked from the live document by digest and is
   not altered to mark the mode; the live document is the mark, and it is
   delivered beside the run record and written per case under `--out`.
3. Spec 026 3.6's other members, 3.7's retention rules and 3.8's
   comparison apply per case. With `compare_with_source` and bundle
   inputs, each case gets the spec 026 comparison; a changed proposal is
   `live-diverged`, never a failed reproduction.
4. No source dataset, bundle, capture, run record, judgment, receipt or
   report is opened for writing; a test checks each source's bytes and
   modification state before and after a run.

### 3.10 Checkpoints and resumption

1. `rustev.live-checkpoint/1` (`REPLAY_V1`, canonical) binds the run id,
   `LiveConfigId`, dataset id and split, every candidate's name, `PlanId`
   and binding identities, the journal header digest and the input
   digests, and lists per work item: `pending`, `completed{live document
   digest, run record digest}`, `unavailable{reason}`, `refused{code}` or
   `interrupted{attempt ids}`. It is rewritten after each item completes
   by writing a temporary file in the same directory, `fsync`, and rename.
2. `--resume` with the same `--out` reopens the checkpoint. Any binding
   above that differs is refused (`checkpoint-mismatch`) before the
   journal is opened.
3. A `completed` item is never re-sent; its per-case files are kept.
4. A work item with a journal reservation but no checkpointed result
   (the process ended while it was in flight) is `interrupted`. Its
   reservation is already liability in the journal (spec 012 3.8 crash
   rules), counted once. It is not re-sent unless `--rerun-interrupted`
   is given; then it runs as a new attempt sequence under the decision
   id with suffix `/rerun-<k>`, reserving anew, and the report states the
   extra exposure. `k` is 1 for an item's first rerun and one more than
   the largest `k` the checkpoint records for that item otherwise; the
   checkpoint records `rerun{k}` durably before the first reservation, so
   a rerun that is itself interrupted is found by its own decision id and
   the next one takes `k + 1`. An item that would need `k` above 99 is
   `refused{rerun-limit}`. Without the flag the item is incomparable with
   reason `live-interrupted`.
5. The run spend on resume is read from the journal (3.5.3), so a cap
   already reached stays reached; a run stopped by a cap can complete
   only after the owner raises the cap or the run cap is raised in a new
   configuration (a new run).

### 3.11 Reports into E1 and E2

1. `rustev_eval::evaluate_observed(dataset, adapter, config,
   observations)` computes spec 004 reports from new judgments and run
   records instead of replay outcomes. It is pure and synchronous, uses
   spec 004's existing metric code, and writes the same
   `rustev.eval-report/1` and `rustev.eval-detail/1` documents; latency
   and cost observations are the candidate's own (spec 029 3.11.2:
   `observation: candidate`).
2. When every work item has a terminal status (`completed`,
   `unavailable`, `refused` for authorization, or `interrupted`), each
   candidate's N runs form a spec 029 repeat set with source `live`: the
   output directory holds spec 029 3.13.1's files per candidate, and
   `comparison.json` and `comparison.md` across candidates, as
   `eval candidates` writes them. Spec 029's `live-source-unavailable`
   refusal no longer applies to these outputs. Spec 029's baselines,
   diffs, reproducibility check and `gate select`, and #76 E1's metrics,
   consume them unchanged.
3. Cases that are not `completed` are incomparable in that run with
   their reason, outside the stable cohort (spec 029 3.1.3).
4. `rustev.live-run/1` (`REPLAY_V1`, record canonical) summarizes the
   run: identities as in 3.10.1, the probe section (the probe snapshot's
   digest and declared provenance, and per binding its verdict), counts
   per status,
   run spend split into observed, estimated, liability and unknown, the
   journal's totals before and after, the stop reason, and the digests of
   every per-case live document and repeat report.

### 3.12 Authorization in the CLI

The CLI is a host, so it supplies spec 026's authorizer from
`authorize`: an ordered list of rules, each naming a backend target,
artifact, operation set and `max_exposure_units`. An attempt is allowed
when the first rule matching its target and artifact lists its operation
and its exposure does not exceed the rule's maximum; anything else is
refused (`authorization_refused`, spec 026 3.3). The decision reference
recorded is the `LiveConfigId` and the rule index. An empty list refuses
all external work; `cost_unit: none` bindings still need a rule.

### 3.13 CLI surface (amends spec 006)

`rustev eval live` takes `--live` (mandatory), `--live-config <file>`,
`--journal <file>`, `--run-id <id>`, the inputs of `rustev eval` (spec 006
3.9.1: `--dataset`, `--split`, `--config`, `--adapter`, the scope flags,
`--now-ms`), `--repeat-config <file>`, `--candidates <dir>` (spec 029
3.13.5's layout), one of `--snapshots <dir>` (with the scope flags) or
`--bundles <dir>` (spec 006's `<dir>/<case id>.json` naming), optional
`--captures <dir>`, `--admission <file>`, spec 029's ledger flags,
`--run-deadline-ms <ms>`, the switches `--probe-only`, `--resume` and
`--rerun-interrupted`, and `--out <dir>`. `--repeat-config` supplies the
statistics of spec 029; the repeat count is the live configuration's
`repeats`, and a `--repeat` flag is refused.

1. Without `--live`, or without `--live-config` or `--journal`, the command is a
   usage error (exit 2) before any file is read. There is no default
   journal path and no default cap: the cap is the configuration's
   `run_cap_units` and the journal's header.
2. The CLI links only the backends it already constructs (the rules
   backend of spec 005), so through the CLI live runs measure those
   backends. Provider backends (spec 012, spec 030 and later) are composed
   with `rustev_live::run` by a host program, the Node binding or the
   service; `rustev-cli` still depends on no crate under `integrations/`.
3. `--out` receives `live-config.json`, `checkpoint.json`, `cases/` with
   one `<candidate>.<repeat>.<case_id>.live.json` and `.run.json` per
   completed item, and at the end `live-run.json` plus the repeat outputs
   of 3.11.2, all or none for the final files. An existing `--out`
   without `--resume` is `io_error`.
4. stdout is one line: the status, the run id, the run spend and the
   journal's headroom in units, and the counts per status.

### 3.14 Exit codes

Spec 006 3.2's table, as extended by spec 029, gains three statuses. The
numbers 14, 15 and 16 below are provisional: under the #76 coordination
rule each takes the next free code in merge order, and the status names
never change.

| Code | Status | Meaning |
|---|---|---|
| 0 | `evaluated_live`, `probed` | Every work item is terminal and the reports are written; or a probe-only run passed. |
| 3 | `io_error` | Also: `journal-locked`, a journal or checkpoint that cannot be written durably. |
| 4 | `invalid_input` | Also: `live-config`, `journal-budget`, `spend-unit`, `acquisition-undeclared`, `probe-snapshot`, `evaluation-time-unavailable`, `data-not-admitted`, `checkpoint-mismatch`, `identity-mismatch`. |
| 14 | `spend_capped` | A reservation was refused by the run cap or the journal cap; nothing over the cap was sent; checkpoint written. |
| 15 | `probe_failed` | A binding failed its probe; no dataset case was sent. |
| 16 | `interrupted` | Cancelled by signal or by the run deadline; checkpoint written; resumable. |

No existing code or status changes meaning.

### 3.15 Compatibility and identities

1. Every existing command, library function and document keeps its
   inputs, bytes, stdout and exit codes. `decide` takes no spend gate.
2. The journal's file format and schema name are unchanged; a journal
   written by spec 012's code before the move opens, totals identically
   and continues (3.5.1).
3. No plan or definition construct is added: every plan golden keeps its
   bytes and `PlanId`. A live run records the candidate's `PlanId`; the
   `LiveConfigId` and run id identify the run, not the plan.
4. `rustev.live-reexecution/1` gains members before its first
   implementation (3.9.1); `rustev.live-config/1`,
   `rustev.live-checkpoint/1` and `rustev.live-run/1` are new.
   `rustev.live-run/1` joins spec 004 5.6's record-canonical list.

## 4. Out of scope

Raising, resetting or per-provider splitting of the R-29 caps; a
production live mode or production spend; scheduled or automatic live
runs; fetching initial snapshots; a provider binary or `eval-live`
subcommand under `integrations/` (each integration's own change);
cross-process sharing of one journal by concurrent runs; real-user data
outside spec 023 admission; storing results anywhere but host-named
paths; quality, calibration, latency or cost claims; the optimization
objective API (#76 future).

## 5. Observable negative cases

| Case | Expected |
|---|---|
| `rustev eval live` without `--live` | Usage error, exit 2, nothing read, no journal opened. |
| Attempt whose reservation would pass the journal's cap | Refused before any byte, `spend-cap-exceeded`; run ends `spend_capped`, exit 14; backend call count unchanged. |
| Attempt over the run cap but under the journal cap | `run-cap-exceeded`, exit 14, nothing sent. |
| Backend discloses `unknown` exposure | `spend-exposure-unknown`, nothing sent. |
| `cost_unit: none` through the CLI on a backend it does not link, or any binding with an `unknown` cost model | `invalid_input`, `spend-unit`, before probe. |
| Journal locked by another process | `io_error`, `journal-locked`, before any work. |
| A `production` journal | `invalid_input`, `journal-budget`. |
| Process killed after a reservation, before settlement; then `--resume` | Item `interrupted`, its reservation counted once as liability, not re-sent; completed items not re-sent. |
| `--resume` with `--rerun-interrupted` | Item re-sent under a `/rerun-<k>` decision id with a new reservation; old liability stays. |
| `--resume` with a different configuration or candidate | `invalid_input`, `checkpoint-mismatch`. |
| Captured acquisition with no page of equal `AcquisitionRequestId` | `missing_evidence{fields: [<step>]}`; case `incomparable{acquisition-not-captured}`; zero source calls. |
| Candidate name of 33 bytes, or containing `/` | `invalid_input`, `live-config`, before any journal is opened. |
| A rerun that is itself interrupted, then resumed with `--rerun-interrupted` | Next rerun under `/rerun-<k+1>`; each reservation counted once. |
| Probe: second of three calls fails | `probe_failed`, exit 15, no dataset case sent. |
| Probe: served identity differs from the binding | `mismatched`, `probe_failed`. |
| Probe snapshot equal to a case snapshot | `invalid_input`, `probe-snapshot`. |
| Probe snapshot digest differs from `probe.digest`, or `probe.provenance` absent or `real` | `invalid_input`, `probe-snapshot`, before any journal is opened. |
| Probe: one call reports `unknown`, another a different identity | `mismatched`, `probe_failed`. |
| Bundle input whose `run` and `supplies` hold outputs | Outputs never read; the new judgment comes only from new backend output. |
| Expired source bundle | Case `unavailable`, not restored. |
| Live run of the source plan with captured acquisitions and a backend scripted to return the source outputs | Proposal equal to replay's; only run observations differ. |
| `real` dataset without `--admission` | `invalid_input`, `data-not-admitted`, no journal opened. |
| Authorization rule absent for a binding | `authorization_refused` per attempt, nothing sent, run continues. |
| `rate_limited` with retry-after above the cap | Pause capped; retry reserved anew. |
| Offline `rustev eval` and spec 029 commands | Byte-identical outputs; zero backend calls. |

## Acceptance

- Every row of section 5 has a deterministic test with scripted backends,
  a scripted authorizer, a manual clock and a temporary journal; no
  network access and no wall-clock sleep.
- Spend: a test proves that a refused reservation sends nothing (scripted
  backend call count) and that the journal's lines after a refusal are
  unchanged; a property test over random cost bounds, charges, retries
  and caps checks that observed plus estimated plus liability plus
  outstanding never exceeds either cap.
- Resumption: a crash-injection test kills the run at every journal and
  checkpoint write point and resumes; across all points no case is sent
  twice without `--rerun-interrupted`, and the journal total equals the
  sum of distinct attempts.
- Captured acquisitions: the run of 3.4.4 against a spec 004 replay of
  the same bundle differs only in backend outputs and run observations.
  It includes captured and missing pages and an ambiguous page under
  spec 036. Until spec 036 is implemented, this is tested with the
  initial snapshot only and the acquisition part is reported as
  externally blocked, never as passed.
- Evidence: every live document carries `mode: live_reexecution` and its
  source link; mutation tests prove no source file changes.
- Separation: the `rustev-eval` dependency test of 3.1.2; spec 026's
  compile-time tests; existing offline tests keep a backend-call count of
  zero.
- Journal move: the committed journal fixture written by spec 012's code
  opens under `rustev_live::journal` with identical totals, and spec
  012's `tests/spend.rs` passes unchanged through the re-export.
- Reports: a live run's repeat outputs are accepted by spec 029's
  `baseline record`, `eval diff` and `gate select`, golden-tested.
- Compatibility: every existing CLI, eval, runtime and plan golden, with
  its `PlanId`, is byte-identical.
- `make code`, declared verification, and `make gate` pass.
- Live provider stage (manual, not in CI): a host program composing a
  provider backend runs a synthetic dataset under the work order's terms;
  its journal totals are recorded in this spec's implementation record.
  Without credentials it is reported as externally blocked.

## Verification

Pending with implementation. Approval satisfies no acceptance item, and no
live run has been made under this spec.

## Decisions at approval

The owner ratified in advance every spec implementing a #76 child
faithfully and completely; #74 leaves the points below open, and each is
decided by the implementer within this spec, as written:

1. **A new crate.** `rustev-live`, not `rustev-eval` (which must stay free
   of the runtime, spec 026 3.1) and not `rustev-runtime` (which holds no
   dataset or file I/O). It is not under `integrations/` because it adds
   no new dependency class.
2. **The command name.** `rustev eval live`, as the issue names it. The
   CLI cannot link provider backends (spec 001 3.4.4, approved, stands),
   so provider runs go through the library; a provider binary is each
   integration's own change.
3. **Journal generalization.** Spec 012's journal moves into
   `rustev-live` with its format unchanged and is reached through a
   runtime spend gate, so any backend with a cost model is held by it.
   One journal and one cap across providers (R-29; spec 030 Decision 11).
   Spec 030's by-path compilation of the journal reads it from its new
   location.
4. **Two caps.** A per-run cap in the configuration plus the journal's
   R-29 cap; both refuse before sending. There is no default for either.
5. **Estimated exposure.** Reserved as if it were a bound; the hard stop
   acts on estimates and observed charges, and the provider's dashboard
   budget is the backstop for the difference (R-29). `unknown` exposure is
   refused rather than reserved at zero.
6. **Stop on first spend refusal.** Continuing with cheaper cases would
   bias the evaluated sample; the run stops and can resume after a cap
   change.
7. **"No-spend probe."** Read as: the probe sends no dataset content and
   spends nothing on dataset cases. A reachability check of a paid
   provider needs billed calls (spec 030 3.11), so probe calls are
   minimal, on a synthetic snapshot, reserved and capped like any
   attempt; `cost_unit: none` bindings spend nothing. `unknown` served identity is
   `unverified` and does not fail the probe, because spec 012's Gateway
   and spec 030's Converse never report it; a mismatch fails.
8. **Interrupted attempts.** Not re-sent by default: their charge may be
   real and is counted once as liability. Re-running needs an explicit
   flag and new identities.
9. **Snapshot and time from the source.** Allowed for dataset-scale runs
   (amending spec 026 3.2) so a run can isolate a model change; every
   other identity and observation stays new, and the choice is recorded.
10. **Marking.** The run record (`rustev.run/1` or `/3`) is not changed; the
    `live_reexecution` mark is in spec 026's live document, extended
    before its first implementation, and delivered beside the run record.
11. **Reports.** Live outputs are spec 029 repeat sets with source `live`,
    so E1 and E2 need no change.
12. **Live activity (settled by the work order).** The 2026-10-16
    delivery work order: "Live provider calls are limited to smoke tests,
    qualification and E3 live evaluation, on synthetic or spec 023
    admitted data only. The R-29 caps stay in force: USD 5 in total for
    all testing across all providers unless I raise it in writing. Use
    the persistent spend journal, and refuse before sending any call that
    would exceed the cap." Live runs under this spec are therefore
    limited to that scope, held by the journal of 3.5, and refused before
    sending over the cap.
13. **Exit codes.** Three new, distinct statuses (`spend_capped`,
    `probe_failed`, `interrupted`), numbered 14, 15 and 16 provisionally;
    under the #76 coordination rule the numbers become the next free
    codes in merge order at rebase, and the names never change.
14. **Shared document versions.** This spec introduces and adds to none
    of the #76 track versions; it reads `rustev.replay/2` and accepts
    `rustev.run/3` as spec 032 introduces them (032 3.1.1: "This spec
    introduces the document versions the #76 decision-model specs share",
    listing `rustev.run/3`; spec 036 adds members to it under 032 3.1.2),
    and its own documents are new names.

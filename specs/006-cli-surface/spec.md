---
id: "006-cli-surface"
title: "CLI surface"
status: approved
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev`, a command-line host over the library. Plan check,
  compile and show against descriptor, rules-program, calibration and
  execution-policy files; run one snapshot through `rustev-runtime` on
  rules-program backends with a file evidence sink and optional bounded
  capture into a replay bundle; offline replay under an explicitly supplied
  trusted scope; evaluation reports with their detail and configuration
  companions, regression gates, and temperature fitting with its lineage
  record, through `rustev-eval`. Bounded file access, stable JSON output and
  distinct exit codes. No network, no hosting, no live inference beyond the
  deterministic rules backend.
establishes:
  - { kind: directory, path: "crates/rustev-cli/" }
extends:
  # Records the CLI crate and its dependencies in the lockfile.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  # Adds 006 to `make verify` when the implementation is complete.
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
---

# 006: CLI surface

Approved; implementation pending. The owner approved making this spec
concrete and delivering it through verified merge within the scope below
(R-25 and A-06 in `docs/decisions/00-founding-decisions.md`). The concrete
rules were written by the agent within that scope; its engineering choices
are listed as its own. Rationale: design section 15.

## 1. Purpose

Let a package author compile, inspect, run, replay and evaluate a plan
without writing Rust, using only files and the pinned library. The CLI is a
host: it owns file I/O, transport buffering, the wall clock it is told to
read, the evidence file and the retention store. Every decision rule stays
in the library crates; the CLI adds none.

## 2. Territory

`crates/rustev-cli/`, package `rustev-cli`, binary `rustev`, plus a library
target holding the command implementations so tests can drive them in
process. Normal dependencies: `rustev-contract`, `rustev-core`,
`rustev-runtime`, `rustev-eval`, `rustev-backend-rules`, `serde`,
`serde_json` (with `raw_value`), `sha2` and `tokio` (`rt`, `time`, `sync`,
`signal`). No HTTP stack, no ecosystem crate, no argument-parsing crate:
the argument grammar of 3.1 is small and parsed by hand. Additive edits to
spec 001's `Cargo.lock` and, at completion, `Makefile`. No change to any
unit owned by specs 002 to 005: if one proves necessary it is a separate,
reviewed amendment first (R-16).

## 3. Behavior

### 3.1 Invocation and output

1. Grammar: `rustev <command> [<subcommand>] [--flag value | --switch]...`.
   Flags are long only; each takes exactly one value unless listed as a
   switch; a flag listed as repeatable may occur several times, any other
   flag at most once. An unknown command or flag, a missing value, a
   repeated single flag, a missing required flag, or two flags that exclude
   each other is a usage error (exit 2) reported on stderr, before any file
   is read. `rustev help` and `--help` print usage on stdout and exit 0.
   Integers are unsigned decimal ASCII without sign or leading zeros
   (except `0`), within `u64` or the flag's stated range.
2. Every command other than `help` writes exactly one JSON object and a
   newline to stdout, including on every failure except usage errors. Keys
   are sorted, the object is compact, and it always has `command` (for
   example `plan check`) and `status`. Judgments, and run records whose
   delivery failed, are embedded as their record canonical bytes (spec 004,
   5.6), never re-serialized; plans, bundles and reports go to files and
   are never embedded. Human diagnostics go to stderr and are not part of
   the contract.
3. Wall-clock time is an explicit input: `--now-ms <ms>` is either a
   timestamp in milliseconds since the Unix epoch within the contract's
   range, or the word `system`, which reads the system clock once at
   startup. Commands that need it require it; nothing reads a clock
   otherwise. Domain evaluation time is always `--evaluation-time <ms>`,
   never a clock. The runtime's elapsed-time clock is `TokioClock`.
4. Isolation scope is given only by flags, never inferred:
   `--scope principal|tenant-only` (default `principal`), `--tenant`,
   `--context-revision`, and `--principal-scope` in principal mode. In
   principal mode a missing `--principal-scope` is a usage error, never a
   downgrade to tenant-only; in tenant-only mode `--principal-scope` is a
   usage error (mixed variants). Handles are checked by the contract's
   `Handle` rules (non-empty, at most 256 bytes); a violation is an input
   error (exit 4). Scope handles are never passed to backends: the
   runtime's `principal_handle` is empty for every CLI decision.

### 3.2 Exit codes

| Code | Status values | Meaning |
|---|---|---|
| 0 | `ok`, `compiled`, `judged`, `reproduced`, `reported`, `pass`, `fitted`, `qualified` | The command did what it states. A judged escalation or unresolved outcome is still 0: the decision was made. |
| 2 | (stderr only) | Usage error. |
| 3 | `io_error` | A file could not be read or written, a path to be written already exists, or a byte cap of 3.3 was exceeded. Names the path. |
| 4 | `invalid_input` | A supplied document other than a definition or plan was refused (parse, schema, validation, rules program, snapshot start, dataset, configuration, adapter, capture configuration, scope handle, assembly), naming the file and reason. |
| 5 | `refused` | The plan cannot be used: a compile refusal (categories 1 to 12), a load diagnostic (spec 004, 5.2), a runtime prepare error or a rules `check_plan` mismatch. |
| 6 | `rejected` | The decision was not admitted; nothing ran and there is no run record. |
| 7 | `cancelled` | The decision was cancelled; a run record was delivered. |
| 8 | `evidence_not_delivered` | Work happened but the run record did not reach the evidence file; the record is embedded in the output instead. |
| 9 | `diverged` | Replay completed with different judgment bytes. |
| 10 | `incomparable` | Replay could not compare (spec 004, 3.3), with its reason code. |
| 11 | `fail`, `refused_qualification` | A gate failed, or a calibration qualification was refused. |
| 12 | `unknown` | A gate or qualification is unknown. Unknown is never a pass. |

A panic is a defect, not a status; it exits with Rust's own code.

### 3.3 Bounded file access

1. Every input file is read by opening it, requiring a regular file, and
   reading at most its document's limit plus one byte: definitions under
   `DEFINITION_V1`; descriptors, calibrations, rules programs, execution
   policies and task adapters under `DESCRIPTOR_V1`; plans under `PLAN_V1`;
   snapshots under `SNAPSHOT_V1`; run records and evaluation reports under
   `RECORD_V1`; bundles, datasets, evaluator configurations, evaluation
   details and fit records under `REPLAY_V1`. A file past its limit reaches
   the parser with the extra byte and is refused by the document's own
   bound as that document's normal refusal (category 1 for a definition),
   so the CLI never buffers more than the limit plus one byte per file.
2. One command reads at most 256 MiB of files in total, including store
   items it resolves, and at most 64 `--rules`, 4 096 `--descriptor` and
   4 096 `--calibration` files; beyond that it stops with `io_error`
   before reading further. Spec 004's 16 MiB per-case resolution budget
   applies inside that.
3. Output files are created, never overwritten: an existing path is an
   `io_error` checked before any work, and a write that fails midway
   removes the partial file. The run record file is synced before the sink
   acknowledges it. Written documents are their canonical bytes (plans,
   calibration artifacts, configurations) or record canonical bytes (run
   records, bundles, reports, details, fit records). Output directories must
   already exist.
4. Paths are used as given; the CLI follows no path found inside a
   document, except store references (3.6.4), which are confined to the
   store directory.

### 3.4 `rustev plan check | compile | show`

1. Inputs for `check` and `compile`: `--definition` (required), and backend
   descriptors from `--descriptor` (repeatable) and `--rules` (repeatable;
   the rules backend's derived descriptor, spec 005, 3.9), `--calibration`
   (repeatable) and optionally `--execution` (a `rustev.execution/1`
   policy). Without `--execution` the plan is compiled with `compile`
   (`execution: none`); with it, `compile_with`. A definition is compiled
   with `compile_bytes`-equivalent parsing, so a malformed definition is a
   category 1 refusal. A malformed descriptor, calibration or execution
   policy file is also category 1, with subject `descriptor:<path>`,
   `calibration:<path>` or `execution:<path>`; a rules program that fails
   validation is `invalid_input`.
2. `check` prints `ok` with the `plan_id`, or `refused` (exit 5) with
   `category` (1 to 12), `category_name` (snake case of the core's
   category), `subject`, `detail` and `shortfalls` (each backend id with its
   reasons as tagged objects). It writes nothing.
3. `compile` additionally requires `--out` and writes the canonical
   `rustev.plan/2` bytes there, printing `compiled` with `plan_id` and the
   path. On refusal nothing is written.
4. `show` takes `--plan` and the same dependency flags (no `--definition`,
   no `--execution`: the plan embeds both). It loads the plan with
   `Compiled::load_checked` and prints `ok` with `plan_id`, `compiler`,
   `registry`, `definition_id`, the definition's `package`, `name` and
   `version`, `execution` (`none` or the policy id), `order`, and per step
   its `id`, `derivation` and detail (exact operator and version; semantic
   backend id, artifact, descriptor, output, required kind, normalization,
   calibration id or `none`, whether the capability fallback was taken, and
   its request bound; or unsupported capability), plus declared runtime
   fallbacks. A load failure is `refused` with `load_error`
   (`compiler_changed` with both identities, `missing_descriptor`,
   `missing_calibration`, or `plan_mismatch` with the recompile refusal if
   any). `show` never prints a plan it could not reproduce from its
   definition.

### 3.5 `rustev run`

1. Inputs: `--plan`, `--rules` (repeatable, at least one; each program is
   one `RulesBackend`), `--calibration` (repeatable), `--snapshot`,
   `--evaluation-time`, `--decision-id` (1 to 256 bytes), `--record-out`;
   optional `--deadline-ms` (can only tighten the plan's), and
   `--max-parallel-requests` (1 to 64, default 4). Each backend's
   concurrency is `--max-parallel-requests`; one decision is admitted with
   no queue.
2. Setup, in order, each failure stopping before any decision: read and
   validate every rules program (`invalid_input`); load the plan with
   `load_checked` against the programs' derived descriptors and the given
   calibrations (`refused`); run each backend's `check_plan` and refuse on
   any mismatch, listing all of them (`refused`); build the runtime and
   `prepare` the plan (`refused` with the prepare error); parse the
   snapshot (`invalid_input`); check `--record-out` does not exist
   (`io_error`).
3. The evidence sink appends nothing and overwrites nothing: it creates
   `--record-out`, writes the record canonical `rustev.run/1` bytes, syncs
   and closes it, and acknowledges with receipt
   `file:<record digest>`. The sink policy is `fail_decision` with
   `--sink-timeout-ms` (default 5 000). A sink failure is
   `evidence_not_delivered` (exit 8) with `detail`, `uncertain` and the
   record embedded, whatever the completion.
4. The first interrupt signal (Ctrl-C) raises the decision's cancel
   signal; the runtime and the rules backend observe it as spec 003 and
   spec 005 3.13 state. A later signal is not handled specially.
5. Output: `judged` (exit 0) with `decision_id`, `plan_id`, the judgment,
   the record path and receipt; `cancelled` (exit 7) with the same minus
   the judgment; `rejected` (exit 6) with the rejection kind
   (`overloaded`, `queue_deadline`, `cancelled`, `invalid_request`) and
   detail. The run record is evidence; the printed judgment is a proposal,
   never a grant (spec 001, 3.2).

### 3.6 Capture and bundles on `run`

1. Capture is off unless `--capture-bytes <n>` (1 to 16 MiB) is given. It
   then requires `--bundle-out`, the scope flags (3.1.4) and `--now-ms`, and
   `run` uses `decide_with_capture`. A capture configuration the runtime
   refuses is `invalid_input` before admission.
2. After the decision, whatever its completion and also after a sink
   failure, the CLI assembles a `rustev.replay/1` bundle with
   `rustev_eval::assemble` from the plan, the programs' descriptors, the
   calibrations, the snapshot, the run record and the capture, with
   `created_at_ms` from `--now-ms`, and writes it to `--bundle-out`. A
   rejected decision has no capture and no bundle. The bundle's capture
   status and entry count are printed.
3. Retention is explicit: `--retain digest-only` (default), `embedded` or
   `external`, `--lifetime-ms` (default 7 days) and `--host-cap-ms`, both
   within spec 004's 30-day cap. `external` requires `--store <dir>`;
   the other modes forbid it.
4. The store is content-addressed: each externally retained item is the
   file `<dir>/<ref>` where `<ref>` is the lowercase hex SHA-256 of its
   bytes, and the bundle holds `<ref>` as the opaque reference. An existing
   file of that name is left unchanged (the digest check at replay decides
   whether it is intact). Store files are written only after assembly
   succeeds, then the bundle. The CLI never deletes store files: erasure,
   expiry enforcement and backups are the host's, and a removed file
   resolves as `missing`, because the store cannot distinguish erasure from
   absence (spec 004, 3.1.8).
5. An assembly error is `invalid_input` after the decision; the run record
   has already been delivered and the output says so.

### 3.7 `rustev replay`

1. Inputs: `--bundle`, the scope flags (the caller's trusted scope),
   `--now-ms`, optional `--store` and `--host-cap-ms`. The store resolver
   accepts only 64-hex-digit references, reads at most the requested
   maximum plus one byte (so an oversized item is `oversized`, never
   truncated), returns `missing` for an absent file and `inaccessible` for
   any other failure. Without `--store` every external item is
   `inaccessible`.
2. It calls `rustev_eval::replay::reproduce` and prints `reproduced`
   (exit 0) with `decision_id`, `plan_id`, the number of supplies and the
   judgment; `diverged` (exit 9) with both judgments; or `incomparable`
   (exit 10) with `reason` (the library's code) and `detail`. No backend is
   constructed: replay takes no `--rules` flag.

### 3.8 Task adapters, `rustev.task-adapter/1`

Evaluation needs a task adapter (spec 004, 3.5). The CLI owns a declarative
one, parsed under `DESCRIPTOR_V1` with unknown fields and duplicate keys
refused:

```text
adapter = { schema: "rustev.task-adapter/1", name, version,
            labels: { values: [string], prefixes: [string] },
            rules: [rule] }
rule    = { action, label: string }
        | { action, param, select: enum | text | integer | first_ranked,
            map: {string: string}, otherwise: string | null }
```

1. `name` and `version` are 1 to 64 bytes and form the `AdapterRef` the
   dataset and configuration must name. A label is well formed when it
   equals one of `values` or starts with one of `prefixes`; at least one
   list is non-empty. At most 64 rules, one per action.
2. A proposal is correct for a label when the rule for its action exists
   and: for a `label` rule, the label equals it; for a `param` rule, the
   selected value (an enum's option, a text value, an integer's decimal
   digits, or the candidate of the first entry of a ranking) is mapped
   through `map` when `map` is non-empty (to `otherwise` when not listed,
   and never correct when `otherwise` is null), and equals the label. A
   missing parameter, a value of another type or an empty ranking is not
   correct. An action with no rule is never correct.
3. The adapter is identified to reports only by name and version; the
   evaluator configuration does not digest its rules. A changed adapter
   document must change its version. `eval` prints the adapter document's
   digest so a host can retain it.

### 3.9 `rustev eval` and `rustev gate`

1. `eval` inputs: `--dataset`, `--split` (`training`, `model_selection`,
   `calibration`, `final_test`), `--config`, `--adapter`, `--bundles <dir>`,
   the scope flags, `--now-ms`, optional `--store`, `--host-cap-ms`, and
   `--out <dir>`. Each case's bundle is `<dir>/<case id>.json`; a case id
   that is not 1 to 128 bytes of `[A-Za-z0-9._-]` starting with an
   alphanumeric is `invalid_input` for the whole command, and an absent
   file is left to the library as `bundle-missing`. A candidate is
   evaluated when `--candidate-plan` is given, loaded with `load_checked`
   against `--descriptor`, `--rules` and `--calibration`, which then
   describe the candidate only.
2. The manifest is checked with the adapter's label check; the
   configuration, adapter and manifest must agree (`invalid_input`
   otherwise). The CLI calls `rustev_eval::report::evaluate` and writes
   `report.json` (the `rustev.eval-report/1` envelope), `detail.json`
   (`rustev.eval-detail/1`) and `config.json` (the evaluator configuration,
   canonical) into `--out`, all or none. It prints `reported` with the
   report's record digest, dataset id, split, config id, adapter digest and
   every metric as the envelope states it. Synthetic provenance is printed
   as the envelope carries it; the CLI never upgrades a claim.
3. `gate` inputs: `--baseline <dir>` and `--candidate <dir>` (each holding
   the three files `eval` wrote) and `--gate <name>`. It parses the three
   documents per side, finds the gate in the candidate's configuration
   (absent is `unknown`), and prints `pass`, `fail` (exit 11) or `unknown`
   (exit 12) from `evaluate_gate`, with the reason.

### 3.10 `rustev calibrate fit | qualify`

1. `fit` inputs: `--dataset`, `--config` (its temperature grid), `--step`,
   `--bundles <dir>`, the scope flags, `--now-ms`, optional `--store`,
   `--host-cap-ms`, and `--out <dir>`. Samples come only from cases of the
   calibration split: each bundle is reproduced, and the step's
   instance-free retained output is the sample. Every reproduced case must
   have the same plan (`invalid_input` otherwise). The fit target
   (artifact, task, question, options) is the step's binding in that plan,
   and the tolerance the plan's `distribution_tolerance`. A case that does
   not reproduce, has no output for the step or whose value is a runtime
   reason gives no sample; the CLI prints these by reason, and the fit
   record counts them as `no-sample`.
2. It calls `fit_temperature` and writes `calibration.json`
   (`rustev.calibration/1`, canonical) and `calibration-fit.json`
   (`rustev.calibration-fit/1`) into `--out`, all or none, printing
   `fitted` with the artifact id, temperature, loss and counts. A fit error
   is `invalid_input` naming it.
3. `qualify` inputs: `--calibration`, optional `--fit`, `--dataset`,
   `--split`. It prints `qualified` (exit 0), `refused_qualification`
   (exit 11) or `unknown` (exit 12) from `qualify`, with the reason. A
   missing fit record is `unknown`, never qualified.
4. A fitted artifact changes the plan's identity when a definition is
   recompiled with it (spec 002, category 10 binding); the CLI does not
   rewrite plans. Synthetic fits establish mechanics only (R-04).

## 4. Out of scope

HTTP serving (`integrations/rustev-serve`), any network access, remote or
model backends (spec 011), live re-execution (spec 004, 4), a persistent
store beyond the content-addressed directory of 3.6.4, deletion or expiry
enforcement, authentication, configuration files, environment-variable
configuration, shell completion, publication to crates.io, releases and
packaging. The CLI adds no decision semantics: it never retries, falls
back, calibrates or judges on its own.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| An unknown flag, a missing value, a repeated single flag, `--principal-scope` with `--scope tenant-only`, principal mode without `--principal-scope` | Usage error, exit 2, nothing read. |
| A definition one byte past `DEFINITION_V1` | Category 1 refusal, exit 5; at most the limit plus one byte read. |
| A definition triggering each category 1 to 12 | `refused` with that category, exit 5. |
| A plan whose embedded definition was edited | `show` and `run` refuse with `plan_mismatch`. |
| A plan binding a backend no `--rules` program provides | `refused` with `missing_descriptor`. |
| A rules program whose task does not match a plan step | `refused` listing the `check_plan` mismatch. |
| A snapshot the plan cannot start on | `rejected` with `invalid_request`, exit 6, no record file. |
| `--record-out` or an output file that already exists | `io_error`, exit 3, before any decision. |
| A record path whose directory vanishes before delivery | `evidence_not_delivered`, exit 8, record embedded. |
| Cancellation before the backend's first observation point | `cancelled`, exit 7, a record with termination `cancelled`. |
| `--capture-bytes 0` or past 16 MiB | `invalid_input` before admission. |
| `--retain external` without `--store` | Usage error. |
| Replay under another tenant, revision, principal scope or mode | `incomparable` with `scope-mismatch`, exit 10, nothing resolved. |
| A store file removed, altered or replaced by a larger one | `incomparable` with `missing`, `corrupt` or `oversized`. |
| A store reference that is not 64 hex digits | `inaccessible`; no file outside the store is opened. |
| `--now-ms` at or past an item's expiry | `incomparable` with `expired`. |
| An adapter whose name or version differs from the configuration's | `invalid_input`. |
| A gate whose inputs are unknown or mismatched | `unknown`, exit 12, never pass. |
| `qualify` on the fitting split, or without a fit record | `refused_qualification` (exit 11) or `unknown` (exit 12). |

## Acceptance

- The argument grammar's usage errors each have a test that also shows no
  file was read.
- Golden stdout for both reference plans: `plan check`, `plan compile` and
  `plan show`, and one refusal per category 1 to 12, compared byte for
  byte. Reference inputs are emitted at test time from the shared core
  builders and rules fixtures (R-02), never maintained as a second source.
- Both reference plans run end to end through the binary on the synthetic
  rules programs: one run record per termination (`judged`, and
  `cancelled` through the in-process entry with a raised signal), a
  rejection with no record, and evidence not delivered with the record
  embedded. Goldens normalize only the runtime's elapsed-time fields.
- With capture on, each retention mode writes a bundle that `replay`
  reproduces byte for byte (digest-only is `incomparable` with `missing`);
  the isolation matrix of spec 004 section 6 holds through the flags; a
  removed, altered or oversized store file and an expired item are
  incomparable with their reasons; no rules program is read by `replay`.
- `eval` produces reports whose detail reconciles, for both reference
  tasks through declarative adapters, as baseline and with a candidate;
  `gate` returns pass, fail and unknown on constructed pairs; `calibrate
  fit` picks the grid winner on the support-routing topic step and
  `qualify` refuses the fitting split and is unknown without a record.
- Every byte cap of 3.3, including the aggregate, has a failing test.
- Negative controls: seeded defects in the CLI (scope flag handling, byte
  caps, no-clobber, exit-code mapping, store confinement, adapter
  correctness, sink acknowledgement) are each detected by the tests.
- `rustev-cli` depends on no HTTP stack or ecosystem crate; the boundary
  check passes.

## Verification

Declared when the implementation is complete; until then `make verify`
does not run this spec.

## Implementation record

- 2026-09-24: approved and made concrete; no implementation yet.

## Decision history

- 2026-09-23: drafted with the increment 2 contracts, then aligned with the
  delivered runtime seam and spec 004's approved contract.
- 2026-09-24: the owner approved making this spec concrete and delivering
  it through verified merge within the scope of R-25 (A-06). Spec 011
  stays a draft.

## Engineering choices

Made by the agent within A-06; open to the owner's review.

| Id | Choice | Reason |
|---|---|---|
| E-24 | Hand-parsed long flags, no argument-parsing crate. | Six commands with fixed flags; keeps the lockfile and the reviewable surface small. |
| E-25 | One sorted, compact JSON object per command on stdout, embedding library documents as record canonical bytes. | Machine-readable, golden-comparable, and never a second serialization of a record. |
| E-26 | Wall-clock time is a required flag (`--now-ms <ms>` or `system`). | Keeps expiry and bundle creation explicit and testable; the CLI reads a clock only when told to. |
| E-27 | Outputs are created, never overwritten; the run record is synced before acknowledgement. | Evidence is never silently replaced; a receipt means the bytes are on disk. |
| E-28 | A content-addressed store directory for external retention, with no deletion command. | Opaque references without path traversal; erasure stays the host's, as spec 004 requires. |
| E-29 | A declarative, CLI-owned task adapter document. | Evaluation needs an adapter, and a package author should not need Rust to define correctness. |
| E-30 | Run uses only rules-program backends, one admitted decision, no queue, `fail_decision` delivery. | The only production backend without inference; a CLI run has one decision and must know whether its evidence was delivered. |

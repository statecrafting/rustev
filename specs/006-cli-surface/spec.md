---
id: "006-cli-surface"
title: "CLI surface"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev`, a command-line surface over the library: plan check,
  show and compile against descriptor, calibration and execution-policy
  files; run and replay with a declared backend configuration through
  `rustev-runtime`; eval and calibrate over named datasets. No hosting, no
  HTTP.
establishes:
  - { kind: directory, path: "crates/rustev-cli/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
  - "005-reference-backends"
---

# 006: CLI surface

Draft. A proposal, not a claim about code. Rationale: design section 15.
Updated after spec 003 was delivered, to build on its actual interfaces.

## 1. Purpose

Let a package author compile, inspect, run, replay and evaluate a plan
without writing Rust, using only files and the pinned library.

## 2. Territory

`crates/rustev-cli/`, binary `rustev`. Depends on the library crates only.

## 3. Behavior (to be made concrete before approval)

1. `rustev plan check|compile|show`: reads a definition, descriptors,
   calibrations and optionally a `rustev.execution/1` policy (parsed under
   `DESCRIPTOR_V1`); compiles with `compile` or `compile_with`; prints the
   refusal (category 1 to 12, subject, shortfalls) or the canonical
   `rustev.plan/2` document and its `PlanId`. Exit codes distinguish
   refusal from I/O failure.
2. `rustev run`: evaluates one snapshot through `rustev-runtime` with a
   declared backend configuration (spec 005's backends only; no network),
   an explicit evaluation time, `TokioClock`, a `RuntimeConfig` (admission
   bounds, parallelism, sink policy) and a file sink that writes the
   `rustev.run/1` record. Prints the judgment, or the typed rejection, or
   `evidence_not_delivered` with the record path. Exit codes distinguish
   judged, cancelled, rejected, evidence not delivered and I/O failure.
3. `rustev replay`, `rustev eval` and `rustev calibrate`: thin surfaces over
   spec 004.
4. File reads are bounded by the same limit sets as the contract; the CLI
   owns transport buffering for files (spec 002, 3.2.4). Run records are
   read under `RECORD_V1`.

## 4. Out of scope

HTTP serving (`integrations/rustev-serve`), publication to crates.io.

## Acceptance (draft)

- Golden CLI outputs for both reference plans, including one refusal per
  category (1 to 12) and one run record per termination.

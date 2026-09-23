---
id: "006-cli-surface"
title: "CLI surface"
status: draft
implementation: pending
created: "2026-09-23"
summary: >
  Increment 2: `rustev`, a command-line surface over the library: plan check,
  show and compile against descriptor and calibration files; run and replay
  with a declared backend configuration; eval and calibrate over named
  datasets. No hosting, no HTTP.
establishes:
  - { kind: directory, path: "crates/rustev-cli/" }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "004-evaluation-and-replay"
---

# 006: CLI surface

Draft. A proposal, not a claim about code. Rationale: design section 15.

## 1. Purpose

Let a package author compile, inspect, run, replay and evaluate a plan
without writing Rust, using only files and the pinned library.

## 2. Territory

`crates/rustev-cli/`, binary `rustev`. Depends on the library crates only.

## 3. Behavior (to be made concrete before approval)

1. `rustev plan check|compile|show`: reads a definition, descriptors and
   calibrations; prints the refusal (category, subject, shortfalls) or the
   canonical plan and its `PlanId`. Exit codes distinguish refusal from I/O
   failure.
2. `rustev run`: evaluates one snapshot with a declared backend
   configuration and an explicit evaluation time or clock choice; prints the
   judgment and writes the evidence record.
3. `rustev replay` and `rustev eval` and `rustev calibrate`: thin surfaces
   over spec 004.
4. File reads are bounded by the same limit sets as the contract; the CLI
   owns transport buffering for files (spec 002, 3.2.4).

## 4. Out of scope

HTTP serving (`integrations/rustev-serve`), publication to crates.io.

## Acceptance (draft)

- Golden CLI outputs for both reference plans, including one refusal per
  category.

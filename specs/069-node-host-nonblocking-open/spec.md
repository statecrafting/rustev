---
id: "069-node-host-nonblocking-open"
title: "The Node binding's filesystem host opens inputs without blocking (amends 006, 027 and 067)"
status: approved
implementation: pending
created: "2026-11-08"
summary: >
  An amendment of approved specs 067 (3.4.2 and its implementation
  record's residual race), 027 (the Node binding's host) and 006 (3.3.1's
  non-blocking open, which moves to a shared home), for #566, ratified by
  the owner on 2026-11-08 (A-89). The Node binding's filesystem host for
  `engine.liveRun` opens every input file and store item with the standard
  library's blocking open, so a regular file swapped for a FIFO or device
  between its metadata check and the open can block the call; the CLI
  closes that window with a non-blocking open. This spec moves the CLI's
  open into `rustev-surface`, has the CLI and the Node host both use it,
  and requires a test that a FIFO at an input path or store item is
  refused without blocking. Nothing read changes: the check on the open
  handle already refused a non-regular file.
amends:
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "067-node-live-repeat-runs"
extends:
  # The shared non-blocking open (3.1).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "crates/rustev-surface/" }, nature: amending }
  # The Node filesystem host opens through it (3.2).
  - { spec: "027-node-and-wasm-bindings", unit: { kind: directory, path: "integrations/rustev-node/" }, nature: amending }
  # The CLI calls the moved open (3.1.3).
  - { spec: "006-cli-surface", unit: { kind: directory, path: "crates/rustev-cli/" }, nature: amending }
  # `rustev-surface` gains a Unix-only `libc` dependency (3.1.2).
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
depends_on:
  - "001-boundaries-and-authority"
  - "006-cli-surface"
  - "027-node-and-wasm-bindings"
  - "067-node-live-repeat-runs"
references:
  - { unit: { kind: file, path: "docs/decisions/00-founding-decisions.md" }, role: context }
obligations:
  - id: "I-1"
    kind: invariant
    text: "On Unix, every input file and store item the Node binding's filesystem host reads is opened with `O_NONBLOCK` (store items also with `O_NOFOLLOW`) through `rustev_surface::host::open_input`, so a FIFO or device swapped in after the metadata check opens at once and is refused by the check on the handle; no read through that host waits for a FIFO writer."
    anchor: "3-2-the-node-filesystem-host"
  - id: "I-2"
    kind: invariant
    text: "The CLI and the Node host share one implementation of the non-blocking open; the CLI's `rustev_cli::io::open_input` behavior (006 3.3.1) is unchanged."
    anchor: "3-1-one-shared-open"
  - id: "R-1"
    kind: requirement
    text: "A Unix test creates a FIFO with no writer at an input path and at a store item, and asserts that the Node host's read returns the not-a-regular-file refusal (input) or `Inaccessible` (store item) within a bounded time instead of blocking."
    anchor: "3-3-tests"
---

# 069: The Node binding's filesystem host opens inputs without blocking (amends 006, 027 and 067)

Approved: ratified by the owner on 2026-11-08 (A-89) for the 0.7.0
cycle; the implementation is pending. The approved texts of 006, 027 and
067 are not edited; this spec records what it changes in them.

## 1. Purpose

Spec 067 gave the Node binding `engine.liveRun` with a filesystem host
(`integrations/rustev-node/src/live_fs.rs`, #556). The host refuses a
non-regular input by its metadata before opening and again on the open
handle, but it opens with `std::fs::File::open`, which blocks on a FIFO
until a writer appears. A regular file replaced by a FIFO or device
between the check and the open therefore blocks the call. Nothing wrong is
read (the check on the handle refuses it), but a host whose input
directory another local process can rewrite during the call can be held
indefinitely. 067's implementation record names this residual race and
leaves it to #566.

The CLI closed the same window in 2026-09 (006 3.3.1): `rustev_cli::io::
open_input` opens with `O_NONBLOCK`, and with `O_NOFOLLOW` for store
items. This spec gives the Node host that open, from one shared home.

## 2. Territory

`crates/rustev-surface/src/host.rs` (the shared open), its
`Cargo.toml` (a Unix-only `libc` dependency), `crates/rustev-cli/src/io.rs`
(calls the shared open), and `integrations/rustev-node/src/live_fs.rs`
(both open sites). No document, schema, CLI flag, Node declaration or
error class changes.

### 2.1 Binding principles

`rustev-surface` keeps `#![forbid(unsafe_code)]`: the open uses
`std::os::unix::fs::OpenOptionsExt::custom_flags` with `libc`'s
constants, which needs no `unsafe`. `libc` is not an HTTP stack, async
runtime or integration dependency, so 001's dependency boundaries and
`make boundaries` are unaffected. `rustev-contract` and `rustev-core` do
not change.

## 3. Behavior

### 3.1 One shared open

1. `rustev_surface::host::open_input(path: &Path, no_follow: bool) ->
   std::io::Result<File>` is the CLI's current `open_input` moved
   unchanged: read-only; on Unix `O_NONBLOCK`, plus `O_NOFOLLOW` when
   `no_follow`; elsewhere a plain open.
2. `rustev-surface` depends on `libc` under
   `[target.'cfg(unix)'.dependencies]` only.
3. `rustev_cli::io::open_input` calls (or re-exports) the shared
   function; every CLI caller (`io.rs`, `trace.rs`, `replay.rs`) keeps its
   behavior and its tests.

### 3.2 The Node filesystem host

1. `Reads::read_classified` opens with `open_input(path, false)`: the
   metadata check before the open and the check on the handle stay, and
   their refusals and messages are unchanged.
2. The store resolver opens each item with `open_input(path, true)`,
   matching its `symlink_metadata` check (a final symbolic link is
   refused, as the CLI's store does); a failed open or a non-regular
   handle is `Inaccessible`, as today.
3. The module comment and the in-code notes that describe the residual
   race are replaced by a statement of the non-blocking open.

### 3.3 Tests

1. Unix-only unit tests in `live_fs.rs` create a FIFO (`mkfifo` through
   `libc` in the test, or `std::process::Command::new("mkfifo")`) with no
   writer, at an input path and at a store item, call the host's read on
   a thread, and assert the refusal arrives within a bound (10 seconds)
   rather than the call blocking. The swap itself cannot be timed
   deterministically, so the tests open the FIFO directly through the
   same code path the swap would reach.
2. A `rustev-surface` unit test pins `open_input` on a FIFO (returns at
   once) and, with `no_follow`, on a symbolic link (refused).

### 3.4 What the amended specs now say

- **067 3.4.2 and its implementation record:** the filesystem host's
  inputs are opened without blocking; the residual race recorded there
  is closed by this spec.
- **027:** the Node binding's filesystem host meets 006 3.3.1's
  non-blocking open requirement for inputs and store items.
- **006 3.3.1:** the open's implementation lives in `rustev-surface`;
  the requirement and behavior are unchanged.

## 4. Out of scope

Output writes (`publish_outputs`, `publish_live_outputs`), the journal's
open (043 3.5, `rustev_live::journal`) and the memory host; Windows named
pipes, which `File::open` on a regular path cannot reach; the WebAssembly
binding, which has no filesystem host.

## 5. Observable negative cases

| Case | Expected |
|---|---|
| An input path that is a FIFO with no writer, read by the Node host | Refused `not a regular file` at once; the call does not block. |
| A store item that is a FIFO with no writer | `Inaccessible` at once. |
| A store item that is a symbolic link to a regular file | `Inaccessible` (not followed). |
| An input path that is a regular file | Read as before, bounded by the document limit. |
| A CLI input path that is a FIFO | Refused at once, as before (006 tests unchanged). |

## Acceptance

- The tests of 3.3 pass on Linux and macOS in `make code` and in the
  bindings workflow's native jobs; the Windows build compiles the plain
  open.
- `rustev-cli`'s existing non-blocking open tests pass unchanged.
- `cargo semver-checks` reports `rustev-surface`'s new public function as
  an addition only.
- `make code`, declared verification and `make gate` pass.

## Verification

Pending with implementation.

## Implementation record

None yet.

## Decisions at approval

Decided in this text, with the reason beside each: the open moves to
`rustev-surface` rather than being copied into `rustev-node` (3.1),
because #566 offers both and one implementation keeps the CLI and the
binding from drifting; store items use `no_follow` (3.2.2), because the
resolver already refuses links by `symlink_metadata` and the CLI's store
opens the same way; the swap race is tested through the FIFO path rather
than timed (3.3.1), because a timed swap is nondeterministic.

Ratified by the owner on 2026-11-08 (A-89: "I approve all of them", in
reply to the proposal to spec every open issue that needed one). Target:
the 0.7.0 cycle.

<!-- spec-spine:begin generated id="header" -->
# `044-runtime-optimization-increment-2` Runtime optimization increment 2: duplicate suppression, persistent and decision-result caches, batching qualification (amends 017, 003, 002, 009, 012 and 020)

- Path: `specs/044-runtime-optimization-increment-2/spec.md`
- Summary: An amendment of approved spec 017 (and, through it, of spec 003's concurrency and cancellation rules) for #76 child F1, which also amends specs 002, 009, 012 and 020 where its members and batching rules reach them. Completes 017 increment 2 and adds a decision-result cache. Concurrent duplicate suppression keeps spec 003's rule that the runtime spawns no task per request: a shared attempt lives in a runtime-owned slot that is driven by whichever waiter is polled, a waiter that cancels returns at once and hands charge ownership to the oldest remaining waiter, and the last waiter to leave cancels the attempt under spec 003's existing rules. Persistent caches sit behind a host-implemented `RuntimeStore` seam with durable generations, conditional writes, byte budgets and integrity-checked `rustev.cache-entry/1` entries. The decision-result cache reuses a whole decision's semantic work under a compiler-derived key that always holds plan identity, the complete authorized scope, the profile and declared upstream-origin dimensions; on a hit the core recomputes the judgment and the runtime re-runs the compiler-derived dependency closure of every declared volatile source, with a declared policy when refresh fails that never serves stale evidence as a value. Capabilities may declare `isolation: per_item`, which every batching path honours, and adapter-scoped batching gains a qualification procedure and report (`rustev.batch-qualification/1`). Plans, descriptors and records that use none of this keep their bytes and identities. Approved (A-41); implementation is in progress.
- Reference page schema: `1.0.0`
<!-- spec-spine:end generated id="header" -->

<!-- spec-spine:begin generated id="lifecycle" -->
## Lifecycle

| Field | Value |
| --- | --- |
| status | `approved` |
| implementation | `complete` |
| superseded_by | None declared. |
| retirement_rationale | None declared. |
| created | `2026-10-17` |
<!-- spec-spine:end generated id="lifecycle" -->

<!-- spec-spine:begin generated id="requirements" -->
## Requirements

| Obligation | Kind | Text | Anchor | Withdrawn |
| --- | --- | --- | --- | --- |
| `044-runtime-optimization-increment-2#I-1` | `invariant` | Duplicate suppression spawns no task: a shared attempt is polled only from the tasks of its waiters, progresses whenever any remaining waiter is polled, and a waiter that cancels, times out or is dropped returns or drops without waiting for the attempt while another waiter remains. | `3-2-concurrent-duplicate-suppression` | no |
| `044-runtime-optimization-increment-2#I-2` | `invariant` | One shared attempt has exactly one charge owner at every instant and one settlement; a handoff moves ownership to a waiter that already holds the full reservation, and an unknown charge stays one liability on the final owner's record, linked by shared-call id from every waiter. | `3-2-4-charge-reservation-and-liability` | no |
| `044-runtime-optimization-increment-2#I-3` | `invariant` | Every cache key, request-level or decision-level, memory or persistent, is derived by Rustev from the plan's compiled key schema and always contains the plan identity, the complete authorized scope, the profile identity or `none`, and the key-schema and namespace versions; no host API accepts a key, and a request that cannot supply a declared key dimension is never served from or admitted to a cache. | `3-5-key-discipline` | no |
| `044-runtime-optimization-increment-2#I-4` | `invariant` | A persistent entry is served only after its schema, complete key material, integrity digest, generations, expiry and output validation all check; anything else is a miss with a diagnostic, never a value, and a write commits only if the store's generations still equal those captured before the work began. | `3-4-runtime-store-and-persistent-entries` | no |
| `044-runtime-optimization-increment-2#I-5` | `invariant` | A decision served from the decision-result cache has its judgment recomputed by the core from the new request's snapshot and evaluation time; every request in the refresh closure of a declared volatile source is dispatched again; and a failed refresh never supplies a value outside its declared freshness. | `3-6-decision-result-cache` | no |
| `044-runtime-optimization-increment-2#I-6` | `invariant` | A capability that declares `isolation: per_item` is never dispatched as part of any batch, runtime-planned or adapter-scoped, whatever the batching configuration. | `3-7-per-item-isolation` | no |
| `044-runtime-optimization-increment-2#R-1` | `requirement` | Plans, descriptors, run records and policies that use none of this spec's members keep byte-identical documents and identities, proved by the existing goldens. | `3-10-compatibility` | no |
| `044-runtime-optimization-increment-2#R-2` | `requirement` | Adapter-scoped batching on a production binding is constructible only with a cited `rustev.batch-qualification/1` report whose verdict is `qualified`; enabling it remains the owner's act under R-28. | `3-8-adapter-scoped-batching-qualification` | no |
| `044-runtime-optimization-increment-2#R-3` | `requirement` | The decision-result cache (3.6) is built on the versions it adds to, `rustev.plan/3` and `rustev.run/3` (both introduced by spec 032), and is not implemented before those versions are; specs 032, 034, 035, 036 and 037 are in this spec's `depends_on`. | `3-1-policy-members-and-plan-identity` | no |
<!-- spec-spine:end generated id="requirements" -->

<!-- spec-spine:begin generated id="ownership" -->
## Ownership

| Edge | Target spec | Unit | Planned | Resolution |
| --- | --- | --- | --- | --- |
| `extends` | `006-cli-surface` | `{"kind":"directory","path":"crates/rustev-cli/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-contract/"}` | no | `resolved` |
| `extends` | `002-decision-contract-and-pure-core` | `{"kind":"directory","path":"crates/rustev-core/"}` | no | `resolved` |
| `extends` | `004-evaluation-and-replay` | `{"kind":"directory","path":"crates/rustev-eval/"}` | no | `resolved` |
| `extends` | `003-runtime-execution-and-evidence` | `{"kind":"directory","path":"crates/rustev-runtime/"}` | no | `resolved` |
| `extends` | `012-jev-integration` | `{"kind":"directory","path":"integrations/rustev-jev/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-node/"}` | no | `resolved` |
| `extends` | `009-remote-adapter-protocol` | `{"kind":"directory","path":"integrations/rustev-remote-http/"}` | no | `resolved` |
| `extends` | `027-node-and-wasm-bindings` | `{"kind":"directory","path":"integrations/rustev-wasm/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"directory","path":"scripts/acceptance/"}` | no | `resolved` |
| `extends` | `031-public-api-and-release-family` | `{"kind":"directory","path":"tools/rustev-release/"}` | no | `resolved` |
| `extends` | `001-boundaries-and-authority` | `{"kind":"file","path":"Makefile"}` | no | `resolved` |

### References (non-owning)

- `{"kind":"file","path":"docs/backlog.md"}`
- `{"kind":"file","path":"docs/decisions/00-founding-decisions.md"}`
- `{"kind":"file","path":"docs/design/001-decision-engine-architecture.md"}`
<!-- spec-spine:end generated id="ownership" -->

<!-- spec-spine:begin generated id="signatures" -->
## Signatures

None declared.
<!-- spec-spine:end generated id="signatures" -->

<!-- spec-spine:begin generated id="relations" -->
## Relations

Not supplied.
<!-- spec-spine:end generated id="relations" -->

<!-- spec-spine:begin generated id="operations" -->
## Operations

Not supplied.
<!-- spec-spine:end generated id="operations" -->

<!-- spec-spine:begin generated id="evidence" -->
## Evidence

### Declared inputs

None declared.

### Acceptance commands (declared, not executed)

```console
cargo test -p rustev-contract --locked --test documents
cargo test -p rustev-core --locked --test execution --test generation_compile --test identity --test reference_plans
cargo test -p rustev-core --locked --lib execution::tests
cargo test -p rustev-runtime --locked --lib -- shared::tests store::tests optimization::
cargo test -p rustev-runtime --locked --test shared_calls
cargo test -p rustev-runtime --locked --test persistent --test decision_cache
cargo test -p rustev-runtime --locked --test acquisition --test generation --test extraction
cargo test -p rustev-runtime --locked --test optimization
cargo test -p rustev-remote-http --locked --test isolation
cargo test -p rustev-eval --locked --lib batch_qualification
cargo test -p rustev-remote-http --locked --test batch_qualification
cargo test -p rustev-jev --locked --test batch_qualification
cargo test -p rustev-jev --locked --test production
cargo test -p rustev-runtime --locked --test optimization_benchmark_044 -- --ignored --nocapture
```
<!-- spec-spine:end generated id="evidence" -->

<!-- spec-spine:begin generated id="limitations" -->
## Limitations

None declared.

This page restates declared and structurally resolved records. It does not state that code is correct, that a test ran or passed, or that anything was accepted, released, adopted, or observed.
<!-- spec-spine:end generated id="limitations" -->

<!-- spec-spine:begin generated id="changes" -->
## Changes

| Edge | Specs |
| --- | --- |
| amends | `002-decision-contract-and-pure-core`, `003-runtime-execution-and-evidence`, `009-remote-adapter-protocol`, `012-jev-integration`, `017-general-runtime-optimization`, `020-remote-protocol-refinements` |
| amends_sections | None declared. |
| amends_verification | None declared. |
| supersedes | None declared. |
| amendment_record | None declared. |
| amended by | `063-node-binding-parity` |
| superseded by | None declared. |
| extended by | None declared. |
<!-- spec-spine:end generated id="changes" -->

<!-- spec-spine:begin generated id="inputs" -->
## Inputs

| Identity | Digest |
| --- | --- |
| `index:044-runtime-optimization-increment-2` | `sha256:7ee9a6e0b9f48ddfad5a4e058839cf707b30b891b1875cf7f00b2d8f6d9890f2` |
| `registry:044-runtime-optimization-increment-2` | `sha256:7ee9a6e0b9f48ddfad5a4e058839cf707b30b891b1875cf7f00b2d8f6d9890f2` |
<!-- spec-spine:end generated id="inputs" -->

<!-- spec-spine:begin authored id="notes" -->
<!-- spec-spine:end authored id="notes" -->

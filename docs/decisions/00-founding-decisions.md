# 00: Founding decisions

What is **adopted**, what the **owner decided**, what remains **open**, and the
**facts** those rest on. A recommendation is not a decision until this file
records it as one. Engineering choices made while implementing an approved
spec are recorded in that spec, not here.

## Adopted

| Id | Decision | Date |
|---|---|---|
| D-01 | Rust, one Cargo workspace, one owning spec per crate. | 2026-09-23 |
| D-02 | Governed by spec-spine, pinned exactly (`required_version = "=0.24.0"` in `spec-spine.toml`), installed repository-locally into `.tooling/bin` by `make tools`. A binary elsewhere on `PATH` does not answer for this repository. | 2026-09-23 |
| D-03 | The repository was initialized by `statecraft-cli init apply` (built from its checkout at `8e22444`), which scaffolded governance through the spec-spine library and registered the project. It is registered and qualified, not armed. | 2026-09-23 |
| D-04 | Rustev is the decision component of the ecosystem: it compiles and executes typed decision plans. It does not own knowledge, authority, identity or agent workflows. Normative text: spec `001`. | 2026-09-23 |
| D-05 | Registered decision plans first; dynamic questions later, as a separately evaluated class with narrower claims. | 2026-09-23 |
| D-06 | Pluggability is by Rust traits with explicit composition, plus a versioned protocol for remote adapters. No dynamic plugin loading. | 2026-09-23 |
| D-07 | Apache-2.0, as the repository was created. | 2026-09-23 |

## Owner decisions

Decided by the owner on 2026-09-23 in response to the recommendations in the
design's section 18. Recorded here as the owner's decisions, not the agent's.

| Id | Decision |
|---|---|
| R-01 | First semantic-backend candidate: frozen embeddings plus a linear head. ONNX Runtime through `ort` is the preferred implementation candidate, subject to a later bounded check of checkpoint compatibility, licensing, supported operators, linking, packaging and measured resource use. Increment 1 adds no inference dependency. Not authorized: paid inference, custom-model training, unsupported accuracy claims. A task-specific linear head is never represented as supporting arbitrary new labels. |
| R-02 | Authoring: a typed Rust builder and versioned canonical JSON interchange, both entering the same validated representation. Golden JSON is an emitted compatibility fixture; there are never two independently maintained sources of behavior. |
| R-03 | Contract and core are executor-independent. The first runtime uses Tokio, behind explicit time, cancellation and scheduling boundaries where useful. No general executor abstraction, and no promise of another executor before there is evidence for one. |
| R-04 | Clearly labeled synthetic fixtures implement and verify increment 1 and infrastructure behavior. Real labeled data gates empirical quality, calibration and usefulness claims, not infrastructure. Before semantic quality qualification, an appropriately licensed public dataset or an independently human-labeled dataset is identified, with provenance, licensing, labeling method, splits and limitations recorded. Generated examples are not independent evidence of semantic quality. |
| R-05 | Primitive names `classify`, `proposition`, `rubric`, `rank`, with distinct semantics. No vendor-specific names; no claim of Jev compatibility. |
| R-06 | The versioned evaluation report envelope lives in `rustev-contract`. Metrics, datasets, evaluation execution and calibration fitting live outside the contract crate. Extraction into a separate crate requires a demonstrated need. |
| R-07 | O-01 closed (2026-09-23). The bootstrap's `determinism-requirement` anchor and principle IV are read as governing compiled governance artifacts. Principle XIII governs Rustev's own distinction between canonical compilation, defined exact computation, backend numerical repeatability and runtime observations. The missing anchor text (C-07) remains a defect of the scaffold; this reading does not repair it, and the frozen bootstrap is not edited. |
| R-08 | The first runtime slice (spec `003`) includes bounded execution, admission, deadlines, concurrency, budget accounting, cancellation, bounded retries, explicit runtime fallback and evidence delivery. Automatic batching, cross-request duplicate suppression, reusable inference caches and persistent caches are deferred to a separately specified optimization increment, and the deferral is recorded in spec `003` rather than dropped silently. |
| R-09 | The runtime is verified with deterministic scripted test backends whose dispatch, completion, cancellation and cost are observable. They are test fixtures, not spec `005`'s production backends. No inference dependency, checkpoint download, paid provider call or real-user dataset is needed. |
| R-10 | Compile-time capability fallback and runtime failure fallback are different policies. Runtime retries and fallback are explicitly declared, validated and included in plan identity, and are never inferred from whichever backend happens to be installed. |
| R-11 | Delivery authority for spec `003`: the bounded contract changes it needs, its implementation, tests and documentation, local commits, scoped pushes and pull requests, remediation of review and CI findings, and merge after verification. It does not extend to approving specs `004` to `006`. |

## Approvals

| Id | Act | Date |
|---|---|---|
| A-01 | The owner approved spec `001` with bounded corrections: proposals never grants, with the signature-restriction limitation stated; no Rustev-owned authorization type; derivation classes with lineage; network implementations under `integrations/`; ownership of workspace machinery; constitution principles VI onward under the existing `VI onward` heading. | 2026-09-23 |
| A-02 | The owner approved spec `002` with bounded corrections: enforceable parse bounds with transport buffering owned separately; calibration as identity and binding, with one explicit initial method and fitting deferred; determinism scoped to canonical compilation and defined exact computation; explicit acceptance and executable verification. Its lifecycle fields are recorded in the change that makes it concrete. | 2026-09-23 |
| A-03 | The owner approved spec `003` within the bounded requirements of their runtime brief: time, admission and concurrency, budgets, retries and fallback, cancellation, evidence and judgment equivalence as stated there, with the optimization deferral of R-08 and executable acceptance. The concrete rules were then written by the agent within those bounds and are reviewable in the change that recorded this approval. | 2026-09-23 |

No approval authorizes ratifying any other spec.

## Open

None. O-01 was closed by the owner as R-07.

## Closed

| Id | Question | Resolution |
|---|---|---|
| O-01 | The bootstrap freezes the anchor `determinism-requirement`, and its summary says "every artifact is a deterministic function of (config, file contents)". Its body has no text for that anchor (C-07). Does the anchor reach Rustev's runtime outputs (evidence records with latency, backend outputs), which cannot be byte-deterministic? | Closed by the owner on 2026-09-23 (R-07): it governs compiled governance artifacts; principle XIII governs Rustev's runtime outputs. C-07 stands. |

## Facts

| Id | Fact | Source |
|---|---|---|
| C-01 | spec-spine 0.24.0 is released (`spec-spine-cli` on crates.io, tag `v0.24.0`), and `.tooling/bin/spec-spine --version` reports `0.24.0`. | `cargo search spec-spine-cli`; the local binary, 2026-09-23 |
| C-02 | statecraft-cli links `spec-spine-core =0.23.0` (still true at its checkout `e1d74fa`), so its governance scaffold is produced by the 0.23.0 library even when the 0.24.0 CLI runs the corpus step. Its environment manifest records the CLI pin as `0.24.0`, observed from `PATH`. | `crates/statecraft-home/Cargo.toml` in statecraft-cli; `.statecraft/environment.json` |
| C-03 | statecraft-cli resolves a bare `spec-spine` on `PATH` for the corpus step; it does not consult `.tooling/bin`. Initialization was run with `.tooling/bin` prepended to `PATH`. | statecraft-cli README, "What each step needs" |
| C-04 | The scaffold writes `spec-spine.toml` with the pin commented out and `specs/000-bootstrap/spec.md` with a placeholder date, and records both as `managed`. Customizing them, as the scaffold instructs, makes `statecraft-cli doctor` report both as `drifted`; replacing the constitution placeholder, as it also instructs, adds `standards/spec/constitution.md`. This is environment drift of managed files, not corpus health: `make gate` judges the corpus and passes. A dogfood finding for statecraft-cli; the ownership records are not rewritten to hide it. | `doctor` output, 2026-09-23 |
| C-04a | The same scaffold set `[layout] derived_dir = ".statecraft/derived"` but left `[index] resolver_exclusions` at the old `.derived`, and did not exclude `.tooling`. Corrected here at adoption. | `spec-spine.toml` as scaffolded |
| C-05 | `.statecraft/derived/` is committed, so `check` is a freshness gate on the committed shards. | the managed `.gitignore` block |
| C-06 | Aicortex and Rahi were not assessed beyond their README summaries, which describe Aicortex as specified and largely `pending` and Rahi as a 0.2.0 candidate. Nothing here claims their readiness. No Rustev component depends on or waits for either. | their READMEs, 2026-09-21 |
| C-07 | The scaffolded bootstrap lists five `unamendable` anchors (`markdown-truth-boundary`, `json-truth-boundary`, `determinism-requirement`, `typed-authority-graph`, `refusal-rule`). None is a heading slug in its body, and three have no body text at all; spec-spine's lint does not check them. A dogfood finding for the scaffold. | `specs/000-bootstrap/spec.md`; `make gate` |
| C-08 | Review of spec `002`'s implementation record against its approved text (`git diff 75462ff 96a794c -- specs/002-*`), made before spec `003` consumed it. No contradiction with an approved requirement was found. Clarifications: record-field references `input:<name>/<field>` give the approved `record` type an address; arithmetic faults in `count_where@1`, `member_of@1` and `compare@1` follow approved 3.4.2 (no saturation, no wrapping) where the approved operator table said "none"; `supply` accepting only the four runtime reasons is required for approved 3.11.2's static coverage to be sound; exact output-kind matching is the literal reading of "output kind as bound". For the owner's attention: (1) "policy parameters are exact values, never semantic values" narrows approved 3.11.4 beyond what A-02 stated; it refuses rather than permits, so it is safe, and relaxing it later is additive; (2) `div` rounds at 10^-9 and `round` rescales, so `round(div(a, b), s)` and `fx` round more than once; this follows approved 3.4.2, differs from a single rounding at `s`, and a change would be a new operator version (E-06); (3) approved normative text in sections 3.8, 3.10 and 3.11 was edited in the implementing change, with the edits listed in its implementation record. | the diff above; `crates/rustev-core/src/expr.rs` |

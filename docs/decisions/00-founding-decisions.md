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

## Approvals

| Id | Act | Date |
|---|---|---|
| A-01 | The owner approved spec `001` with bounded corrections: proposals never grants, with the signature-restriction limitation stated; no Rustev-owned authorization type; derivation classes with lineage; network implementations under `integrations/`; ownership of workspace machinery; constitution principles VI onward under the existing `VI onward` heading. | 2026-09-23 |
| A-02 | The owner approved spec `002` with bounded corrections: enforceable parse bounds with transport buffering owned separately; calibration as identity and binding, with one explicit initial method and fitting deferred; determinism scoped to canonical compilation and defined exact computation; explicit acceptance and executable verification. Its lifecycle fields are recorded in the change that makes it concrete. | 2026-09-23 |

Neither approval authorizes ratifying any other spec.

## Open

| Id | Question | Recommendation |
|---|---|---|
| O-01 | The bootstrap freezes the anchor `determinism-requirement`, and its summary says "every artifact is a deterministic function of (config, file contents)". Its body has no text for that anchor (C-07). Does the anchor reach Rustev's runtime outputs (evidence records with latency, backend outputs), which cannot be byte-deterministic? | Read it as governing this corpus's compiled artifacts, as the constitution's own text scopes principles I to V to the corpus, and as spec-spine's own bootstrap section 6 does. Principle XIII is written under that reading. No bootstrap edit is needed; the owner confirming the reading closes this. Work proceeds under it because no delivered code depends on a different reading. |

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

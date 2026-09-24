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

### Owner decisions of 2026-09-23, increment 2 continued

Decided by the owner on 2026-09-23 in their brief for the rules backend.
Recorded here as the owner's decisions, not the agent's.

| Id | Decision |
|---|---|
| R-12 | Sequencing: the rules-backend portion of spec `005` is implemented before spec `004`, because the backend supplies execution fixtures for evaluation and CLI work. This is an explicit, single exception to ordinal build order; it does not authorize any other reordering. |
| R-13 | Spec `005` is narrowed to the deterministic rules backend. The semantic-backend work (R-01's frozen embeddings plus a linear head, the bounded `ort` check, numerical tolerance) moves to spec `011-semantic-backend`, a draft allocated at the next free ordinal after the design roadmap's `007` to `010`. R-01's direction and rationale are unchanged; no spec carrying unfinished semantic-backend obligations is marked complete. |
| R-14 | C-08 item (1) is resolved: the requirement that selection-policy control parameters (proposal parameters, thresholds, weights and every other policy control) are exact values or literals, never semantic values, is retained (spec `002`, 3.11.4). Semantic results remain usable as decision inputs according to their value kinds; they cannot silently redefine a threshold, weight or other policy control. Relaxing this is a reviewed amendment, not an implementation detail. |
| R-15 | C-08 item (2) is resolved: per-operation fixed-point rounding is retained, including the composed `round(div(a, b), s)` and `fx` (each step half-even at 10^-9, then half-even to `s`). Representative boundary examples are documented with spec `002` and kept as regression tests. Final-only rounding would be a separately versioned arithmetic contract (a new operator or registry version, E-06), never an edit. |
| R-16 | C-08 item (3) is preserved, not rewritten: normative text of approved spec `002` (sections 3.8, 3.10, 3.11) was edited in its implementing change, as its implementation record lists. From now on a substantive change to approved behavior is proposed and reviewed before implementation, through an `amends` edge in a separately reviewable change (as spec `003` did), never by editing approved text while implementing it. |
| R-17 | No semantic inference in this increment: no `ort` dependency, model download, training, paid inference or semantic quality qualification. R-01 stands for spec `011`. |
| R-18 | Delivery authority for the rules backend: the narrowed spec `005` and any bounded prerequisite contract change it needs, its implementation, verification, review fixes and documentation, scoped commits, pushes, pull requests and merge after checks pass, and updating later drafts to match delivered interfaces while they stay drafts. It does not extend to approving specs `004`, `006` or `011`, implementing them, releases, publication, deployments, paid services, real-user data, sibling repositories, managed-environment repair, branch protection or waivers. |

### Delegated replay decisions of 2026-09-23

The owner asked to keep pressure on delivery and stated: "I approve optimal
decisions that push us onwards". In the context of the four replay questions,
this authorizes choosing and recording
the next bounded replay work order. The choices below are the agent's
selections under that delegation, not quotations of choices individually
made by the owner. They do not claim a mathematically optimal design.

| Id | Decision |
|---|---|
| R-19 | Host-owned replay storage, access control, expiry and erasure. Default capture is off and content retention is digest-only. Bundle metadata defaults to seven days, with a hard maximum lifetime of 30 days; embedded and external bytes require explicit retention and cannot outlive the bundle. The host may tighten the cap. These engineering defaults bound exposure and storage without creating a Rustev storage service. They do not establish legal suitability or prove deletion of copies. |
| R-20 | Live re-execution is deferred to a later spec. Spec 004 delivers offline historical reproduction and candidate comparison from retained raw outputs. This keeps fresh observations and provider execution outside reproduction and keeps eval's normal dependencies limited to contract and core. |
| R-21 | Request identity includes both tenant and effective principal-scope handles. The host supplies opaque non-secret handles, rotating the scope handle when permissions change. Equality isolates retained evidence; it never grants access. This deliberately forgoes cross-principal reuse even within a tenant. |
| R-22 | Runtime handoff is opt-in bounded capture returned separately with the normal result, including sink-delivery failure. Existing APIs and run-record bytes stay compatible. No callback, persistence or sink payload enlargement. Capture overflow is explicit incompleteness, never a reason to alter the decision. |
| R-23 | Delivery authority for the bounded spec 004 work order: record these decisions and A-05; deliver its contract amendments separately before implementation under R-16; implement and verify the approved replay/evaluation increment; make scoped commits, pushes and PRs; remediate review/CI findings and merge verified changes. Spec 006 and 011 approval/implementation, live inference, releases, publication, deployment, paid services, real-user data, sibling repositories, managed-environment repair, protection changes, waivers and recurring monitors remain outside this authority. |

### Owner refinement during replay review, 2026-09-23

The owner explicitly requested tenant-only and principal-scope isolation as
configurable alternatives while the approval PR was still under review.
This refines R-21 before implementation; its original selection above is
preserved as history.

| Id | Decision |
|---|---|
| R-24 | Replace R-21's fixed principal requirement with two mutually exclusive scope modes: `tenant_only{tenant, context_revision}` and `principal{tenant, context_revision, principal_scope}`. Principal scope remains the default; tenant-only is an explicit host choice that permits cross-principal reuse only within a tenant and revision, where computation is principal-independent and each reader is authorized by the host. Mode and all scope fields enter request identity. Mode changes never relabel existing evidence; omitted principal data never downgrades isolation. This configuration affects replay equivalence, not backend call authorization. Spec 004's pending implementation and acceptance cover both modes within A-05 and R-23. |

### Owner decision of 2026-09-24, the CLI

Decided by the owner on 2026-09-24, choosing the recommended option when
asked whether to make spec `006` concrete and deliver it. Recorded here as
the owner's decision, not the agent's.

| Id | Decision |
|---|---|
| R-25 | Spec `006` (CLI) is made concrete and delivered through verified merge within this scope: plan checking, compilation and inspection; rules-backed execution with optional bounded capture; offline replay with both isolation modes and an explicitly supplied trusted scope; evaluation and calibration with their companion records; bounded file access, clear exit codes and end-to-end tests for both reference tasks. Delivery authority: record this decision and A-06, deliver the concrete spec before code, implement and verify it in reviewed increments, make scoped commits, pushes and pull requests, remediate review and CI findings, and merge verified changes. Spec `011` stays a deferred draft until that workflow is usable. Outside this authority: spec `011` approval or implementation, live or model inference, network access, releases, publication, deployment, paid services, real-user data, sibling repositories, managed-environment repair, branch-protection changes, waivers and recurring monitors. |

### Owner decision of 2026-09-24, a remote backend

Decided by the owner on 2026-09-24, as stated in the work order that
drafted specs `009` and `012`. Recorded here as the owner's decision, not
the agent's.

| Id | Decision |
|---|---|
| R-26 | Rustev becomes the decision engine of a new product, travel-memory, with TypeSafe's Jev, reached through the Vercel AI Gateway, as a remote backend. Specs `009` (remote adapter protocol, the roadmap's reserved ordinal) and `012` (the Jev integration, the next free ordinal) are drafted for it. D-06, R-04, R-05, R-10, R-16, R-17 and R-23 stand: the integration is an ordinary adapter under `integrations/`, Rustev claims no Jev compatibility and exposes no Jev wire surface, and no quality claim precedes real labeled data. This decision authorizes drafting only; it does not authorize paid inference (O-02), approval or implementation of either spec, or live calls. |

## Approvals

| Id | Act | Date |
|---|---|---|
| A-01 | The owner approved spec `001` with bounded corrections: proposals never grants, with the signature-restriction limitation stated; no Rustev-owned authorization type; derivation classes with lineage; network implementations under `integrations/`; ownership of workspace machinery; constitution principles VI onward under the existing `VI onward` heading. | 2026-09-23 |
| A-02 | The owner approved spec `002` with bounded corrections: enforceable parse bounds with transport buffering owned separately; calibration as identity and binding, with one explicit initial method and fitting deferred; determinism scoped to canonical compilation and defined exact computation; explicit acceptance and executable verification. Its lifecycle fields are recorded in the change that makes it concrete. | 2026-09-23 |
| A-03 | The owner approved spec `003` within the bounded requirements of their runtime brief: time, admission and concurrency, budgets, retries and fallback, cancellation, evidence and judgment equivalence as stated there, with the optimization deferral of R-08 and executable acceptance. The concrete rules were then written by the agent within those bounds and are reviewable in the change that recorded this approval. | 2026-09-23 |
| A-04 | The owner approved spec `005` within the narrowed rules-backend scope of R-13 and the requirements of their brief (a bounded, versioned rules program; disclosure of exactly what is returned; authored interpretations for logits; program and configuration bound to artifact and descriptor identity; explicit setup; defined cost units; bounded work; honest cancellation; provenance as the existing contract defines it; executable acceptance). The approval was given in advance of the concrete contract and is recorded in the change that makes the contract concrete. It does not approve specs `004`, `006` or `011`. | 2026-09-23 |
| A-05 | The owner delegated resolution of the four replay choices and approved forward progress in that context. Recorded as approval of spec `004` within R-19 to R-23: bounded host-owned retention, scoped request equivalence, opt-in runtime capture, offline reproduction/comparison, explicit evaluation denominators and split-aware temperature fitting, with the bounded prerequisite amendments to specs `002` and `003`. Concrete engineering rules are authored by the agent in the separate approval change before code. Implementation remains pending; specs `006` and `011` remain unapproved. | 2026-09-23 |
| A-06 | The owner approved spec `006` within R-25, in advance of its concrete contract. The concrete rules are written by the agent and recorded in the change that makes the contract concrete, with its engineering choices listed as its own. Implementation is pending; spec `011` remains unapproved. | 2026-09-24 |

No approval authorizes ratifying any other spec.

## Open

| Id | Question | Gates |
|---|---|---|
| O-02 | Paid inference for Jev through the Vercel AI Gateway (or TypeSafe's API): is it authorized, under what spend cap, with which keys and environments, and which data may be sent to the provider? Not yet decided by the owner. | Spec `012` stages 2 and 3 of its qualification plan (live smoke calls, quality qualification) and any production use. It does not gate drafting specs `009` or `012`. |

O-01 was closed by the owner as R-07.

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
| C-09 | Vercel AI Gateway serves TypeSafe's Jev as model `typesafe-ai/jev` at `POST https://ai-gateway.vercel.sh/v1/evaluate` with `Authorization: Bearer <AI_GATEWAY_API_KEY>` and body `{model, state, questions}`: `state` is a string, object or array; `questions` maps a key to `{type, instructions, criteria}` with `type` one of `boolean`, `choice`, `score`; several questions are answered against one state in one request. The response is `{model, answers, usage{inputTokens, outputTokens}, providerMetadata.gateway{routing{originalModelId, resolvedProvider, canonicalSlug, finalProvider}, cost, generationId}}` and exposes no resolved model version. Provider options `providerOptions.gateway{zeroDataRetention: true, only: ["typesafe-ai"]}` are accepted. A TypeSafe-compatible base exists under `/typesafe`; Jev is not available through the Gateway's OpenAI-, Anthropic- or Cohere-compatible endpoints. TypeSafe's own API documents version pinning (`jev-1.13.0`), up to 255 choice options and 2 to 10 score levels, and Jev 1.13 weaknesses in arithmetic, counting, date comparison, multi-hop reasoning and adversarial content. Unverified: Gateway version pinning, and the Gateway's per-answer field names. | Vercel AI Gateway documentation, read 2026-09-24 as relayed in the work order; TypeSafe's API documentation for the direct-API items |

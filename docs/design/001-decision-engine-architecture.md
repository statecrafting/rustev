# 001: Rustev decision engine architecture

Status: **proposed**, 2026-09-23; reconciled the same day with the owner's
decisions R-01 to R-06 and the approved corrections to specs `001` and `002`.
Not normative. The normative contract is the spec corpus under `specs/`, and
nothing here binds code except through an `approved` spec. The discussion
archive under `discussions/` is rationale, not contract.

This document answers the first architectural deliverable named in the
ecosystem brief: a Rustev design covering its boundaries, its extension
contracts and two reference decision plans, followed by a bounded
implementation specification (`specs/002-decision-contract-and-pure-core`).

## 0. Reading guide

| Section | Question it answers |
|---|---|
| 1 | What in the inputs holds, and what this design corrects or sharpens |
| 2 to 4 | What Rustev is for, where its edges are, and the words used for them |
| 5 to 7 | The decision path, the value semantics, and the plan compiler |
| 8 to 11 | The five seams, the runtime, evidence and replay, evaluation |
| 12 to 14 | The learning cycle, the security posture, the determinism promises |
| 15 | The codebase layout and the dependency rules that keep it pluggable |
| 16 | Two reference decision plans |
| 17 to 19 | Increments, evidence gates, the spec roadmap, open decisions, deferrals |

## 1. Evaluation of the inputs

The inputs are the discussion archive (`discussions/001` to `010`) and the
ecosystem brief that reframed Rustev as the decision component of a larger
system. They agree on the center: **a versioned decision definition compiled
into an execution plan**, with exact work in Rust and a model only where a
semantic judgment is required. This design commits to that center.

### 1.1 What holds, and is adopted

- **Registered plans before dynamic questions** (`005`). Arbitrary labels are
  syntactically trivial and behaviorally expensive. Dynamic questions arrive
  later with a narrower quality claim, as a distinct plan class.
- **Distinct value semantics** (brief). A ranking score is not a probability,
  a probability estimate is not proof, an ordinal level is not an interval.
  Section 6 turns this into types that do not convert into one another.
- **Capability disclosure over uniformity** (brief). A backend states what it
  returns; an adapter never manufactures a distribution or a calibration claim.
- **Rejection over silent substitution** (brief). A plan that needs something
  no bound backend provides is refused at compile time unless it declares the
  fallback.
- **Judgment separated from authority** (brief, and `005`'s enforceable
  boundary). Section 5 makes the separation a signature property, not a
  convention.
- **Three determinism promises** (`005`): deterministic policy, measured
  numerical repeatability, and bitwise identity as a separate and costly
  requirement. Section 14.
- **Evidence gates instead of a calendar** (`005`), merged with the brief's
  four increments. Section 17.

### 1.2 What `002` claimed that this design does not carry forward

`005` already assessed these; they are recorded so they are not reintroduced.

| Claim | Disposition |
|---|---|
| Reproduce Jev's inferred topology, or its `/v1/systemone` wire shape | Not a goal. The useful part is the typed decision interface. A compatibility adapter may be written later as an ordinary integration. |
| Late interaction (MaxSim) as the universal mechanism | One backend family among several, chosen per task by measurement. |
| Parser boundaries prevent prompt injection | False. They prevent specific software defects. Authority boundaries are what bound the damage. |
| `const fn` guarantees cross-hardware float identity | False. |
| Temperature scaling yields perfect calibration | False. Calibration is a measured property of a (backend artifact, task, dataset) triple. |
| Replicated edge storage in the lite runtime | Deferred. An evidence sink with a declared durability policy covers the need. |
| The `n*p_max - 1 / (n - 1)` confidence statistic as a correctness signal | It is a transform of `p_max` and `n`. Rustev may expose it as a named, documented summary, never as a correctness probability. |

### 1.3 What the brief leaves open, and this design resolves

1. **"Policy" means two different things.** The support-routing example in `005`
   computes the queue with "deterministic policy over those results", inside
   the decision. The brief says application policy decides what may happen.
   Both are right about different things. This design names them
   **selection policy** (inside a plan: maps judgments to a proposed outcome,
   including abstention) and **authority policy** (outside Rustev: decides
   whether a proposed effect is permitted for an actor, scope and revision).
   Rustev owns the first and must not own the second. Section 4.
2. **Replay against erasure.** Replay wants the inputs; erasure removes them.
   Evidence records therefore hold snapshot *references and digests* by
   default, with snapshot retention chosen per plan. A case whose inputs were
   erased replays as `incomparable: inputs-erased`, never as a pass or a
   silent drop. Section 10.
3. **Where authority can still leak.** Rustev produces proposals, never
   grants. Keeping the judgment out of the authority function's signature is a
   useful restriction, but it is not proof: a model-derived value can still be
   copied into an input the application treats as a trusted fact. Section 5.2
   states the rule and its limit; provenance (section 12) is what lets the
   application refuse model-derived values as authority inputs.
4. **Calibration is not a backend capability.** A backend returns what it
   computes. A calibration map is a separate artifact, bound to a backend
   artifact, a task, a question, a dataset and a method, and applied by the
   core. Only a plan bound to one may emit `CalibratedProbability`. Applying
   it establishes which transformation was applied and its binding; whether
   the result is calibrated is measured on named evaluation data and
   conditions, and can fail under distribution shift.
5. **Evaluation as governance evidence.** When Statecraft accepts a change to a
   plan, a model binding or a calibration artifact, the evaluator
   configuration and the datasets it reads are members of the authority set:
   they are read at the trusted base, never from the candidate. Otherwise a
   candidate could lower its own bar.
6. **Self-generated evidence.** A recommendation Rustev produced is evidence
   that the system produced it, never independent evidence of the preference
   or fact it was based on. Outcome adapters
   record what the actor did, with the exposure that preceded it. Section 12.

### 1.4 Risks the inputs understate

- **No real datasets exist yet** for either reference domain. Synthetic
  fixtures establish mechanics (validation, abstention, replay, capability
  refusal). They cannot establish quality, and increment 2's baseline must say
  which of the two it measured. Owner decision `R-04`.
- **Aicortex and Rahi were not assessed beyond their README summaries**,
  and nothing here claims their readiness. Rustev integrations target their
  published contracts only, and nothing in Rustev's core may wait on either.
- **The traveler application is referenced but not present** in this
  workspace. It is treated as an external consumer whose needs inform the
  lodging reference plan, not as a source of requirements.

## 2. Goal and non-goals

**Goal.** A pluggable, embeddable Rust engine that makes, evaluates and
records decisions over evolving, partially trusted information: supplied state
and registered questions produce bounded, typed answers with explicit
uncertainty, explicit abstention, and execution evidence.

**Investment criterion.** A capability earns its place by unlocking more than
one application, even before one consumer requires it, provided its value can
be demonstrated across at least two contrasting domains (section 16).

**Non-goals, stated by name.** Owning a knowledge store. Granting authority.
Running arbitrary agent workflows. Authentication. Training frontier models.
Distributed execution. Dynamic plugin loading. A plugin marketplace. A hosted
service as a requirement.

## 3. Ecosystem boundaries

### 3.1 Responsibilities

| Component | Owns | Must not absorb |
|---|---|---|
| **spec-spine** | Governance semantics and contract identity: obligations, ownership, declared dependencies | Runtime inference, operational permissions, application state |
| **Rustev** | Decision computation and selection policy: exact computation, semantic backends behind capability contracts, selection policy, runtime bounds, evidence emission, evaluation machinery | The knowledge store, authority, action execution, arbitrary agent workflows, identity |
| **Aicortex** | Attributed claims: sources, actors, trust classes, revisions, validity, corrections, retrieval, lifecycle, outcome-derived observations | Deciding that recalled content is authoritative, or that an action is permitted |
| **Rahi** | Optional operational infrastructure and enforcement: identity, storage, durable audit, hosting chassis | Domain judgment, model-quality assessment |
| **statecraft-cli** | Governing development; independently accepting changes to all of the above and to application packages | Serving production decision requests |
| **Domain packages** | Task schemas, decision definitions, evaluators, integrations, selection policy for their domain | Reimplementing shared machinery |
| **Applications** | Authorization, action execution, domain-specific authority policy | Delegating authorization to a judgment |

### 3.2 Dependency direction

```
            application (authority policy, action executors, identity)
              |                 |                    |
              v                 v                    v
   domain packages ----> rustev-core <---- integrations (aicortex, rahi, serve)
              \                 ^                    |
               \                |                    v
                `--------> rustev-contract  <--- external consumers
                                                 (statecraft, aicortex eval)
```

Arrows point at what is depended on. `rustev-contract` is the only Rustev
crate an outside project needs in order to read Rustev's records and reports.
No Rustev crate outside `integrations/` depends on Aicortex, Rahi,
statecraft-cli or an HTTP stack; `rustev-contract` and `rustev-core` also
depend on no async runtime executor. Rustev is usable without any of the other
ecosystem projects.

### 3.3 Partial adoption

| Adopter wants | Takes | Supplies |
|---|---|---|
| Embedded decisions with their own database and identity | `rustev-core`, `rustev-runtime`, one backend, a package | A context source, an evidence sink, their own authority and executors |
| Only the evaluation harness for an existing classifier | `rustev-eval`, `rustev-contract` | A dataset and an evaluator |
| Aicortex with a different decision engine | Nothing from Rustev | Aicortex's own read contract |
| Rustev as a hosted service | `integrations/rustev-serve`, optionally `rustev-rahi` | Deployment |

Each row is a test obligation for increment 3 or 4: a build of that subset,
with no path dependency on the omitted crates.

## 4. Vocabulary

| Term | Meaning |
|---|---|
| **Decision definition** | A versioned, authored description of a recurring decision: inputs, steps, questions, selection policy, outputs. |
| **Plan** | A definition compiled against a concrete set of bound backends, calibration artifacts and limits. Content-identified. |
| **Context snapshot** | The inputs for one decision, fetched under authorization, each value carrying provenance and freshness. Immutable once taken. |
| **Exact step** | A deterministic computation from a closed, versioned operator set. |
| **Semantic step** | A question answered by a decision backend. |
| **Judgment** | The typed result of a plan: resolved, partially resolved, or unresolved with a reason. Never an effect. |
| **Selection policy** | The plan's deterministic mapping from facts and judgments to a *proposed* outcome, including abstention and escalation. Owned by the package, executed by Rustev. |
| **Authority policy** | The application's decision whether a proposed effect is permitted for an actor, scope and revision. Never executed by Rustev core. |
| **Effect** | A change in the world, performed by an action executor after authorization. |
| **Evidence record** | What was used and computed for one decision: identities, digests, steps, costs, fallbacks, truncation. |
| **Outcome** | What was observed after an effect, linked to the decision that preceded it. |
| **Observation** | An attributed claim derived from an outcome, proposed to a knowledge store under its write rules. |

## 5. The decision path

### 5.1 Seven steps and their owners

| # | Step | Owner | Type crossing the boundary |
|---|---|---|---|
| 1 | Obtain an authorized context snapshot | Context source (app or Aicortex adapter) | `ContextSnapshot` |
| 2 | Validate structure, provenance and freshness requirements | Rustev core | `ValidatedContext` or `Unresolved::MissingEvidence` / `InvalidInput` |
| 3 | Run exact and semantic steps | Rustev runtime over core | step values, trace |
| 4 | Produce a typed judgment, possibly unresolved | Rustev core (selection policy) | `Judgment<P>` |
| 5 | Apply authority policy | Application | an application-owned authorization, or refusal |
| 6 | Execute the permitted effect | Action executor | `ExecutionRecord` |
| 7 | Record the observed outcome | Outcome adapter, evidence sink | `OutcomeRecord`, optional proposed `Observation` |

Steps 1 to 4 are Rustev's. Steps 5 and 6 are the application's. Step 7 is
shared through contracts.

### 5.2 Proposals, never grants

Rustev produces proposals and unresolved outcomes. Before any effect, the
application validates the requested action, resource, parameters, principal,
scope and current revision against its own authority policy. Model output
cannot create or broaden permissions: a judgment may lead the application to a
narrower disposition (act, confirm, escalate, refuse) within what it permits,
never to a wider one.

```rust
// Application-owned, sketched for illustration only. No Judgment in the
// signature: what is permitted is computed from trusted facts.
fn authorize(action: &RequestedAction, principal: &Principal, scope: &Scope,
             revision: &Revision) -> Result<AppAuthorization, Refusal>;
```

Rustev's crates define no `Principal`, no `Permitted` and no type an executor
accepts as authorization. The limit of this rule is stated in spec `001`: a
signature that excludes `Judgment` restricts Rustev-supplied interfaces; it
does not prove a model-derived value was not copied into another input. Every
Rustev value therefore carries its derivation class (section 12), so the
application can refuse model-derived values as authority inputs.

Example from the brief: a model judges that an imported statement probably
expresses a durable preference. The judgment is a `Proposal` to write. The
write still needs an authenticated actor, an allowed operation, a scope and a
valid revision, and those are checked by the knowledge store's own rules.

## 6. Value semantics

Every value a step produces has exactly one of these kinds. Conversions
between kinds are explicit functions with an identity, or do not exist.

| Kind | Meaning | May be produced by | Must not be read as |
|---|---|---|---|
| `Exact<T>` | A deterministic function of validated inputs | Exact operators | A statement about reality beyond its inputs |
| `ModelScore` | An unnormalized score, comparable only within a declared scope (question, request) | Backends advertising `Scores` | A probability, or comparable across backends |
| `Distribution` | Non-negative mass over the declared options, summing to 1 within tolerance, **uncalibrated** | Backends advertising `Distribution` or `Logits` (normalized by core) | A calibrated probability |
| `CalibratedProbability` | A distribution passed through a calibration artifact bound to this backend artifact, task, question and dataset | Core only, with a bound `CalibrationId` | Proof, or valid outside the calibration's stated domain |
| `SelectedLabel` | One option, no mass | Backends advertising `Label` | Any uncertainty measure |
| `OrdinalLevel` | A level on a declared rubric, with an optional distribution over levels | Rubric steps | An interval quantity; its expectation is an index summary only |
| `RankPosition` | An order over candidates, with the scores or rules that produced it | Rank steps | A probability of being best |
| `Unresolved` | No value, with a reason | Any step | A default |

Rules the core enforces:

- A threshold on a probability requires `CalibratedProbability` unless the
  plan declares `uncalibrated_threshold` with a reason, which is carried into
  every evidence record and evaluation report.
- `SelectedLabel` cannot feed a step that requires mass. The compiler refuses
  the binding.
- Two `ModelScore`s from different scopes cannot be compared, added or ranked
  together.
- `Unresolved` propagates. A selection policy must handle it explicitly; there
  is no `unwrap_or_default` path.
- Non-finite numbers are refused at the backend boundary as
  `Unresolved::InvalidBackendOutput`, never clamped.

`Unresolved` reasons are a closed enum: `MissingEvidence{fields}`,
`StaleEvidence{fields}`, `InvalidInput`, `Abstained{rule}`,
`BackendUnavailable`, `InvalidBackendOutput`, `BudgetExhausted{resource}`,
`DeadlineExceeded`, `Conflict{facts}`, `Unsupported{capability}`.

## 7. Decision definitions and the plan compiler

### 7.1 A definition declares

- **Identity**: name, semantic version, owning package.
- **Inputs**: schemas, and per field the required provenance class (for
  example `authenticated-app-field`, `system-of-record`, `user-supplied`,
  `third-party`, `model-derived`) and maximum age.
- **Exact steps**: operator id and version from the closed set, arguments.
- **Semantic steps**: operation (`classify`, `proposition`, `rubric`,
  `rank`), permitted outputs (options, levels, candidate source), the minimum
  value kind required, input projection (which fields the backend sees).
- **Dependencies** between steps: a DAG, declared, never inferred from data.
- **Backend requirements**: operation, value kind, input limits, determinism
  tier, and optionally a pinned backend artifact.
- **Resource ceilings**: deadline, token or byte budget, cost budget, fan-out
  limits (for example candidates evaluated semantically).
- **Abstention, escalation and fallback**: rules over values and reasons,
  each fallback an explicitly declared alternative step.
- **Selection policy**: a deterministic table or expression over values.
- **Outputs**: schema of the judgment, and the evidence it must carry.

### 7.2 Authoring form (owner decision `R-02`)

Owner decision R-02. Packages author definitions with a typed Rust builder
or supply versioned canonical JSON; both enter the same validated
representation, so there is one source of behavior. Each package commits the
emitted document as a golden compatibility fixture, so a review sees a data
diff and statecraft can classify it. Operators referenced by the document are
registered, versioned Rust implementations; the document never carries code.

### 7.3 Compilation phases

1. **Parse and bound**: the supplied bytes are scanned against total-size,
   nesting, string, collection and cumulative value-count limits before any
   typed value is constructed; duplicate keys and unknown fields are refused;
   constructors are fallible. Reading bytes from a transport, and bounding that
   buffer, is the transport's job, not the parser's.
2. **Resolve**: operator ids and versions, input schemas, calibration
   artifacts.
3. **Type check**: value kinds flow correctly across the DAG (section 6).
4. **Bind backends**: match each semantic step's requirements against the
   registered backends' capability descriptors. No match and no declared
   fallback is a compile error naming the step, the requirement and each
   candidate's shortfall.
5. **Optimize** (only where semantics are preserved): deduplicate identical
   exact subexpressions, group independent semantic steps that share a
   projection and a backend into one batch, precompute static label
   representations where the backend advertises it.
6. **Budget**: compute worst-case fan-out and refuse a plan whose declared
   ceilings cannot cover its declared maxima, unless it declares visible
   truncation.
7. **Emit** the plan with its `PlanId`: the digest of the canonical plan,
   including the definition digest, every bound `ArtifactId` and
   `CalibrationId`, operator versions, limits and the compiler version.

### 7.4 Identities

All identities are digests over canonical bytes; spec `002` fixes the
canonical form and the digest composition.
`DefinitionId`, `PlanId`, `ArtifactId` (model, tokenizer, preprocessing,
truncation policy, precision), `CalibrationId`, `SnapshotId`, `DatasetId`,
`EvaluatorConfigId`, `DecisionId` (a per-execution id, not a digest).

## 8. The five seams

First implementation: Rust traits with explicit composition for trusted,
in-process components, plus a versioned, transport-neutral request/response
protocol in `rustev-contract` for remote adapters. Network implementations of
that protocol (HTTP client and server) live under `integrations/`. No dynamic
loading.

Every seam call receives a `CallContext`: deadline, budget reservation,
cancellation signal, trace id, and an opaque tenant/principal handle supplied
by the host and passed through for authorization and cache isolation only.

### 8.1 Context source

```rust
trait ContextSource {
    fn descriptor(&self) -> &SourceDescriptor; // id, version, fields, freshness semantics
    fn fetch(&self, req: &ContextRequest, cx: &CallContext)
        -> BoxFuture<'_, Result<ContextSnapshot, SourceError>>;
    fn invalidations(&self) -> Option<InvalidationStream>; // correction, revocation, erasure
}
```

Contract: returns only what the caller is authorized to read; every value
carries provenance and an as-of time; a missing field is reported as missing,
never filled. The invalidation stream is how correction and erasure reach
Rustev's caches.

### 8.2 Decision backend

```rust
trait DecisionBackend {
    fn capabilities(&self) -> &BackendCapabilities;
    fn infer(&self, batch: InferenceBatch, cx: &CallContext)
        -> BoxFuture<'_, Result<InferenceOutput, BackendError>>;
}

struct BackendCapabilities {
    artifact: ArtifactId,
    operations: Vec<OperationSupport>, // op, max options/levels/candidates, output kind
    input_limits: InputLimits,         // tokens or bytes, truncation: Refuse | Truncate{side}
    cancellation: Cancellation,        // Cooperative | LocalOnly { remote_work_continues, charges_continue }
    determinism: Determinism,          // Bitwise | Tolerance{..} | Unspecified
    shared_state_encoding: bool,
    cost: CostModel,
}
```

Output kinds are `Label`, `Scores{scope}`, `Distribution`, `Logits`. The
core normalizes logits; it never invents mass for a `Label` backend.

### 8.3 Evaluator

```rust
trait Evaluator {
    fn descriptor(&self) -> &EvaluatorDescriptor; // task, metrics, tolerances, config id
    fn evaluate(&self, dataset: &Dataset, runs: &RunSet) -> EvalReport;
}
```

A task adapter, not a universal metric. Aicortex contributes retrieval
evaluators; packages contribute theirs.

### 8.4 Evidence sink

```rust
trait EvidenceSink {
    fn policy(&self) -> &SinkPolicy; // durability, on_failure, retention, snapshot mode
    fn record(&self, rec: EvidenceRecord, cx: &CallContext)
        -> BoxFuture<'_, Result<Receipt, SinkError>>;
}
```

`on_failure` is one of `FailDecision`, `Backpressure{max_pending}`,
`DropCounted`. A dropped record is counted and the count is itself reported;
loss is never silent.

### 8.5 Action executor

Not part of increment 1, and never called by Rustev core. If an optional
helper is ever supplied, its interface is generic over an authorization type
the application owns, which Rustev never constructs:

```rust
trait ActionExecutor<A, Auth> {
    fn execute(&self, action: A, authorization: Auth, key: IdempotencyKey,
               cx: &CallContext) -> BoxFuture<'_, Result<ExecutionRecord, ExecError>>;
}
```

Validation, idempotency and outcome recording are the executor's obligations.

## 9. Runtime

`rustev-runtime` executes a compiled plan under bounds:

- **Deadline propagation**: one deadline per decision, subdivided per step by
  declared share; a step that cannot start before its share expires yields
  `Unresolved::DeadlineExceeded` without dispatch.
- **Admission and concurrency**: bounded queues per backend; admission
  refusal is a typed result, not a timeout.
- **Budget reservation before dispatch**: tokens, bytes and cost are reserved
  against the decision's ceiling and settled after; a reservation that fails
  is `BudgetExhausted`.
- **Retries and fallback**: bounded, only where the step declares them, and
  recorded. A fallback changes the evidence record's `PlanId` path, never
  silently.
- **Batching and duplicate suppression**: independent semantic steps sharing
  a backend and projection are batched; concurrent identical requests share
  one in-flight computation.
- **Caches**: keyed by `ArtifactId` plus the canonical projected input plus
  tenant; byte-budgeted with eviction; purged by source invalidations.
- **Truncation and partial results**: visible in the judgment and the
  evidence, with the count of candidates or tokens not evaluated.
- **Cancellation honesty**: the evidence record states when a cancelled
  remote call may still have run or been charged, as the backend declared.

Owner decision R-03: contract and core are executor-independent; the first
runtime uses Tokio, behind explicit time, cancellation and scheduling
boundaries where useful. No general executor abstraction is built and no
other executor is promised. The core has no clock at all; time is an input.

## 10. Evidence and replay

An `EvidenceRecord` carries: `DecisionId`, `PlanId`, `DefinitionId`, each
`SnapshotId` with per-field provenance digests, each step's value kind,
value, backend `ArtifactId`, cache hit, retries, fallback taken, truncation,
latency and cost, the judgment, and the sink policy in force.

Snapshot retention is chosen per plan: `DigestOnly` (default), `Retained{ttl}`,
or `External{ref}` (the context source can reproduce the snapshot by id).
Replay re-runs a plan, or a candidate plan, over recorded snapshots and
reports each case as `comparable`, or `incomparable` with a reason:
`inputs-erased`, `inputs-expired`, `artifact-unavailable`,
`nondeterministic-backend`.

Candidate for the hash-linked evidence log in the Rahi-hosted option:
`attest-ledger` or Rahi's ledger, behind the evidence sink seam.

## 11. Evaluation

`rustev-eval` is the shared runner; evaluators are task adapters.

- Dataset and artifact identity on every result.
- Baseline and candidate replay over the same cases.
- Per-case and per-subgroup comparison.
- Calibration (log loss, Brier, reliability by task and subgroup) and
  abstention (coverage against error rate among accepted decisions, the
  central product measure).
- Latency (cold and warm, p50, p95, p99), memory and cost.
- Regression reports against declared tolerances.
- Explicit `incomplete` and `incomparable` results; an absent measurement is
  `unknown`, never a pass.

The report schema lives in `rustev-contract::eval`, dependency-light, so
statecraft and Aicortex can read it without building the engine. Extraction
of the runner into its own repository waits for a second consumer that needs
it independent of decision plans.

Datasets are split into training, model selection, calibration and final
test, and a report names which split it used. A split used to fit a head or a
calibration map is not a holdout afterwards.

## 12. Knowledge and the learning cycle

Rustev consumes a bounded snapshot of Aicortex claims through a context
source adapter and may **propose** observations through Aicortex's write
rules. It never writes knowledge directly.

The cycle:

1. A decision records which evidence it used.
2. An executor records what actually happened.
3. An outcome adapter creates an attributed observation (actor, source,
   trust class, the exposure that preceded it).
4. Evaluation determines whether the new evidence supports a changed model,
   plan, calibration or policy.
5. A reviewed update, accepted through statecraft, changes future behavior.

Rules:

- Every Rustev value carries a derivation class, `exact-derived`,
  `model-derived` or `mixed-derived`, and its input lineage. Not every result
  is model-derived: an exact count over system-of-record events is
  exact-derived. A decision is evidence that the system produced it, never
  independent evidence of the preference, fact or claim it was computed from.
- Correction, revocation and erasure arrive through the context source's
  invalidation stream and purge caches keyed on the affected snapshot fields;
  derived artifacts (calibration maps, evaluation datasets) record their
  source snapshot ids so they can be found and rebuilt.
- Erasure across independent consumers cannot be achieved by central deletion
  alone. Each consumer's obligation is stated at its seam.

## 13. Security posture

The enforceable boundary, carried from `005`:

- State cannot alter the registered plan. Plans are compiled before any state
  is read and identified by digest.
- Model output cannot create tools, permissions or operations; it is a value
  of a declared kind.
- Model output is validated before selection policy reads it.
- Authorization uses trusted application facts and never a judgment (5.2).
- Missing or invalid evidence produces `Unresolved`, never a default.
- Input bounds are separate controls: transport byte limits (owned by the
  transport), bounded parsing of supplied bytes before typed construction,
  aggregate token and work budgets, fallible validated constructors. A check
  after deserialization cannot undo an allocation already made, so the bounds
  that matter are enforced before it.

Not claimed: resistance of any semantic backend to adversarial content.
Injection heuristics may contribute a signal step; they establish nothing.

## 14. Determinism promises

1. **Byte determinism**, scoped: canonical compilation (definition and plan
   documents and their identities) and explicitly defined exact computation
   (spec `002` lists which operations qualify) produce identical bytes for
   identical inputs. Selection over identical validated inputs, `PlanId` and
   step values produces an identical judgment. Runtime observations (latency,
   cost, scheduling) are recorded, never promised to repeat.
2. **Numerical repeatability**: per backend, measured tolerances for a pinned
   artifact and configuration, declared in its capabilities and checked by
   `rustev-eval`.
3. **Bitwise identity across platforms**: not promised. A backend that needs
   it declares `Determinism::Bitwise` and must prove it.

## 15. Codebase layout

```
rustev/
  Cargo.toml                       workspace: crates/*, backends/*, packages/*, integrations/*
  crates/
    rustev-contract/               versioned serde types: definitions, plans, judgments, evidence,
                                   eval reports, remote seam protocol. Deps: serde, canonical JSON.
    rustev-core/                   validation, value semantics, identities, operator registry and
                                   exact operators, compiler, selection policy evaluation, output
                                   math, seam traits. No I/O, no clock, no async executor,
                                   forbid(unsafe_code).
    rustev-runtime/                plan execution: scheduling, deadlines, budgets, batching, caches,
                                   retries, fallback, trace capture, replay driver.
    rustev-eval/                   datasets, replay comparison, metrics, calibration fitting,
                                   regression reports.
    rustev-cli/                    `rustev`: plan check|show|compile, run, replay, eval, calibrate.
    rustev-act/                    optional, not before evidence of need: ActionExecutor<A, Auth>
                                   generic over an application-owned authorization type.
  backends/
    rustev-backend-rules/          deterministic backend: rules, lookup tables, linear heads over
                                   exact features. Reference and test double.
    rustev-backend-<semantic>/     first semantic backend (R-01).
                                   (the remote protocol's messages are in rustev-contract)
  packages/
    rustev-pkg-support-routing/    reference plan 16.1: definition, golden document, evaluator.
    rustev-pkg-lodging/            reference plan 16.2.
  integrations/                    increment 4
    rustev-aicortex/               context source over Aicortex claims; observation proposer.
    rustev-rahi/                   hosting and evidence sink on Rahi.
    rustev-serve/                  optional HTTP surface.
    rustev-remote-http/            HTTP client and server for the remote adapter protocol.
  tools/
    rustev-boundaries/             workspace dependency-rule check (spec 001).
  fixtures/                        synthetic datasets, golden definitions and plans, replay cases.
```

Dependency rules, enforced by a workspace test over `cargo metadata`:

- `contract` <- `core` <- `runtime` <- `eval`, `cli`.
- `backends/*` depend on `core` and `contract`, never on `runtime`.
- `packages/*` depend on `core` and `contract`; `runtime`, backends and
  `eval` only as dev-dependencies.
- Only `integrations/*` may depend on Aicortex, Rahi, statecraft-cli or HTTP
  stacks, and no crate depends on an integration.
- `rustev-contract` and `rustev-core` have no async runtime executor, HTTP or
  ecosystem dependency, even transitively; `rustev-core` has no dependency
  that performs I/O or reads the clock.

One crate has one owning spec, the convention statecraft-cli uses.

## 16. Reference decision plans

The two plans are chosen to contrast: one classifies and routes, the other
filters and ranks a candidate fan-out. A new domain must need a package, not
a change to the core.

### 16.1 Support routing (`rustev-pkg-support-routing`)

| Input | Provenance required | Max age |
|---|---|---|
| `ticket.message` | `user-supplied` (untrusted content) | none |
| `account.tier` | `authenticated-app-field` | 1 day |
| `payments.events[]` | `system-of-record` | 5 minutes |

| Step | Kind | Implementation |
|---|---|---|
| `failed_payments_30d` | `Exact<u32>` | count events with status `failed` in window |
| `hours_since_first_failure` | `Exact<Option<u32>>` | timestamp arithmetic over the same events |
| `topic` | `classify` over {billing, integration_defect, account_access, other}; requires `Distribution` | semantic backend |
| `frustration` | `rubric` over {calm, frustrated, very_angry}; requires `OrdinalLevel` with distribution | semantic backend |
| `explicit_deadline` | `proposition` "the customer states a time limit"; requires `Distribution` | semantic backend |

Selection policy (deterministic table):

- `topic` unresolved, or its top calibrated probability below the task
  threshold: `Escalate(ambiguous-topic)`.
- `billing` and (`failed_payments_30d >= 2` or tier is enterprise):
  `billing-priority`.
- `billing`: `billing`. `integration_defect`: `engineering`.
  `account_access`: `identity`. `other`: `general`.
- `frustration` expectation at or above level 1.5 or `explicit_deadline`
  above threshold raises priority by one step; never changes the queue.

If the bound backend offers only `Scores`, the compiler refuses the threshold
rule unless the definition declares `uncalibrated_threshold` with a margin
rule, which then appears on every judgment.

Output: `Proposal<RouteTicket{queue, priority}>` with contributing facts.
The helpdesk's authority policy decides whether this service account may
assign to that queue; the executor performs the assignment idempotently.
Outcome: final resolving queue and reassignment count.

Evaluation: macro-F1 per queue, coverage versus error rate among
automatically routed tickets, reassignment rate, subgroup by tier and
language.

### 16.2 Lodging recommendation (`rustev-pkg-lodging`)

| Input | Provenance required | Max age |
|---|---|---|
| `trip.request` (dates, party, budget, free text) | `user-supplied` | none |
| `traveler.claims[]` (preferences, constraints, with trust class and validity) | from a context source; Aicortex claims in increment 4 | per claim validity |
| `inventory.candidates[]` (availability, price, attributes, description) | `third-party` | 15 minutes for price and availability |
| `fx.rates` | `system-of-record` | 1 hour |

| Step | Kind | Implementation |
|---|---|---|
| `missing_trip_fields` | `Exact<Vec<Field>>` | dates, party size, budget present and well-formed |
| `eligible` | `Exact<Vec<CandidateId>>` with per-exclusion reasons | availability, price after FX within budget, occupancy, hard accessibility constraints |
| `trip_intent` | `classify` over a closed intent set; requires `Distribution` | semantic backend |
| `supports_preference[c, p]` | `proposition` per eligible candidate and active preference; fan-out bounded to K candidates by a cheap exact pre-rank | semantic backend |
| `suitability[c]` | `rubric` over a 4-level suitability rubric | semantic backend |
| `ranking` | `RankPosition` | deterministic aggregation: preference support weighted by claim trust class and recency, suitability expectation, price position |

Selection policy: any `missing_trip_fields` yields
`Unresolved::MissingEvidence{fields}` (the application asks the traveler);
an empty `eligible` set yields a judgment naming each exclusion reason
count; otherwise a ranked list with per-candidate contributing facts, the
excluded candidates and why, and a truncation notice if more than K were
eligible.

The ranking score is a `RankPosition`, not a probability of preference.
Nothing is booked by Rustev. A booking is an effect authorized by the
traveler's own action.

Outcome and learning: a booking or a dismissal becomes an observation "the
traveler booked or dismissed X after being shown this ranked list", with the
exposure recorded. It is never admitted as "the traveler prefers attribute
Y"; that inference, if made, is a separate, reviewable claim with its own
provenance.

Evaluation: ranking metrics against held-out choices with an explicit
exposure-bias caveat, eligibility precision (exact, expected 100% against
fixtures), abstention correctness on incomplete requests, per-preference
proposition calibration where labels exist.

### 16.3 What the pair demonstrates

| Property | Support routing | Lodging |
|---|---|---|
| Classification plus deterministic selection | yes | intent only |
| Candidate fan-out with a bounded semantic budget | no | yes |
| Abstention by missing evidence | stale payments | missing trip fields |
| Abstention by uncertainty | ambiguous topic | not used for selection |
| Knowledge with trust classes as input | no | yes |
| A backend swap measured without changing authority | yes | yes |

## 17. Increments and evidence gates

| Increment | Delivers | Gate to leave it |
|---|---|---|
| 1. Contract and pure core | `rustev-contract`, `rustev-core`: value kinds, identities, validation, operator registry, compiler with capability matching, selection policy, unresolved outcomes | Both reference definitions compile; each refusal class in section 7.3 has a failing case; property tests for determinism promise 1 |
| 2. Runtime and evaluation | `rustev-runtime`, `rustev-eval`, `rustev-backend-rules`, one semantic backend, `rustev-cli` | Traces and replay reproduce judgments; a baseline report exists for each reference task, labeled synthetic or real; failure behavior (deadline, budget, backend error, sink failure) covered |
| 3. Two domain packages | `rustev-pkg-support-routing`, `rustev-pkg-lodging`, second semantic backend or remote adapter (network side under `integrations/`) | No core change was needed for the second package; a backend swap produces a comparable report and no change in any authority path; partial adoption builds (3.3) pass |
| 4. Ecosystem integrations | `rustev-aicortex`, `rustev-rahi`, `rustev-serve`, spec-spine contract references, statecraft acceptance of a plan change | Standalone library build still passes with no integration crate |

Behind later evidence gates, by name: custom model training, distributed
execution, dynamic plugin loading, sandboxed extensions, arbitrary-question
intelligence, a Jev-compatible HTTP surface.

### 17.1 Spec roadmap

Ordinals are build order. Only `001` and `002` are filed now, as drafts.

| Spec | Subject | Increment |
|---|---|---|
| 001 | Ecosystem boundaries, judgment versus authority, product principles | all |
| 002 | Decision contract and pure core (`rustev-contract`, `rustev-core`) | 1 |
| 003 | Runtime execution and evidence emission | 2 |
| 004 | Evaluation and replay | 2 |
| 005 | Reference backends | 2 |
| 006 | CLI surface | 2 |
| 007 | Reference package: support routing | 3 |
| 008 | Reference package: lodging recommendation | 3 |
| 009 | Remote adapter protocol | 3 |
| 010 | Integrations | 4 |

## 18. Owner decisions

The six questions this section raised were decided by the owner on
2026-09-23. The decisions, with their limits, are recorded in
`docs/decisions/00-founding-decisions.md` (R-01 to R-06): frozen embeddings
plus a linear head as the first semantic-backend candidate, with `ort` subject
to a bounded check; a typed builder and canonical JSON entering one validated
representation; Tokio for the first runtime with an executor-independent
core; synthetic fixtures for infrastructure and real, independently labeled
data before quality claims; `classify`, `proposition`, `rubric`, `rank`; and
the evaluation report envelope in `rustev-contract`.

## 19. Deferred, by name

Custom model training and distillation pipelines. Distributed execution.
Dynamic plugin loading, sandboxed extension runtime, plugin marketplace.
Dynamic arbitrary questions beyond a narrow, separately evaluated class.
Replicated storage. A Jev wire-compatible API. Publication to crates.io.

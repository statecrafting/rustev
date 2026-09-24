---
id: "004-evaluation-and-replay"
title: "Evaluation and replay"
status: approved
implementation: in-progress
created: "2026-09-23"
summary: >
  Increment 2: offline reproduction from bounded, host-retained replay
  bundles; configurable tenant-only or principal-scoped equivalence for candidate
  comparison; explicit availability, coverage and abstention; named dataset
  splits and temperature fitting. Amends 002 with replay and request
  documents and typed loading diagnostics, and 003 with opt-in bounded
  capture of supplied values. Live re-execution is deferred. Synthetic
  fixtures establish mechanics only, never empirical quality.
establishes:
  - { kind: directory, path: "crates/rustev-eval/" }
amends:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
extends:
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-contract/" }, nature: amending }
  - { spec: "002-decision-contract-and-pure-core", unit: { kind: directory, path: "crates/rustev-core/" }, nature: amending }
  - { spec: "003-runtime-execution-and-evidence", unit: { kind: directory, path: "crates/rustev-runtime/" }, nature: amending }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.toml" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Cargo.lock" }, nature: additive }
  - { spec: "001-boundaries-and-authority", unit: { kind: file, path: "Makefile" }, nature: additive }
depends_on:
  - "002-decision-contract-and-pure-core"
  - "003-runtime-execution-and-evidence"
  - "005-reference-backends"
---

# 004: Evaluation and replay

Approved work order, in progress: the implementation record states what is
delivered. The owner delegated the four replay
choices and approved forward progress on 2026-09-23; R-19 to R-23 and A-05
record the scope and the agent's selected defaults. The owner then requested
both isolation modes as a configurable choice (R-24), incorporated in this
same approval change before implementation. This amendment is
reviewed and delivered separately before implementation (R-16). The text of
approved specs 002 and 003 remains their historical contract.

## 1. Purpose

Make reproduction a checked statement about available retained inputs.
Compare candidate decisions only where evidence permits it. Report coverage,
abstention and missing measurements explicitly. A digest proves integrity
relative to expected bytes, never truth, authorization or producer identity.

## 2. Territory

`crates/rustev-eval/` owns replay execution, datasets, metrics, fitting and
regression gates. Normal dependencies are contract and core only, with no
runtime, backend, HTTP stack, clock or filesystem access. Host resolution is
a synchronous, fallible byte-provider interface supplied by the caller;
the library itself performs no storage or network operation. Tests may use
runtime and the rules backend. The amendment edges above authorize only the
contract, core and runtime additions in section 5 and workspace wiring.

## 3. Behavior

### 3.1 Storage, scope and retention

1. The host owns storage, access checks, expiry, erasure and transport
   buffering. Rustev validates declarations and availability at a caller-
   supplied `now_ms`. It neither schedules deletion nor claims deletion
   occurred. A bundle is evidence of what the host retained, not an
   attestation of a producer or an authorization token.
2. Default capture is off and default content retention is `digest_only`.
   A newly assembled bundle defaults to a seven-day metadata lifetime.
   Keeping bytes, including via an external reference, requires an explicit
   retention choice. Every mode has an expiry; the maximum bundle lifetime
   is 30 days from `created_at_ms`, and the host may enforce a shorter cap.
   Each item's expiry is no later than the bundle's. An expiry must be
   strictly after creation; arithmetic overflow or exceeding the cap is
   refused. These are engineering defaults, not legal retention advice.
   Extending the hard cap requires an amendment. Copying a bundle does not
   reset creation or expiry.
3. Isolation is a mutually exclusive, host-configured choice per capture
   and bundle, represented by a tagged scope value:
   - `tenant_only{tenant, context_revision}` permits reuse across principals
     within that tenant and revision;
   - `principal{tenant, context_revision, principal_scope}` additionally
     requires the same effective principal scope. This is the configuration
     default; a missing principal handle is an error, never an automatic
     downgrade to tenant-only.
   Every handle/revision is non-empty, opaque, non-secret and at most 256
   bytes. Serialized documents always state the mode and all its fields;
   omitted modes, unknown modes or fields from both variants are refused.
   The host increments/rotates context revision when shared visibility or
   output-affecting authorization context changes, and rotates principal
   scope when principal-specific permissions change. Raw credentials never
   belong in a bundle.
4. Tenant-only is an explicit host assertion that the reusable computation
   is independent of principal-specific permissions and hidden backend
   context, and that the host authorizes each reader to access the retained
   snapshot and outputs. It does not make the bundle tenant-public. If that
   assertion does not hold, the host must select principal scope or disable
   reuse. Rustev cannot establish principal independence from a digest.
   The mode scopes reuse, not the authorization context passed to live
   backends: selecting tenant-only never strips `principal_handle` from
   `CallContext` or grants access.
5. Replay requires the caller's trusted scope configuration to equal the
   bundle's scope before resolving any item. Comparison includes the mode,
   tenant and context revision, plus principal scope in principal mode.
   Any difference yields `incomparable{scope-mismatch}`. Tenant-only never
   crosses tenants. A principal-scoped capture cannot be relabeled or
   downgraded to tenant-only, nor reused in the other direction. Selecting
   a new mode requires a new capture; no automatic migration is provided.
   No empty handle or omitted scope implies a public/shared namespace.
6. Retention modes are `retained{bytes, digest, expires_at_ms}`,
   `external{reference, digest, expires_at_ms}` and
   `digest_only{digest, expires_at_ms}`. Embedded bytes use a JSON UTF-8
   string containing the canonical document. External references are opaque
   strings, not instructions to fetch a URL; the host resolves them.
   Every item states its expected document schema and identity where one
   exists. Digests use the existing schema-tagged canonical digest rule,
   over the record canonical form of 5.6 for the documents it names.
   Retained bytes, embedded or external, must be exactly that form: bytes
   whose digest matches but which are not in that form are mismatched.
7. Availability is exactly `available`, `missing`, `expired`, `erased`,
   `corrupt`, `inaccessible` or `mismatched`. At `now_ms >= expires_at_ms`,
   report expired without resolving bytes. For an unexpired external item,
   the resolver returns bounded bytes, missing, erased or inaccessible;
   panics/errors must not become missing or available. Digest-only is
   missing. A digest mismatch is corrupt; intact bytes with a wrong schema,
   identity, cross-reference or semantic role are mismatched. Invalid JSON
   is corrupt. Only available items can contribute to reproduction.
   When expiry and erasure overlap, expiry takes precedence at replay; an
   erasure reported before expiry remains a distinct outcome.
8. Embedded bytes may themselves outlive erasure in a copied bundle. The
   host must remove every retained copy, including backups, or use external
   storage that enforces erasure. Rustev cannot discover removed consent
   from bytes alone. This limit is stated wherever replayability is claimed.

### 3.2 Replay bundle, `rustev.replay/1`

One bundle describes one admitted decision and includes:

- schema, decision id, scope, creation and expiry times, evaluation time;
- `PlanId` and retained plan document; all descriptors and calibration
  artifacts needed to recompile that plan, with their identities and
  retention items (the compiler currently needs these separately);
- `SnapshotId` and the snapshot retention item;
- the delivered `rustev.run/1` record as a retention item, including its
  digest. Its core evidence contains the expected judgment. A digest alone
  is insufficient to obtain the expected judgment or historical costs;
- capture status: complete, disabled, limit-exceeded, cancelled or
  abandoned. Only complete can be reproduced;
- one entry per value successfully supplied to the core: step, instance,
  `RequestId`, supply sequence number, optional producing
  attempt id, target index, and a retention item holding the
  output document or a `rustev.runtime-reason/1` document containing
  `schema` and `reason` (one of the four existing runtime reasons). The
  reason document uses `RECORD_V1`; no other unresolved kind is accepted.

The entry records the `RequestId`, not the request document: the document
contains the projection, which is snapshot content that `digest_only`
retention must not embed. Replay recomputes the document from the retained
plan and snapshot (3.3.3) and compares identities. The target index is
always stated. For an output it is the producing target. For a failure it
is the target the request was on when it stopped: the `to` of its last
`fallback` transition, else the primary (0), including when nothing was
dispatched there. The identity is computed for that target. A failure's
producing attempt is its last attempt only when no `retry` or `fallback`
transition has an `after` equal to that attempt's ordinal; otherwise, and
when nothing was dispatched, it has none.

A no-dispatch budget/deadline failure has no producing attempt. An output
names the actual producing target, including fallback, and its artifact
and descriptor. A reason is a historical runtime observation, not a backend
output. `not_supplied` is never encoded as a supplied failure.

Supply sequence is the actual successful `Evaluation::supply` order,
contiguous from zero. Run-record request order remains the order the core
listed requests; concurrent completion order can differ. Entries are unique
by step and instance. Reproduction checks both orders against their
respective roles, never assumes they are interchangeable.

The bundle has at most `min(plan.max_semantic_requests, 4096)` supplies,
at most 4096 descriptors and at most 4096 calibrations. The `REPLAY_V1`
parse set is 16 MiB total, depth 48, 8 MiB per string, 10,000 members per
collection, 1,000,000 values, 40 bytes per number, fractional numbers
allowed. All nested documents are also checked against their own existing
limits before typed construction. Total resolved bytes, including external
items, are capped at 16 MiB per case. Bounds apply to typed constructors as
well as parsing. Oversized cases are explicitly incomparable, never
truncated into a successful replay. The host bounds external reads before
allocation; the library checks the supplied slices again.

### 3.3 Historical reproduction

1. Validate scope, capture completeness, lifetimes, resource bounds,
   integrity and cross-references before evaluation. Load every required
   document under its own limits. Match decision, plan, snapshot, evaluation
   time, request entries, results and producing attempts to the retained
   run record. Missing, duplicate, extra or inconsistent entries are
   incomparable with a typed reason and item location.
2. Load the plan using retained descriptors and calibrations. A changed
   compiler or exact-operator registry is `compiler-changed`, carrying
   recorded/current identities. A same-compiler plan that does not
   recompile identically is `plan-mismatch`, not compiler-changed.
   Missing dependencies keep their availability reasons, not a generic
   compiler error. There is no automatic recompile-and-accept fallback.
3. Start the pure core with the retained snapshot and recorded evaluation
   time. For every supply, find the pending request, recompute its identity,
   validate the actual target against the plan and attempt record, then
   supply the retained raw output or runtime reason. Fallback raw outputs
   use `supply` after these checks, not `supply_doc`, whose existing artifact
   check expects the primary binding. Finish with the original decision id.
4. `reproduced` means record-canonical (5.6) judgment bytes equal those
   in the retained run record. A valid, complete replay with different
   bytes is `diverged`. An unavailable input or failed precondition is
   `incomparable{reason}`.
   A cancelled/abandoned execution has no expected judgment and is
   incomparable, even if a prefix of its supplied values was captured.
5. Zero backend calls occur. A nondeterministic backend is replayable when
   its actual outputs are retained. Historical latency, queueing, attempts
   and costs are copied as observations; they are never remeasured or
   inferred from replay duration. Estimated units and unknown liability
   remain distinct from observed units, and logical units are not money.
6. Integrity and equality establish reproduction under this core build and
   platform. No new cross-platform floating-point guarantee is made. A
   numerical difference remains divergence, never silently accepted under
   a tolerance intended for live inference.

### 3.4 Request identity and candidate comparison

1. `rustev.request/1` has a `RequestId` using the existing tagged canonical
   digest rule. Its content is the full tagged scope (including mode and
   context revision), actual backend id, artifact,
   descriptor identity, exact canonical projection bytes, bound output kind,
   required value kind and normalization. Input-limit handling is covered
   by the descriptor identity, which commits to `input_limit.max_bytes` and
   `input_limit.on_excess`; it is not guessed from a digest. Projection
   bytes already include operation, task, question, ordered options,
   candidates, instance bindings and projected values. No textual
   normalization or reordered option equivalence is inferred.
2. Core computes this document for a pending request and a validated target
   index. The target must be the primary or an explicit fallback bound by
   that plan. It refuses unknown requests, targets and invalid scope.
   Runtime and eval use this one function. `REQUEST_V1` uses `REPLAY_V1`
   bounds, tightened to 4 MiB total; projection bytes still obey the plan's
   bound. The containing bundle's aggregate bound also applies.
3. Decision id, step id, plan id, timing, retry count and core-applied
   calibration are excluded. They do not identify a backend computation.
   Instance content inside the projection is included. This permits
   calibration/policy comparison while preventing accidental reuse based
   solely on a step name. Scope is an isolation key, never an authority
   grant. Context-sensitive providers must declare every other output-
   affecting configuration in artifact/descriptor identity; hidden provider
   context invalidates reuse eligibility.
4. Candidate comparison first requires a reproduced historical case.
   Compile/load the candidate against explicitly supplied descriptors and
   calibration artifacts, using the same snapshot, time and scope. Only
   retained raw successful outputs are reusable, and only for a candidate's
   primary request with exactly equal request identity. An old fallback
   output may serve a new primary only if that actual producer identity is
   equal. Candidate fallback execution is not simulated offline.
5. Historical runtime failures are never reused to predict a candidate's
   failure. A candidate request without an equivalent successful output
   makes the case incomparable with `request-mismatch` or
   `historical-runtime-failure`, even if the candidate policy could handle
   an invented absence. No substitute failure is supplied. A candidate that
   legitimately issues no such request is unaffected.
6. If several retained entries share one request identity, they are usable
   only if their outputs agree: equal record-canonical bytes of the
   producing artifact and raw output, never of the whole output document,
   which also names step and instance. Otherwise the candidate case
   is incomparable with `ambiguous-output`; choosing an arbitrary output
   from a nondeterministic backend is forbidden. Candidate use rebinds only
   the destination step/instance, then the core validates the raw output.
7. Candidate outcome change is reported separately from historical
   divergence. A changed `PlanId` alone does not prevent comparison, and
   equality of entire judgments (which include plan identity) is not the
   candidate agreement metric. The task adapter compares explicitly named
   proposal payloads or outcome categories; this definition is recorded in
   the evaluator configuration.

### 3.5 Datasets, metrics and regression gates

1. The eval crate defines a bounded `rustev.dataset/1` manifest using
   `REPLAY_V1`, with a content-derived `DatasetId`, provenance (R-04),
   unique case ids, source/snapshot identities, labels or explicit missing
   labels, subgroup membership and disjoint training, model-selection,
   calibration and final-test membership. Repeated source/snapshot identity
   across splits is refused. Label shape is checked by a named task adapter.
   Semantic duplicates the manifest cannot identify remain a stated limit.
2. Reports use the existing `rustev.eval-report/1` envelope (R-06).
   Synthetic provenance always prevents empirical quality claims. Real
   provenance requires source, license, labeling method and limitations;
   a declared real dataset is not independently verified by Rustev.
3. A versioned `rustev.evaluator-config/1` document in eval, also under
   `REPLAY_V1`, identifies task adapter/version, label interpretation,
   metric formulas, subgroup/bin boundaries, exclusions and gate thresholds.
   Its canonical digest is the report's `EvaluatorConfigId`. Reports are
   accompanied by this document and a bounded `rustev.eval-detail/1`
   document in eval, under the same limits, binding report digest, dataset,
   split and configuration, with numerator, denominator, exclusion counts
   and per-case scope, outcome and reason. A standalone envelope never proves these
   denominators or definitions. All counts reconcile to the named cohort.
4. Coverage is comparable cases / all cases, reported first. Historical
   comparison requires reproduction; divergence is reported and excluded
   from candidate quality. Acceptance coverage is proposals / comparable
   cases. Escalations, missing-evidence and unresolved outcomes are
   abstentions in that denominator. Incomparable cases are excluded from
   quality and counted by reason. Cancellation never becomes agreement.
5. Error among accepted is wrong labeled proposals / labeled proposals,
   alongside acceptance coverage and labeled-proposal coverage. Unlabeled
   proposals are counted and excluded, never treated as correct. Zero
   denominators yield unknown. Task adapters explicitly define correctness.
6. On labeled, comparable probability vectors: mean log loss is
   `-sum(ln(p[label])) / n` (a zero true-class probability yields an explicit
   infinite-loss diagnostic and unknown finite metric, never clipping);
   multiclass Brier is `sum_case sum_class (p-y)^2 / n`. Scores, labels,
   ranks and unresolved values never silently become probabilities.
   Reliability reports bin counts, mean confidence and empirical accuracy
   for top-label confidence, by declared subgroup. Bins are half-open except
   the final bin includes 1. Empty bins are unknown. Use the core's same
   validation/calibration math; duplicating a different softmax is forbidden.
7. Latency uses retained run records, with queueing separate and a named
   cohort. Quantiles use nearest rank (`ceil(q*n)`, one-based). Observed,
   estimated and unknown cost are separate series, with no mixed total
   presented as observed spend. Unit comparability must be declared by the
   evaluator; different backend units cannot be added without it.
8. A gate states metric, direction, absolute tolerance, minimum comparable
   and labeled coverage, and baseline/candidate dataset, split and config.
   Missing, nonfinite or incompatible measurements yield unknown, never
   pass; below-minimum coverage fails the coverage gate. Gates never grant
   authority to an executor.

### 3.6 Calibration fitting

1. Fit `temperature/1` on the calibration split only, against valid labeled
   logits or distributions using the core's defined transformation. Refuse
   empty data, nonfinite inputs, zero support for a true label that no
   temperature can repair, and inconsistent artifact/task/question/options.
2. The caller supplies a non-empty, strictly increasing list of at most
   4096 candidate Decimal temperatures in `(0, 1000]`. Evaluate mean log
   loss at each; choose the lowest loss, ties choosing the smaller
   temperature. This bounded grid search claims only the best evaluated
   candidate, not a globally optimal fit. The grid and all fit counts are
   recorded in the evaluator configuration and fit record.
3. Emit the existing `rustev.calibration/1` artifact and an eval-owned
   `rustev.calibration-fit/1` companion document under `REPLAY_V1`, binding
   artifact id, dataset id, split, source/case identities, configuration and
   fit result. The existing calibration schema has no split field; never
   pretend its dataset id alone proves disjoint evaluation.
4. Qualification requires the companion record and a different, disjoint
   split with no fit-source overlap. Missing lineage yields unknown;
   overlap or reuse of the fitting split is refused. Applying an artifact
   in core is still possible under the existing contract, but does not by
   itself establish calibration quality. Synthetic fitting tests establish
   mechanics only.

## 4. Out of scope

Live re-execution is deferred to a future spec, including bitwise/tolerance
backend comparison and new runtime observations. This work adds no live
inference command, model dependency, training, paid service, dataset
acquisition, persistent store, deletion scheduler, authorization service or
new run-record retention obligation. CLI and semantic-backend specs 006 and
011 remain drafts. No release, publication, deployment, branch-protection
change, waiver, sibling-repository edit or background monitor is authorized
by this work order.

## 5. Amendments and compatibility

1. **Spec 002, contract:** add the replay/request/runtime-reason documents, typed
   retention/availability vocabulary and request identity above. Existing
   documents and their canonical bytes stay unchanged; no existing schema
   gains a field silently. Eval-owned dataset/detail/config/fit documents
   compose the existing report and calibration envelopes.
2. **Spec 002, core:** add the pending-request identity function in 3.4 and
   an additive typed loading diagnostic distinguishing compiler/registry
   mismatch, missing binding dependencies and invalid/recompiled plan
   mismatch. Keep `Compiled::load` behavior and refusal API compatible;
   the new entry point must share its actual recompile/equality logic.
   Neither an error string match nor a version difference inferred from
   arbitrary corruption is an adequate diagnostic.
3. **Spec 003, runtime:** add an opt-in `decide_with_capture` path returning
   the normal decide result plus a separate capture outcome, also when sink
   delivery fails. A rejected decision returns `not-admitted` capture with
   no entries; it cannot become a replay bundle. Existing `decide`,
   `Decided`, `DecisionRequest` and `RuntimeConfig` retain their API and
   wire behavior. Capture configuration
   supplies the scope and a positive byte limit no greater than 16 MiB;
   all scope handles are separate from secret-bearing call context. Mode
   selection is explicit as in 3.1; a missing principal never selects the
   tenant-only variant.
   Capture records only successfully supplied values in actual supply order,
   with actual target/attempt references and request identity. It is a
   bounded in-memory handoff, not persistent retention or a callback.
4. Capture refuses invalid configuration before admission. If capture
   exceeds its byte/request cap, discard captured payloads and return
   `limit-exceeded`; decision execution and sink policy still complete
   normally. Never report a partial capture as complete. Cancellation is
   explicit; dropping the future returns nothing and provides no durable
   capture guarantee. Existing accounting and abandonment rules still hold.
   Failed delivery does not become acknowledged because capture succeeded.
   No capture data is sent to the evidence sink by default.
5. Assembly takes caller-retained plan dependencies, snapshot, run record
   and capture, validates all bindings, and applies explicit retention
   choices. It does not obtain them from an output digest. Returning a
   capture is not a promise that a serializable bundle fits the size cap;
   assembly checks escaped document sizes and returns a typed bound error.
6. **Record canonical form (spec 002, 3.3).** `backend-output/1`,
   `run/1`, `judgment/1` and `evidence/1` (inside run records),
   `eval-report/1`, `runtime-reason/1`, `replay/1` and `eval-detail/1`
   hold binary64 numbers, which the existing canonical form refuses. Their
   record canonical bytes are computed from the typed document only (never
   from an untyped JSON value) and equal the existing form except that
   every number the typed document holds as binary64, whole-valued or not,
   is written as follows:
   - digits: the shortest decimal digit string that parses back to the
     same binary64 value; among several, the one nearest the exact value;
     an exact tie takes the larger magnitude (Rust's `core::fmt` shortest
     mode). Rustev owns this writer: `serde_json`'s own float output does
     not conform, since it breaks such ties to even
     (`1658206780088562.25` is `1658206780088562.3` here, `...2.2` there);
   - layout, for decimal exponent `e` of the first significant digit:
     `-5 <= e < 16` is plain notation with at least one fractional digit
     (`3.0`, `0.00001`, `1000000000000000.0`); otherwise scientific,
     `d[.ddd]e+N` or `d[.ddd]e-N`, `N` without leading zeros and the sign
     always written (`1e+16`, `1e-6`, `1.5e-7`); zero is `0.0` or `-0.0`
     by its sign bit;
   - a non-finite value has no form and is refused, never written as
     `null`; the bytes must parse to an equal document whose record
     canonical bytes are the same bytes (a fixpoint).
   Parsing is correctly rounded: the workspace enables `serde_json`'s
   `float_roundtrip`, whose default best-effort parsing does not return
   the written value for a large share of binary64 values. So a retained
   output parses to exactly the value that was supplied. This intentionally
   amends how approved specs 002 (`supply_doc`, document parsing) and 003
   (the runtime) read supplied numbers: a value the best-effort parser
   misread by a unit in the last place now reads as written. Identity-bearing
   documents, including the eval-owned `dataset/1`, `evaluator-config/1`
   and `calibration-fit/1`, carry no binary64 numbers: fractional values
   there are Decimal strings (spec 002, 3.3.1), and their identities use
   the existing form unchanged. A later change of the digits or layout is
   a new form, pinned by known-answer tests at the boundaries above.
7. **Capture accounting (spec 003).** A capture holds, per supplied value,
   its value document and its `RequestId`, not the request document. The
   configured byte limit bounds the sum of the record-canonical lengths of
   the value documents plus the length of each `RequestId` string.
8. Wire schemas `definition/1`, `plan/2`, `run/1`, `backend-output/1`,
   `calibration/1` and `eval-report/1` are unchanged. The existing plan and
   definition goldens must remain byte-identical. Any later need to change
   them is another reviewed amendment before implementation.

## 6. Delivery sequence and acceptance

Deliver this approval and amendment before code. Then implement in bounded
changes: contract/core diagnostics and identities; runtime capture; offline
reproduction and comparison; metrics/fitting and both reference reports.
Each implementation change edits this spec's implementation record, keeps
its lifecycle honest and passes gate, code, declared acceptance and coupling
on the exact reviewed head before merge. Only the final accepted increment
sets `implementation: complete` and adds 004 to `make verify`.

Required executable acceptance before completion:

- Both reference tasks produce synthetic runtime fixtures and reproduce
  byte-identical judgments offline, with a backend-call counter proving no
  inference. Include zero-request, dependent-request, out-of-order completion,
  nondeterministic-output, retry, fallback and pre-dispatch failure cases.
- Every availability reason, exact expiry boundary, invalid lifetime,
  external byte cap, embedded parse bound, wrong scope, changed context or
  permission revision, missing descriptor/calibration, compiler change, same-compiler
  plan corruption, wrong attempt/target, duplicate/extra/missing supply,
  cancellation and absent expected judgment has a failing fixture.
- Isolation matrix: tenant-only permits otherwise identical requests from
  different principals in the same tenant/revision, but refuses another
  tenant or revision. Principal mode additionally refuses another principal
  scope. Both directions of mode mismatch, absent mode/handle, mixed-variant
  fields and attempts to downgrade an existing capture are refused before
  resolution. Tenant-only leaves the live call's principal context unchanged.
- Changing artifact, descriptor, question, options, projection, scope or
  normalization prevents reuse. A policy/calibration-only change can reuse
  raw outputs. Historical failures, conflicting duplicate outputs and a
  fallback mistaken for the primary never qualify as comparable.
- Capture-off preserves the existing API tests. Capture-on preserves the
  judgment and accounting, including sink failure and cancellation. Cap
  exhaustion reports incomplete capture without altering decision results.
- Both baseline reports name synthetic provenance, configuration and all
  denominators. Count reconciliation includes abstentions, missing labels,
  divergence and each incomparable reason. Zero denominators, zero true-
  class probability, wrong kinds, empty bins, mixed cost units and unknown
  gate inputs never pass by default.
- Temperature fitting picks the expected grid winner on a hand-computable
  fixture, emits bound lineage and rejects same-split/source qualification.
- Negative controls seed at least one compiling defect in scope isolation,
  expiry, output equivalence, fallback identity, capture completeness,
  denominator accounting and split leakage. Every defect must be detected.
  The bounded seed harness belongs under `crates/rustev-eval/`.
- Normal dependency trees keep eval independent of runtime/backends and
  keep contract/core free of runtime, I/O and network dependencies.

## Verification

These commands are the future implementation acceptance, not a claim that
an eval crate exists. They must fail until that work is delivered. Do not
add 004 to the aggregate completed-spec acceptance list prematurely.

```verify:cli
cargo test -p rustev-contract --locked
cargo test -p rustev-core --locked
cargo test -p rustev-runtime --locked
cargo test -p rustev-eval --locked
cargo run -p rustev-boundaries --locked --quiet
cargo clippy -p rustev-eval --all-targets --locked -- -D warnings
cargo fmt --all --check
sh crates/rustev-eval/mutation/seeds.sh
```

## Implementation record

- 2026-09-23: approved contract and amendments only. No replay, capture,
  metrics or fitting implementation is claimed. Review against current
  source found and addressed missing plan-loading dependencies, missing
  retained expected judgment, actual fallback identity, successful supply
  order, cancellation, candidate runtime-failure reuse and calibration
  split lineage. These were gaps in the draft, not delivered features.
- 2026-09-23: during review of this approval change, the owner requested
  configurable tenant-only and principal-scope isolation (R-24). Both modes,
  principal-mode default, explicit context revisions, mode-separated
  identities and the acceptance matrix are specified before implementation.
- 2026-09-23: amended before implementation, in a separate reviewed change
  (R-16). Designing the contract against current source showed gaps:
  backend outputs, run records and judgments carry binary64 numbers the
  existing canonical form refuses, so their digests and "canonical
  judgment bytes" were undefined, and `serde_json`'s default parser does
  not return the written value for about 30% of random binary64 values
  (measured: 888,746 of 2,998,528 with the locked 1.0.151; 0 with
  `float_roundtrip`), so a retained output could not be relied on to
  replay exactly (5.6); a supply entry holding the full request document
  would embed projection content under `digest_only` retention (3.2); a
  failed supply's identity target and producing attempt were unstated or
  wrong after a fallback with no dispatch (3.2); and duplicate-output
  agreement compared bytes that include step and instance (3.4.6).
  Capture accounting is made explicit (5.7). An independent review of the
  first draft of this amendment found the writer misnamed (the locked
  `serde_json` formats with `zmij`, which resolves exact digit ties
  differently from Rust's `core::fmt`), identity-bearing eval documents
  uncovered, and the failure-target rule wrong; all are fixed here. No
  code yet.
- 2026-09-23: increment 1 of 4, contract and core (5.1, 5.2, 5.6).
  `rustev-contract` gains `scope` (tagged `tenant_only`/`principal` with
  bounded handles and no default mode), `retention` (items, modes,
  availability, the 7-day default and 30-day cap), `request`
  (`rustev.request/1`, `RequestId`, `REQUEST_V1`), `reason`
  (`rustev.runtime-reason/1`, the four runtime reasons only) and `replay`
  (`rustev.replay/1` with structural and lifetime checks under
  `REPLAY_V1`, and the in-memory `Capture` handoff type), plus
  `ContentDigest`, `Document::record_canonical`/`record_digest` and the
  record canonical form with Rustev's own binary64 writer. The workspace
  enables `serde_json/float_roundtrip`; every existing test passes
  unchanged, and `Cargo.lock` does not change. `rustev-core` gains
  `Evaluation::request_document` (primary or bound fallback target of a
  pending request) and `Compiled::load_checked` with `LoadError`
  (`compiler-changed` with both identities, missing descriptor or
  calibration, `plan-mismatch` with the recompile refusal if any); `load`
  now delegates to the same recompile-and-compare function and keeps its
  refusal. Not yet delivered: runtime capture, replay, comparison,
  datasets, metrics, fitting, reports and the seed harness. An
  independent review found no correctness defect but seeded 15 compiling
  defects of which the tests missed 8 (duplicate keys ignoring instance,
  missing supply and calibration caps, role schemas, a `Some(NaN)`
  fixpoint, handle length in characters, hard-coded normalization); each
  now has a test and all 15 are detected. It also led to bundle checks of
  declared plan and snapshot identities and of supplies under a disabled
  or limit-exceeded capture, and to an exact (not lossy) projection
  string. The plan's own request bound is checked at replay.

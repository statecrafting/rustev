// Rustev's Node-API package: the hand-written API declarations (spec 027,
// 3.3 to 3.9). The document types are generated (`./types`) and identical
// to the WebAssembly package's.

/** A document as UTF-8 canonical JSON bytes, a string, or an object the
 * package serializes with JSON.stringify; the Rust side parses it with the
 * document's bounded parser (3.2.1). */
export type DocumentInput = Uint8Array | string | object;

/** A charge, as the contract encodes it. */
export type Charge = { observed: { units: number } } | { estimated: { units: number } } | 'unknown';
export type RemoteEnd = 'finished' | 'possibly_continuing';

export interface RuntimeConfig {
  maxInFlight: number;
  maxQueued: number;
  maxParallelRequests: number;
  /** The `fail_decision` sink policy's delivery timeout. */
  sinkTimeoutMs: number;
  /** Tokio worker threads, 1 to 64; 2 when omitted. */
  workerThreads?: number;
  /** Tokio blocking threads, 1 to 512; 4 when omitted. */
  blockingThreads?: number;
}

/** A host sink (3.6, 3.7): delivered the run record's canonical bytes. */
export interface EvidenceSink {
  /** 1 to 65,536; there is no default. */
  queueCapacity: number;
  /** Whether it accepts `rustev.run/3` records. */
  acceptsRun3?: boolean;
  deliver(record: Uint8Array, call: { decisionId: string; traceparent?: string }): Promise<string>;
}

/** One attempt as a host backend receives it (3.6.3). */
export interface InferCall {
  attemptId: string;
  projection: Uint8Array;
  remainingMs: number;
  reservedBound: unknown;
  principalHandle: Uint8Array;
  signal: AbortSignal;
  traceparent?: string;
}

export interface InferAnswer {
  output: unknown;
  charge: Charge;
  remote?: RemoteEnd;
}

/** A host backend (3.6.1): declared data for every synchronous trait
 * method, and no batch capability. */
export interface DecisionBackend {
  descriptor: DocumentInput;
  costModel: 'bounded' | 'estimated' | 'unknown';
  costBound: unknown;
  maxConcurrency: number;
  /** 1 to 65,536; there is no default. */
  queueCapacity: number;
  infer(call: InferCall): Promise<InferAnswer>;
}

/** A host context source (3.7): called once before admission when a
 * request carries no snapshot. */
export interface ContextSource {
  /** 1 to 65,536; there is no default. */
  queueCapacity: number;
  fetch(fields: string[], call: { decisionId: string; traceparent?: string }): Promise<DocumentInput>;
}

/** What a host scorer is given with the views (spec 041, 3.9.1). */
export interface ScoreCall {
  /** Absent when `scoreViews` was given no deadline. */
  remainingMs?: number;
  signal: AbortSignal;
  /** The views' record canonical bytes. */
  raw: { caseView: Uint8Array; proposalView: Uint8Array };
}

/** A score inside the scorer's output, or why there is none: 1 to 64
 * bytes of `[a-z0-9_]`, not a reason eval reserves. */
export type ScoreAnswer = { score: number } | { unscored: string };

/** A host scorer (spec 041, 3.9). A throw or rejection is
 * `scorer_failed`, never a score. */
export interface Scorer {
  /** The scorer's `ScorerDescriptor`, as the configuration declares it. */
  descriptor: DocumentInput;
  /** 1 to 65,536 each; there is no default. */
  maxConcurrency: number;
  queueCapacity: number;
  /** `caseView.gold` is null unless the descriptor declares `reads_gold`. */
  score(caseView: unknown, proposalView: unknown, call: ScoreCall): Promise<ScoreAnswer>;
}

/** One acquisition attempt as a host source receives it (spec 063, 3.5). */
export interface AcquireCall {
  decisionId: string;
  operation: string;
  /** `rustev.acquisition-request/1` canonical bytes. */
  request: Uint8Array;
  /** Its `AcquisitionRequestId`. */
  requestId: string;
  /** The idempotency key spec 036 3.14.2 assigns. */
  attemptId: string;
  /** Until the step's acquisition window closes. */
  remainingMs: number;
  principalHandle: Uint8Array;
  /** Aborted at the window's close, on cancellation or at shutdown. */
  signal: AbortSignal;
  traceparent?: string;
}

/** A page (`rustev.acquisition-page/1`, at most 4 MiB) and its charge. */
export interface AcquireAnswer {
  page: DocumentInput;
  charge: Charge;
}

/** A host acquisition source (spec 063, 3.5). `acquire` is an asynchronous
 * host callback (3.4); a thrown `SeamFailure` keeps its class (`denied` is
 * never retried), anything else is `permanent` with an unknown charge. */
export interface AcquisitionSource {
  /** A `rustev.source/1` descriptor declaring `operations`. */
  descriptor: DocumentInput;
  /** 1 to 65,536 each; there is no default. */
  maxConcurrency: number;
  queueCapacity: number;
  acquire(call: AcquireCall): Promise<AcquireAnswer>;
}

/** What every store call carries (spec 063, 3.6). */
export interface StoreCall {
  /** Until the decision's deadline; absent for `plan.invalidate`. */
  remainingMs?: number;
  signal: AbortSignal;
  traceparent?: string;
}

/** A namespace and a scope generation, safe integers. */
export interface Generations {
  namespace: number;
  scope: number;
}

/** A host store behind the persistent tier (spec 063, 3.6), honoring the
 * atomicity contract of 3.7 (G1 to G5). `namespace`, `scope` and `keyId`
 * are Rustev-derived opaque strings the host stores and never computes. */
export interface RuntimeStore {
  /** Nine non-empty terms, recorded as given. */
  contract: {
    schema: string; namespace: string; durability: string; encryption: string;
    atomicity: string; conflict: string; expiry: string; invalidation: string; erasure: string;
  };
  /** 1 to 65,536; there is no default. */
  queueCapacity: number;
  get(call: StoreCall & { namespace: string; scope: string; keyId: string }):
    Promise<{ bytes: Uint8Array | null; generations: Generations }>;
  put(call: StoreCall & { namespace: string; keyId: string; bytes: Uint8Array;
    terms: { scope: string; generations: Generations; maxScopeBytes: number;
             maxNamespaceBytes: number; expiresMs: number } }):
    Promise<'stored' | 'generation_advanced' | 'over_budget' | 'conflict'>;
  delete(call: StoreCall & { namespace: string; keyId: string }): Promise<void>;
  invalidate(call: StoreCall & { namespace: string; selector: 'namespace' | { scope: string } }):
    Promise<{ generation: number; removed: number }>;
}

/** `plan.invalidate`'s result (spec 063, 3.6.4). */
export interface InvalidationResult {
  generation: number;
  removedEntries: number;
  removedBytes: number;
  persistent: null | 'pending' | { generation: number; removed: number };
  complete: boolean;
}

export interface EngineOptions {
  runtime: RuntimeConfig;
  /** Required; there is no default sink. */
  sink: EvidenceSink;
  rulesBackends?: { program: DocumentInput; maxConcurrency: number }[];
  /** Opened local semantic backends (spec 011, 3.17.1). */
  localEmbedBackends?: { backend: LocalEmbedBackend; maxConcurrency: number }[];
  backends?: DecisionBackend[];
  contextSource?: ContextSource;
  clock?: 'system' | 'manual';
  telemetry?: TelemetryOptions;
  /** Spec 063 3.5: installed by descriptor name; never what a plan binds
   * (plans bind the `sources` named on compile). */
  acquisitionSources?: AcquisitionSource[];
  /** Spec 063 3.6: the store behind a plan's persistent tier. */
  store?: RuntimeStore;
}

export interface DecisionRequest {
  decisionId: string;
  /** Omitted only when the engine has a context source. */
  snapshot?: DocumentInput;
  evaluationTimeMs: number;
  deadlineMs?: number;
}

/** Captured content is unredacted personal data. The host owns protection,
 * expiry and erasure of every copy, including embedded bundle copies. */
export interface CaptureOptions {
  scope: import('./types/rustev-documents').RustevReplay1.Scope;
  maxBytes: number;
  createdAtMs: number;
  retain?: 'digest_only' | 'embedded' | 'external';
  lifetimeMs?: number;
  hostCapMs?: number;
}
export interface CaptureResult {
  status: 'complete' | 'limit_exceeded' | 'cancelled' | 'abandoned';
  bundle?: Uint8Array;
  items?: Record<string, Uint8Array>;
  error?: { code: string; detail: unknown };
}
export interface DecideOptions {
  capture?: CaptureOptions;
  /** 049 3.4.2: printable ASCII, at most 512 bytes, with traceparent. */
  tracestate?: string;
  signal?: AbortSignal;
  /** Can only tighten the plan's deadline. */
  deadlineMs?: number;
  /** Opaque bytes passed to the runtime; never interpreted. */
  principalHandle?: Uint8Array;
  /** A W3C Trace Context version 00 header (3.8). */
  traceparent?: string;
}

export interface DecideResult {
  capture?: CaptureResult;
  trace?: { traceId: string; rootSpanId: string };
  completion: 'judged' | 'cancelled';
  /** The parsed judgment; null when cancelled. An unresolved outcome is a
   * value here, never an exception. */
  judgment: unknown | null;
  /** The parsed run record. */
  evidence: unknown;
  delivery: { kind: 'acknowledged'; receipt: string } | { kind: 'queued' } | { kind: 'dropped'; droppedTotal: number };
  /** The contract's canonical bytes: for evidence-grade use. */
  raw: { judgment: Uint8Array | null; evidence: Uint8Array };
}

export declare class PlanHandle {
  readonly planId: string;
  readonly raw: Uint8Array;
  decide(request: DecisionRequest, options?: DecideOptions): Promise<DecideResult>;
  /** Spec 063 3.6.4: per process; `null` without an optimization namespace. */
  invalidate(options?: { principalHandle?: Uint8Array; timeoutMs?: number }): Promise<InvalidationResult | null>;
}

export interface CommandResult {
  /** The CLI's exit code for this outcome. */
  code: number;
  /** The output object's bytes, as the CLI prints them. */
  out: Uint8Array;
  artifacts: Record<string, Uint8Array>;
}

export declare class Engine {
  explain(evidence: ExplainEvidence, query: ExplainQuery, options?: ExplainOptions): Promise<ExplainResult>;
  static create(options: EngineOptions): Promise<Engine>;
  /** Compile a definition, or load a compiled `plan`, and prepare it:
   * exactly one of the two, or `InvalidDocument`. */
  compile(args: {
    definitions?: DocumentInput[];
    execution?: DocumentInput;
    profile?: DocumentInput;
    profileBases?: DocumentInput[];
    variant?: string;
    definitionVersion?: string;
    definition?: DocumentInput;
    plan?: DocumentInput;
    descriptors?: DocumentInput[];
    calibrations?: DocumentInput[];
    /** `rustev.source/1` descriptors the plan binds (spec 063 3.5.2):
     * never taken from the engine's installed sources. */
    sources?: DocumentInput[];
  }): Promise<PlanHandle>;
  /** Like `compile`, on the calling thread (blocking). */
  compileSync(args: {
    definitions?: DocumentInput[];
    execution?: DocumentInput;
    profile?: DocumentInput;
    profileBases?: DocumentInput[];
    variant?: string;
    definitionVersion?: string;
    definition?: DocumentInput;
    plan?: DocumentInput;
    descriptors?: DocumentInput[];
    calibrations?: DocumentInput[];
    /** `rustev.source/1` descriptors the plan binds (spec 063 3.5.2):
     * never taken from the engine's installed sources. */
    sources?: DocumentInput[];
  }): PlanHandle;
  /** A pure command through the shared surface (`check`, `compile`,
   * `show`, `replay`, `eval`, `evalViews`, `gate`, `calibrateFit`,
   * `calibrateQualify`),
   * on the engine's blocking pool. */
  command(name: string, inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  commandSync(name: string, inputs: Record<string, DocumentInput>, args: object): CommandResult;
  profileCheck(inputs: Record<string, DocumentInput>, args: ProfileCheckArgs): CommandResult;
  planDiff(inputs: Record<string, DocumentInput>, args: PlanDiffArgs): Promise<CommandResult>;
  check(inputs: Record<string, DocumentInput>, args: object): CommandResult;
  show(inputs: Record<string, DocumentInput>, args: object): CommandResult;
  replay(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  evaluate(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  gate(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  /** `rustev eval views`: `args.out` names the one artifact written. */
  evalViews(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  /** One host scorer's `rustev.scores/1` over the views (041 3.9). */
  scoreViews(
    views: DocumentInput,
    scorer: Scorer,
    options?: { deadlineMs?: number; signal?: AbortSignal },
  ): Promise<{ scores: unknown; raw: Uint8Array }>;
  calibrateFit(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  calibrateQualify(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  /** Spec 063, 3.9: the repeatability commands. `evaluate` with
   * `repeatConfig` (`EvalRepeatArgs`) is `eval --repeat-config`. */
  evalCandidates(inputs: Record<string, DocumentInput>, args: EvalCandidatesArgs): Promise<CommandResult>;
  baselineRecord(inputs: Record<string, DocumentInput>, args: BaselineRecordArgs): Promise<CommandResult>;
  evalReproducibility(inputs: Record<string, DocumentInput>, args: EvalCompareArgs): Promise<CommandResult>;
  evalDiff(inputs: Record<string, DocumentInput>, args: EvalCompareArgs & { out: string }): Promise<CommandResult>;
  gateSelect(inputs: Record<string, DocumentInput>, args: GateSelectArgs): Promise<CommandResult>;
  evalRepeatImport(inputs: Record<string, DocumentInput>, args: EvalRepeatImportArgs): Promise<CommandResult>;
  onGauges(cb: (gauges: EngineGauges) => void | Promise<void>, options: { everyMs: number }): () => void;
  advanceClock(ms: number): void;
  clockSleepers(): number;
  clockDelays(): number[];
  gauges(): EngineGauges;
  recordExchange(record: DocumentInput): void;
  flushTelemetryMetrics(): Promise<{ outcome: 'exported' | 'not_taken' | 'lost'; reason?: string }>;
  shutdown(options?: { drainMs?: number }): Promise<{ drained: boolean; cancelled: number; lateDiscarded: number; stillRunning: number; telemetryUnflushed: number; abandonedPending: number }>;
}

export declare class RustevError extends Error {
  readonly code: string;
  readonly detail: unknown;
}
export declare class CompileRefusal extends RustevError {}
export declare class InvalidDocument extends RustevError {}
export declare class ConfigRefused extends RustevError {}
export declare class PlanPrepareRefused extends RustevError {}
export declare class AdmissionRejected extends RustevError {}
export declare class DeadlineExceeded extends RustevError {}
export declare class CancelledBeforeAdmission extends RustevError {}
export declare class SinkFailure extends RustevError {
  capture?: CaptureResult;
  /** The run record: evidence, not a result to act on. */
  readonly record?: Uint8Array;
}
export declare class CaptureConfigRefused extends RustevError {}
export declare class ContextUnavailable extends RustevError {}
export declare class InvalidTraceContext extends RustevError {}
export declare class EngineShutDown extends RustevError {}
/** Reserved: no path throws it today (3.4.1). */
export declare class BudgetExhausted extends RustevError {}
export declare class InternalError extends RustevError {}
export declare class PlatformPackageMissing extends RustevError {}

/** A host seam's typed failure (3.6.5). */
export declare class SeamFailure extends Error {
  readonly failureClass: 'transient' | 'overloaded' | 'permanent' | 'denied' | 'cancelled';
  readonly charge: Charge;
  readonly remote: RemoteEnd;
  readonly cancel?: 'stopped' | 'unconfirmed';
  constructor(args: {
    failureClass: 'transient' | 'overloaded' | 'permanent' | 'denied' | 'cancelled';
    charge: Charge;
    remote: RemoteEnd;
    cancel?: 'stopped' | 'unconfirmed';
    detail?: string;
  });
}

export declare function version(): string;

/** What this build of the addon contains. */
export declare function features(): { localEmbed: boolean };

/** The configuration of spec 011, 3.7.1, as the host provisions it. */
export interface LocalEmbedConfig {
  /** An absolute path to the bundle directory. */
  bundleDir: string;
  /** An absolute path to the ONNX Runtime library; unused by a synthetic bundle. */
  runtimePath?: string;
  /** The bundle's expected `ArtifactId`, `sha256:<hex>`. */
  expectedArtifact: string;
  /** 1 to 16. */
  workers: number;
  /** 0 to 1,024. */
  maxQueued: number;
}

/** The local semantic backend of spec 011 (3.17.1): a Rust backend
 * registered like the rules backend. Its logits are `model-derived` and
 * never authorization. Without the `local-embed` build feature `open`
 * rejects with `ConfigRefused` (`feature_disabled`). */
export declare class LocalEmbedBackend {
  private constructor();
  /** A setup refusal is `ConfigRefused` whose code is the setup code
   * (`digest_mismatch`, `artifact_mismatch`, `unqualified_platform`, ...)
   * and whose detail names the item. */
  static open(config: LocalEmbedConfig): Promise<LocalEmbedBackend>;
  /** The descriptor derived from the bundle's manifest, canonical bytes. */
  readonly descriptor: Uint8Array;
  /** The bundle's `ArtifactId`. */
  readonly artifact: string;
}

/** 049 3.2: Explicit JSONL or allowlisted OTLP/HTTP telemetry. */
export interface TelemetryOptions {
  exporter: { kind: 'jsonl'; path: string; maxBytes?: number }
    | { kind: 'otlp-http'; endpoint: string; allowedEndpoints: string[];
        headers?: Record<string, string>; caPem?: Uint8Array; timeoutMs?: number };
  policy?: DocumentInput;
  key?: { bytes: Uint8Array; keyId: string };
  acknowledgement?: 'accepted' | 'exported';
  requireTelemetry?: boolean;
  telemetryTimeoutMs?: number;
  sampling?: {
    parent?: 'honor' | 'ignore';
    root?: 'always' | 'never' | { ratioPerMillion: number };
    alwaysSampleOn?: ('unresolved' | 'refusal' | 'fallback' | 'cancelled')[];
  };
  logs?: boolean;
  bindings?: {
    providers?: Record<string, string>;
    usageKeys?: Record<string, { input: string; output: string }>;
  };
  limits?: {
    maxQueuedSpans?: number; batchSize?: number; exportTimeoutMs?: number;
    valueBound?: number; maxPendingContexts?: number; contextTtlMs?: number;
    joinBuffer?: number;
  };
}

/** 049 3.5.4: operator counters, never evidence. */
export interface TelemetryFlow {
  accepted: number; exported: number; lost: Record<string, number>; inFlight: number;
}
export interface TelemetryGauges {
  spans: TelemetryFlow; logs: TelemetryFlow; metricExports: TelemetryFlow;
  decisionsLost: Record<string, number>;
  sampledOut: number; invalidTraceparent: number; tableFull: number;
  contextExpired: number; planUnregistered: number; exchangeUnjoined: number;
  attributesCut: number; eventsCut: number;
}
export interface EngineGauges {
  inFlight: number; queued: number; rejected: number; abandoned: number;
  backendInFlight: Record<string, number>;
  sink: { acknowledged: number; failed: number; timedOut: number; dropped: number; queued: number; inDelivery: number };
  seams: { name: string; queueDepth: number; refusedFull: number; lateResolutionsDiscarded: number;
    /** Acquisition sources and the store only (spec 063 3.11). */
    abandonedPending?: number }[];
  telemetry?: TelemetryGauges;
}

/** Retained bytes, parsed by the shared explanation surface. */
export type ExplainEvidence = { record: Uint8Array; plan?: Uint8Array } | { bundle: Uint8Array };
export type ExplainQuery = { why_selected: { item: string } } | { why_excluded: { item: string } }
  | { why_order: { a: string; b: string } } | 'why_outcome' | 'text_origin' | 'models'
  | { diff: { other: ExplainEvidence } };
export interface ExplainOptions {
  nowMs?: number;
  scope?: { mode: 'principal'; tenant: string; context_revision: string; principal_scope: string }
    | { mode: 'tenant_only'; tenant: string; context_revision: string };
  resolve?: (reference: string, maxBytes: number) => Uint8Array | null;
  redaction?: Uint8Array;
}
export interface ExplainResult { explanation: Uint8Array; markdown: string }
export declare class InvalidInput extends RustevError {}

export interface ProfileCheckArgs {
  definitions: string[];
  profile?: string;
  profileBases?: string[];
  variant?: string;
  definitionVersion?: string;
}
export type PlanDiffArgs = { leftPlan: string; rightPlan: string; out: string }
  | { definitions: string[]; leftProfile?: string; rightProfile?: string;
      leftVariant?: string; rightVariant?: string; profileBases?: string[];
      definitionVersion?: string; descriptors?: string[]; rules?: string[];
      calibrations?: string[]; execution?: string; out: string };

/** Spec 063, 3.9: the repeatability commands' arguments, the CLI's flags in
 * camel case. Names are inputs; a directory is an input prefix; `out` names
 * the artifact or artifact prefix written, all or none. */
type RepeatScope = import('./types/rustev-documents').RustevReplay1.Scope;
interface RepeatEvalCommon {
  dataset: string;
  split: 'training' | 'model_selection' | 'calibration' | 'final_test';
  config: string;
  adapter: string;
  repeatConfig: string;
  /** One input prefix (with `repeat`), or 2 to 64 in run order. */
  bundles: string | string[];
  repeat?: number;
  purpose?: 'measurement' | 'optimization';
  /** `final_test` only, with `ledgerOut` and one of `ledger` and `ledgerNew`. */
  lineage?: string;
  ledger?: string;
  ledgerNew?: boolean;
  ledgerOut?: string;
  nowMs: number;
  scope: RepeatScope;
  hostCapMs?: number;
  store?: string;
  out: string;
}
export interface EvalRepeatArgs extends RepeatEvalCommon {
  candidatePlan?: string;
  descriptors?: string[];
  rules?: string[];
  calibrations?: string[];
}
export interface EvalCandidatesArgs extends RepeatEvalCommon {
  candidates: string;
}
export interface BaselineRecordArgs { name: string; eval: string; out: string; nowMs: number }
export interface EvalCompareArgs { baseline: string; baselineEval: string; run: string }
export interface GateSelectArgs {
  selectionGate: string;
  baseline: string;
  baselineEval: string;
  candidates: string;
  out: string;
}
export interface EvalRepeatImportArgs {
  repeatConfig: string;
  config: string;
  /** 1 to 64 input prefixes, in run order. */
  runs: string[];
  out: string;
}

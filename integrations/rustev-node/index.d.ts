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

export interface EngineOptions {
  runtime: RuntimeConfig;
  /** Required; there is no default sink. */
  sink: EvidenceSink;
  rulesBackends?: { program: DocumentInput; maxConcurrency: number }[];
  backends?: DecisionBackend[];
  contextSource?: ContextSource;
  clock?: 'system' | 'manual';
}

export interface DecisionRequest {
  decisionId: string;
  /** Omitted only when the engine has a context source. */
  snapshot?: DocumentInput;
  evaluationTimeMs: number;
  deadlineMs?: number;
}

export interface DecideOptions {
  signal?: AbortSignal;
  /** Can only tighten the plan's deadline. */
  deadlineMs?: number;
  /** Opaque bytes passed to the runtime; never interpreted. */
  principalHandle?: Uint8Array;
  /** A W3C Trace Context version 00 header (3.8). */
  traceparent?: string;
}

export interface DecideResult {
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
}

export interface CommandResult {
  /** The CLI's exit code for this outcome. */
  code: number;
  /** The output object's bytes, as the CLI prints them. */
  out: Uint8Array;
  artifacts: Record<string, Uint8Array>;
}

export declare class Engine {
  static create(options: EngineOptions): Promise<Engine>;
  /** Compile a definition, or load a compiled `plan`, and prepare it:
   * exactly one of the two, or `InvalidDocument`. */
  compile(args: {
    definition?: DocumentInput;
    plan?: DocumentInput;
    descriptors?: DocumentInput[];
    calibrations?: DocumentInput[];
  }): Promise<PlanHandle>;
  /** Like `compile`, on the calling thread (blocking). */
  compileSync(args: {
    definition?: DocumentInput;
    plan?: DocumentInput;
    descriptors?: DocumentInput[];
    calibrations?: DocumentInput[];
  }): PlanHandle;
  /** A pure command through the shared surface (`check`, `compile`,
   * `show`, `replay`, `eval`, `gate`, `calibrateFit`, `calibrateQualify`),
   * on the engine's blocking pool. */
  command(name: string, inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  commandSync(name: string, inputs: Record<string, DocumentInput>, args: object): CommandResult;
  check(inputs: Record<string, DocumentInput>, args: object): CommandResult;
  show(inputs: Record<string, DocumentInput>, args: object): CommandResult;
  replay(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  evaluate(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  gate(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  calibrateFit(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  calibrateQualify(inputs: Record<string, DocumentInput>, args: object): Promise<CommandResult>;
  onGauges(cb: (gauges: unknown) => void | Promise<void>, options: { everyMs: number }): () => void;
  advanceClock(ms: number): void;
  clockSleepers(): number;
  clockDelays(): number[];
  gauges(): unknown;
  shutdown(options?: { drainMs?: number }): Promise<{ drained: boolean; cancelled: number; lateDiscarded: number; stillRunning: number }>;
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
  readonly failureClass: 'transient' | 'overloaded' | 'permanent' | 'cancelled';
  readonly charge: Charge;
  readonly remote: RemoteEnd;
  readonly cancel?: 'stopped' | 'unconfirmed';
  constructor(args: {
    failureClass: 'transient' | 'overloaded' | 'permanent' | 'cancelled';
    charge: Charge;
    remote: RemoteEnd;
    cancel?: 'stopped' | 'unconfirmed';
    detail?: string;
  });
}

export declare function version(): string;

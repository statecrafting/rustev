'use strict';
// Rustev's Node-API package (spec 027, 3.3 to 3.9): the TypeScript API over
// the native addon. The addon is loaded from the platform package that
// matches this process (3.13.2); nothing is downloaded at install or load
// time, and a missing platform package is a typed error naming it.

const { existsSync } = require('node:fs');
const { join } = require('node:path');

// ---- errors (3.4) -------------------------------------------------------

class RustevError extends Error {
  constructor(code, detail, message) {
    super(message ?? `${code}${detail === undefined || detail === null ? '' : `: ${JSON.stringify(detail)}`}`);
    this.name = new.target.name;
    this.code = code;
    this.detail = detail;
  }
}
const classes = {};
for (const name of [
  'CompileRefusal',
  'InvalidDocument',
  'ConfigRefused',
  'PlanPrepareRefused',
  'AdmissionRejected',
  'DeadlineExceeded',
  'CancelledBeforeAdmission',
  'SinkFailure',
  'CaptureConfigRefused',
  'ContextUnavailable',
  'InvalidTraceContext',
  'EngineShutDown',
  'BudgetExhausted',
  'InternalError',
  'PlatformPackageMissing',
]) {
  classes[name] = { [name]: class extends RustevError {} }[name];
}

/** A host seam's typed failure (3.6.5): its class, the charge it observed
 * and the remote end it knows. */
class SeamFailure extends Error {
  /** `failureClass` is `transient`, `overloaded`, `permanent` or, answering
   * a cancellation, `cancelled` with `cancel` `stopped` (the default) or
   * `unconfirmed`. */
  constructor({ failureClass, charge, remote, cancel, detail = '' }) {
    super(detail || failureClass);
    this.name = 'SeamFailure';
    this.failureClass = failureClass;
    this.charge = charge;
    this.remote = remote;
    this.cancel = cancel;
  }
}

/** The typed error a native failure stands for. */
function typed(e) {
  // Already typed (thrown by this layer): unchanged.
  if (!(e instanceof Error) || e instanceof RustevError) return e;
  let parsed;
  try {
    parsed = JSON.parse(e.message);
  } catch {
    return new classes.InternalError('internal', e.message);
  }
  const C = classes[parsed.class] ?? classes.InternalError;
  const err = new C(parsed.code, parsed.detail);
  if (parsed.class === 'SinkFailure' && parsed.detail?.recordHex) {
    err.record = Uint8Array.from(Buffer.from(parsed.detail.recordHex, 'hex'));
  }
  return err;
}

function wrap(f) {
  try {
    return f();
  } catch (e) {
    throw typed(e);
  }
}

// ---- loading (3.13.2) ---------------------------------------------------

function platformName() {
  const { platform, arch } = process;
  if (platform === 'linux') {
    // The C library decides the package (gnu or musl). The diagnostic
    // report names it; it runs once, on Linux only, and without the
    // network section, the costly part, where Node allows leaving it out.
    // The setting is restored afterwards, so other code's reports are
    // unaffected.
    const report = process.report;
    // An own accessor of process.report: writing it back calls its setter.
    const had = report && Object.hasOwn(report, 'excludeNetwork') ? report.excludeNetwork : undefined;
    if (had !== undefined) report.excludeNetwork = true;
    const glibc = report?.getReport?.().header?.glibcVersionRuntime;
    if (had !== undefined) report.excludeNetwork = had;
    return `linux-${arch}-${glibc ? 'gnu' : 'musl'}`;
  }
  if (platform === 'win32') return `win32-${arch}-msvc`;
  return `${platform}-${arch}`;
}

function load() {
  const name = platformName();
  // A local build (`make bindings`) sits beside this file.
  const local = join(__dirname, `rustev.${name}.node`);
  if (existsSync(local)) {
    try {
      return require(local);
    } catch (e) {
      // A local build that does not load (corrupt, or built for another
      // ABI) is a typed failure naming it, never an untyped one.
      throw new classes.InternalError('local_build_unloadable', { path: local, reason: String(e) });
    }
  }
  const pkg = `@statecrafting/rustev-${name}`;
  // Absent is one failure, decided by resolution alone; installed but not
  // loadable (corrupt, built for another ABI, or missing a dependency) is
  // another, and is named as such.
  let resolved;
  try {
    resolved = require.resolve(pkg);
  } catch {
    throw new classes.PlatformPackageMissing('platform_package_missing', { package: pkg });
  }
  try {
    return require(resolved);
  } catch (e) {
    throw new classes.InternalError('platform_package_unloadable', { package: pkg, reason: String(e) });
  }
}
const native = load();

// ---- documents ----------------------------------------------------------

/** Documents cross as UTF-8 canonical JSON bytes or a string; an object is
 * serialized with JSON.stringify and parsed on the Rust side by the bounded
 * parser (3.2.1). */
function bytes(doc) {
  if (doc instanceof Uint8Array) return doc;
  if (typeof doc === 'string') return Buffer.from(doc, 'utf8');
  return Buffer.from(JSON.stringify(doc), 'utf8');
}

// ---- seams (3.6) ---------------------------------------------------------

function sinkSeam(sink) {
  return {
    queueCapacity: sink.queueCapacity,
    acceptsRun3: Boolean(sink.acceptsRun3),
    deliver: (call) =>
      Promise.resolve().then(() =>
        sink.deliver(call.record, { decisionId: call.decisionId, traceparent: call.traceparent ?? undefined }),
      ),
  };
}

function contextSeam(source) {
  return {
    queueCapacity: source.queueCapacity,
    fetch: (call) =>
      Promise.resolve()
        .then(() =>
          source.fetch(call.fields, {
            decisionId: call.decisionId,
            traceparent: call.traceparent ?? undefined,
          }),
        )
        .then((snapshot) => {
          // A source that resolves with nothing broke its contract; that is
          // said plainly, and the decision is ContextUnavailable.
          if (snapshot == null) throw new Error('the context source resolved with no snapshot');
          return bytes(snapshot);
        }),
  };
}

function backendSeam(b) {
  const controllers = new Map();
  // An abort and its attempt's call reach JavaScript through two separate
  // queues, so the abort can arrive first: it is remembered, and the
  // attempt starts with its signal already aborted.
  // Kept briefly: an abort that crossed with its attempt's answer (the
  // attempt already done) is never consumed, and is pruned after a minute
  // (checked whenever an attempt starts or an abort arrives).
  const abortedEarly = new Map();
  const prune = () => {
    const stale = Date.now() - 60_000;
    for (const [id, at] of abortedEarly) if (at < stale) abortedEarly.delete(id);
  };
  return {
    descriptor: bytes(b.descriptor),
    costModel: JSON.stringify(b.costModel),
    costBound: JSON.stringify(b.costBound),
    maxConcurrency: b.maxConcurrency,
    queueCapacity: b.queueCapacity,
    infer: async (call) => {
      const controller = new AbortController();
      controllers.set(call.attemptId, controller);
      if (abortedEarly.delete(call.attemptId)) controller.abort();
      // Pruned whenever an attempt starts too, so stale entries never
      // outlive a minute of activity.
      prune();
      try {
        const answer = await b.infer({
          attemptId: call.attemptId,
          projection: call.projection,
          remainingMs: call.remainingMs,
          reservedBound: JSON.parse(call.reservedBound),
          principalHandle: call.principalHandle,
          signal: controller.signal,
          traceparent: call.traceparent ?? undefined,
        });
        return {
          output: answer.output,
          charge: answer.charge,
          remote: answer.remote ?? 'finished',
        };
      } catch (e) {
        if (e instanceof SeamFailure) {
          return {
            failure: { class: e.failureClass, detail: e.message },
            charge: e.charge,
            remote: e.remote,
            cancel: e.failureClass === 'cancelled' ? (e.cancel ?? 'stopped') : undefined,
          };
        }
        throw e;
      } finally {
        controllers.delete(call.attemptId);
      }
    },
    abort: (attemptId) => {
      prune();
      const controller = controllers.get(attemptId);
      if (controller) controller.abort();
      else abortedEarly.set(attemptId, Date.now());
    },
  };
}

// ---- the engine (3.3) ----------------------------------------------------

// While any decision is in flight the process stays alive: the seams'
// threadsafe functions are weak (an idle engine never holds the event loop),
// so this one referenced timer stands for the work Rust is doing.
let inFlight = 0;
let keepAlive = null;
function begin() {
  if (inFlight++ === 0) keepAlive = setInterval(() => {}, 1 << 30);
}
function end() {
  if (--inFlight === 0) {
    clearInterval(keepAlive);
    keepAlive = null;
  }
}

const TRACEPARENT = /^00-(?!0{32})[0-9a-f]{32}-(?!0{16})[0-9a-f]{16}-[0-9a-f]{2}$/;

class PlanHandle {
  #engine;
  #native;
  constructor(engine, nativePlan) {
    this.#engine = engine;
    this.#native = nativePlan;
  }
  get planId() {
    return this.#native.planId;
  }
  get raw() {
    return this.#native.raw;
  }
  /** Decide one request (3.3.3); an unresolved outcome is a value. */
  async decide(request, options = {}) {
    if (options.traceparent !== undefined && !TRACEPARENT.test(options.traceparent)) {
      throw new classes.InvalidTraceContext('invalid_traceparent', { traceparent: options.traceparent });
    }
    const cancel = wrap(() => natives.get(this.#engine).cancelHandle());
    running.get(this.#engine).add(cancel);
    const signal = options.signal;
    // An already-aborted signal aborts before admission (3.5.3).
    if (signal?.aborted) cancel.raise();
    const onAbort = () => cancel.raise();
    signal?.addEventListener('abort', onAbort, { once: true });
    begin();
    try {
      const r = await this.#native.decide(
        {
          decisionId: request.decisionId,
          snapshot: request.snapshot === undefined ? undefined : bytes(request.snapshot),
          traceparent: options.traceparent,
          evaluationTimeMs: request.evaluationTimeMs,
          deadlineMs: options.deadlineMs ?? request.deadlineMs,
          principalHandle: options.principalHandle,
        },
        cancel,
      );
      const judgment = r.judgment ?? null;
      return {
        completion: r.completion,
        judgment: judgment && JSON.parse(Buffer.from(judgment).toString('utf8')),
        evidence: JSON.parse(Buffer.from(r.evidence).toString('utf8')),
        // The native layer returns the delivery as a JSON string.
        delivery: JSON.parse(r.delivery),
        raw: { judgment, evidence: r.evidence },
      };
    } catch (e) {
      throw typed(e);
    } finally {
      end();
      running.get(this.#engine).delete(cancel);
      signal?.removeEventListener('abort', onAbort);
    }
  }
}

/** Each engine's cancellation handles of decisions in flight (3.9.2): held
 * here, out of reach of code using the engine, so nothing can drop one
 * before shutdown raises it. */
const running = new WeakMap();

/** Each engine's native binding, likewise out of reach: code using an
 * engine cannot replace what its plans decide through. */
const natives = new WeakMap();

class Engine {
  constructor(nativeEngine) {
    natives.set(this, nativeEngine);
    running.set(this, new Set());
  }

  /** Create an engine (3.3); every refusal happens before any thread. */
  static async create(options) {
    // Required: there is no default runtime configuration.
    if (options.runtime === undefined || options.runtime === null) {
      throw new classes.ConfigRefused('missing_runtime', { field: 'runtime' });
    }
    const rt = options.runtime;
    // Required too: there is no default sink.
    if (options.sink === undefined || options.sink === null) {
      throw new classes.ConfigRefused('missing_sink', { field: 'sink' });
    }
    return wrap(
      () =>
        new Engine(
          native.Engine.create(
            {
              maxInFlight: rt.maxInFlight,
              maxQueued: rt.maxQueued,
              maxParallelRequests: rt.maxParallelRequests,
              sinkTimeoutMs: rt.sinkTimeoutMs,
              workerThreads: rt.workerThreads ?? 2,
              blockingThreads: rt.blockingThreads ?? 4,
              clock: options.clock ?? 'system',
            },
            sinkSeam(options.sink),
            (options.rulesBackends ?? []).map((r) => ({
              program: bytes(r.program),
              maxConcurrency: r.maxConcurrency,
            })),
            (options.backends ?? []).map(backendSeam),
            options.contextSource ? contextSeam(options.contextSource) : undefined,
          ),
        ),
    );
  }

  /** Compile a definition and prepare the plan (3.3.1), on the engine's
   * blocking pool (3.5.2); given `plan` instead, load that compiled plan
   * against the descriptors and calibrations, as the CLI's `run` does. */
  async compile({ definition, plan, descriptors = [], calibrations = [] }) {
    // Refused here, as `compileSync` refuses it, before any native call.
    if ((definition == null) === (plan == null)) {
      throw new classes.InvalidDocument('invalid_document', 'compile takes exactly one of a definition and a plan');
    }
    try {
      const p = await natives.get(this).prepareAsync(
        definition == null ? undefined : bytes(definition),
        plan == null ? undefined : bytes(plan),
        descriptors.map(bytes),
        calibrations.map(bytes),
      );
      return new PlanHandle(this, p);
    } catch (e) {
      throw typed(e);
    }
  }

  /** `compile` on the calling thread: blocking, bounded by the same parse
   * limits (3.5.2). */
  compileSync({ definition, plan, descriptors = [], calibrations = [] }) {
    return wrap(() => {
      if ((definition == null) === (plan == null)) {
        throw new classes.InvalidDocument('invalid_document', 'compile takes exactly one of a definition and a plan');
      }
      const ds = descriptors.map(bytes);
      const cs = calibrations.map(bytes);
      const p =
        plan != null
          ? natives.get(this).load(bytes(plan), ds, cs)
          : natives.get(this).compile(bytes(definition), ds, cs);
      return new PlanHandle(this, p);
    });
  }

  /** A pure command through the shared surface, on the blocking pool: the
   * CLI's exit code, its output bytes and the artifacts it published. */
  async command(name, inputs, args) {
    try {
      return await natives.get(this).commandAsync(name, named(inputs), args);
    } catch (e) {
      throw typed(e);
    }
  }

  /** A pure command on the calling thread (blocking). */
  commandSync(name, inputs, args) {
    return wrap(() => natives.get(this).command(name, named(inputs), args));
  }

  /** `plan check` and `plan show`: pure, synchronous (3.3). */
  check(inputs, args) {
    return this.commandSync('check', inputs, args);
  }
  show(inputs, args) {
    return this.commandSync('show', inputs, args);
  }
  /** Offline replay, evaluation, gates and calibration (3.3): `nowMs` is
   * always an explicit argument. */
  replay(inputs, args) {
    return this.command('replay', inputs, args);
  }
  evaluate(inputs, args) {
    return this.command('eval', inputs, args);
  }
  gate(inputs, args) {
    return this.command('gate', inputs, args);
  }
  calibrateFit(inputs, args) {
    return this.command('calibrateFit', inputs, args);
  }
  calibrateQualify(inputs, args) {
    return this.command('calibrateQualify', inputs, args);
  }

  advanceClock(ms) {
    return wrap(() => natives.get(this).advanceClock(ms));
  }

  clockSleepers() {
    return natives.get(this).clockSleepers();
  }

  /** How far ahead each pending timer of a manual clock is due. */
  clockDelays() {
    return natives.get(this).clockDelays();
  }

  /** An operator view, not evidence (3.9.3). */
  gauges() {
    return natives.get(this).gauges();
  }

  /** The same view every `everyMs` (100 to 3,600,000) through a bounded,
   * drop-oldest queue of capacity 1: a slow callback sees the latest
   * snapshot, never a backlog. Returns the unsubscribe function. */
  onGauges(cb, { everyMs }) {
    if (!Number.isInteger(everyMs) || everyMs < 100 || everyMs > 3_600_000) {
      throw new classes.ConfigRefused('out_of_range', { field: 'everyMs', min: 100, max: 3_600_000 });
    }
    let pending = null;
    let busy = false;
    const drain = async () => {
      busy = true;
      while (pending !== null) {
        const g = pending;
        pending = null;
        try {
          await cb(g);
        } catch {
          // A subscriber's failure never reaches the engine.
        }
      }
      busy = false;
    };
    const timer = setInterval(() => {
      pending = this.gauges();
      if (!busy) drain();
    }, everyMs);
    timer.unref();
    return () => clearInterval(timer);
  }

  /** Stop admission at once, wait for decisions in flight up to `drainMs`
   * (0 to 600,000), then raise cancellation on the rest, which end with
   * the runtime's honest cancellation evidence (3.9.2). */
  async shutdown({ drainMs = 0 } = {}) {
    if (!Number.isInteger(drainMs) || drainMs < 0 || drainMs > 600_000) {
      throw new classes.ConfigRefused('out_of_range', { field: 'drainMs', min: 0, max: 600_000 });
    }
    const lateCount = () => this.gauges().seams.reduce((n, s) => n + s.lateResolutionsDiscarded, 0);
    const lateBefore = lateCount();
    let active = natives.get(this).shutdown();
    const until = Date.now() + drainMs;
    while (active > 0 && Date.now() < until) {
      await new Promise((r) => setTimeout(r, 5));
      active = natives.get(this).shutdown();
    }
    const live = running.get(this);
    const cancelled = live.size;
    for (const c of live) c.raise();
    // A cancelled decision ends once the runtime records its cancellation;
    // the wait is bounded all the same, and what still runs is reported.
    const settleBy = Date.now() + 30_000;
    let remaining = natives.get(this).shutdown();
    while (remaining > 0 && Date.now() < settleBy) {
      await new Promise((r) => setTimeout(r, 5));
      remaining = natives.get(this).shutdown();
    }
    // The late settlements discarded during this shutdown.
    return { drained: active === 0, cancelled, lateDiscarded: lateCount() - lateBefore, stillRunning: remaining };
  }
}

function named(inputs) {
  const out = {};
  for (const [k, v] of Object.entries(inputs)) out[k] = bytes(v);
  return out;
}

module.exports = {
  Engine,
  PlanHandle,
  RustevError,
  SeamFailure,
  version: native.version,
  ...classes,
};

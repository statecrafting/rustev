// Spec 027, section 5: every observable negative case of the native
// package, deterministic, without network access or wall-clock sleeps (a
// manual clock drives time). SYNTHETIC fixtures from the conformance suite.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { copyFileSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, '..');
const root = join(pkg, '../..');
const require = createRequire(import.meta.url);
const rustev = require(pkg);
const fx = (n) => readFileSync(join(root, 'tools/rustev-conformance/fixtures/runtime', n));

const NOW = 1_790_164_800_000;
const DESCRIPTORS = [
  'synthetic-linear-head.descriptor.json',
  'synthetic-label-only.descriptor.json',
  'synthetic-linear-head-replica.descriptor.json',
  'synthetic-other-head.descriptor.json',
].map(fx);
const OUTPUTS = {
  'support.topic': { logits: { billing: 4, integration_defect: 0, account_access: 0, other: 0 } },
  'support.frustration': { logits: { calm: 3, frustrated: 0, very_angry: 0 } },
  'support.deadline': { logits: { false: 3, true: 0 } },
};
const task = (call) => JSON.parse(Buffer.from(call.projection).toString('utf8')).task;
// The engine answers on its own threads, so a condition can take many event
// loop turns on a loaded runner: a count of turns flaked in CI (100,000
// turns were not enough on a shared runner). The bound is wall time instead,
// generous because it only ends a wait that would otherwise hang; each turn
// still yields with setImmediate, never a timed sleep.
const UNTIL_MS = 120_000;
const until = async (cond) => {
  const start = Date.now();
  while (Date.now() - start < UNTIL_MS) {
    if (cond()) return;
    await new Promise((r) => setImmediate(r));
  }
  if (cond()) return;
  throw new Error(`waited ${UNTIL_MS} ms for a condition`);
};

/** An engine on a manual clock with one host backend answering `infer`. */
async function engine({ infer, queueCapacity = 64, sink, runtime = {}, contextSource, plan = 'support.plan.json' }) {
  const e = await rustev.Engine.create({
    runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 3, sinkTimeoutMs: 1000, ...runtime },
    clock: 'manual',
    sink: sink ?? { queueCapacity: 8, deliver: async () => 'ack' },
    contextSource,
    backends: [
      {
        descriptor: DESCRIPTORS[0],
        costModel: 'bounded',
        costBound: { bounded: { max_units: 5 } },
        maxConcurrency: 8,
        queueCapacity,
        infer,
      },
    ],
  });
  const p = await e.compile({ plan: fx(plan), descriptors: DESCRIPTORS, calibrations: [fx('topic.calibration.json')] });
  return { e, p };
}

const request = (id = 'd1', extra = {}) => ({
  decisionId: id,
  snapshot: fx('support.snapshot.json'),
  evaluationTimeMs: NOW,
  ...extra,
});
/** The host backend's seam gauges, by name rather than position. */
const backendGauge = (e) => e.gauges().seams.find((g) => g.name.startsWith('backend:'));
const ok = async (call) => ({ output: OUTPUTS[task(call)], charge: { observed: { units: 1 } } });
const attempts = (r) => r.evidence.requests.flatMap((q) => q.attempts);

test('a full host backend queue refuses at once: overloaded, nothing charged, finished', async () => {
  let held = [];
  const { e, p } = await engine({
    queueCapacity: 1,
    infer: (call) => new Promise((resolve) => held.push(() => resolve(ok(call)))),
  });
  const d = p.decide(request());
  await until(() => held.length === 1 && backendGauge(e).refusedFull >= 2);
  const depth = backendGauge(e).queueDepth;
  assert.equal(depth, 1, 'the queue never exceeds its capacity');
  held.forEach((f) => f());
  const r = await d;
  const refused = attempts(r).filter((a) => JSON.stringify(a.end).includes('overloaded'));
  assert.ok(refused.length >= 2, JSON.stringify(attempts(r)));
  for (const a of refused) {
    assert.deepEqual(a.cost.charge, { observed: { units: 0 } });
    assert.equal(a.remote, 'finished');
  }
});

test('a seam queue capacity of 0 or above 65,536 is refused before any thread', async () => {
  for (const queueCapacity of [0, 65_537]) {
    await assert.rejects(engine({ queueCapacity, infer: ok }), (err) => {
      assert.ok(err instanceof rustev.ConfigRefused, String(err));
      assert.equal(err.code, 'out_of_range');
      return true;
    });
  }
});

test('a host backend resolving after the deadline is discarded and counted', async () => {
  let late;
  const { e, p } = await engine({
    infer: (call) =>
      task(call) === 'support.topic' ? new Promise((resolve) => (late = () => resolve(ok(call)))) : ok(call),
  });
  const d = p.decide(request('d1', { deadlineMs: 50 }));
  await until(() => late !== undefined);
  // The plan's other two steps run beside the topic; until the runtime has
  // read their answers, the deadline would discard and count them too.
  await until(() => Object.values(e.gauges().backendInFlight)[0] === 1);
  e.advanceClock(50);
  const r = await d;
  const topic = r.evidence.requests.find((q) => q.step === 'topic');
  assert.deepEqual(topic.result, { failed: 'deadline_exceeded' });
  late();
  // On a timeout, say what the seam saw: an intermittent CI failure here
  // (2026-10-24, after the fix for one race) needs the counts to diagnose.
  try {
    await until(() => backendGauge(e).lateResolutionsDiscarded === 1);
  } catch (err) {
    throw new Error(`${err.message}; gauges: ${JSON.stringify(e.gauges())}`);
  }
});

test('a plain Error is permanent with an unknown charge and possibly continuing work', async () => {
  const { p } = await engine({
    infer: async (call) => {
      if (task(call) === 'support.topic') throw new Error('boom');
      return ok(call);
    },
  });
  const r = await p.decide(request());
  const a = r.evidence.requests.find((q) => q.step === 'topic').attempts[0];
  assert.equal(a.cost.charge, 'unknown');
  assert.equal(a.remote, 'possibly_continuing');
  assert.match(JSON.stringify(a.end), /permanent/);
});

test('an output the core refuses is invalid backend output: a value, not an exception', async () => {
  const { p } = await engine({
    infer: async (call) =>
      task(call) === 'support.topic'
        ? { output: { logits: { not_an_option: 1 } }, charge: { observed: { units: 1 } } }
        : ok(call),
  });
  const r = await p.decide(request());
  assert.equal(r.completion, 'judged');
  const topic = r.evidence.requests.find((q) => q.step === 'topic');
  assert.ok(JSON.stringify(topic.result).includes('invalid_backend_output'), JSON.stringify(topic.result));
});

test('budget exhaustion during a decision is an unresolved value', async () => {
  const { p } = await engine({
    plan: 'budget.plan.json',
    runtime: { maxParallelRequests: 1 },
    infer: async (call) => {
      if (task(call) === 'support.topic') {
        throw new rustev.SeamFailure({ failureClass: 'transient', charge: { observed: { units: 4 } }, remote: 'finished' });
      }
      return { output: OUTPUTS[task(call)], charge: { observed: { units: 4 } } };
    },
  });
  // The budget plan's backend bound is 4 units per attempt.
  const r = await p.decide(request());
  assert.equal(r.completion, 'judged');
  const topic = r.evidence.requests.find((q) => q.step === 'topic');
  assert.ok(JSON.stringify(topic.result).includes('budget_exhausted'), JSON.stringify(topic.result));
});

test('the deadline passing while queued is DeadlineExceeded with no record', async () => {
  let release;
  const { e, p } = await engine({
    runtime: { maxQueued: 1 },
    infer: (call) => new Promise((resolve) => (release ??= () => resolve(ok(call)))),
  });
  const d1 = p.decide(request('d1'));
  await until(() => release !== undefined);
  const d2 = p.decide(request('d2', { deadlineMs: 10 }));
  await until(() => e.gauges().queued === 1);
  e.advanceClock(10);
  await assert.rejects(d2, (err) => err instanceof rustev.DeadlineExceeded && err.code === 'queue_deadline');
  e.advanceClock(5000);
  await d1;
});

test('aborting after dispatch records the honest answer: possibly continuing', async () => {
  // The runtime polls a cancelled attempt once for its answer (spec 003,
  // 3.7.3); a host seam answers across threads, so its answer is usually
  // not observed then, and observed (`unconfirmed`) only when it crosses
  // before that poll. Either way the work stays possibly continuing.
  const controller = new AbortController();
  let dispatched = 0;
  const { p } = await engine({
    infer: (call) =>
      new Promise((_, reject) => {
        dispatched++;
        call.signal.addEventListener('abort', () =>
          reject(new rustev.SeamFailure({ failureClass: 'cancelled', charge: 'unknown', remote: 'possibly_continuing', cancel: 'unconfirmed' })),
        );
      }),
  });
  const d = p.decide(request(), { signal: controller.signal });
  await until(() => dispatched === 3);
  controller.abort();
  const r = await d;
  assert.equal(r.completion, 'cancelled');
  for (const a of attempts(r)) {
    assert.equal(a.remote, 'possibly_continuing');
    const answer = a.cancellation.requested?.answer;
    assert.ok(['not_observed', 'unconfirmed'].includes(answer), JSON.stringify(a.cancellation));
    assert.deepEqual(a.cancellation, { requested: { answer } });
  }
});

test('an already-aborted signal aborts before admission', async () => {
  const { p } = await engine({ infer: ok });
  const c = new AbortController();
  c.abort();
  await assert.rejects(p.decide(request(), { signal: c.signal }), (err) => err instanceof rustev.CancelledBeforeAdmission);
});

test('a sink rejecting delivery is SinkFailure carrying the record', async () => {
  const { p } = await engine({
    infer: ok,
    sink: {
      queueCapacity: 4,
      deliver: async () => {
        throw new Error('disk full');
      },
    },
  });
  await assert.rejects(p.decide(request()), (err) => {
    assert.ok(err instanceof rustev.SinkFailure, String(err));
    assert.ok(err.record instanceof Uint8Array && err.record.length > 0);
    assert.equal(JSON.parse(Buffer.from(err.record).toString('utf8')).decision_id, 'd1');
    return true;
  });
});

test('a context source that resolves with nothing is ContextUnavailable, said plainly', async () => {
  const { p } = await engine({ infer: ok, contextSource: { queueCapacity: 2, fetch: async () => undefined } });
  await assert.rejects(p.decide({ decisionId: 'd1', evaluationTimeMs: NOW }), (err) => {
    assert.ok(err instanceof rustev.ContextUnavailable, String(err));
    assert.match(JSON.stringify(err.detail), /resolved with no snapshot/);
    return true;
  });
});

test('a context source that throws is ContextUnavailable: no decision, nothing defaulted', async () => {
  let dispatched = 0;
  const { p } = await engine({
    infer: async (call) => (dispatched++, ok(call)),
    contextSource: {
      queueCapacity: 2,
      fetch: async () => {
        throw new Error('down');
      },
    },
  });
  await assert.rejects(p.decide({ decisionId: 'd1', evaluationTimeMs: NOW }), (err) => {
    assert.ok(err instanceof rustev.ContextUnavailable, String(err));
    assert.equal(err.code, 'host_seam_failed');
    return true;
  });
  assert.equal(dispatched, 0);
});

test('a request without a snapshot or a context source is refused before admission', async () => {
  const { p } = await engine({ infer: ok });
  await assert.rejects(
    p.decide({ decisionId: 'd1', evaluationTimeMs: NOW }),
    (err) => err instanceof rustev.AdmissionRejected && err.code === 'invalid_request',
  );
});

test('a context source snapshot decides, and its traceparent reaches every seam', async () => {
  const seen = new Set();
  const tp = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01';
  const { p } = await engine({
    infer: async (call) => (seen.add(call.traceparent), ok(call)),
    sink: { queueCapacity: 4, deliver: async (_r, c) => (seen.add(c.traceparent), 'ack') },
    contextSource: {
      queueCapacity: 2,
      fetch: async (fields, c) => {
        seen.add(c.traceparent);
        assert.ok(fields.includes('ticket.message'));
        return fx('support.snapshot.json');
      },
    },
  });
  const r = await p.decide({ decisionId: 'd1', evaluationTimeMs: NOW }, { traceparent: tp });
  assert.equal(r.completion, 'judged');
  assert.deepEqual([...seen], [tp]);
  // The trace context enters no record (3.8).
  assert.ok(!Buffer.from(r.raw.evidence).toString().includes(tp));
});

test('a malformed traceparent is refused before admission', async () => {
  const { p } = await engine({ infer: ok });
  for (const traceparent of ['00-0', '00-00000000000000000000000000000000-00f067aa0ba902b7-01', 'ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01']) {
    await assert.rejects(p.decide(request(), { traceparent }), (err) => err instanceof rustev.InvalidTraceContext);
  }
});

test('a document past its parse bound is InvalidDocument before any decision', async () => {
  const { p } = await engine({ infer: ok });
  const huge = Buffer.alloc(64 * 1024 * 1024, 0x20);
  await assert.rejects(p.decide(request('d1', { snapshot: huge })), (err) => err instanceof rustev.InvalidDocument);
});

test('a missing platform package is a typed error naming it; nothing is downloaded', () => {
  const dir = mkdtempSync(join(tmpdir(), 'rustev-node-'));
  copyFileSync(join(pkg, 'index.js'), join(dir, 'index.js'));
  // Precondition: no platform package resolves from here (Node also looks
  // in every ancestor's node_modules); if one did, this fails, not passes.
  const from = createRequire(join(dir, 'index.js'));
  for (const name of ['darwin-arm64', 'darwin-x64', 'linux-x64-gnu', 'linux-x64-musl', 'linux-arm64-gnu', 'linux-arm64-musl', 'win32-x64-msvc']) {
    assert.throws(() => from.resolve(`@rustev/rustev-${name}`), /Cannot find module/);
  }
  assert.throws(
    () => require(join(dir, 'index.js')),
    (err) => err.name === 'PlatformPackageMissing' && /@rustev\/rustev-/.test(JSON.stringify(err.detail)),
  );
});

test('a local build that does not load is a typed error naming it', () => {
  const built = readdirSync(pkg).find((f) => /^rustev\..+\.node$/.test(f));
  // Every test here loads that local build; without one the suite cannot
  // run at all, so its absence fails rather than skips.
  assert.ok(built, 'a local build sits beside the package (make bindings copies it)');
  const dir = mkdtempSync(join(tmpdir(), 'rustev-node-'));
  copyFileSync(join(pkg, 'index.js'), join(dir, 'index.js'));
  writeFileSync(join(dir, built), 'not a shared library');
  assert.throws(
    () => require(join(dir, 'index.js')),
    (err) => err.name === 'InternalError' && err.code === 'local_build_unloadable',
  );
});

test('compileSync takes exactly one of a definition and a plan', async () => {
  const { e } = await engine({ infer: ok });
  for (const args of [{}, { definition: null }, { definition: '{}', plan: fx('support.plan.json') }]) {
    assert.throws(() => e.compileSync(args), (err) => err instanceof rustev.InvalidDocument);
  }
});

test('a call after shutdown is EngineShutDown', async () => {
  const { e, p } = await engine({ infer: ok });
  const s = await e.shutdown({ drainMs: 0 });
  assert.equal(s.drained, true);
  await assert.rejects(p.decide(request()), (err) => err instanceof rustev.EngineShutDown);
  await assert.rejects(
    e.compile({ plan: fx('support.plan.json'), descriptors: DESCRIPTORS }),
    (err) => err instanceof rustev.EngineShutDown,
  );
});

test('compile refusals and duplicate backends map to their classes and codes', async () => {
  const { e } = await engine({ infer: ok });
  await assert.rejects(e.compile({ definition: '{' }), (err) => err instanceof rustev.InvalidDocument);
  await assert.rejects(
    e.compile({ definition: '{}', plan: fx('support.plan.json'), descriptors: DESCRIPTORS }),
    (err) => err instanceof rustev.InvalidDocument && /exactly one/.test(JSON.stringify(err.detail)),
  );
  await assert.rejects(
    rustev.Engine.create({
      runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1 },
      sink: { queueCapacity: 1, deliver: async () => 'x' },
      backends: [0, 1].map(() => ({
        descriptor: DESCRIPTORS[0],
        costModel: 'bounded',
        costBound: { bounded: { max_units: 1 } },
        maxConcurrency: 1,
        queueCapacity: 1,
        infer: ok,
      })),
    }),
    (err) => err instanceof rustev.ConfigRefused && err.code === 'duplicate_backend',
  );
});

test('shutdown drains, then cancels what is left with honest evidence', async () => {
  const { e, p } = await engine({ infer: () => new Promise(() => {}) });
  const d = p.decide(request());
  await until(() => Object.values(e.gauges().backendInFlight).some((n) => n > 0));
  const s = await e.shutdown({ drainMs: 0 });
  assert.deepEqual([s.drained, s.cancelled], [false, 1]);
  const r = await d;
  assert.equal(r.completion, 'cancelled');
});

test('advancing the manual clock refuses a value it would truncate', async () => {
  const { e } = await engine({ infer: ok });
  for (const ms of [-1, 1.5, 2 ** 53]) {
    assert.throws(() => e.advanceClock(ms), (err) => err instanceof rustev.ConfigRefused && err.code === 'out_of_range');
  }
  e.advanceClock(2 ** 32);
});

test('an engine without a runtime configuration is refused before any thread', async () => {
  await assert.rejects(
    rustev.Engine.create({ sink: { queueCapacity: 1, deliver: async () => 'ack' } }),
    (err) => err instanceof rustev.ConfigRefused && err.code === 'missing_runtime',
  );
});

test('an engine without a sink is refused before any thread', async () => {
  await assert.rejects(
    rustev.Engine.create({ runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1 } }),
    (err) => err instanceof rustev.ConfigRefused && err.code === 'missing_sink',
  );
});

test('advancing an engine on the system clock is refused as such', async () => {
  const e = await rustev.Engine.create({
    runtime: { maxInFlight: 1, maxQueued: 0, maxParallelRequests: 1, sinkTimeoutMs: 1 },
    sink: { queueCapacity: 1, deliver: async () => 'ack' },
  });
  assert.throws(() => e.advanceClock(1), (err) => err instanceof rustev.ConfigRefused && err.code === 'not_a_manual_clock');
});

test('gauges arrive through a drop-oldest subscription; its period is bounded', async () => {
  const { e } = await engine({ infer: ok });
  assert.throws(() => e.onGauges(() => {}, { everyMs: 50 }), (err) => err instanceof rustev.ConfigRefused);
  const seen = [];
  const stop = e.onGauges((g) => seen.push(g), { everyMs: 100 });
  await until(() => seen.length >= 1);
  stop();
  assert.ok('inFlight' in seen[0] && Array.isArray(seen[0].seams));
});
